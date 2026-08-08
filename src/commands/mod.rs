//! CLI verbs: each module orchestrates the domain + io layers for one command.
//! Shared helpers (harness selection, JSON output) live here.

pub mod config;
pub mod detect;
pub mod gate;
pub mod history;
pub mod init;
pub mod list;
pub mod mock;
pub mod run;
pub mod sync;
pub mod usage;

use std::io::Write;

use serde::Serialize;

use oneharness_core::domain::config::{names_absolute_path, valid_env_name};
use oneharness_core::domain::harness::{self, HarnessSpec};
use oneharness_core::errors::OneharnessError;

/// Preserve selector order while removing exactly repeated ids.
// llmlint: ignore[invalid_states_unrepresentable] This boundary intentionally normalizes raw CLI/config selector text before select_specs validates it; a validated selector type could not carry malformed input into the precise usage diagnostic.
pub fn dedupe_exact_ids(ids: &[String]) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    ids.iter()
        .filter(|id| seen.insert((*id).clone()))
        .cloned()
        .collect()
}

/// Resolve `all` in registry order or an explicit selection in caller order.
///
/// `all` selects every harness (minus `exclude`); otherwise `include` lists the
/// ids to run. An empty selection, or any unknown id, is a usage error.
// llmlint: ignore[invalid_states_unrepresentable] Raw external selector strings are the input contract here so unknown/malformed ids can be reported verbatim; every id is registry-validated before any HarnessSpec is returned, and callers retain the same ordered selectors only for variant resolution.
pub fn select_specs(
    all: bool,
    include: &[String],
    exclude: &[String],
) -> Result<Vec<&'static HarnessSpec>, OneharnessError> {
    // Validate every named id up front so a typo fails loudly, not silently.
    for id in include.iter().chain(exclude.iter()) {
        let base = id.split_once(':').map_or(id.as_str(), |(base, _)| base);
        if harness::by_id(base).is_none() {
            return Err(OneharnessError::UnknownHarness {
                id: id.clone(),
                valid: harness::valid_ids(),
            });
        }
    }

    if all {
        let excluded: Vec<&str> = exclude.iter().map(String::as_str).collect();
        let specs: Vec<_> = harness::all()
            .iter()
            .filter(|s| !excluded.contains(&s.id))
            .collect();
        if specs.is_empty() {
            return Err(OneharnessError::NoSelection);
        }
        return Ok(specs);
    }

    if include.is_empty() {
        return Err(OneharnessError::NoSelection);
    }

    // Preserve caller order: variants of one base harness are distinct
    // candidates, while an exactly repeated composed id is de-duplicated.
    Ok(dedupe_exact_ids(include)
        .iter()
        .map(|id| {
            let base = id.split_once(':').map_or(id.as_str(), |(base, _)| base);
            harness::by_id(base).expect("validated above")
        })
        .collect())
}

/// The environment a variant selects for one harness: `[env]`, then
/// `[harness.<id>.env]`, then the variant's `env_file`, `env`, and `env_from`,
/// last write winning — the layering the runner applies to a child.
///
/// Shared by `run` and `usage` on purpose: an identity is an environment, so a
/// usage probe must be pointed at a subscription by the same machinery that
/// points a run at one. A second selector here would be a second thing to keep
/// in step, and a usage report attributed to an identity `run` would not have
/// used is worse than no report.
// llmlint: ignore[invalid_states_unrepresentable] This spawn-boundary helper only receives selectors after config validation and select_specs resolution; introducing a second identity type here would duplicate VariantName while its real subprocess tests pin masking, sourcing, and isolation.
pub fn variant_environment(
    cfg: &oneharness_core::domain::config::FileConfig,
    composed: &str,
    project_start: &std::path::Path,
) -> Result<Vec<(String, String)>, OneharnessError> {
    let (base, _) = cfg.split_harness_id(composed);
    let mut env: Vec<(String, String)> = cfg
        .env
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    if let Some(harness) = cfg.harness.get(base) {
        env.extend(
            harness
                .env
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
    }
    let Some(variant) = cfg.variant_for(composed) else {
        return Ok(env);
    };
    if let Some(file) = &variant.env_file {
        let path = {
            let path = std::path::PathBuf::from(file);
            if path.is_absolute() {
                path
            } else {
                project_start.join(path)
            }
        };
        let metadata =
            std::fs::metadata(&path).map_err(|source| OneharnessError::VariantEnvFile {
                path: path.display().to_string(),
                source,
            })?;
        #[cfg(unix)]
        let private = {
            use std::os::unix::fs::PermissionsExt;
            metadata.is_file() && metadata.permissions().mode() & 0o077 == 0
        };
        #[cfg(not(unix))]
        let private = metadata.is_file();
        if !private {
            return Err(OneharnessError::VariantEnvFilePermissions {
                path: path.display().to_string(),
            });
        }
        let text =
            std::fs::read_to_string(&path).map_err(|source| OneharnessError::VariantEnvFile {
                path: path.display().to_string(),
                source,
            })?;
        for (index, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(OneharnessError::VariantEnvFileLine {
                    path: path.display().to_string(),
                    line: index + 1,
                });
            };
            if !valid_env_name(key) {
                return Err(OneharnessError::VariantEnvFileLine {
                    path: path.display().to_string(),
                    line: index + 1,
                });
            }
            if value.contains('\0') {
                return Err(OneharnessError::VariantEnvFileLine {
                    path: path.display().to_string(),
                    line: index + 1,
                });
            }
            env.push((key.to_string(), value.to_string()));
        }
    }
    env.extend(
        variant
            .env
            .iter()
            .map(|(key, value)| (key.clone(), value.clone())),
    );
    for (target, source) in &variant.env_from {
        let value =
            std::env::var(source).map_err(|_| OneharnessError::VariantEnvSourceMissing {
                name: source.clone(),
            })?;
        env.push((target.clone(), value));
    }
    Ok(env)
}

