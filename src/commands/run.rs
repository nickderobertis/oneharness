//! `oneharness run` — drive selected harnesses in parallel and emit a JSON report.

use std::time::Duration;

use crate::cli::RunArgs;
use crate::commands::{print_json, select_specs};
use crate::domain::harness::{self, BuildCtx, HarnessSpec};
use crate::domain::report::{Capture, OutputFormat, RunReport, RunResult, Status, SCHEMA_VERSION};
use crate::domain::signals::Usage;
use crate::domain::{normalize, signals};
use crate::errors::OneharnessError;
use crate::io::config as config_io;
use crate::io::detect::{self, BinOverrides};
use crate::io::runner::{self, Job};

/// Exit codes (clap uses 2 for argument errors).
const EXIT_OK: i32 = 0;
const EXIT_FAILURE: i32 = 1;

pub fn run(args: &RunArgs) -> Result<i32, OneharnessError> {
    // Project config is discovered from where the harnesses will run (--cwd,
    // else the current directory): the project being operated on is the one
    // whose config should apply.
    let project_start = match &args.cwd {
        Some(dir) => dir.clone(),
        None => std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
    };
    let loaded = config_io::load(args.config.as_deref(), args.no_config, &project_start)?;
    let cfg = &loaded.config;

    let prompt = resolve_prompt(args)?;
    // A CLI selection (--all / --harness) replaces the config selection
    // entirely; config `exclude` still applies unless --exclude is given.
    let (all, include) = if args.all || !args.harness.is_empty() {
        (args.all, args.harness.clone())
    } else {
        (
            cfg.all.unwrap_or(false),
            cfg.harnesses.clone().unwrap_or_default(),
        )
    };
    let exclude = if args.exclude.is_empty() {
        cfg.exclude.clone().unwrap_or_default()
    } else {
        args.exclude.clone()
    };
    let specs = select_specs(all, &include, &exclude)?;
    let resume = args.resume.as_deref();
    validate_resume(resume, &specs)?;
    validate_enforceable(args, cfg, &specs)?;
    let config_bins: std::collections::HashMap<String, String> = cfg
        .harness
        .iter()
        .filter_map(|(id, h)| h.bin.clone().map(|bin| (id.clone(), bin)))
        .collect();
    let overrides = BinOverrides::parse(&args.bin)?.with_config_bins(config_bins);
    let cli_env = parse_env(&args.env)?;
    let bypass = if args.no_bypass {
        false
    } else {
        args.bypass || cfg.bypass.unwrap_or(true)
    };
    let model = args.model.as_deref().or(cfg.model.as_deref());
    let system = args.system.as_deref().or(cfg.system.as_deref());
    let timeout = args.timeout.or(cfg.timeout).unwrap_or(120);
    let require_available = args.require_available || cfg.require_available.unwrap_or(false);

    // Build a plan entry for every selected harness; queue jobs only for the
    // ones that are available and actually being executed.
    let mut plan: Vec<Plan> = Vec::with_capacity(specs.len());
    let mut jobs: Vec<Job> = Vec::new();

    for spec in &specs {
        let resolved = detect::resolve(spec, &overrides);
        let output_format = args
            .output_format
            .or(cfg.output_format)
            .unwrap_or(spec.output_format);
        // Hooks are pure data: the TOML table is serialized to JSON here and
        // delivered by the adapter (verified enforceable above).
        let hooks_json = match cfg.hooks_for(spec.id) {
            Some(table) => Some(serde_json::to_string(table)?),
            None => None,
        };
        let ctx = BuildCtx {
            bin: &resolved.bin,
            // A CLI --model beats config; within config, the harness's own
            // [harness.<id>] model beats the top-level one.
            model: args.model.as_deref().or_else(|| cfg.model_for(spec.id)),
            prompt: &prompt,
            system,
            resume,
            allowed_tools: effective_rules(&args.allowed_tools, cfg.allowed_tools_for(spec.id)),
            denied_tools: effective_rules(&args.denied_tools, cfg.denied_tools_for(spec.id)),
            hooks_json: hooks_json.as_deref(),
            bypass,
            output_format,
        };
        let mut command = (spec.build_argv)(&ctx);
        command.extend(cfg.args_for(spec.id).iter().cloned());
        command.extend(args.passthrough.iter().cloned());

        if args.print_command {
            plan.push(Plan::Ready(Box::new(planned_result(
                spec,
                &resolved.bin,
                resolved.available,
                command,
                output_format,
            ))));
        } else if !resolved.available {
            plan.push(Plan::Ready(Box::new(skipped_result(
                spec,
                &resolved.bin,
                command,
                output_format,
            ))));
        } else {
            let job_index = jobs.len();
            // Env layers, applied in order (the runner is last-write-wins):
            // the harness's declared defaults, then config ([env], then
            // [harness.<id>.env]), then the explicit `--env`, which always wins.
            let mut job_env: Vec<(String, String)> = spec
                .default_env
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            job_env.extend(cfg.env_for(spec.id));
            job_env.extend(cli_env.iter().cloned());
            jobs.push(Job {
                argv: command.clone(),
                cwd: args.cwd.clone(),
                env: job_env,
                timeout: Duration::from_secs(timeout),
            });
            plan.push(Plan::Pending {
                spec,
                bin: resolved.bin,
                command,
                output_format,
                job_index,
            });
        }
    }

    let captures = if jobs.is_empty() {
        Vec::new()
    } else {
        let max_parallel = args.max_parallel.or(cfg.max_parallel).unwrap_or(jobs.len());
        runner::run_jobs(&jobs, max_parallel)
    };

    let results: Vec<RunResult> = plan
        .into_iter()
        .map(|entry| match entry {
            Plan::Ready(result) => *result,
            Plan::Pending {
                spec,
                bin,
                command,
                output_format,
                job_index,
            } => executed_result(spec, bin, command, output_format, &captures[job_index]),
        })
        .collect();

    if let Some(dir) = &args.output_dir {
        write_output_dir(dir, &results)?;
    }

    let exit = exit_code(&results, require_available);

    let report = RunReport {
        schema_version: SCHEMA_VERSION,
        oneharness_version: env!("CARGO_PKG_VERSION"),
        prompt,
        // The effective top-level model (CLI, else config); a per-harness
        // config model is visible in that result's `command`.
        model: model.map(str::to_string),
        resume: args.resume.clone(),
        bypass_permissions: bypass,
        dry_run: args.print_command,
        config_files: loaded.files,
        results,
    };
    print_json(&report, args.compact)?;

    if exit != EXIT_OK {
        let failed = report
            .results
            .iter()
            .filter(|r| is_failure(r.status, r.available, require_available))
            .count();
        eprintln!(
            "oneharness: {failed}/{} harness run(s) did not succeed (see results[].status and results[].error)",
            report.results.len()
        );
    }
    Ok(exit)
}

