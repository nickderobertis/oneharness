//! Resolving and probing harness binaries. This is an I/O boundary: it reads the
//! environment, searches PATH, and may spawn `<bin> --version`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use wait_timeout::ChildExt;

use crate::domain::harness::HarnessSpec;
use crate::errors::OneharnessError;

/// Per-harness binary overrides, resolved from `--bin ID=PATH` then the
/// `ONEHARNESS_BIN_<ID>` environment variable, falling back to the spec default.
pub struct BinOverrides {
    map: HashMap<String, String>,
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
        Ok(Self { map })
    }

    /// The binary to invoke for `id`: explicit override, then env var, then default.
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
    let bin = overrides.binary_for(spec.id, spec.default_bin);
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
}
