//! `oneharness sync` — merge the unified settings (permission rules, hooks,
//! raw settings tables) into each harness's own project config file, so the
//! policy also governs the tools when they're used directly, without
//! oneharness. Emits a JSON report of what happened per harness; `--check`
//! writes nothing and exits 1 if anything is out of sync.

use serde::Serialize;

use crate::cli::SyncArgs;
use crate::commands::{print_json, select_specs};
use crate::domain::report::SCHEMA_VERSION;
use crate::domain::{harness, sync as sync_domain};
use crate::errors::OneharnessError;
use crate::io::config as config_io;
use crate::io::sync::{self as sync_io, FileStatus};

#[derive(Serialize)]
struct SyncReport {
    schema_version: &'static str,
    /// The oneharness config files the synced settings came from.
    config_files: Vec<String>,
    /// True under `--check`: statuses describe what *would* happen.
    check: bool,
    results: Vec<SyncResult>,
}

#[derive(Serialize)]
struct SyncResult {
    harness: &'static str,
    /// The harness config file written (or that would be written); `null`
    /// when there was nothing to sync for this harness.
    file: Option<String>,
    /// `created` / `updated` / `unchanged`, or `skipped` when nothing is
    /// configured for this harness.
    status: &'static str,
    /// Top-level settings that have no mapping for this harness (e.g. a
    /// top-level `allowed_tools` while the harness has no allow-list concept)
    /// — visible here and warned on stderr, never silently dropped.
    unmapped: Vec<&'static str>,
}

pub fn run(args: &SyncArgs) -> Result<i32, OneharnessError> {
    // Mirror `run`: the project being synced is --cwd (else the current
    // directory), and that's also where project config discovery starts.
    let project_dir = match &args.cwd {
        Some(dir) => dir.clone(),
        None => std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
    };
    let loaded = config_io::load(args.config.as_deref(), args.no_config, &project_dir)?;
    let cfg = &loaded.config;

    // Default selection: every harness (those with nothing to sync report
    // `skipped`, so the report always covers the full registry).
    let specs = if args.harness.is_empty() {
        harness::all().iter().collect()
    } else {
        select_specs(false, &args.harness, &[])?
    };

    let mut results = Vec::with_capacity(specs.len());
    let mut pending_changes = false;

    for spec in specs {
        let plan = sync_domain::plan(cfg, spec).map_err(|message| {
            OneharnessError::HarnessConfigUnmergeable {
                path: format!("[harness.{}]", spec.id),
                message,
            }
        })?;
        for setting in &plan.unmapped {
            eprintln!(
                "oneharness: warning: `{setting}` has no mapping for harness `{}` and was NOT applied to it",
                spec.id
            );
        }
        let result = match plan.fragment {
            None => SyncResult {
                harness: spec.id,
                file: None,
                status: "skipped",
                unmapped: plan.unmapped,
            },
            Some(fragment) => {
                let sync_spec = spec.sync.as_ref().expect("fragment implies a sync target");
                let (path, status) =
                    sync_io::apply(&project_dir, sync_spec, &fragment, args.check)?;
                let status = match status {
                    FileStatus::Created => "created",
                    FileStatus::Updated => "updated",
                    FileStatus::Unchanged => "unchanged",
                };
                pending_changes |= status != "unchanged";
                SyncResult {
                    harness: spec.id,
                    file: Some(path.display().to_string()),
                    status,
                    unmapped: plan.unmapped,
                }
            }
        };
        results.push(result);
    }

    let report = SyncReport {
        schema_version: SCHEMA_VERSION,
        config_files: loaded.files,
        check: args.check,
        results,
    };
    print_json(&report, args.compact)?;

    // `--check` is the CI mode: exit 1 when a sync is pending, like a
    // formatter's check mode. A real sync that wrote files exits 0.
    if args.check && pending_changes {
        eprintln!("oneharness: harness configs are out of sync (run `oneharness sync`)");
        return Ok(1);
    }
    Ok(0)
}
