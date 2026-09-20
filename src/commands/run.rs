//! `oneharness run` — the shell over [`oneharness_core::io::run`].
//!
//! The verb's orchestration lives in the engine, which **returns** a
//! [`RunReport`] and publishes streamed events to a caller-supplied sink. All
//! that is left here is the CLI's own three jobs: turn the clap arguments into a
//! [`RunRequest`], own stdout (the buffered report as JSON or `--format text`,
//! or the NDJSON stream protocol), and map the outcome to a process exit code.

use oneharness_core::domain::config;
use oneharness_core::domain::events::ActionEvent;
use oneharness_core::domain::mode::PermissionMode;
use oneharness_core::domain::report::{RunReport, RunResult, RunStreamEnvelope};
use oneharness_core::errors::{JsonOnlySelection, OneharnessError, StreamOrigin};
use oneharness_core::io::cancel::CancelToken;
use oneharness_core::io::run::{EventSink, Resume, RunControls, RunRequest, SinkStep};

use crate::cli::{RunArgs, StdoutFormat};
use crate::commands::{indented, or_null, print_report, printable};

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
    // Refused before anything spawns: a run that ran and then exited 2 over
    // its flags would have billed a turn for nothing. A streaming run is the
    // one whose stdout `--format` cannot render — it is the NDJSON protocol
    // from the first event — so an explicit `text` beside it is the same kind
    // of contradiction `--compact` is (which clap refused while parsing
    // `StdoutFormat`), and refused the same way, whether the stream came from
    // the flag or from the `stream` config/ONEHARNESS_STREAM layer — and the
    // refusal says which.
    if args.stdout == StdoutFormat::Text {
        if let Some(origin) = stream_origin(args)? {
            return Err(OneharnessError::FormatConflict {
                selection: JsonOnlySelection::Stream(origin),
            });
        }
    }
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

    // A streaming run's stdout is the NDJSON protocol: its consumer has been
    // reading `event` lines all along, and the terminal `result` line is the
    // envelope that closes them (an explicit `--format text` was refused
    // above, so nothing is dropped here).
    if outcome.streamed {
        emit_stream_result(&outcome.report)?;
    } else {
        print_report(&outcome.report, args.stdout, render_text)?;
    }
    if let Some(summary) = &outcome.failure_summary {
        eprintln!("{summary}");
    }
    Ok(outcome.exit_code)
}

