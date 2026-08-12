//! `oneharness sync` — merge the unified settings (permission rules, hooks,
//! raw settings tables) into each harness's own project config file, so the
//! policy also governs the tools when they're used directly, without
//! oneharness.
//!
//! The merge is a library call ([`oneharness_core::io::sync::sync`]), so a Rust
//! consumer gets the same [`SyncReport`] without spawning anything; this is the
//! shell that prints it and maps `--check` to an exit code.

use crate::cli::SyncArgs;
use crate::commands::print_json;
use oneharness_core::errors::OneharnessError;
use oneharness_core::io::sync::{self as sync_io, SyncRequest};

// Re-exported so a consumer keeps one import path for the CLI's output
// contract, wherever the type is defined.
pub use oneharness_core::io::sync::{HookFileResult, SyncReport, SyncResult};

pub fn run(args: &SyncArgs) -> Result<i32, OneharnessError> {
    let report = sync_io::sync(&SyncRequest {
        cwd: args.cwd.clone(),
        harness: args.harness.clone(),
        check: args.check,
        global: args.global,
        config: args.config.clone(),
        no_config: args.no_config,
    })?;
    // `--check` is the CI mode: it writes nothing, so a difference it found is
    // still pending and exits 1, like a formatter's check mode. A real sync
    // has already written that same difference, so it exits 0 — which is why
    // the mode is tested here, at the exit mapping, rather than folded into
    // `changes()` where a write-mode report would have to deny its own writes.
    let pending_changes = report.check && report.changes();
    print_json(&report, args.compact)?;

    if pending_changes {
        eprintln!("oneharness: harness configs are out of sync (run `oneharness sync`)");
        return Ok(1);
    }
    Ok(0)
}
