//! Reading and writing harness config files for `oneharness sync`. This is an
//! I/O boundary: it touches the project's filesystem. The merge itself is pure
//! (`src/domain/sync.rs`); this layer only locates, reads, compares, and
//! (atomically) writes.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::domain::harness::{self, HarnessSpec, SyncSpec};
use crate::domain::sync as sync_domain;
use crate::domain::sync::deep_merge;
use crate::errors::OneharnessError;

/// What applying a fragment did (or, under `check`, would do) to one file.
///
/// Serialized as the report token itself, so the wire value and the variant a
/// consumer matches on cannot drift apart.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, schemars::JsonSchema, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum FileStatus {
    /// The file did not exist and was (or would be) created.
    Created,
    /// The file existed and its content changed (or would change).
    Updated,
    /// The file already contains everything the fragment asks for.
    Unchanged,
}

/// What one harness's permission/settings sync did (or would do).
///
/// [`FileStatus`] plus the one outcome a *file* never has: a harness with no
/// permission/settings fragment to apply at all. Keeping them one closed set —
/// rather than the report's earlier free string — is what makes an unreachable
/// status unconstructible and lets the contract publish the four values.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, schemars::JsonSchema, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum SyncStatus {
    /// The file did not exist and was (or would be) created.
    Created,
    /// The file existed and its content changed (or would change).
    Updated,
    /// The file already contains everything the fragment asks for.
    Unchanged,
    /// No permission/settings fragment applies to this harness, so there was no
    /// file to write. Hook files carry their own [`FileStatus`].
    Skipped,
}

impl From<FileStatus> for SyncStatus {
    fn from(status: FileStatus) -> Self {
        match status {
            FileStatus::Created => Self::Created,
            FileStatus::Updated => Self::Updated,
            FileStatus::Unchanged => Self::Unchanged,
        }
    }
}

/// Merge `fragment` into the harness's config file under `project_dir`.
///
/// The target is the registry's `file`, unless one of the higher-precedence
/// `alt_files` already exists — merging into the file the harness actually
/// reads, rather than creating a second, shadowed one. A file that exists but
/// cannot be parsed as JSON is a loud error and is left untouched: oneharness
/// only rewrites files it can round-trip (a JSONC file with comments would
/// lose them). Under `check`, nothing is written.
pub fn apply(
    project_dir: &Path,
    spec: &SyncSpec,
    fragment: &Value,
    check: bool,
) -> Result<(PathBuf, FileStatus), OneharnessError> {
    let target = spec
        .alt_files
        .iter()
        .map(|name| project_dir.join(name))
        .find(|path| path.is_file())
        .unwrap_or_else(|| project_dir.join(spec.file));

    let existing: Option<Value> = match std::fs::read_to_string(&target) {
        Ok(text) => Some(serde_json::from_str(&text).map_err(|e| {
            OneharnessError::HarnessConfigUnmergeable {
                path: target.display().to_string(),
                message: format!("not valid JSON ({e}); fix or remove it and re-run"),
            }
        })?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(source) => {
            return Err(OneharnessError::HarnessConfigRead {
                path: target.display().to_string(),
                source,
            })
        }
    };

    let (merged, status) = match &existing {
        Some(existing) => {
            let merged = deep_merge(existing, fragment);
            let status = if &merged == existing {
                FileStatus::Unchanged
            } else {
                FileStatus::Updated
            };
            (merged, status)
        }
        None => (fragment.clone(), FileStatus::Created),
    };

    if !check && status != FileStatus::Unchanged {
        write_atomically(&target, &merged)?;
    }
    Ok((target, status))
}

/// Pretty-print and write via a temp file + rename, so a crash mid-write can
/// never leave a harness with a truncated config file.
pub(crate) fn write_atomically(target: &Path, value: &Value) -> Result<(), OneharnessError> {
    let write_err = |source: std::io::Error| OneharnessError::HarnessConfigWrite {
        path: target.display().to_string(),
        source,
    };
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(write_err)?;
    }
    let mut text = serde_json::to_string_pretty(value)?;
    text.push('\n');
    let tmp = target.with_extension("oneharness.tmp");
    std::fs::write(&tmp, &text).map_err(write_err)?;
    std::fs::rename(&tmp, target).map_err(write_err)
}