/// A planned harness: either fully resolved (skipped/planned) or awaiting a job.
/// `Ready` is boxed because `RunResult` is far larger than `Pending`'s fields.
enum Plan {
    Ready(Box<RunResult>),
    Pending {
        spec: &'static HarnessSpec,
        bin: String,
        command: Vec<String>,
        output_format: OutputFormat,
        job_index: usize,
    },
}

fn planned_result(
    spec: &HarnessSpec,
    bin: &str,
    available: bool,
    command: Vec<String>,
    output_format: OutputFormat,
) -> RunResult {
    RunResult {
        harness: spec.id.to_string(),
        bin: bin.to_string(),
        available,
        status: Status::Planned,
        exit_code: None,
        duration_ms: None,
        command,
        output_format,
        text: None,
        text_source: None,
        usage: Usage::default(),
        usage_source: None,
        session_id: None,
        failure_kind: None,
        failure_kind_source: None,
        stdout: String::new(),
        stderr: String::new(),
        error: None,
    }
}

fn skipped_result(
    spec: &HarnessSpec,
    bin: &str,
    command: Vec<String>,
    output_format: OutputFormat,
) -> RunResult {
    RunResult {
        harness: spec.id.to_string(),
        bin: bin.to_string(),
        available: false,
        status: Status::Skipped,
        exit_code: None,
        duration_ms: None,
        command,
        output_format,
        text: None,
        text_source: None,
        usage: Usage::default(),
        usage_source: None,
        session_id: None,
        failure_kind: None,
        failure_kind_source: None,
        stdout: String::new(),
        stderr: String::new(),
        error: Some(format!(
            "`{bin}` not found on PATH; harness skipped. Install it: {}",
            spec.install_hint
        )),
    }
}

