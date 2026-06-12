//! CLI verbs: each module orchestrates the domain + io layers for one command.
//! Shared helpers (harness selection, JSON output) live here.

pub mod config;
pub mod detect;
pub mod list;
pub mod run;

use serde::Serialize;

use crate::domain::harness::{self, HarnessSpec};
use crate::errors::OneharnessError;

/// Resolve a harness selection into specs, in registry order.
///
/// `all` selects every harness (minus `exclude`); otherwise `include` lists the
/// ids to run. An empty selection, or any unknown id, is a usage error.
pub fn select_specs(
    all: bool,
    include: &[String],
    exclude: &[String],
) -> Result<Vec<&'static HarnessSpec>, OneharnessError> {
    // Validate every named id up front so a typo fails loudly, not silently.
    for id in include.iter().chain(exclude.iter()) {
        if harness::by_id(id).is_none() {
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

    // Preserve registry order and de-duplicate a repeated id.
    let wanted: Vec<&str> = include.iter().map(String::as_str).collect();
    Ok(harness::all()
        .iter()
        .filter(|s| wanted.contains(&s.id))
        .collect())
}

/// Write a value as JSON to stdout (pretty unless `compact`).
pub fn print_json<T: Serialize>(value: &T, compact: bool) -> Result<(), OneharnessError> {
    let json = if compact {
        serde_json::to_string(value)?
    } else {
        serde_json::to_string_pretty(value)?
    };
    println!("{json}");
    Ok(())
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
    fn include_preserves_registry_order_and_dedupes() {
        let specs = select_specs(
            false,
            &["cursor".into(), "claude-code".into(), "cursor".into()],
            &[],
        )
        .unwrap();
        let ids: Vec<_> = specs.iter().map(|s| s.id).collect();
        assert_eq!(ids, vec!["claude-code", "cursor"]);
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
