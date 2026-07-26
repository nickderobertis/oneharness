//! `oneharness sync` — merge the unified settings (permission rules, hooks,
//! raw settings tables) into each harness's own project config file, so the
//! policy also governs the tools when they're used directly, without
//! oneharness. Emits a JSON report of what happened per harness; `--check`
//! writes nothing and exits 1 if anything is out of sync.

use serde::Serialize;

use crate::cli::SyncArgs;
use crate::commands::{dedupe_exact_ids, print_json, select_specs};
use oneharness_core::domain::report::SCHEMA_VERSION;
use oneharness_core::domain::{harness, sync as sync_domain};
use oneharness_core::errors::OneharnessError;
use oneharness_core::io::config as config_io;
use oneharness_core::io::hooks as hooks_io;
use oneharness_core::io::sync::{self as sync_io, FileStatus};

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
    /// The permission/settings config file written (or that would be written);
    /// `null` when nothing of that kind is configured for this harness.
    file: Option<String>,
    /// `created` / `updated` / `unchanged` for `file`, or `skipped` when no
    /// permission/settings fragment applies. Hook files carry their own status.
    status: &'static str,
    /// Normalized `[[hooks]]` files installed into this harness (a Goose hook
    /// writes two). Empty when no `[[hooks]]` entry targets it.
    hooks: Vec<HookFileResult>,
    /// Top-level settings that have no mapping for this harness (e.g. a
    /// top-level `allowed_tools` while the harness has no allow-list concept)
    /// — visible here and warned on stderr, never silently dropped.
    unmapped: Vec<&'static str>,
}

#[derive(Serialize)]
struct HookFileResult {
    file: String,
    status: &'static str,
}

/// The report token for a file status.
fn status_str(status: FileStatus) -> &'static str {
    match status {
        FileStatus::Created => "created",
        FileStatus::Updated => "updated",
        FileStatus::Unchanged => "unchanged",
    }
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
    let selected_ids = if args.harness.is_empty() {
        cfg.harnesses.clone().unwrap_or_default()
    } else {
        args.harness.clone()
    };
    let selected_ids = dedupe_exact_ids(&selected_ids);
    for id in &selected_ids {
        if let Some((base, variant)) = id.split_once(':') {
            if cfg.variant_for(id).is_none() {
                return Err(OneharnessError::UnknownHarnessVariant {
                    id: id.clone(),
                    base: base.to_string(),
                    variant: variant.to_string(),
                });
            }
        }
    }
    for (index, first_id) in selected_ids.iter().enumerate() {
        let (base, _) = cfg.split_harness_id(first_id);
        let spec = harness::by_id(base).expect("selected harness id validated");
        let first = sync_domain::plan_for(cfg, spec, first_id).map_err(|message| {
            OneharnessError::HarnessConfigUnmergeable {
                path: format!("[harness.{base}]"),
                message,
            }
        })?;
        for second_id in selected_ids.iter().skip(index + 1) {
            let (second_base, _) = cfg.split_harness_id(second_id);
            if base != second_base {
                continue;
            }
            let second = sync_domain::plan_for(cfg, spec, second_id).map_err(|message| {
                OneharnessError::HarnessConfigUnmergeable {
                    path: format!("[harness.{base}]"),
                    message,
                }
            })?;
            if first.fragment != second.fragment || first.unmapped != second.unmapped {
                return Err(OneharnessError::VariantSyncConflict {
                    base: base.to_string(),
                    first: first_id.clone(),
                    second: second_id.clone(),
                });
            }
        }
    }

    // With no CLI/config selection, cover every harness; those with nothing to
    // sync report `skipped`. A configured selection stays explicit and ordered.
    let specs = if selected_ids.is_empty() {
        harness::all().iter().collect()
    } else {
        select_specs(false, &selected_ids, &[])?
    };

    let mut results = Vec::with_capacity(specs.len());
    let mut pending_changes = false;

    // Resolved once: the user-global base dirs a `--global` hook install anchors
    // under. Unused (but harmless) for a project sync.
    let global_dirs = hooks_io::GlobalDirs::from_env();

    for (index, spec) in specs.into_iter().enumerate() {
        let selected_id = selected_ids.get(index).map_or(spec.id, String::as_str);
        let plan = sync_domain::plan_for(cfg, spec, selected_id).map_err(|message| {
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
        let (file, status) = match plan.fragment {
            None => (None, "skipped"),
            // A configured permission/settings fragment has no user-global
            // mapping, so refuse it loudly rather than silently writing only the
            // hooks and leaving the rules behind.
            Some(_) if args.global => {
                return Err(OneharnessError::GlobalSyncOnlyHooks { id: spec.id.into() })
            }
            Some(fragment) => {
                let sync_spec = spec.sync.as_ref().expect("fragment implies a sync target");
                let (path, status) =
                    sync_io::apply(&project_dir, sync_spec, &fragment, args.check)?;
                let status = status_str(status);
                pending_changes |= status != "unchanged";
                (Some(path.display().to_string()), status)
            }
        };

        // Normalized `[[hooks]]` install into this harness's native shape,
        // independent of the permission/settings fragment above.
        let mut hooks = Vec::new();
        for hook in cfg.hook_specs_for(spec.id) {
            let scope = if args.global {
                hooks_io::Scope::Global(&global_dirs)
            } else {
                hooks_io::Scope::Project(&project_dir)
            };
            for write in hooks_io::install(scope, spec, &hook, args.check)? {
                let status = status_str(write.status);
                pending_changes |= status != "unchanged";
                hooks.push(HookFileResult {
                    file: write.path.display().to_string(),
                    status,
                });
            }
        }

        results.push(SyncResult {
            harness: spec.id,
            file,
            status,
            hooks,
            unmapped: plan.unmapped,
        });
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
