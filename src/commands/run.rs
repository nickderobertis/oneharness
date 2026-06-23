//! `oneharness run` — drive selected harnesses in parallel and emit a JSON report.

use std::time::Duration;

use crate::cli::RunArgs;
use crate::commands::{print_json, select_specs};
use oneharness_core::domain::harness::{self, BuildCtx, HarnessSpec};
use oneharness_core::domain::report::{
    Capture, OutputFormat, RunReport, RunResult, Status, SCHEMA_VERSION,
};
use oneharness_core::domain::signals::Usage;
use oneharness_core::domain::structured::{self, Schema};
use oneharness_core::domain::{normalize, signals};
use oneharness_core::errors::OneharnessError;
use oneharness_core::io::config as config_io;
use oneharness_core::io::detect::{self, BinOverrides};
use oneharness_core::io::runner::{self, Job};

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
    // Structured output: an optional compiled schema and the per-harness retry
    // budget. Compiling here (once) turns an invalid schema into a loud usage
    // error before any harness is spawned.
    let schema = load_schema(args, cfg, &project_start)?;
    let max_retries = args
        .schema_max_retries
        .or(cfg.schema_max_retries)
        .unwrap_or(2);
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
    // ones that are available and actually being executed. `job_plans` parallels
    // `jobs` and retains what the structured-output retry loop needs to rebuild
    // a harness's argv with a feedback prompt.
    let mut plan: Vec<Plan> = Vec::with_capacity(specs.len());
    let mut jobs: Vec<Job> = Vec::new();
    let mut job_plans: Vec<HarnessPlan> = Vec::new();

    for spec in &specs {
        let resolved = detect::resolve(spec, &overrides);
        let chosen_format = args
            .output_format
            .or(cfg.output_format)
            .unwrap_or(spec.output_format);
        // A native-schema harness must receive its schema as JSON; force the
        // format so the conforming value lands where we read it (Claude Code's
        // `structured_output`, which needs `--output-format json`).
        let native = schema.is_some() && spec.native_schema.is_some();
        let output_format = if native {
            OutputFormat::Json
        } else {
            chosen_format
        };
        let mut extra = cfg.args_for(spec.id).to_vec();
        extra.extend(args.passthrough.iter().cloned());
        let harness_plan = HarnessPlan {
            spec,
            bin: resolved.bin.clone(),
            // A CLI --model beats config; within config, the harness's own
            // [harness.<id>] model beats the top-level one.
            model: args
                .model
                .as_deref()
                .or_else(|| cfg.model_for(spec.id))
                .map(str::to_string),
            system: system.map(str::to_string),
            resume: resume.map(str::to_string),
            bypass,
            output_format,
            native,
            base_prompt: prompt.clone(),
            extra,
        };
        let command = harness_plan.argv(schema.as_ref(), None);

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
            job_plans.push(harness_plan);
        }
    }

    let outcomes = if jobs.is_empty() {
        Vec::new()
    } else {
        let max_parallel = args.max_parallel.or(cfg.max_parallel).unwrap_or(jobs.len());
        match schema.as_ref() {
            // Structured output: after each run, validate and (if it failed and
            // retries remain) re-run with a feedback prompt. The closure is pure
            // domain validation; the runner owns the spawning.
            Some(sch) => runner::run_jobs_with(&jobs, max_parallel, |i, attempt, capture| {
                retry_decision(&job_plans[i], sch, attempt, max_retries, capture)
            }),
            None => runner::run_jobs_with(&jobs, max_parallel, |_, _, _| None),
        }
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
            } => {
                let outcome = &outcomes[job_index];
                executed_result(
                    spec,
                    bin,
                    command,
                    output_format,
                    &outcome.capture,
                    schema.as_ref(),
                    outcome.attempts,
                )
            }
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
        schema: schema.as_ref().map(|s| s.as_value().clone()),
        schema_max_retries: schema.as_ref().map(|_| max_retries),
        config_files: loaded.files,
        results,
    };
    print_json(&report, args.compact)?;

    if exit != EXIT_OK {
        let failed = report
            .results
            .iter()
            .filter(|r| is_failure(r.status, r.available, require_available, r.schema_valid))
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
        structured: None,
        schema_valid: None,
        schema_attempts: None,
        schema_error: None,
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
        structured: None,
        schema_valid: None,
        schema_attempts: None,
        schema_error: None,
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
    schema: Option<&Schema>,
    attempts: u32,
) -> RunResult {
    // Best-effort signals are extracted only from a run that actually produced
    // output (ok or non-zero), never fabricated for a timeout/spawn failure.
    let extracted = match capture.status {
        Status::Ok | Status::Nonzero => normalize::extract(&capture.stdout, output_format),
        _ => None,
    };
    let (text, text_source) = match &extracted {
        Some(e) => (Some(e.text.clone()), Some(e.source.clone())),
        None => (None, None),
    };
    // Structured output: re-derive the validated value for the final capture so
    // the report and the retry loop share one source of truth. Null fields when
    // no schema was requested, or when the run produced nothing to validate.
    let (structured, schema_valid, schema_attempts, schema_error) = match schema {
        Some(sch) if matches!(capture.status, Status::Ok | Status::Nonzero) => {
            let answer = extracted.as_ref().map(|e| e.text.as_str()).unwrap_or("");
            let check = structured::check(sch, spec.native_schema, answer, &capture.stdout);
            if check.is_valid() {
                (check.value, Some(true), Some(attempts), None)
            } else {
                (
                    check.value,
                    Some(false),
                    Some(attempts),
                    Some(check.errors.join("; ")),
                )
            }
        }
        // Ran under a schema but timed out / could not be spawned: nothing to
        // validate, but the attempt count is still meaningful.
        Some(_) => (None, None, Some(attempts), None),
        None => (None, None, None, None),
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
        structured,
        schema_valid,
        schema_attempts,
        schema_error,
        failure_kind,
        failure_kind_source,
        stdout: capture.stdout.clone(),
        stderr: capture.stderr.clone(),
        error: capture.error.clone(),
    }
}

