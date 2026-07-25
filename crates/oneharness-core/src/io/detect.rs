//! Resolving and probing harness binaries. This is an I/O boundary: it reads the
//! environment, searches PATH, and may spawn `<bin> --version`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use wait_timeout::ChildExt;

use crate::domain::harness::HarnessSpec;
use crate::errors::OneharnessError;

/// Per-harness binary overrides, resolved from `--bin ID=PATH`, then the
/// `ONEHARNESS_BIN_<ID>` environment variable, then a config-file
/// `[harness.<id>] bin`, falling back to the spec default.
pub struct BinOverrides {
    map: HashMap<String, String>,
    /// Config-file bins: the lowest-precedence override layer, since an
    /// explicit flag or env var is more deliberate than a persisted default.
    config: HashMap<String, String>,
}

impl BinOverrides {
    /// Parse `--bin ID=PATH` values. Errors on a value missing its `=`.
    pub fn parse(values: &[String]) -> Result<Self, OneharnessError> {
        let mut map = HashMap::new();
        for value in values {
            let (id, path) = value
                .split_once('=')
                .ok_or_else(|| OneharnessError::BadBinOverride(value.clone()))?;
            if id.is_empty() || path.is_empty() {
                return Err(OneharnessError::BadBinOverride(value.clone()));
            }
            map.insert(id.to_string(), path.to_string());
        }
        Ok(Self {
            map,
            config: HashMap::new(),
        })
    }

    /// Attach the config-file bins (`[harness.<id>] bin = "..."`) as fallbacks.
    pub fn with_config_bins(mut self, bins: HashMap<String, String>) -> Self {
        self.config = bins;
        self
    }

    /// The binary to invoke for `id`: explicit override, then env var, then the
    /// config-file bin, then default.
    fn binary_for(&self, id: &str, default_bin: &str) -> String {
        if let Some(path) = self.map.get(id) {
            return path.clone();
        }
        let env_key = format!("ONEHARNESS_BIN_{}", id.to_uppercase().replace('-', "_"));
        if let Ok(value) = std::env::var(&env_key) {
            if !value.is_empty() {
                return value;
            }
        }
        if let Some(path) = self.config.get(id) {
            return path.clone();
        }
        default_bin.to_string()
    }
}

/// The result of locating a harness binary on the current system.
pub struct Resolved {
    /// The binary string oneharness will invoke (name or path).
    pub bin: String,
    /// The absolute path it resolved to, if found.
    pub path: Option<PathBuf>,
    /// Whether the binary was found and is executable.
    pub available: bool,
}

/// Resolve the binary for a harness without running it.
pub fn resolve(spec: &HarnessSpec, overrides: &BinOverrides) -> Resolved {
    resolve_named(spec, spec.id, overrides)
}

/// Resolve a composed harness id, allowing a variant-specific config/CLI bin
/// while retaining the base adapter's default.
// llmlint: ignore[invalid_states_unrepresentable] Callers pass only config-validated selectors paired with their registry-resolved base spec; keeping the serialized id here lets BinOverrides preserve exact per-variant lookup, covered by detect integration tests for valid and unknown variants.
pub fn resolve_named(spec: &HarnessSpec, id: &str, overrides: &BinOverrides) -> Resolved {
    let bin = overrides.binary_for(id, spec.default_bin);
    match which::which(&bin) {
        Ok(path) => Resolved {
            bin,
            path: Some(path),
            available: true,
        },
        Err(_) => Resolved {
            bin,
            path: None,
            available: false,
        },
    }
}

/// Best-effort `<bin> --version`, returning the first non-empty output line.
/// Never fails loudly: a probe that errors or times out simply yields `None`.
pub fn probe_version(bin: &str) -> Option<String> {
    let mut child = Command::new(bin)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;

    let status = child.wait_timeout(Duration::from_secs(5)).ok()?;
    if status.is_none() {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }
    let output = child.wait_with_output().ok()?;
    first_line(&output.stdout).or_else(|| first_line(&output.stderr))
}

fn first_line(bytes: &[u8]) -> Option<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serializes the env-mutating tests: the process environment is global, so
    // concurrent set/remove from two tests in the same binary would race.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn override_takes_precedence_over_default() {
        let ov = BinOverrides::parse(&["claude-code=/opt/claude".to_string()]).unwrap();
        assert_eq!(ov.binary_for("claude-code", "claude"), "/opt/claude");
    }

    #[test]
    fn default_used_when_no_override() {
        let ov = BinOverrides::parse(&[]).unwrap();
        assert_eq!(ov.binary_for("codex", "codex"), "codex");
    }

    #[test]
    fn config_bin_is_used_but_loses_to_an_explicit_flag() {
        let bins = HashMap::from([("codex".to_string(), "/cfg/codex".to_string())]);
        let ov = BinOverrides::parse(&[]).unwrap().with_config_bins(bins);
        assert_eq!(ov.binary_for("codex", "codex"), "/cfg/codex");

        let bins = HashMap::from([("codex".to_string(), "/cfg/codex".to_string())]);
        let ov = BinOverrides::parse(&["codex=/flag/codex".to_string()])
            .unwrap()
            .with_config_bins(bins);
        assert_eq!(ov.binary_for("codex", "codex"), "/flag/codex");
    }

    #[test]
    fn malformed_override_is_rejected() {
        assert!(BinOverrides::parse(&["no-equals".to_string()]).is_err());
        assert!(BinOverrides::parse(&["=/path".to_string()]).is_err());
        assert!(BinOverrides::parse(&["id=".to_string()]).is_err());
    }

    #[test]
    fn first_line_skips_blanks() {
        assert_eq!(
            first_line(b"\n  \nv1.2.3\nextra"),
            Some("v1.2.3".to_string())
        );
        assert_eq!(first_line(b"   "), None);
    }

    #[test]
    fn env_var_override_is_used_above_config_and_default() {
        // With no `--bin` flag, the per-harness `ONEHARNESS_BIN_<ID>` env var
        // (id upper-cased, `-`→`_`) selects the binary, ahead of any config-file
        // bin and the spec default. A unique key keeps this independent of the
        // ambient environment.
        let _guard = ENV_LOCK.lock().unwrap();
        let key = "ONEHARNESS_BIN_CLAUDE_CODE";
        let prev = std::env::var(key).ok();

        std::env::set_var(key, "/env/claude");
        let bins = HashMap::from([("claude-code".to_string(), "/cfg/claude".to_string())]);
        let ov = BinOverrides::parse(&[]).unwrap().with_config_bins(bins);
        assert_eq!(ov.binary_for("claude-code", "claude"), "/env/claude");

        // An empty env value is ignored — the next layer (config, then default)
        // wins instead.
        std::env::set_var(key, "");
        assert_eq!(ov.binary_for("claude-code", "claude"), "/cfg/claude");

        match prev {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }

    #[test]
    fn explicit_flag_beats_env_var() {
        // A `--bin` flag takes precedence over the env var for the same id.
        let _guard = ENV_LOCK.lock().unwrap();
        let key = "ONEHARNESS_BIN_CODEX";
        let prev = std::env::var(key).ok();
        std::env::set_var(key, "/env/codex");

        let ov = BinOverrides::parse(&["codex=/flag/codex".to_string()]).unwrap();
        assert_eq!(ov.binary_for("codex", "codex"), "/flag/codex");

        match prev {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }
}
