//! `oneharness run` — the shell over [`oneharness_core::io::run`].
//!
//! The verb's orchestration lives in the engine, which **returns** a
//! [`RunReport`] and publishes streamed events to a caller-supplied sink. All
//! that is left here is the CLI's own three jobs: turn the clap arguments into a
//! [`RunRequest`], own stdout (the buffered report, or the NDJSON stream
//! protocol), and map the outcome to a process exit code.

use oneharness_core::domain::events::ActionEvent;
use oneharness_core::domain::mode::PermissionMode;
use oneharness_core::domain::report::{RunReport, RunStreamEnvelope};
use oneharness_core::errors::OneharnessError;
use oneharness_core::io::cancel::CancelToken;
use oneharness_core::io::run::{EventSink, Resume, RunControls, RunRequest, SinkStep};

use crate::cli::RunArgs;
use crate::commands::print_json;

/// Collapse a clap-exclusive `--x` / `--no-x` pair into the single override the
/// engine takes: `None` when neither was passed (the config layer still
/// applies), else the direction that was. The positive wins if a future clap
/// change ever let both through, matching the precedence the engine documents.
fn toggle(yes: bool, no: bool) -> Option<bool> {
    if yes {
        Some(true)
    } else if no {
        Some(false)
    } else {
        None
    }
}

pub fn run(args: &RunArgs) -> Result<i32, OneharnessError> {
    let request = RunRequest::from(args);
    let mut sink = StdoutEvents;
    let outcome = oneharness_core::io::run::run(
        &request,
        RunControls {
            events: Some(&mut sink),
            cancel: CancelToken::new(),
            // The CLI owns the host's signal disposition for a run: a harness is
            // its own process-group leader, so a SIGINT that simply killed
            // oneharness would leave one running (and billing). Cancelling
            // instead tears each tree down and still reports.
            signal_cancel: true,
            // The report names the shipped binary, not the engine crate behind
            // it — the two are versioned separately.
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
        },
    )?;

    if outcome.streamed {
        emit_stream_result(&outcome.report)?;
    } else {
        print_json(&outcome.report, args.compact)?;
    }
    if let Some(summary) = &outcome.failure_summary {
        eprintln!("{summary}");
    }
    Ok(outcome.exit_code)
}

/// The CLI's event sink: each normalized event as one NDJSON
/// `{"type":"event","event":{…}}` line on stdout, the streaming protocol a
/// consumer reads to short-circuit mid-turn.
///
/// A failed write is that consumer closing the stream, and answering
/// [`SinkStep::Stop`] is what turns it into the run's own teardown of the
/// harness — the documented short-circuit, not an error to report.
struct StdoutEvents;

impl EventSink for StdoutEvents {
    fn event(&mut self, _harness_id: &str, event: &ActionEvent) -> SinkStep {
        use std::io::Write;
        let envelope = RunStreamEnvelope::Event {
            event: event.clone(),
        };
        let mut out = std::io::stdout().lock();
        let written = serde_json::to_string(&envelope)
            .map_err(|_| ())
            .and_then(|line| writeln!(out, "{line}").map_err(|_| ()))
            .and_then(|()| out.flush().map_err(|_| ()));
        if written.is_err() {
            SinkStep::Stop
        } else {
            SinkStep::Continue
        }
    }
}

/// Write the terminal `{"type":"result","report":<RunReport>}` line that closes a
/// streaming run — the same envelope a non-streaming run emits, so a consumer
/// that ignored the incremental events still gets the full report. A broken pipe
/// (the consumer already short-circuited and left) is not an error.
fn emit_stream_result(report: &RunReport) -> Result<(), OneharnessError> {
    use std::io::Write;
    let line = serde_json::to_string(&RunStreamEnvelope::Result {
        report: report.clone(),
    })?;
    // A broken pipe (the consumer already short-circuited and left) is expected,
    // not an error; any other write failure on the terminal line is non-fatal.
    let _ = writeln!(std::io::stdout(), "{line}");
    Ok(())
}