/// Everything needed to (re)build one harness's argv, retained so the
/// structured-output loop can re-run it with a feedback prompt. Holds owned
/// data because the retry closure runs on the runner's worker threads.
struct HarnessPlan {
    spec: &'static HarnessSpec,
    bin: String,
    model: Option<String>,
    system: Option<String>,
    resume: Option<String>,
    bypass: bool,
    output_format: OutputFormat,
    /// The harness takes the schema through a native flag (so the prompt is left
    /// alone); otherwise the schema instruction is appended to the prompt.
    native: bool,
    base_prompt: String,
    /// Config `args` + CLI passthrough, appended verbatim after the built argv.
    extra: Vec<String>,
}

impl HarnessPlan {
    /// Build the argv for one attempt. `schema` drives structured output:
    /// non-native harnesses get the schema instruction appended to the prompt,
    /// native ones get it on the flag. `feedback` (the prior answer + validation
    /// errors) is appended on a retry so the model can correct itself.
    fn argv(&self, schema: Option<&Schema>, feedback: Option<(&str, &[String])>) -> Vec<String> {
        let mut prompt = self.base_prompt.clone();
        if let Some(sch) = schema {
            // Join with a space, not a newline: the structured-output additions
            // must keep the prompt argument newline-free so it can still be passed
            // to a `.bat`/`.cmd` harness shim on Windows (Rust's std rejects an
            // argument with `\n`/`\r` there — see `structured::prompt_instruction`).
            if !self.native {
                prompt.push(' ');
                prompt.push_str(&structured::prompt_instruction(sch.as_text()));
            }
            if let Some((previous, errors)) = feedback {
                prompt.push(' ');
                prompt.push_str(&structured::retry_instruction(
                    sch.as_text(),
                    previous,
                    errors,
                ));
            }
        }
        let ctx = BuildCtx {
            bin: &self.bin,
            prompt: &prompt,
            model: self.model.as_deref(),
            system: self.system.as_deref(),
            resume: self.resume.as_deref(),
            bypass: self.bypass,
            output_format: self.output_format,
            schema: if self.native {
                schema.map(Schema::as_text)
            } else {
                None
            },
        };
        let mut argv = (self.spec.build_argv)(&ctx);
        argv.extend(self.extra.iter().cloned());
        argv
    }
}

