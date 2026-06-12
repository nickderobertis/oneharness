//! Unified configuration: the schema of `oneharness.toml`, plus the pure
//! parse / validate / merge logic. Discovery and reading of the actual files
//! is I/O and lives in `src/io/config.rs`.
//!
//! Two levels exist — a user-level file and a project-level file — and they
//! layer per field: CLI flags beat the project file, which beats the user
//! file, which beats the built-in defaults. Within one resolved config, a
//! `[harness.<id>]` value beats the top-level value for that harness. The
//! layering is resolved here, with no I/O, so it is unit-testable.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::domain::harness;
use crate::domain::report::OutputFormat;

/// One config file, as written by the user. Every field is optional: an absent
/// field defers to the next layer down. Unknown fields are rejected so a typo
/// fails loudly instead of being silently ignored.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    /// Default selection: run every harness (like `--all`). Mutually exclusive
    /// with `harnesses`. Used only when the CLI passes no selection.
    pub all: Option<bool>,
    /// Default selection: the harness ids to run (like `--harness`).
    pub harnesses: Option<Vec<String>>,
    /// Harness ids excluded from an `all` selection (like `--exclude`).
    pub exclude: Option<Vec<String>>,
    /// Model passed to each harness that supports a model flag (like `--model`).
    pub model: Option<String>,
    /// Portable system prompt (like `--system`).
    pub system: Option<String>,
    /// Request each harness's bypass mode (default true; like `--no-bypass`
    /// when false). The CLI's `--bypass` / `--no-bypass` always win.
    pub bypass: Option<bool>,
    /// Per-harness timeout in seconds (like `--timeout`).
    pub timeout: Option<u64>,
    /// Output format override (like `--output-format`).
    pub output_format: Option<OutputFormat>,
    /// Concurrency cap (like `--max-parallel`).
    pub max_parallel: Option<usize>,
    /// Treat a missing harness as a failure (like `--require-available`).
    pub require_available: Option<bool>,
    /// Extra environment for every harness process (like repeated `--env`).
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Per-harness overrides, keyed by canonical harness id.
    #[serde(default)]
    pub harness: BTreeMap<String, HarnessConfig>,
}

/// Overrides for one harness (`[harness.<id>]`). These beat the top-level
/// fields for that harness only — e.g. each harness can name its own model.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessConfig {
    /// Model for this harness (model names differ per provider).
    pub model: Option<String>,
    /// Binary name or path (like `--bin <id>=<path>`, lowest precedence:
    /// `--bin` and `ONEHARNESS_BIN_<ID>` both beat it).
    pub bin: Option<String>,
    /// Extra arguments appended verbatim to this harness's command (the
    /// configurable counterpart of the CLI's trailing `-- <args…>`).
    pub args: Option<Vec<String>>,
    /// Extra environment for this harness only; beats the top-level `[env]`.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// Parse one config file's text. Pure: the caller supplies the text and
/// attaches the path to any error. Rejects unknown fields, unknown harness
/// ids, and a selection that sets both `all` and `harnesses`.
pub fn parse(text: &str) -> Result<FileConfig, String> {
    let config: FileConfig = toml::from_str(text).map_err(|e| e.to_string())?;
    validate(&config)?;
    Ok(config)
}

fn validate(config: &FileConfig) -> Result<(), String> {
    if config.all == Some(true) && config.harnesses.is_some() {
        return Err("`all = true` and `harnesses` are mutually exclusive".to_string());
    }
    let named = config
        .harnesses
        .iter()
        .flatten()
        .chain(config.exclude.iter().flatten())
        .map(String::as_str)
        .chain(config.harness.keys().map(String::as_str));
    for id in named {
        if harness::by_id(id).is_none() {
            return Err(format!(
                "unknown harness id `{id}`. valid ids: {}",
                harness::valid_ids()
            ));
        }
    }
    for key in config
        .env
        .keys()
        .chain(config.harness.values().flat_map(|h| h.env.keys()))
    {
        if key.is_empty() {
            return Err("environment variable names must be non-empty".to_string());
        }
    }
    Ok(())
}

