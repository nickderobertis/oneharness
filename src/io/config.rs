//! Locating and reading config files. This is an I/O boundary: it reads the
//! environment, the platform config directory, and the filesystem. Parsing,
//! validation, and layering stay pure in `src/domain/config.rs`.

use std::path::{Path, PathBuf};

use crate::domain::config::{self, FileConfig};
use crate::errors::OneharnessError;

/// Project-level file names, checked in this order in each directory.
const PROJECT_FILE_NAMES: &[&str] = &["oneharness.toml", ".oneharness.toml"];

/// Set to `1`/`true` to ignore all config files (same as `--no-config`); lets
/// wrappers and hermetic test suites pin the binary's behavior regardless of
/// what is configured on the machine.
const NO_CONFIG_ENV: &str = "ONEHARNESS_NO_CONFIG";

/// Points the user-level config at an explicit file (which must then exist),
/// instead of the platform default `<config dir>/oneharness/config.toml`.
const USER_CONFIG_ENV: &str = "ONEHARNESS_CONFIG";

/// The fully layered configuration plus the files it actually came from.
#[derive(Debug, Default)]
pub struct LoadedConfig {
    /// User and project files merged (project wins per field).
    pub config: FileConfig,
    /// Paths loaded, in layering order (user first, project last). Surfaced in
    /// the run report so a consumer can see which files shaped a run.
    pub files: Vec<String>,
}

/// Load the effective config for an invocation: [`load_layers`], folded with
/// the domain's per-field merge.
pub fn load(
    explicit: Option<&Path>,
    no_config: bool,
    project_start: &Path,
) -> Result<LoadedConfig, OneharnessError> {
    let mut loaded = LoadedConfig::default();
    for (path, layer) in load_layers(explicit, no_config, project_start)? {
        loaded.config = config::merge(loaded.config, layer);
        loaded.files.push(path);
    }
    Ok(loaded)
}

/// Locate and parse the config layers for an invocation, in layering order
/// (user first, project last). `oneharness config` consumes the layers
/// directly to attribute each value to its file; `run`/`detect` use [`load`].
///
/// - `no_config` (or `ONEHARNESS_NO_CONFIG=1`) loads nothing.
/// - `explicit` (`--config <path>`) loads exactly that file — no discovery —
///   and a missing file is an error, since the user named it.
/// - Otherwise: the user-level file (`$ONEHARNESS_CONFIG`, else the platform
///   config dir) layered under the project-level file (`oneharness.toml` /
///   `.oneharness.toml`, walking up from `project_start`). A missing
///   discovered file is simply an absent layer, never an error.
pub fn load_layers(
    explicit: Option<&Path>,
    no_config: bool,
    project_start: &Path,
) -> Result<Vec<(String, FileConfig)>, OneharnessError> {
    if no_config || env_flag(NO_CONFIG_ENV) {
        return Ok(Vec::new());
    }

    if let Some(path) = explicit {
        let config = read_required(path)?;
        return Ok(vec![(path.display().to_string(), config)]);
    }

    let mut layers = Vec::new();
    if let Some(path) = user_config_path()? {
        if let Some(user) = read_optional(&path)? {
            layers.push((path.display().to_string(), user));
        }
    }
    if let Some(path) = find_project_file(project_start) {
        if let Some(project) = read_optional(&path)? {
            layers.push((path.display().to_string(), project));
        }
    }
    Ok(layers)
}

/// Truthy env flag: set and not `""`/`0`/`false`.
fn env_flag(key: &str) -> bool {
    match std::env::var(key) {
        Ok(v) => !matches!(v.as_str(), "" | "0" | "false"),
        Err(_) => false,
    }
}

/// The user-level config path: `$ONEHARNESS_CONFIG` if set (the file must
/// exist — an explicitly named config that is missing is a configuration
/// error, not a silent no-op), else the platform config directory.
fn user_config_path() -> Result<Option<PathBuf>, OneharnessError> {
    if let Ok(value) = std::env::var(USER_CONFIG_ENV) {
        if !value.is_empty() {
            let path = PathBuf::from(&value);
            if !path.is_file() {
                return Err(OneharnessError::ConfigInvalid {
                    path: value,
                    message: format!("{USER_CONFIG_ENV} points at a file that does not exist"),
                });
            }
            return Ok(Some(path));
        }
    }
    Ok(platform_config_dir().map(|d| d.join("oneharness").join("config.toml")))
}

/// The per-user configuration directory, resolved like `gh` and friends:
/// `%APPDATA%` on Windows; `$XDG_CONFIG_HOME` (else `~/.config`) everywhere
/// else, macOS included — a dotfile-style path suits a developer CLI better
/// than `Library/Application Support`.
fn platform_config_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        return std::env::var_os("APPDATA")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
    }
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(xdg));
    }
    std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .map(|home| PathBuf::from(home).join(".config"))
}

/// Walk up from `start` looking for a project config; the first (deepest)
/// match wins, so a nested project shadows its parent.
fn find_project_file(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(d) = dir {
        for name in PROJECT_FILE_NAMES {
            let candidate = d.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        dir = d.parent();
    }
    None
}

/// Read and parse a discovered file; `Ok(None)` when it does not exist.
fn read_optional(path: &Path) -> Result<Option<FileConfig>, OneharnessError> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse_at(path, &text).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(OneharnessError::ConfigRead {
            path: path.display().to_string(),
            source,
        }),
    }
}

/// Read and parse an explicitly named file; missing is an error.
fn read_required(path: &Path) -> Result<FileConfig, OneharnessError> {
    let text = std::fs::read_to_string(path).map_err(|source| OneharnessError::ConfigRead {
        path: path.display().to_string(),
        source,
    })?;
    parse_at(path, &text)
}

fn parse_at(path: &Path, text: &str) -> Result<FileConfig, OneharnessError> {
    config::parse(text).map_err(|message| OneharnessError::ConfigInvalid {
        path: path.display().to_string(),
        message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oneharness-cfg-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn project_file_is_found_walking_up() {
        let root = temp_dir("walk");
        let nested = root.join("a").join("b");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(root.join("oneharness.toml"), "model = \"outer\"").unwrap();
        let found = find_project_file(&nested).unwrap();
        assert_eq!(found, root.join("oneharness.toml"));

        // A deeper file shadows the outer one, and the dotted name is honored.
        std::fs::write(nested.join(".oneharness.toml"), "model = \"inner\"").unwrap();
        let found = find_project_file(&nested).unwrap();
        assert_eq!(found, nested.join(".oneharness.toml"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_discovered_file_is_an_absent_layer() {
        let dir = temp_dir("missing");
        assert!(read_optional(&dir.join("oneharness.toml"))
            .unwrap()
            .is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_file_carries_its_path_in_the_error() {
        let dir = temp_dir("invalid");
        let path = dir.join("oneharness.toml");
        std::fs::write(&path, "not = valid = toml").unwrap();
        let err = read_optional(&path).unwrap_err();
        assert!(err.to_string().contains("oneharness.toml"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