fn executed_result(
    spec: &HarnessSpec,
    bin: String,
    command: Vec<String>,
    output_format: OutputFormat,
    capture: &Capture,
) -> RunResult {
    // Best-effort signals are extracted only from a run that actually produced
    // output (ok or non-zero), never fabricated for a timeout/spawn failure.
    let extracted = match capture.status {
        Status::Ok | Status::Nonzero => normalize::extract(&capture.stdout, output_format),
        _ => None,
    };
    let (text, text_source) = match extracted {
        Some(e) => (Some(e.text), Some(e.source)),
        None => (None, None),
    };
    let usage_reading = match capture.status {
        Status::Ok | Status::Nonzero => signals::extract_usage(&capture.stdout),
        _ => None,
    };
    let (usage, usage_source) = match usage_reading {
        Some(r) => (r.usage, Some(r.source)),
        None => (Usage::default(), None),
    };
    let session_id = match capture.status {
        Status::Ok | Status::Nonzero => signals::extract_session(&capture.stdout),
        _ => None,
    };
    // Classify only an actual non-zero run: timeouts/spawn failures already carry
    // a oneharness-generated `error`, and `status` explains them.
    let failure = match capture.status {
        Status::Nonzero => signals::classify_failure(&capture.stdout, &capture.stderr),
        _ => None,
    };
    let (failure_kind, failure_kind_source) = match failure {
        Some(f) => (Some(f.kind), Some(f.source)),
        None => (None, None),
    };
    RunResult {
        harness: spec.id.to_string(),
        bin,
        available: true,
        status: capture.status,
        exit_code: capture.exit_code,
        duration_ms: capture.duration_ms,
        command,
        output_format,
        text,
        text_source,
        usage,
        usage_source,
        session_id,
        failure_kind,
        failure_kind_source,
        stdout: capture.stdout.clone(),
        stderr: capture.stderr.clone(),
        error: capture.error.clone(),
    }
}

/// A harness "failed" when it ran and did not exit cleanly, when it could not be
/// spawned, or — under `--require-available` — when it was skipped as missing.
fn is_failure(status: Status, available: bool, require_available: bool) -> bool {
    match status {
        Status::Nonzero | Status::Timeout | Status::SpawnError => true,
        Status::Skipped => require_available && !available,
        Status::Ok | Status::Planned => false,
    }
}

fn exit_code(results: &[RunResult], require_available: bool) -> i32 {
    let failed = results
        .iter()
        .any(|r| is_failure(r.status, r.available, require_available));
    if failed {
        EXIT_FAILURE
    } else {
        EXIT_OK
    }
}

fn resolve_prompt(args: &RunArgs) -> Result<String, OneharnessError> {
    if let Some(path) = &args.prompt_file {
        if path == "-" {
            let mut buf = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf).map_err(|source| {
                OneharnessError::PromptFile {
                    path: "<stdin>".to_string(),
                    source,
                }
            })?;
            return Ok(buf);
        }
        return std::fs::read_to_string(path).map_err(|source| OneharnessError::PromptFile {
            path: path.clone(),
            source,
        });
    }
    args.prompt.clone().ok_or(OneharnessError::NoPrompt)
}

/// Write each result's raw stdout/stderr to `<dir>/<harness>.{stdout,stderr}`.
/// Lets consumers (e.g. allowlister's e2e scripts) read the transcript from files
/// without a JSON parser, preserving their existing `$stream`/`$stream.err`
/// contract.
fn write_output_dir(dir: &std::path::Path, results: &[RunResult]) -> Result<(), OneharnessError> {
    std::fs::create_dir_all(dir).map_err(|source| OneharnessError::OutputDir {
        path: dir.display().to_string(),
        source,
    })?;
    for result in results {
        for (suffix, contents) in [("stdout", &result.stdout), ("stderr", &result.stderr)] {
            let path = dir.join(format!("{}.{suffix}", result.harness));
            std::fs::write(&path, contents).map_err(|source| OneharnessError::OutputDir {
                path: path.display().to_string(),
                source,
            })?;
        }
    }
    Ok(())
}