/// An identity a variant selects through `env_from` whose home directory is not
/// on disk — so nothing has been provisioned there yet.
pub struct UnprovisionedIdentity {
    /// The variable the child would have received (e.g. `CODEX_HOME`).
    // llmlint: ignore[invalid_states_unrepresentable] These are the configured names/paths as written, kept verbatim for a terminal diagnostic and never used for further I/O.
    pub target: String,
    /// The parent variable it was sourced from (e.g. `ORCHESTRATOR_CODEX_ALT_HOME`).
    pub source: String,
    /// The absolute path that does not exist.
    pub path: String,
}

/// The first `env_from` indirection of `composed`'s variant that names an
/// absolute path which is **not on disk**, if any.
///
/// A variant's `env_from` is the identity-selection seam: it points a child at
/// one account's home directory without the parent switching to it. A home
/// directory that does not exist holds no credentials, which is the same
/// "account not set up yet" state an *empty* one is in — and an empty one is
/// already an `auth` failure that a fallback chain routes around, because the
/// harness starts, gets rejected, and says so. An absent one is not: the CLI
/// refuses before it can report anything readable (Codex exits 2 with `CODEX_HOME
/// points to … but that path does not exist`), so the chain stops at a candidate
/// nobody has logged into. Reading it here makes the two states behave alike, and
/// makes an unauthenticated candidate free to leave in a committed chain without
/// pre-creating a directory for it.
///
/// Scoped to `env_from` on purpose. It is the only environment field whose value
/// comes from *outside* the config — a committed `env` path is the author's own
/// declaration, and treating its absence as a provisioning state would silently
/// route around a typo. Only [`names_absolute_path`] values are probed, so a
/// credential is never touched.
pub fn variant_unprovisioned_identity(
    cfg: &oneharness_core::domain::config::FileConfig,
    composed: &str,
) -> Option<UnprovisionedIdentity> {
    let variant = cfg.variant_for(composed)?;
    variant.env_from.iter().find_map(|(target, source)| {
        let value = std::env::var(source).ok()?;
        (names_absolute_path(&value) && !std::path::Path::new(&value).exists()).then(|| {
            UnprovisionedIdentity {
                target: target.clone(),
                source: source.clone(),
                path: value,
            }
        })
    })
}

/// Write `text` to stdout verbatim, reporting a write failure rather than
/// panicking on it.
///
/// `print!`/`println!` panic when stdout cannot be written — which a reader
/// closing the pipe (`oneharness usage | head -1`) makes an ordinary event, not
/// a bug. A command whose output *is* its deliverable should say that the
/// deliverable was truncated, through the same error channel as every other I/O
/// fault, instead of dying mid-sentence with a stack trace.
pub fn print_text(text: &str) -> Result<(), OneharnessError> {
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
        .map_err(OneharnessError::StdoutWrite)
}

/// Write a value as JSON to stdout (pretty unless `compact`).
pub fn print_json<T: Serialize>(value: &T, compact: bool) -> Result<(), OneharnessError> {
    let json = if compact {
        serde_json::to_string(value)?
    } else {
        serde_json::to_string_pretty(value)?
    };
    print_text(&format!("{json}\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_minus_exclude() {
        let specs = select_specs(true, &[], &["codex".into(), "goose".into()]).unwrap();
        assert_eq!(specs.len(), harness::all().len() - 2);
        assert!(!specs.iter().any(|s| s.id == "codex" || s.id == "goose"));
    }

    #[test]
    fn include_preserves_caller_order_and_dedupes_exact_ids() {
        let specs = select_specs(
            false,
            &["cursor".into(), "claude-code".into(), "cursor".into()],
            &[],
        )
        .unwrap();
        let ids: Vec<_> = specs.iter().map(|s| s.id).collect();
        assert_eq!(ids, vec!["cursor", "claude-code"]);
    }

    #[test]
    fn empty_selection_is_an_error() {
        assert!(matches!(
            select_specs(false, &[], &[]),
            Err(OneharnessError::NoSelection)
        ));
    }

    #[test]
    fn unknown_id_is_an_error() {
        assert!(matches!(
            select_specs(false, &["nope".into()], &[]),
            Err(OneharnessError::UnknownHarness { .. })
        ));
    }
}
