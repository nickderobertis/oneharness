//! Reading and writing harness config files for `oneharness sync`. This is an
//! I/O boundary: it touches the project's filesystem. The merge itself is pure
//! (`src/domain/sync.rs`); this layer only locates, reads, compares, and
//! (atomically) writes.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::domain::harness::SyncSpec;
use crate::domain::sync::deep_merge;
use crate::errors::OneharnessError;

/// What applying a fragment did (or, under `check`, would do) to one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    /// The file did not exist and was (or would be) created.
    Created,
    /// The file existed and its content changed (or would change).
    Updated,
    /// The file already contains everything the fragment asks for.
    Unchanged,
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
fn write_atomically(target: &Path, value: &Value) -> Result<(), OneharnessError> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_project(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("oneharness-sync-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    const SPEC: SyncSpec = SyncSpec {
        file: "sub/config.json",
        alt_files: &[".alt.json"],
        allow_path: None,
        deny_path: None,
        hooks_path: None,
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