/// What a [`sync`] call merges, as plain data.
///
/// Every field is one thing the CLI resolves from its own flags, so an embedder
/// states them rather than inheriting them from a process it does not own.
#[derive(Debug, Clone, Default)]
pub struct SyncRequest {
    /// The project directory whose harness config files are written; also where
    /// project-config discovery starts. `None` means the process's current
    /// directory, which is what the CLI uses.
    pub cwd: Option<PathBuf>,
    /// Harness id(s) to sync. Empty falls back to the configured `harnesses`,
    /// and then to every harness that has something to sync.
    pub harness: Vec<String>,
    /// Report what would change and write nothing — the `--check` flag.
    pub check: bool,
    /// Install hooks into the user-global location instead of the project.
    pub global: bool,
    /// Load configuration from exactly this file, skipping discovery.
    pub config: Option<PathBuf>,
    /// Ignore every configuration file.
    pub no_config: bool,
}

/// The `oneharness sync` output contract.
#[derive(Debug, Clone, schemars::JsonSchema, serde::Serialize)]
pub struct SyncReport {
    pub schema_version: &'static str,
    /// The oneharness config files the synced settings came from.
    pub config_files: Vec<String>,
    /// True under `--check`: statuses describe what *would* happen.
    pub check: bool,
    pub results: Vec<SyncResult>,
}

impl SyncReport {
    /// Whether this report describes a config file that differs from the
    /// policy — created, updated, or a hook that is not already installed.
    ///
    /// This answers only "did anything differ", not "should a caller fail".
    /// Under `--check` nothing was written, so a difference is still pending
    /// and [`SyncReport::check`] is what turns it into the CLI's non-zero
    /// exit; after a real sync the same difference has just been written, so
    /// the caller is looking at what *changed*, not at work left to do.
    /// Folding `check` in here would make a write-mode report claim nothing
    /// changed when it had rewritten every file, which is the opposite of what
    /// a library consumer inspecting the report is asking.
    #[must_use]
    pub fn changes(&self) -> bool {
        self.results.iter().any(|result| {
            matches!(result.status, SyncStatus::Created | SyncStatus::Updated)
                || result
                    .hooks
                    .iter()
                    .any(|hook| hook.status != FileStatus::Unchanged)
        })
    }
}

/// What one harness's sync did (or would do).
#[derive(Debug, Clone, schemars::JsonSchema, serde::Serialize)]
pub struct SyncResult {
    pub harness: &'static str,
    /// The permission/settings config file written (or that would be written);
    /// `null` when nothing of that kind is configured for this harness.
    pub file: Option<String>,
    // NO doc comment, and a `//` rather than a `///` one for the same reason: a
    // `$ref` with a sibling `description` is merged inline by
    // json-schema-to-typescript instead of resolving to the named type, so the
    // generated SDK would lose `SyncStatus` as an exported name and `zod.ts`
    // would fail to import it. `SyncStatus`'s own definition carries the
    // description, including what `skipped` means here.
    pub status: SyncStatus,
    /// Normalized `[[hooks]]` files installed into this harness (a Goose hook
    /// writes two). Empty when no `[[hooks]]` entry targets it.
    pub hooks: Vec<HookFileResult>,
    /// Top-level settings that have no mapping for this harness (e.g. a
    /// top-level `allowed_tools` while the harness has no allow-list concept)
    /// — visible here and warned on stderr, never silently dropped.
    pub unmapped: Vec<&'static str>,
}

/// One installed `[[hooks]]` file.
#[derive(Debug, Clone, schemars::JsonSchema, serde::Serialize)]
pub struct HookFileResult {
    pub file: String,
    pub status: FileStatus,
}