/// Where this run's streaming was selected, if it will stream — resolved
/// exactly as the engine resolves it: the `--stream`/`--no-stream` flag, else
/// the `stream` value of the config layers (files and `ONEHARNESS_STREAM`)
/// discovered from `--cwd`, attributed to its layer by the same
/// [`config::explain`] the `config` verb reports provenance with. Read here
/// only to refuse `--format text` before a turn is spent: the engine loads the
/// same layers again for the run, and a config it cannot load fails here with
/// the error it would have raised there. Skips the load when the flag settles
/// it, so an ordinary run reads its config once.
fn stream_origin(args: &RunArgs) -> Result<Option<StreamOrigin>, OneharnessError> {
    match toggle(args.stream, args.no_stream) {
        Some(true) => return Ok(Some(StreamOrigin::Flag)),
        Some(false) => return Ok(None),
        None => {}
    }
    let project_start = match &args.cwd {
        Some(dir) => dir.clone(),
        None => std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
    };
    let layers = oneharness_core::io::config::load_layers(
        args.config.as_deref(),
        args.no_config,
        &project_start,
    )?;
    let stream = config::explain(&layers).stream;
    Ok(match (stream.value, stream.source) {
        (Some(true), Some(source)) if source == config::ENV_SOURCE => {
            Some(StreamOrigin::Environment)
        }
        (Some(true), Some(path)) => Some(StreamOrigin::ConfigFile {
            path: std::path::PathBuf::from(path),
        }),
        _ => None,
    })
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

/// The report for a person at a terminal: the run's own settings first (what
/// was asked, the mode, the session handle, the batch/fallback blocks), then
/// one block per result. Every value is the JSON's own — a `null` there is said
/// to be null here, with the `text_source` that explains it — and every string
/// a harness wrote is flattened before it is drawn.
fn render_text(report: &RunReport) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "prompt: {}\n",
        printable(first_line(&report.prompt))
    ));
    out.push_str(&format!(
        "mode: {}{}\n",
        report.permission_mode.as_str(),
        if report.dry_run { " · dry run" } else { "" }
    ));
    if let Some(models) = &report.models {
        out.push_str(&format!("models: {}\n", printable(&models.join(", "))));
    } else if let Some(model) = &report.model {
        out.push_str(&format!("model: {}\n", printable(model)));
    }
    if let Some(resume) = &report.resume {
        out.push_str(&format!(
            "resume: {}{}\n",
            printable(resume),
            if report.fork { " (forked)" } else { "" }
        ));
    }
    if let Some(session) = &report.session {
        out.push_str(&format!(
            "session: {} ({}) · token {} · store {}\n",
            printable(&session.name),
            session.phase.as_str(),
            or_null(session.token.as_deref()),
            or_null(session.store_file.as_deref()),
        ));
    }
    if let Some(batch) = &report.batch {
        out.push_str(&format!(
            "batch: {} · {} prompts · forked {}\n",
            batch.strategy.as_str(),
            batch.prompt_count,
            if batch.forked { "yes" } else { "no" }
        ));
    }
    if report.schema.is_some() {
        out.push_str(&format!(
            "schema: applied · max retries {}\n",
            report
                .schema_max_retries
                .map_or_else(|| "null".to_string(), |n| n.to_string())
        ));
    }
    if let Some(fallback) = &report.fallback {
        out.push_str(&format!(
            "fallback: ran {}{}\n",
            or_null(fallback.ran.as_deref()),
            if fallback.stopped_without_work {
                " (stopped without work evidence)"
            } else {
                ""
            }
        ));
        for fell in &fallback.fell_through {
            out.push_str(&format!(
                "  fell through {}: {}{}\n",
                printable(&fell.harness),
                fell.reason.as_str(),
                fell.detail
                    .as_deref()
                    .map_or_else(String::new, |d| format!(" — {}", printable(d)))
            ));
        }
    }
    if let Some(control) = &report.control {
        out.push_str(&format!(
            "control: {} · socket {} · {} interrupt{}\n",
            control.mechanism.as_str(),
            printable(&control.socket.to_string()),
            control.interrupts.len(),
            if control.interrupts.len() == 1 {
                ""
            } else {
                "s"
            }
        ));
    }
    if let Some(file) = &report.history_file {
        out.push_str(&format!("history: {}\n", printable(file)));
    }
    if let Some(file) = &report.spy_file {
        out.push_str(&format!("spy log: {}\n", printable(file)));
    }
    for result in &report.results {
        out.push('\n');
        out.push_str(&render_result(result, report.batch.is_some()));
    }
    out
}