/// Decide whether to re-run one harness under the structured-output loop. Returns
/// the next attempt's argv when the response failed validation and retries
/// remain, else `None`. `attempt` is the number of runs completed so far.
fn retry_decision(
    plan: &HarnessPlan,
    schema: &Schema,
    attempt: u32,
    max_retries: u32,
    capture: &Capture,
) -> Option<Vec<String>> {
    // Only a run that produced output can be validated; a timeout / spawn error
    // is not a validation failure and re-running it would just burn the budget.
    if !matches!(capture.status, Status::Ok | Status::Nonzero) {
        return None;
    }
    let answer = normalize::extract(&capture.stdout, plan.output_format).map(|e| e.text);
    let check = structured::check(
        schema,
        plan.spec.native_schema,
        answer.as_deref().unwrap_or(""),
        &capture.stdout,
    );
    if check.is_valid() || attempt > max_retries {
        return None;
    }
    // Feed back what the harness actually said (its extracted answer, else the
    // raw stdout) so the correction prompt is grounded in its own output.
    let previous = match &answer {
        Some(text) if !text.is_empty() => text.clone(),
        _ => capture.stdout.trim().to_string(),
    };
    Some(plan.argv(Some(schema), Some((&previous, &check.errors))))
}

/// Load and compile the structured-output schema, if one was requested. A
/// `--schema` path is relative to the process's working directory; a config
/// `schema_file` is relative to the project directory (where config was
/// discovered), mirroring how each source is written.
fn load_schema(
    args: &RunArgs,
    cfg: &oneharness_core::domain::config::FileConfig,
    project_start: &std::path::Path,
) -> Result<Option<Schema>, OneharnessError> {
    let path = if let Some(p) = &args.schema {
        p.clone()
    } else if let Some(rel) = &cfg.schema_file {
        project_start.join(rel)
    } else {
        return Ok(None);
    };
    let text = std::fs::read_to_string(&path).map_err(|source| OneharnessError::SchemaFile {
        path: path.display().to_string(),
        source,
    })?;
    Schema::compile(&text)
        .map(Some)
        .map_err(OneharnessError::Schema)
}

/// A harness "failed" when it ran and did not exit cleanly, when it could not be
/// spawned, when — under `--require-available` — it was skipped as missing, or
/// when a structured-output run never produced a schema-conforming answer (a
/// run you asked for JSON from and didn't get is a failure, regardless of exit
/// code).
fn is_failure(
    status: Status,
    available: bool,
    require_available: bool,
    schema_valid: Option<bool>,
) -> bool {
    if schema_valid == Some(false) {
        return true;
    }
    match status {
        Status::Nonzero | Status::Timeout | Status::SpawnError => true,
        Status::Skipped => require_available && !available,
        Status::Ok | Status::Planned => false,
    }
}

