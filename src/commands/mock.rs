//! `oneharness mock <id>` — the runtime mock/spy responder for a harness.
//!
//! An installed `[[hooks]]` hook (see `oneharness sync`) invokes this exactly
//! like `oneharness gate <id>`: the harness pipes its pre-tool event JSON to
//! stdin. This appends the event to the spy log (when one is configured),
//! matches it against the `--rules` ruleset, and — on a match — writes the
//! harness's native verdict to stdout: a deny, or an input rewrite that
//! substitutes the tool's arguments (the mock). A non-match writes nothing
//! (the universal fall-through), and the process exits 0 on any post-startup
//! fault so a mock never blocks a call on its own error. Decision and verdict
//! shapes are pure in `oneharness_core::domain::mock`; this is the thin
//! stdin/stdout wrapper plus the one I/O (the spy-log append).
//!
//! Startup faults are different: an unknown harness, an unreadable or invalid
//! ruleset, or a rule whose action the harness cannot express (no rewrite
//! shape, no deny shape) are loud usage errors (exit 2) — a mock suite whose
//! rules silently degraded to allow-everything would pass vacuously.

use std::io::Read;
use std::path::PathBuf;

use crate::cli::MockArgs;
use oneharness_core::domain::gate;
use oneharness_core::domain::harness;
use oneharness_core::domain::mock::{self, Action, MockRules};
use oneharness_core::errors::OneharnessError;

/// Reason used when a rewrite rule carries no message of its own.
const DEFAULT_REWRITE_REASON: &str = "input rewritten by oneharness mock";

pub fn run(args: &MockArgs) -> Result<i32, OneharnessError> {
    let spec = harness::by_id(&args.harness).ok_or_else(|| OneharnessError::UnknownHarness {
        id: args.harness.clone(),
        valid: harness::valid_ids(),
    })?;
    let rules = args.rules.as_deref().map(load_rules).transpose()?;
    if let Some(rules) = &rules {
        if let Some(action) = mock::unsupported_action(rules, spec.gate_deny, spec.mock_rewrite) {
            return Err(OneharnessError::MockActionUnsupported {
                id: spec.id.to_string(),
                action,
            });
        }
    }

    // Read the whole event. A read failure is a fail-open no-op (exit 0, no
    // output) — never block the call on our own I/O error.
    let mut event = String::new();
    if std::io::stdin().read_to_string(&mut event).is_err() {
        return Ok(0);
    }

    let decision = rules.as_ref().and_then(|r| mock::decide(&event, r));

    // Spy first, verdict second: the observation is recorded even for a call
    // that is about to be denied or rewritten. Best-effort — a spy-write
    // failure is warned about on stderr and never blocks the call.
    if let Some(path) = spy_path(args) {
        let line = mock::spy_line(spec.id, &event, decision);
        if let Err(err) = append_line(&path, &line) {
            eprintln!(
                "oneharness: warning: could not write spy log `{}`: {err}",
                path.display()
            );
        }
    }

    match decision {
        Some((_, Action::Deny { message })) => {
            // Renderability was verified up front; guard defensively anyway.
            if let Some(shape) = spec.gate_deny {
                println!("{}", gate::render_deny(shape, message));
            }
        }
        Some((_, Action::Rewrite { input, message })) => {
            if let Some(shape) = spec.mock_rewrite {
                let reason = message.as_deref().unwrap_or(DEFAULT_REWRITE_REASON);
                println!("{}", mock::render_rewrite(shape, input, reason));
            }
        }
        Some((_, Action::Stub { output, exit_code })) => {
            // A stub is a rewrite whose substituted command oneharness
            // generated itself: a safely-quoted printf of the declared output.
            if let Some(shape) = spec.mock_rewrite {
                let input = mock::stub_input(output, *exit_code);
                println!(
                    "{}",
                    mock::render_rewrite(shape, &input, "stubbed by oneharness mock")
                );
            }
        }
        None => {}
    }
    Ok(0)
}

/// Load and validate the ruleset — loud on any fault, before stdin is read.
fn load_rules(path: &std::path::Path) -> Result<MockRules, OneharnessError> {
    let text = std::fs::read_to_string(path).map_err(|source| OneharnessError::MockRulesFile {
        path: path.display().to_string(),
        source,
    })?;
    mock::parse_rules(&text).map_err(|message| OneharnessError::MockRulesInvalid {
        path: path.display().to_string(),
        message,
    })
}

/// The spy-log path: the `--spy-file` flag, else `ONEHARNESS_SPY_FILE` from
/// the environment (which the harness's own process env reaches, so a
/// `run --env` setting flows to every hook invocation), else none.
fn spy_path(args: &MockArgs) -> Option<PathBuf> {
    args.spy_file
        .clone()
        .or_else(|| std::env::var_os("ONEHARNESS_SPY_FILE").map(PathBuf::from))
}

/// Append one JSONL line, creating the file if needed. Concurrent hook
/// invocations share the file via the OS's append semantics (one small
/// single-line write each, the same property the e2e mock-harness log relies
/// on).
fn append_line(path: &std::path::Path, line: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{line}")
}