/// One result block: the candidate and its envelope on the first line, then
/// what it produced and, when it failed, why.
fn render_result(result: &RunResult, batch: bool) -> String {
    let mut out = format!(
        "{candidate}{model}: {status} · exit {exit} · {duration}\n",
        candidate = printable(&result.harness_id),
        model = result
            .model
            .as_deref()
            .map_or_else(String::new, |m| format!(" [model {}]", printable(m))),
        status = result.status.as_str(),
        exit = result
            .exit_code
            .map_or_else(|| "null".to_string(), |c| c.to_string()),
        duration = result
            .duration_ms
            .map_or_else(|| "not run".to_string(), |ms| format!("{ms} ms")),
    );
    if let Some(observed) = &result.observed_model {
        out.push_str(&format!("  observed model: {}\n", printable(observed)));
    }
    if batch {
        out.push_str(&format!(
            "  prompt: {}\n",
            or_null(result.prompt.as_deref().map(first_line))
        ));
    }
    if result.status == oneharness_core::domain::report::Status::Planned {
        out.push_str(&format!(
            "  command: {}\n",
            printable(&shell_words(&result.command))
        ));
    }
    match &result.text {
        Some(text) => {
            out.push_str(&format!(
                "  text ({}):\n",
                or_null(result.text_source.as_deref())
            ));
            out.push_str(&indented(text, "    "));
        }
        None => out.push_str(&format!(
            "  text: null ({})\n",
            result
                .text_source
                .as_deref()
                .map_or_else(|| text_absence(result), printable)
        )),
    }
    if result.schema_valid.is_some() || result.structured.is_some() {
        out.push_str(&format!(
            "  structured: {} · attempts {}\n",
            match result.schema_valid {
                Some(true) => "valid",
                Some(false) => "invalid",
                None => "not validated",
            },
            result
                .schema_attempts
                .map_or_else(|| "null".to_string(), |n| n.to_string())
        ));
        if let Some(value) = &result.structured {
            out.push_str(&indented(
                &serde_json::to_string_pretty(value).unwrap_or_default(),
                "    ",
            ));
        } else {
            out.push_str("    null (no JSON value could be extracted)\n");
        }
        if let Some(error) = &result.schema_error {
            out.push_str(&format!("  schema error: {}\n", printable(error)));
        }
    }
    if let Some(kind) = result.failure_kind {
        out.push_str(&format!(
            "  failure: {}{}\n",
            kind.as_str(),
            result
                .failure_kind_source
                .as_deref()
                .map_or_else(String::new, |s| format!(" (from {})", printable(s)))
        ));
    }
    if let Some(work) = result.work {
        out.push_str(&format!("  work evidence: {}\n", work.as_str()));
    }
    if let Some(error) = &result.error {
        out.push_str(&format!("  error: {}\n", printable(error)));
    }
    if let Some(id) = &result.session_id {
        out.push_str(&format!("  session id: {}\n", printable(id)));
    }
    if result.usage_source.is_some() {
        out.push_str(&format!(
            "  usage: in {} · out {} · cache read {} · cache write {} · cost {}\n",
            count(result.usage.input_tokens),
            count(result.usage.output_tokens),
            count(result.usage.cache_read_tokens),
            count(result.usage.cache_write_tokens),
            result
                .usage
                .cost_usd
                .map_or_else(|| "null".to_string(), |c| format!("${c}")),
        ));
    }
    if let Some(events) = &result.events {
        out.push_str(&format!(
            "  events: {} ({})\n",
            events.len(),
            or_null(result.events_source.as_deref())
        ));
    }
    out
}

/// Why a result carries no `text` when it also carries no `text_source`: the
/// envelope says whether the harness ran at all.
fn text_absence(result: &RunResult) -> String {
    use oneharness_core::domain::report::Status;
    match result.status {
        Status::Planned => "dry run, nothing executed".to_string(),
        Status::Skipped => "not run".to_string(),
        Status::SpawnError => "the harness could not be spawned".to_string(),
        _ => "no extraction was possible from the harness's output".to_string(),
    }
}

fn count(value: Option<u64>) -> String {
    value.map_or_else(|| "null".to_string(), |n| n.to_string())
}

/// The first line of a prompt, so a multi-line prompt stays one row.
fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("")
}