impl From<&RunArgs> for RunRequest {
    /// Every `run` flag, verbatim — except `--compact`, which is about how this
    /// shell *prints* the report rather than how the engine produces it.
    fn from(args: &RunArgs) -> Self {
        Self {
            all: args.all,
            harness: args.harness.clone(),
            mock_harness: args.mock_harness.clone(),
            exclude: args.exclude.clone(),
            prompt: args.prompt.clone(),
            prompt_file: args.prompt_file.clone(),
            model: args.model.clone(),
            system: args.system.clone(),
            reasoning: args.reasoning.clone(),
            system_file: args.system_file.clone(),
            // Forking is a property of the resume clap already guarantees it
            // implies, so the two flags become one value.
            resume: args.resume.clone().map(|session| Resume {
                session,
                fork: args.fork,
            }),
            session: args.session.clone(),
            session_dir: args.session_dir.clone(),
            control: args.control,
            output_format: args.output_format,
            events: args.events,
            stream: toggle(args.stream, args.no_stream),
            mock_rules: args.mock_rules.clone(),
            spy_file: args.spy_file.clone(),
            schema: args.schema.clone(),
            schema_max_retries: args.schema_max_retries,
            output_dir: args.output_dir.clone(),
            timeout: args.timeout,
            cwd: args.cwd.clone(),
            env: args.env.clone(),
            // `--bypass` / `--no-bypass` are shorthands for a mode, and clap
            // makes `--mode` exclusive with both, so the three collapse into one
            // value in that same precedence order.
            mode: args
                .mode
                .or_else(|| toggle(args.bypass, args.no_bypass).map(PermissionMode::from_bypass)),
            permit_prompts: args.permit_prompts,
            config: args.config.clone(),
            no_config: args.no_config,
            max_parallel: args.max_parallel,
            batch_strategy: args.batch_strategy,
            run_mode: args.run_mode,
            print_command: args.print_command,
            bin: args.bin.clone(),
            require_available: args.require_available,
            history: toggle(args.history, args.no_history),
            history_dir: args.history_dir.clone(),
            history_name: args.history_name.clone(),
            history_label: args.history_label.clone(),
            passthrough: args.passthrough.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    /// Parse a `run` command line into its args, exactly as `main` does.
    fn args_of(argv: &[&str]) -> RunArgs {
        match crate::cli::Cli::parse_from(argv).command {
            crate::cli::Command::Run(args) => *args,
            _ => unreachable!("not a run command"),
        }
    }

    #[test]
    fn every_run_flag_reaches_the_engine_request() {
        // The conversion is the whole seam between the clap surface and the
        // engine: a flag that stops being copied here goes silently missing from
        // every run. Pin a value on each field that is not a plain default.
        let args = args_of(&[
            "oneharness",
            "run",
            "--harness",
            "claude-code",
            "--exclude",
            "codex",
            "--mock-harness",
            "claude-code",
            "--prompt",
            "first",
            "--prompt",
            "second",
            "--prompt-file",
            "p.txt",
            "--model",
            "a",
            "--model",
            "b",
            "--system",
            "sys",
            "--reasoning",
            "high",
            "--session",
            "chat",
            "--session-dir",
            "/tmp/sessions",
            "--control",
            "--output-format",
            "json",
            "--events",
            "--stream",
            "--schema",
            "s.json",
            "--schema-max-retries",
            "4",
            "--output-dir",
            "/tmp/out",
            "--timeout",
            "7",
            "--cwd",
            "/tmp/work",
            "--env",
            "K=V",
            "--mode",
            "plan",
            "--permit-prompts",
            "--config",
            "c.toml",
            "--max-parallel",
            "3",
            "--batch-strategy",
            "min-tokens",
            "--run-mode",
            "fallback",
            "--print-command",
            "--bin",
            "claude-code=/bin/true",
            "--require-available",
            "--history",
            "--history-dir",
            "/tmp/hist",
            "--history-name",
            "run-name",
            "--history-label",
            "k=v",
            "--compact",
            "--",
            "--extra",
        ]);
        let request = RunRequest::from(&args);

        assert!(!request.all);
        assert_eq!(request.harness, ["claude-code"]);
        assert_eq!(request.mock_harness, ["claude-code"]);
        assert_eq!(request.exclude, ["codex"]);
        assert_eq!(request.prompt, ["first", "second"]);
        assert_eq!(request.prompt_file, ["p.txt"]);
        assert_eq!(request.model, ["a", "b"]);
        assert_eq!(request.system.as_deref(), Some("sys"));
        assert_eq!(request.reasoning.as_deref(), Some("high"));
        assert!(request.system_file.is_none());
        assert!(request.resume.is_none());
        assert_eq!(request.session.as_deref(), Some("chat"));
        assert_eq!(
            request.session_dir.as_deref(),
            Some(std::path::Path::new("/tmp/sessions"))
        );
        assert!(request.control);
        assert_eq!(
            request.output_format,
            Some(oneharness_core::domain::report::OutputFormat::Json)
        );
        assert!(request.events);
        assert_eq!(request.stream, Some(true));
        assert!(request.mock_rules.is_none());
        assert!(request.spy_file.is_none());
        assert_eq!(
            request.schema.as_deref(),
            Some(std::path::Path::new("s.json"))
        );
        assert_eq!(request.schema_max_retries, Some(4));
        assert_eq!(
            request.output_dir.as_deref(),
            Some(std::path::Path::new("/tmp/out"))
        );
        assert_eq!(request.timeout, Some(7));
        assert_eq!(
            request.cwd.as_deref(),
            Some(std::path::Path::new("/tmp/work"))
        );
        assert_eq!(request.env, ["K=V"]);
        assert_eq!(request.mode, Some(PermissionMode::Plan));
        assert!(request.permit_prompts);
        assert_eq!(
            request.config.as_deref(),
            Some(std::path::Path::new("c.toml"))
        );
        assert!(!request.no_config);
        assert_eq!(request.max_parallel, Some(3));
        assert_eq!(
            request.batch_strategy,
            Some(oneharness_core::domain::batch::BatchStrategy::MinTokens)
        );
        assert_eq!(
            request.run_mode,
            Some(oneharness_core::domain::fallback::RunMode::Fallback)
        );
        assert!(request.print_command);
        assert_eq!(request.bin, ["claude-code=/bin/true"]);
        assert!(request.require_available);
        assert_eq!(request.history, Some(true));
        assert_eq!(
            request.history_dir.as_deref(),
            Some(std::path::Path::new("/tmp/hist"))
        );
        assert_eq!(request.history_name.as_deref(), Some("run-name"));
        assert_eq!(request.history_label, ["k=v"]);
        assert_eq!(request.passthrough, ["--extra"]);
    }

    #[test]
    fn the_negative_toggles_and_all_selection_carry_over() {
        // The flags the case above cannot set (each conflicts with one it uses).
        // Each `--no-x` must reach the engine as an explicit `Some(false)`, not
        // as the `None` that would silently let a config value stand.
        let args = args_of(&[
            "oneharness",
            "run",
            "--all",
            "--prompt",
            "hi",
            "--no-stream",
            "--no-history",
            "--no-bypass",
            "--no-config",
            "--system-file",
            "sys.txt",
        ]);
        let request = RunRequest::from(&args);
        assert!(request.all);
        assert_eq!(request.stream, Some(false));
        assert_eq!(request.history, Some(false));
        assert_eq!(request.mode, Some(PermissionMode::Default));
        assert!(request.no_config);
        assert_eq!(request.system_file.as_deref(), Some("sys.txt"));

        // Neither half of a toggle passed leaves the config layer in force.
        let plain = RunRequest::from(&args_of(&["oneharness", "run", "--prompt", "hi"]));
        assert_eq!(plain.stream, None);
        assert_eq!(plain.history, None);
        assert_eq!(plain.mode, None);

        let args = args_of(&["oneharness", "run", "--prompt", "hi", "--bypass"]);
        assert_eq!(
            RunRequest::from(&args).mode,
            Some(PermissionMode::Bypass),
            "--bypass is the `--mode bypass` shorthand"
        );

        let args = args_of(&[
            "oneharness",
            "run",
            "--prompt",
            "hi",
            "--resume",
            "sid",
            "--fork",
        ]);
        assert_eq!(
            RunRequest::from(&args).resume,
            Some(Resume {
                session: "sid".to_string(),
                fork: true,
            })
        );
        let args = args_of(&["oneharness", "run", "--prompt", "hi", "--resume", "sid"]);
        assert_eq!(
            RunRequest::from(&args).resume,
            Some(Resume {
                session: "sid".to_string(),
                fork: false,
            })
        );

        let args = args_of(&[
            "oneharness",
            "run",
            "--prompt",
            "hi",
            "--mock-rules",
            "r.json",
            "--spy-file",
            "spy.jsonl",
        ]);
        let request = RunRequest::from(&args);
        assert_eq!(
            request.mock_rules.as_deref(),
            Some(std::path::Path::new("r.json"))
        );
        assert_eq!(
            request.spy_file.as_deref(),
            Some(std::path::Path::new("spy.jsonl"))
        );
    }
}