fn exit_code(results: &[RunResult], require_available: bool) -> i32 {
    let failed = results
        .iter()
        .any(|r| is_failure(r.status, r.available, require_available, r.schema_valid));
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
            structured: None,
            schema_valid: None,
            schema_attempts: None,
            schema_error: None,
            failure_kind: None,
            failure_kind_source: None,
            stdout: String::new(),
            stderr: String::new(),
            error: None,
        }
    }

    fn crush_plan() -> HarnessPlan {
        HarnessPlan {
            spec: harness::by_id("crush").unwrap(),
            bin: "crush".into(),
            model: None,
            system: None,
            resume: None,
            bypass: true,
            output_format: OutputFormat::Text,
            native: false,
            base_prompt: "p".into(),
            extra: Vec::new(),
        }
    }

    fn capture(status: Status, stdout: &str) -> Capture {
        Capture {
            status,
            exit_code: None,
            duration_ms: Some(1),
            stdout: stdout.into(),
            stderr: String::new(),
            error: None,
        }
    }

    #[test]
    fn schema_argv_prompt_stays_newline_free_including_on_retry() {
        // Windows passes the prompt as one argv element to a `.cmd` harness shim,
        // which Rust's std refuses if it contains `\n`/`\r`. The structured-output
        // additions (instruction + retry feedback, even with a multi-line prior
        // answer) must never introduce one.
        let schema = Schema::compile(r#"{"type":"object"}"#).unwrap();
        // Prompt-based (non-native): first attempt appends the instruction.
        let plan = crush_plan();
        let argv = plan.argv(Some(&schema), None);
        assert!(argv.iter().all(|a| !a.contains('\n')), "{argv:?}");
        // ... and a retry with a multi-line prior answer + errors.
        let argv = plan.argv(
            Some(&schema),
            Some((
                "multi\nline\r\nanswer",
                &["e1".to_string(), "e2".to_string()],
            )),
        );
        assert!(
            argv.iter().all(|a| !a.contains('\n') && !a.contains('\r')),
            "{argv:?}"
        );
        // Native (claude) retry: schema rides the flag, feedback rides the prompt.
        let mut native = crush_plan();
        native.spec = harness::by_id("claude-code").unwrap();
        native.native = true;
        native.output_format = OutputFormat::Json;
        let argv = native.argv(Some(&schema), Some(("multi\nline", &["e".to_string()])));
        assert!(argv.iter().all(|a| !a.contains('\n')), "{argv:?}");
    }

    #[test]
    fn retry_decision_covers_valid_invalid_exhausted_and_timeout() {
        let schema = Schema::compile(
            r#"{"type":"object","required":["a"],"properties":{"a":{"type":"integer"}}}"#,
        )
        .unwrap();
        let plan = crush_plan();
        // Conforming → stop.
        assert!(retry_decision(&plan, &schema, 1, 2, &capture(Status::Ok, r#"{"a":1}"#)).is_none());
        // Non-conforming with budget left → re-run with a feedback prompt.
        let next = retry_decision(&plan, &schema, 1, 2, &capture(Status::Ok, r#"{"a":"x"}"#))
            .expect("should retry");
        assert!(next.iter().any(|a| a.contains("did not conform")));
        // Budget spent → stop even though still invalid.
        assert!(retry_decision(&plan, &schema, 3, 2, &capture(Status::Ok, "{}")).is_none());
        // A timeout is not a validation failure, so it is never retried.
        assert!(retry_decision(&plan, &schema, 1, 2, &capture(Status::Timeout, "")).is_none());
        // No extractable answer falls back to the raw stdout in the feedback.
        assert!(retry_decision(&plan, &schema, 1, 2, &capture(Status::Ok, "  ")).is_some());
    }

    #[test]
    fn executed_result_fills_schema_fields_per_status() {
        let schema = Schema::compile(r#"{"type":"object","required":["a"]}"#).unwrap();
        let spec = harness::by_id("crush").unwrap();
        // A conforming ok run: valid, with the value and attempt count.
        let ok = capture(Status::Ok, r#"{"a":1}"#);
        let r = executed_result(
            spec,
            "crush".into(),
            vec!["crush".into()],
            OutputFormat::Text,
            &ok,
            Some(&schema),
            1,
        );
        assert_eq!(r.schema_valid, Some(true));
        assert_eq!(r.schema_attempts, Some(1));
        assert!(r.structured.is_some());
        // Timed out under a schema: nothing to validate, attempts still recorded.
        let to = capture(Status::Timeout, "");
        let r = executed_result(
            spec,
            "crush".into(),
            vec![],
            OutputFormat::Text,
            &to,
            Some(&schema),
            1,
        );
        assert!(r.schema_valid.is_none());
        assert_eq!(r.schema_attempts, Some(1));
        // No schema requested: every structured field is null.
        let r = executed_result(
            spec,
            "crush".into(),
            vec![],
            OutputFormat::Text,
            &ok,
            None,
            1,
        );
        assert!(r.schema_valid.is_none());
        assert!(r.schema_attempts.is_none());
        assert!(r.structured.is_none());
    }

    #[test]
    fn schema_invalid_result_is_a_failure_even_when_status_ok() {
        // A structured-output run that exited cleanly but never conformed is a
        // failure (non-zero exit), so a consumer can gate on it.
        let mut r = result(Status::Ok, true);
        r.schema_valid = Some(false);
        assert_eq!(exit_code(std::slice::from_ref(&r), false), EXIT_FAILURE);
        // A conforming (or schema-less) ok run still passes.
        r.schema_valid = Some(true);
        assert_eq!(exit_code(std::slice::from_ref(&r), false), EXIT_OK);
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