/// Merge the unified policy settings into each selected harness's own config
/// file and return the report.
///
/// Warnings about settings with no mapping for a harness go to the host's
/// stderr, exactly as they do from the CLI, so an embedder inherits them rather
/// than losing them.
///
/// # Errors
///
/// Returns a usage error for an unknown harness id or variant, a configuration
/// that cannot be loaded or merged, two variants of one harness that disagree
/// on what to write, a permission fragment under `global` (which has no
/// user-global mapping), or a config file that cannot be written.
pub fn sync(request: &SyncRequest) -> Result<SyncReport, OneharnessError> {
    // Mirror `run`: the project being synced is the request's cwd (else the
    // current directory), and that is also where discovery starts.
    let project_dir = match &request.cwd {
        Some(dir) => dir.clone(),
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };
    let loaded =
        crate::io::config::load(request.config.as_deref(), request.no_config, &project_dir)?;
    let cfg = &loaded.config;
    let selected_ids = if request.harness.is_empty() {
        cfg.harnesses.clone().unwrap_or_default()
    } else {
        request.harness.clone()
    };
    let selected_ids = crate::domain::select::dedupe_exact_ids(&selected_ids);
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
        let spec = harness::by_id(base).ok_or_else(|| OneharnessError::UnknownHarness {
            id: base.to_string(),
            valid: harness::valid_ids(),
        })?;
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
    let specs: Vec<&'static HarnessSpec> = if selected_ids.is_empty() {
        harness::all().iter().collect()
    } else {
        crate::domain::select::select_specs(false, &selected_ids, &[])?
    };

    let mut results = Vec::with_capacity(specs.len());

    // Resolved once: the user-global base dirs a `global` hook install anchors
    // under. Unused (but harmless) for a project sync.
    let global_dirs = crate::io::hooks::GlobalDirs::from_env();

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
            None => (None, SyncStatus::Skipped),
            // A configured permission/settings fragment has no user-global
            // mapping, so refuse it loudly rather than silently writing only the
            // hooks and leaving the rules behind.
            Some(_) if request.global => {
                return Err(OneharnessError::GlobalSyncOnlyHooks { id: spec.id.into() })
            }
            Some(fragment) => {
                let sync_spec = spec.sync.as_ref().expect("fragment implies a sync target");
                let (path, status) = apply(&project_dir, sync_spec, &fragment, request.check)?;
                (Some(path.display().to_string()), status.into())
            }
        };

        // Normalized `[[hooks]]` install into this harness's native shape,
        // independent of the permission/settings fragment above.
        let mut hooks = Vec::new();
        for hook in cfg.hook_specs_for(spec.id) {
            let scope = if request.global {
                crate::io::hooks::Scope::Global(&global_dirs)
            } else {
                crate::io::hooks::Scope::Project(&project_dir)
            };
            for write in crate::io::hooks::install(scope, spec, &hook, request.check)? {
                hooks.push(HookFileResult {
                    file: write.path.display().to_string(),
                    status: write.status,
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

    Ok(SyncReport {
        schema_version: crate::domain::report::SCHEMA_VERSION,
        config_files: loaded.files,
        check: request.check,
        results,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::scratch::ScratchDir;
    use serde_json::json;

    fn temp_project(tag: &str) -> ScratchDir {
        ScratchDir::new(&format!("oneharness-sync-{tag}-{}", std::process::id()))
    }

    const SPEC: SyncSpec = SyncSpec {
        file: "sub/config.json",
        alt_files: &[".alt.json"],
        allow_path: None,
        deny_path: None,
        hooks_path: None,
        schema_seed: None,
    };

    #[test]
    fn creates_with_parent_dirs_then_reports_unchanged() {
        let dir = temp_project("create");
        let fragment = json!({"a": 1});
        let (path, status) = apply(&dir, &SPEC, &fragment, false).unwrap();
        assert_eq!(status, FileStatus::Created);
        assert_eq!(path, dir.join("sub/config.json"));
        let on_disk: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(on_disk, fragment);
        // Idempotent: a second apply changes nothing.
        let (_, status) = apply(&dir, &SPEC, &fragment, false).unwrap();
        assert_eq!(status, FileStatus::Unchanged);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn existing_alt_file_is_merged_into_instead() {
        let dir = temp_project("alt");
        std::fs::write(dir.join(".alt.json"), "{\"keep\": true}").unwrap();
        let (path, status) = apply(&dir, &SPEC, &json!({"a": 1}), false).unwrap();
        assert_eq!(status, FileStatus::Updated);
        assert_eq!(path, dir.join(".alt.json"));
        let on_disk: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(on_disk, json!({"keep": true, "a": 1}));
        assert!(!dir.join("sub/config.json").exists(), "no shadow file");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn check_mode_writes_nothing() {
        let dir = temp_project("check");
        let (_, status) = apply(&dir, &SPEC, &json!({"a": 1}), true).unwrap();
        assert_eq!(status, FileStatus::Created);
        assert!(!dir.join("sub/config.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unparseable_existing_file_is_a_loud_error_and_untouched() {
        let dir = temp_project("jsonc");
        let path = dir.join(".alt.json");
        std::fs::write(&path, "{ // a comment\n  \"a\": 1 }").unwrap();
        let err = apply(&dir, &SPEC, &json!({"b": 2}), false).unwrap_err();
        assert!(err.to_string().contains("not valid JSON"), "{err}");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{ // a comment\n  \"a\": 1 }",
            "file must be left untouched"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