/// Layer `over` (e.g. the project file) on top of `base` (e.g. the user file):
/// a field set in `over` wins, an absent one falls through to `base`. The env
/// tables merge key-wise (`over` wins per key); the per-harness tables merge
/// per id and then per field. The selection (`all` + `harnesses`) moves as a
/// unit so the layers can never combine into a contradictory selection.
pub fn merge(base: FileConfig, over: FileConfig) -> FileConfig {
    let (all, harnesses) = if over.all.is_some() || over.harnesses.is_some() {
        (over.all, over.harnesses)
    } else {
        (base.all, base.harnesses)
    };

    let mut env = base.env;
    env.extend(over.env);

    let mut harness = base.harness;
    for (id, o) in over.harness {
        let entry = harness.entry(id).or_default();
        let mut merged_env = std::mem::take(&mut entry.env);
        merged_env.extend(o.env);
        *entry = HarnessConfig {
            model: o.model.or(entry.model.take()),
            bin: o.bin.or(entry.bin.take()),
            args: o.args.or(entry.args.take()),
            env: merged_env,
        };
    }

    FileConfig {
        all,
        harnesses,
        exclude: over.exclude.or(base.exclude),
        model: over.model.or(base.model),
        system: over.system.or(base.system),
        bypass: over.bypass.or(base.bypass),
        timeout: over.timeout.or(base.timeout),
        output_format: over.output_format.or(base.output_format),
        max_parallel: over.max_parallel.or(base.max_parallel),
        require_available: over.require_available.or(base.require_available),
        env,
        harness,
    }
}

impl FileConfig {
    /// The model for one harness: its `[harness.<id>]` override, else the
    /// top-level `model`. (A CLI `--model` beats both; the caller applies it.)
    pub fn model_for(&self, id: &str) -> Option<&str> {
        self.harness
            .get(id)
            .and_then(|h| h.model.as_deref())
            .or(self.model.as_deref())
    }

    /// The configured binary override for one harness, if any.
    pub fn bin_for(&self, id: &str) -> Option<&str> {
        self.harness.get(id).and_then(|h| h.bin.as_deref())
    }

    /// Extra args appended to one harness's command (before CLI passthrough).
    pub fn args_for(&self, id: &str) -> &[String] {
        self.harness
            .get(id)
            .and_then(|h| h.args.as_deref())
            .unwrap_or(&[])
    }