/// Resume is stateful and single-session: it requires exactly one selected
/// harness, and that harness must support continuation. Both are usage errors,
/// caught before any process is spawned (`--all` is already excluded by clap).
fn validate_resume(
    resume: Option<&str>,
    specs: &[&'static HarnessSpec],
) -> Result<(), OneharnessError> {
    if resume.is_none() {
        return Ok(());
    }
    if specs.len() != 1 {
        return Err(OneharnessError::ResumeMultipleHarnesses {
            count: specs.len(),
            selected: specs.iter().map(|s| s.id).collect::<Vec<_>>().join(", "),
        });
    }
    let spec = specs[0];
    if !spec.supports_resume {
        return Err(OneharnessError::ResumeUnsupported {
            id: spec.id.to_string(),
            supported: harness::all()
                .iter()
                .filter(|s| s.supports_resume)
                .map(|s| s.id)
                .collect::<Vec<_>>()
                .join(", "),
        });
    }
    Ok(())
}

/// The rules a harness gets: an explicit CLI list replaces the config's
/// (per-harness, else top-level) entirely — the usual CLI-beats-config rule.
fn effective_rules<'a>(cli: &'a [String], config: &'a [String]) -> &'a [String] {
    if cli.is_empty() {
        config
    } else {
        cli
    }
}

/// Permission rules and hooks are enforcement settings: a selected harness
/// that cannot apply them through its invocation is a usage error, never a
/// silently unprotected run (the same loud-absence stance as `--resume`).
/// Per-harness config sections are already rejected at parse time; this guards
/// the CLI flags and the top-level config fields against the actual selection.
fn validate_enforceable(
    args: &RunArgs,
    cfg: &crate::domain::config::FileConfig,
    specs: &[&'static HarnessSpec],
) -> Result<(), OneharnessError> {
    let supported = |pred: fn(&&'static HarnessSpec) -> bool| {
        harness::all()
            .iter()
            .filter(pred)
            .map(|s| s.id)
            .collect::<Vec<_>>()
            .join(", ")
    };
    for spec in specs {
        let allowed = effective_rules(&args.allowed_tools, cfg.allowed_tools_for(spec.id));
        if !allowed.is_empty() && !spec.supports_allowed_tools {
            return Err(OneharnessError::UnenforceableSetting {
                id: spec.id.to_string(),
                setting: "allowed_tools",
                supported: supported(|s| s.supports_allowed_tools),
            });
        }
        let denied = effective_rules(&args.denied_tools, cfg.denied_tools_for(spec.id));
        if !denied.is_empty() && !spec.supports_denied_tools {
            return Err(OneharnessError::UnenforceableSetting {
                id: spec.id.to_string(),
                setting: "denied_tools",
                supported: supported(|s| s.supports_denied_tools),
            });
        }
        if cfg.hooks_for(spec.id).is_some() && !spec.supports_hooks {
            return Err(OneharnessError::UnenforceableSetting {
                id: spec.id.to_string(),
                setting: "hooks",
                supported: supported(|s| s.supports_hooks),
            });
        }
    }
    Ok(())
}

fn parse_env(values: &[String]) -> Result<Vec<(String, String)>, OneharnessError> {
    values
        .iter()
        .map(|v| {
            v.split_once('=')
                .filter(|(k, _)| !k.is_empty())
                .map(|(k, val)| (k.to_string(), val.to_string()))
                .ok_or_else(|| OneharnessError::BadEnv(v.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(status: Status, available: bool) -> RunResult {
        RunResult {
            harness: "x".into(),
            bin: "x".into(),
            available,
            status,
            exit_code: None,
            duration_ms: None,
            command: vec![],
            output_format: OutputFormat::Text,
            text: None,
            text_source: None,
            usage: Usage::default(),
            usage_source: None,
            session_id: None,
            failure_kind: None,
            failure_kind_source: None,
            stdout: String::new(),
            stderr: String::new(),
            error: None,
        }
    }

    #[test]
    fn ok_and_skipped_pass_by_default() {
        let results = vec![result(Status::Ok, true), result(Status::Skipped, false)];
        assert_eq!(exit_code(&results, false), EXIT_OK);
    }

    #[test]
    fn nonzero_fails() {
        let results = vec![result(Status::Ok, true), result(Status::Nonzero, true)];
        assert_eq!(exit_code(&results, false), EXIT_FAILURE);
    }

    #[test]
    fn skipped_fails_under_require_available() {
        let results = vec![result(Status::Skipped, false)];
        assert_eq!(exit_code(&results, true), EXIT_FAILURE);
        assert_eq!(exit_code(&results, false), EXIT_OK);
    }

    #[test]
    fn validate_resume_ok_for_single_supported_harness() {
        let claude = harness::by_id("claude-code").unwrap();
        assert!(validate_resume(Some("sid"), &[claude]).is_ok());
        // No --resume: anything goes.
        assert!(validate_resume(None, &[claude, claude]).is_ok());
    }

    #[test]
    fn validate_resume_rejects_multiple_harnesses() {
        let claude = harness::by_id("claude-code").unwrap();
        assert!(matches!(
            validate_resume(Some("sid"), &[claude, claude]),
            Err(OneharnessError::ResumeMultipleHarnesses { count: 2, .. })
        ));
    }

    #[test]
    fn validate_resume_rejects_unsupported_harness() {
        let codex = harness::by_id("codex").unwrap();
        assert!(matches!(
            validate_resume(Some("sid"), &[codex]),
            Err(OneharnessError::ResumeUnsupported { .. })
        ));
    }

    #[test]
    fn parse_env_rejects_malformed() {
        assert!(parse_env(&["KEY=val".into()]).is_ok());
        assert!(parse_env(&["noeq".into()]).is_err());
        assert!(parse_env(&["=val".into()]).is_err());
    }

    #[test]
    fn parse_env_allows_empty_value_and_equals_in_value() {
        let parsed = parse_env(&["A=".into(), "B=x=y".into()]).unwrap();
        assert_eq!(
            parsed,
            vec![("A".into(), "".into()), ("B".into(), "x=y".into())]
        );
    }
}
