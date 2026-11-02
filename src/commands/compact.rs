//! `oneharness compact --session <NAME>`: compact a named session now.
//!
//! The work is a library call ([`oneharness_core::io::compact::compact_session`]);
//! this shell turns the flags into its request, lets SIGINT/SIGTERM cancel it,
//! and prints the outcome.

use oneharness_core::domain::compact::{CompactOutcome, CompactStatus};
use oneharness_core::domain::harness;
use oneharness_core::errors::OneharnessError;
use oneharness_core::io::compact::{compact_session, CompactRequest};
use oneharness_core::io::run::RunControls;

use crate::cli::{CompactArgs, StdoutFormat};
use crate::commands::{print_report, printable};

impl TryFrom<&CompactArgs> for CompactRequest {
    type Error = OneharnessError;

    /// The flags, with `--harness` checked as a harness identity here, at the
    /// boundary, rather than carried as text.
    fn try_from(args: &CompactArgs) -> Result<Self, OneharnessError> {
        let harness = match &args.harness {
            Some(id) => Some(id.parse().map_err(|_| OneharnessError::UnknownHarness {
                id: id.clone(),
                valid: harness::valid_ids(),
            })?),
            None => None,
        };
        Ok(Self {
            harness,
            session: args.session.clone(),
            session_dir: args.session_dir.clone(),
            cwd: args.cwd.clone(),
            env: args.env.clone(),
            model: args.model.clone(),
            bin: args.bin.clone(),
            config: args.config.clone(),
            no_config: args.no_config,
            timeout: args.timeout,
        })
    }
}

pub fn run(args: &CompactArgs) -> Result<i32, OneharnessError> {
    let controls = RunControls {
        signal_cancel: true,
        ..RunControls::default()
    };
    let outcome = compact_session(&CompactRequest::try_from(args)?, controls, None)?;
    print_report(&outcome, args.stdout, render_text)?;
    // The text view is the report; why a compaction did not complete is a
    // diagnostic, so it goes to stderr (the JSON document carries it as
    // `error`).
    if !matches!(args.stdout, StdoutFormat::Json { .. }) {
        if let Some(error) = &outcome.error {
            eprintln!(
                "oneharness: compact {}: {}",
                outcome.status.as_str(),
                printable(error)
            );
        }
    }
    Ok(if outcome.status == CompactStatus::Completed {
        0
    } else {
        1
    })
}

fn render_text(outcome: &CompactOutcome) -> String {
    let tokens = match (outcome.context_tokens_before, outcome.context_tokens_after) {
        (Some(before), Some(after)) => format!(" · context {before} → {after} tokens"),
        _ => String::new(),
    };
    format!(
        "{status} · session {session} on {harness}{tokens}\n",
        status = outcome.status.as_str(),
        session = printable(&outcome.session),
        harness = outcome.harness,
    )
}