    /// The configured environment for one harness, in application order:
    /// top-level `[env]` first, then `[harness.<id>.env]` so it wins on a key
    /// collision. (The harness's own `default_env` goes before these, and CLI
    /// `--env` after; the runner applies env last-write-wins.)
    pub fn env_for(&self, id: &str) -> Vec<(String, String)> {
        let mut env: Vec<(String, String)> = self
            .env
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if let Some(h) = self.harness.get(id) {
            env.extend(h.env.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        env
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str) -> FileConfig {
        parse(text).expect("config should parse")
    }

    #[test]
    fn empty_config_is_all_defaults() {
        let c = parsed("");
        assert_eq!(c, FileConfig::default());
    }

    #[test]
    fn full_config_round_trips() {
        let c = parsed(
            r#"
            harnesses = ["claude-code", "codex"]
            exclude = ["cursor"]
            model = "haiku"
            system = "be terse"
            bypass = false
            timeout = 90
            output_format = "stream-json"
            max_parallel = 2
            require_available = true

            [env]
            FOO = "bar"

            [harness.claude-code]
            model = "sonnet"
            bin = "/opt/claude"
            args = ["--max-turns", "6"]
            env = { BAZ = "qux" }
            "#,
        );
        assert_eq!(c.harnesses.as_deref().unwrap(), ["claude-code", "codex"]);
        assert_eq!(c.exclude.as_deref().unwrap(), ["cursor"]);
        assert_eq!(c.model.as_deref(), Some("haiku"));
        assert_eq!(c.bypass, Some(false));
        assert_eq!(c.timeout, Some(90));
        assert_eq!(c.output_format, Some(OutputFormat::StreamJson));
        assert_eq!(c.max_parallel, Some(2));
        assert_eq!(c.require_available, Some(true));
        assert_eq!(c.env["FOO"], "bar");
        assert_eq!(c.model_for("claude-code"), Some("sonnet"));
        assert_eq!(c.model_for("codex"), Some("haiku"));
        assert_eq!(c.bin_for("claude-code"), Some("/opt/claude"));
        assert_eq!(c.args_for("claude-code"), ["--max-turns", "6"]);
        assert_eq!(c.args_for("codex"), Vec::<String>::new().as_slice());
    }

    #[test]
    fn unknown_top_level_field_is_rejected() {
        let err = parse("modle = \"typo\"").unwrap_err();
        assert!(err.contains("modle"), "{err}");
    }

    #[test]
    fn unknown_per_harness_field_is_rejected() {
        assert!(parse("[harness.claude-code]\nmodle = \"x\"").is_err());
    }

    #[test]
    fn unknown_harness_id_is_rejected_everywhere() {
        for text in [
            "harnesses = [\"bogus\"]",
            "exclude = [\"bogus\"]",
            "[harness.bogus]\nmodel = \"x\"",
        ] {
            let err = parse(text).unwrap_err();
            assert!(err.contains("bogus"), "{text} -> {err}");
        }
    }

    #[test]
    fn all_and_harnesses_together_are_rejected() {
        let err = parse("all = true\nharnesses = [\"codex\"]").unwrap_err();
        assert!(err.contains("mutually exclusive"), "{err}");
    }

    #[test]
    fn merge_prefers_over_per_field_and_falls_through() {
        let base = parsed("model = \"user\"\nsystem = \"keep me\"\ntimeout = 30");
        let over = parsed("model = \"project\"");
        let merged = merge(base, over);
        assert_eq!(merged.model.as_deref(), Some("project"));
        assert_eq!(merged.system.as_deref(), Some("keep me"));
        assert_eq!(merged.timeout, Some(30));
    }

    #[test]
    fn merge_moves_selection_as_a_unit() {
        // The user file says "all"; the project names harnesses. The merged
        // selection must be the project's alone, not a contradictory mix.
        let base = parsed("all = true");
        let over = parsed("harnesses = [\"codex\"]");
        let merged = merge(base, over);
        assert_eq!(merged.all, None);
        assert_eq!(merged.harnesses.as_deref().unwrap(), ["codex"]);
        // And the reverse: a project `all` drops the user's harness list.
        let merged = merge(parsed("harnesses = [\"codex\"]"), parsed("all = true"));
        assert_eq!(merged.all, Some(true));
        assert_eq!(merged.harnesses, None);
    }

    #[test]
    fn merge_env_is_keywise_with_over_winning() {
        let base = parsed("[env]\nA = \"base\"\nB = \"base\"");
        let over = parsed("[env]\nB = \"over\"\nC = \"over\"");
        let merged = merge(base, over);
        assert_eq!(merged.env["A"], "base");
        assert_eq!(merged.env["B"], "over");
        assert_eq!(merged.env["C"], "over");
    }

    #[test]
    fn merge_harness_tables_per_field() {
        let base = parsed("[harness.claude-code]\nmodel = \"user\"\nbin = \"/usr/bin/claude\"");
        let over = parsed("[harness.claude-code]\nmodel = \"project\"");
        let merged = merge(base, over);
        assert_eq!(merged.model_for("claude-code"), Some("project"));
        assert_eq!(merged.bin_for("claude-code"), Some("/usr/bin/claude"));
    }

    #[test]
    fn env_for_layers_global_then_harness() {
        let c = parsed("[env]\nA = \"global\"\nB = \"global\"\n[harness.qwen.env]\nB = \"qwen\"");
        assert_eq!(
            c.env_for("qwen"),
            vec![
                ("A".to_string(), "global".to_string()),
                ("B".to_string(), "global".to_string()),
                ("B".to_string(), "qwen".to_string()),
            ]
        );
        assert_eq!(
            c.env_for("codex"),
            vec![
                ("A".to_string(), "global".to_string()),
                ("B".to_string(), "global".to_string()),
            ]
        );
    }
}
