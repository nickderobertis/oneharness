//! `oneharness compact --session <NAME>`: compact a named session now.
//!
//! The work is a library call ([`oneharness_core::io::compact::compact_session`]);
//! this shell turns the flags into its request, lets SIGINT/SIGTERM cancel it,
//! and prints the outcome.

use oneharness_core::domain::compact::{CompactOutcome, CompactStatus};
use oneharness_core::errors::OneharnessError;
use oneharness_core::io::compact::{compact_session, CompactRequest};
use oneharness_core::io::run::RunControls;

use crate::cli::CompactArgs;
use crate::commands::{print_report, printable};

impl From<&CompactArgs> for CompactRequest {
    fn from(args: &CompactArgs) -> Self {
        Self {
            harness: args.harness.clone(),
            session: args.session.clone(),
            session_dir: args.session_dir.clone(),
            cwd: args.cwd.clone(),
            env: args.env.clone(),
            model: args.model.clone(),
            bin: args.bin.clone(),
            config: args.config.clone(),
            no_config: args.no_config,
            timeout: args.timeout,
        }
    }
}

pub fn run(args: &CompactArgs) -> Result<i32, OneharnessError> {
    let controls = RunControls {
        signal_cancel: true,
        ..RunControls::default()
    };
    let outcome = compact_session(&CompactRequest::from(args), controls, None)?;
    print_report(&outcome, args.stdout, render_text)?;
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
    let error = outcome
        .error
        .as_deref()
        .map_or_else(String::new, |e| format!("\n  {}", printable(e)));
    format!(
        "{status} · session {session} on {harness}{tokens}{error}\n",
        status = outcome.status.as_str(),
        session = printable(&outcome.session),
        harness = outcome.harness,
    )
}
