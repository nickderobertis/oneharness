//! `oneharness sync` — merge the unified settings (permission rules, hooks,
//! raw settings tables) into each harness's own project config file, so the
//! policy also governs the tools when they're used directly, without
//! oneharness.
//!
//! The merge is a library call ([`oneharness_core::io::sync::sync_with`]), so a Rust
//! consumer gets the same [`SyncReport`] without spawning anything; this is the
//! shell that prints it (JSON, or a per-harness summary under `--format text`)
//! and maps `--check` to an exit code.

use crate::cli::SyncArgs;
use crate::commands::{print_report, printable};
use oneharness_core::domain::sync::RuleChange;
use oneharness_core::errors::OneharnessError;
use oneharness_core::io::sync::{self as sync_io, FileStatus, SyncMode, SyncRequest, SyncStatus};

// Re-exported so a consumer keeps one import path for the CLI's output
// contract, wherever the type is defined.
pub use oneharness_core::io::sync::{HookFileResult, SyncReport, SyncResult};

pub fn run(args: &SyncArgs) -> Result<i32, OneharnessError> {
    let mode = if args.exact {
        SyncMode::Exact
    } else {
        SyncMode::AddOnly
    };
    let report = sync_io::sync_with(
        &SyncRequest {
            cwd: args.cwd.clone(),
            harness: args.harness.clone(),
            check: args.check,
            global: args.global,
            config: args.config.clone(),
            no_config: args.no_config,
        },
        mode,
    )?;
    // `--check` is the CI mode: it writes nothing, so a difference it found is
    // still pending and exits 1, like a formatter's check mode. A real sync
    // has already written that same difference, so it exits 0 — which is why
    // the mode is tested here, at the exit mapping, rather than folded into
    // `changes()` where a write-mode report would have to deny its own writes.
    let pending_changes = report.check && report.changes();
    print_report(&report, args.stdout, render_text)?;

    if pending_changes {
        eprintln!("oneharness: harness configs are out of sync (run `oneharness sync`)");
        return Ok(1);
    }
    Ok(0)
}

/// Per harness: the settings file and what happened to it (or would, under
/// `--check`), each hook file likewise, every top-level setting and every
/// individual rule the harness has no mapping for, and under `--exact` each
/// entry added to or removed from its lists. Closes with the check-mode verdict, so a reader
/// of a `--check` run sees whether anything is pending without reading the
/// exit code.
fn render_text(report: &SyncReport) -> String {
    let mut out = String::new();
    let tense = if report.check { "would be " } else { "" };
    for r in &report.results {
        out.push_str(&format!("{}:\n", r.harness));
        match (&r.file, r.status) {
            (Some(file), status) => out.push_str(&format!(
                "  {}: {tense}{}\n",
                printable(file),
                sync_status(status)
            )),
            (None, SyncStatus::Skipped) => {
                out.push_str("  settings: nothing to sync for this harness\n");
            }
            (None, status) => {
                out.push_str(&format!("  settings: {tense}{}\n", sync_status(status)))
            }
        }
        for hook in &r.hooks {
            out.push_str(&format!(
                "  hook {}: {tense}{}\n",
                printable(&hook.file),
                file_status(hook.status)
            ));
        }
        if !r.unmapped.is_empty() {
            out.push_str(&format!(
                "  unmapped (no mapping for this harness): {}\n",
                r.unmapped.join(", ")
            ));
        }
        for rule in &r.unmapped_rules {
            out.push_str(&format!(
                "  unmapped rule ({}) {}: {}\n",
                rule.list.as_str(),
                printable(&rule.rule),
                rule.reason
            ));
        }
        for (verb, changes) in [("added", &r.added_rules), ("removed", &r.removed_rules)] {
            for change in changes {
                out.push_str(&format!("  {tense}{verb}{}\n", rule_change(change)));
            }
        }
    }
    if report.check {
        out.push_str(if report.changes() {
            "check: out of sync (run `oneharness sync`)\n"
        } else {
            "check: in sync\n"
        });
    }
    out
}

/// ` (<list>): <entry>`, or `: <entry>` for a statement no list renders.
fn rule_change(change: &RuleChange) -> String {
    match change.list {
        Some(list) => format!(" ({}): {}", list.as_str(), printable(&change.rule)),
        None => format!(": {}", printable(&change.rule)),
    }
}

fn sync_status(status: SyncStatus) -> &'static str {
    match status {
        SyncStatus::Created => "created",
        SyncStatus::Updated => "updated",
        SyncStatus::Unchanged => "unchanged",
        SyncStatus::Skipped => "skipped",
    }
}

fn file_status(status: FileStatus) -> &'static str {
    match status {
        FileStatus::Created => "created",
        FileStatus::Updated => "updated",
        FileStatus::Unchanged => "unchanged",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // `SyncResult`/`SyncReport` are `#[non_exhaustive]`, so a report is built
    // through their constructors and then filled in field by field.
    fn report(check: bool) -> SyncReport {
        let mut claude = SyncResult::new(
            "claude-code",
            Some(".claude/settings.json".to_string()),
            SyncStatus::Updated,
        );
        claude.hooks = vec![HookFileResult {
            file: ".claude/settings.json".to_string(),
            status: FileStatus::Unchanged,
        }];
        let mut goose = SyncResult::new("goose", None, SyncStatus::Skipped);
        goose.unmapped = vec!["allowed_tools", "denied_tools"];
        SyncReport::new(vec![], check, vec![claude, goose])
    }

    #[test]
    fn text_view_names_each_file_its_change_and_the_unmapped_settings() {
        let text = render_text(&report(false));
        assert_eq!(
            text,
            "claude-code:\n\
             \x20 .claude/settings.json: updated\n\
             \x20 hook .claude/settings.json: unchanged\n\
             goose:\n\
             \x20 settings: nothing to sync for this harness\n\
             \x20 unmapped (no mapping for this harness): allowed_tools, denied_tools\n"
        );
    }

    #[test]
    fn check_mode_speaks_in_the_conditional_and_closes_with_a_verdict() {
        let text = render_text(&report(true));
        assert!(
            text.contains(".claude/settings.json: would be updated\n"),
            "{text}"
        );
        assert!(
            text.ends_with("check: out of sync (run `oneharness sync`)\n"),
            "{text}"
        );

        let mut clean = report(true);
        clean.results[0].status = SyncStatus::Unchanged;
        assert!(render_text(&clean).ends_with("check: in sync\n"));
    }
}