/// An argv as one line, each word single-quoted when it carries whitespace or
/// a quote — for reading, not for re-executing (the JSON has the exact argv).
fn shell_words(argv: &[String]) -> String {
    argv.iter()
        .map(|word| {
            if word.is_empty()
                || word
                    .chars()
                    .any(|c| c.is_whitespace() || c == '\'' || c == '"')
            {
                format!("'{}'", word.replace('\'', "'\\''"))
            } else {
                word.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

impl From<&RunArgs> for RunRequest {
    /// Every `run` flag, verbatim — except `--compact` and `--format`, which
    /// are about how this shell *prints* the report rather than how the engine
    /// produces it.
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
            server_overloaded_max_retries: args.server_overloaded_max_retries,
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
            history_pointer_file: args.history_pointer_file.clone(),
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
    use oneharness_core::domain::fallback::{FallThroughReason, RunWork};
    use oneharness_core::domain::report::{
        FallThrough, FallbackReport, OutputFormat, SessionReport, Status,
    };
    use oneharness_core::domain::session::SessionPhase;
    use oneharness_core::domain::signals::{FailureKind, Usage};

    fn result(id: &str, status: Status) -> RunResult {
        RunResult {
            harness: id.to_string(),
            variant: None,
            harness_id: id.to_string(),
            bin: id.to_string(),
            available: true,
            status,
            prompt: None,
            model: None,
            observed_model: None,
            exit_code: None,
            duration_ms: None,
            telemetry: None,
            command: vec![id.to_string(), "-p".to_string(), "say hi".to_string()],
            output_format: OutputFormat::Json,
            text: None,
            text_source: None,
            usage: Usage::default(),
            usage_source: None,
            session_id: None,
            events: None,
            events_source: None,
            structured: None,
            schema_valid: None,
            schema_attempts: None,
            schema_error: None,
            failure_kind: None,
            work: None,
            failure_kind_source: None,
            stdout: String::new(),
            stderr: String::new(),
            error: None,
        }
    }

    fn report(results: Vec<RunResult>) -> RunReport {
        RunReport {
            schema_version: "test".to_string(),
            oneharness_version: "test".to_string(),
            prompt: "say hi\nsecond line".to_string(),
            model: None,
            models: None,
            resume: None,
            fork: false,
            session: None,
            permission_mode: PermissionMode::Default,
            bypass_permissions: false,
            dry_run: false,
            schema: None,
            schema_max_retries: None,
            batch: None,
            fallback: None,
            mock_rules: None,
            spy_file: None,
            history_file: None,
            config_files: vec![],
            control: None,
            results,
        }
    }

    #[test]
    fn text_view_lays_out_a_completed_result_with_its_answer_and_signals() {
        let mut ok = result("claude-code", Status::Ok);
        ok.model = Some("opus".to_string());
        ok.exit_code = Some(0);
        ok.duration_ms = Some(1234);
        ok.text = Some("pong\u{1b}[31m\nline two".to_string());
        ok.text_source = Some("json:result".to_string());
        ok.session_id = Some("sess-1".to_string());
        ok.usage = Usage {
            input_tokens: Some(10),
            output_tokens: Some(2),
            cache_read_tokens: None,
            cache_write_tokens: None,
            cost_usd: Some(0.01),
        };
        ok.usage_source = Some("json".to_string());
        let text = render_text(&report(vec![ok]));

        assert!(
            text.starts_with("prompt: say hi\nmode: default\n"),
            "{text}"
        );
        assert!(
            text.contains("claude-code [model opus]: ok · exit 0 · 1234 ms\n"),
            "{text}"
        );
        assert!(
            text.contains("  text (json:result):\n    pong [31m\n    line two\n"),
            "the answer is indented under its label with the escape flattened:\n{text}"
        );
        assert!(text.contains("  session id: sess-1\n"), "{text}");
        assert!(
            text.contains(
                "  usage: in 10 · out 2 · cache read null · cache write null · cost $0.01\n"
            ),
            "{text}"
        );
        assert!(
            serde_json::from_str::<serde_json::Value>(&text).is_err(),
            "the text view is not a JSON document"
        );
    }

    #[test]
    fn text_view_says_why_text_is_null_and_names_a_failure() {
        let mut failed = result("codex", Status::Nonzero);
        failed.exit_code = Some(1);
        failed.duration_ms = Some(5);
        failed.failure_kind = Some(FailureKind::Auth);
        failed.failure_kind_source = Some("stderr".to_string());
        failed.error = Some("codex exited 1: 401\r".to_string());
        let mut skipped = result("goose", Status::Skipped);
        skipped.available = false;
        let mut unclassified = result("crush", Status::Timeout);
        unclassified.work = Some(RunWork::None);
        let text = render_text(&report(vec![failed, skipped, unclassified]));

        assert!(text.contains("codex: nonzero · exit 1 · 5 ms\n"), "{text}");
        assert!(
            text.contains("  text: null (no extraction was possible from the harness's output)\n"),
            "{text}"
        );
        assert!(text.contains("  failure: auth (from stderr)\n"), "{text}");
        assert!(text.contains("  error: codex exited 1: 401 \n"), "{text}");
        assert!(
            text.contains("goose: skipped · exit null · not run\n"),
            "{text}"
        );
        assert!(text.contains("  text: null (not run)\n"), "{text}");
        assert!(text.contains("  work evidence: none\n"), "{text}");
    }

    #[test]
    fn text_view_carries_the_session_fallback_batch_and_schema_blocks() {
        let mut rep = report(vec![]);
        rep.session = Some(SessionReport {
            name: "chat".to_string(),
            phase: SessionPhase::Continue,
            token: Some("tok".to_string()),
            store_file: None,
        });
        rep.fallback = Some(FallbackReport {
            ran: Some("codex".to_string()),
            fell_through: vec![FallThrough {
                harness: "claude-code".to_string(),
                reason: FallThroughReason::Auth,
                detail: Some("claude-code exited 1: 401".to_string()),
            }],
            stopped_without_work: false,
        });
        rep.batch = Some(oneharness_core::domain::report::BatchReport {
            strategy: oneharness_core::domain::batch::BatchStrategy::Speed,
            prompt_count: 2,
            forked: false,
        });
        rep.schema = Some(serde_json::json!({"type": "object"}));
        rep.schema_max_retries = Some(2);
        rep.models = Some(vec!["a".to_string(), "b".to_string()]);
        let mut structured = result("codex", Status::Ok);
        structured.prompt = Some("second prompt".to_string());
        structured.structured = Some(serde_json::json!({"name": "x"}));
        structured.schema_valid = Some(false);
        structured.schema_attempts = Some(3);
        structured.schema_error = Some("missing `age`".to_string());
        rep.results = vec![structured];
        let text = render_text(&rep);

        assert!(text.contains("models: a, b\n"), "{text}");
        assert!(
            text.contains("session: chat (continue) · token tok · store null\n"),
            "{text}"
        );
        assert!(
            text.contains("batch: speed · 2 prompts · forked no\n"),
            "{text}"
        );
        assert!(text.contains("schema: applied · max retries 2\n"), "{text}");
        assert!(text.contains("fallback: ran codex\n"), "{text}");
        assert!(
            text.contains("  fell through claude-code: auth — claude-code exited 1: 401\n"),
            "{text}"
        );
        assert!(text.contains("  prompt: second prompt\n"), "{text}");
        assert!(
            text.contains("  structured: invalid · attempts 3\n"),
            "{text}"
        );
        assert!(
            text.contains("    {\n      \"name\": \"x\"\n    }\n"),
            "{text}"
        );
        assert!(text.contains("  schema error: missing `age`\n"), "{text}");
    }

    #[test]
    fn text_view_shows_each_planned_command_under_a_dry_run() {
        let mut rep = report(vec![result("claude-code", Status::Planned)]);
        rep.dry_run = true;
        let text = render_text(&rep);
        assert!(text.contains("mode: default · dry run\n"), "{text}");
        assert!(
            text.contains("  command: claude-code -p 'say hi'\n"),
            "{text}"
        );
        assert!(
            text.contains("  text: null (dry run, nothing executed)\n"),
            "{text}"
        );
    }

    #[test]
    fn shell_words_quotes_only_what_needs_it() {
        let argv = ["a", "b c", "", "it's"].map(String::from);
        assert_eq!(shell_words(&argv), "a 'b c' '' 'it'\\''s'");
    }

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
            "--server-overloaded-max-retries",
            "5",
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
            "--history-pointer-file",
            "/tmp/run/pointers.jsonl",
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
        assert_eq!(request.server_overloaded_max_retries, Some(5));
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
        assert_eq!(
            request.history_pointer_file.as_deref(),
            Some(std::path::Path::new("/tmp/run/pointers.jsonl"))
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
