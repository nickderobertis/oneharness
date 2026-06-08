//! `oneharness run` — drive selected harnesses in parallel and emit a JSON report.

use std::time::Duration;

use crate::cli::RunArgs;
use crate::commands::{print_json, select_specs};
use crate::domain::harness::{BuildCtx, HarnessSpec};
use crate::domain::normalize;
use crate::domain::report::{Capture, RunReport, RunResult, Status, SCHEMA_VERSION};
use crate::errors::OneharnessError;
use crate::io::detect::{self, BinOverrides};
use crate::io::runner::{self, Job};

/// Exit codes (clap uses 2 for argument errors).
const EXIT_OK: i32 = 0;
const EXIT_FAILURE: i32 = 1;

pub fn run(args: &RunArgs) -> Result<i32, OneharnessError> {
    let prompt = resolve_prompt(args)?;
    let specs = select_specs(args.all, &args.harness, &args.exclude)?;
    let overrides = BinOverrides::parse(&args.bin)?;
    let env = parse_env(&args.env)?;
    let bypass = !args.no_bypass;
    let model = args.model.as_deref();

    // Build a plan entry for every selected harness; queue jobs only for the
    // ones that are available and actually being executed.
    let mut plan: Vec<Plan> = Vec::with_capacity(specs.len());
    let mut jobs: Vec<Job> = Vec::new();

    for spec in &specs {
        let resolved = detect::resolve(spec, &overrides);
        let ctx = BuildCtx {
            bin: &resolved.bin,
            prompt: &prompt,
            model,
            bypass,
        };
        let command = (spec.build_argv)(&ctx);

        if args.print_command {
            plan.push(Plan::Ready(planned_result(
                spec,
                &resolved.bin,
                resolved.available,
                command,
            )));
        } else if !resolved.available {
            plan.push(Plan::Ready(skipped_result(spec, &resolved.bin, command)));
        } else {
            let job_index = jobs.len();
            jobs.push(Job {
                argv: command.clone(),
                cwd: args.cwd.clone(),
                env: env.clone(),
                timeout: Duration::from_secs(args.timeout),
            });
            plan.push(Plan::Pending {
                spec,
                bin: resolved.bin,
                command,
                job_index,
            });
        }
    }

    let captures = if jobs.is_empty() {
        Vec::new()
    } else {
        let max_parallel = args.max_parallel.unwrap_or(jobs.len());
        runner::run_jobs(&jobs, max_parallel)
    };

    let results: Vec<RunResult> = plan
        .into_iter()
        .map(|entry| match entry {
            Plan::Ready(result) => result,
            Plan::Pending {
                spec,
                bin,
                command,
                job_index,
            } => executed_result(spec, bin, command, &captures[job_index]),
        })
        .collect();

    let exit = exit_code(&results, args.require_available);

    let report = RunReport {
        schema_version: SCHEMA_VERSION,
        oneharness_version: env!("CARGO_PKG_VERSION"),
        prompt,
        model: args.model.clone(),
        bypass_permissions: bypass,
        dry_run: args.print_command,
        results,
    };
    print_json(&report, args.compact)?;

    if exit != EXIT_OK {
        let failed = report
            .results
            .iter()
            .filter(|r| is_failure(r.status, r.available, args.require_available))
            .count();
        eprintln!(
            "oneharness: {failed}/{} harness run(s) did not succeed (see results[].status and results[].error)",
            report.results.len()
        );
    }
    Ok(exit)
}

/// A planned harness: either fully resolved (skipped/planned) or awaiting a job.
enum Plan {
    Ready(RunResult),
    Pending {
        spec: &'static HarnessSpec,
        bin: String,
        command: Vec<String>,
        job_index: usize,
    },
}

fn planned_result(
    spec: &HarnessSpec,
    bin: &str,
    available: bool,
    command: Vec<String>,
) -> RunResult {
    RunResult {
        harness: spec.id.to_string(),
        bin: bin.to_string(),
        available,
        status: Status::Planned,
        exit_code: None,
        duration_ms: None,
        command,
        output_format: spec.output_format,
        text: None,
        text_source: None,
        stdout: String::new(),
        stderr: String::new(),
        error: None,
    }
}

fn skipped_result(spec: &HarnessSpec, bin: &str, command: Vec<String>) -> RunResult {
    RunResult {
        harness: spec.id.to_string(),
        bin: bin.to_string(),
        available: false,
        status: Status::Skipped,
        exit_code: None,
        duration_ms: None,
        command,
        output_format: spec.output_format,
        text: None,
        text_source: None,
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
    capture: &Capture,
) -> RunResult {
    let extracted = match capture.status {
        Status::Ok | Status::Nonzero => normalize::extract(&capture.stdout, spec.output_format),
        _ => None,
    };
    let (text, text_source) = match extracted {
        Some(e) => (Some(e.text), Some(e.source)),
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
        output_format: spec.output_format,
        text,
        text_source,
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
    use crate::domain::report::OutputFormat;

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
