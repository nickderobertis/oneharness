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

use serde::{Deserialize, Serialize};

use crate::domain::harness;
use crate::domain::hooks::HookSpec;
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
    /// Tool/permission rules the harness may use without prompting, in each
    /// harness's native rule syntax. Delivered by `oneharness sync`, which
    /// merges them into the harness's own project config file (Claude Code's
    /// `.claude/settings.json` `permissions.allow`, Cursor's `.cursor/cli.json`,
    /// Qwen's `.qwen/settings.json`, crush's `crush.json`) so the policy also
    /// applies outside oneharness. A harness with no mapping reports the rule
    /// as `unmapped` in the sync report rather than dropping it silently.
    pub allowed_tools: Option<Vec<String>>,
    /// Deny rules; synced like `allowed_tools` (for crush this lands in
    /// `options.disabled_tools`, hiding the tool entirely).
    pub denied_tools: Option<Vec<String>>,
    /// Normalized pre-tool hooks, each fanned across the synced harnesses by
    /// `oneharness sync` and rendered into every harness's native shape (a
    /// shared config file, a dedicated hooks file, or a plugin). Unlike the
    /// verbatim per-harness `[harness.<id>.hooks]` table — which is one
    /// harness's own schema, limited to harnesses whose hooks share their
    /// config file — a `[[hooks]]` entry is harness-agnostic and reaches all of
    /// them. The two are independent and may both be set.
    pub hooks: Option<Vec<HookEntry>>,
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
    /// Allow rules for this harness only; beats the top-level `allowed_tools`.
    /// Rejected at parse time when the harness's config file has no allow-list
    /// concept (`oneharness sync` could not deliver it), so a rule that cannot
    /// apply fails loudly.
    pub allowed_tools: Option<Vec<String>>,
    /// Deny rules for this harness only; beats the top-level `denied_tools`.
    pub denied_tools: Option<Vec<String>>,
    /// Lifecycle hooks, as an uninterpreted table in the harness's own hooks
    /// schema. Synced into the harness's config file (Claude Code's
    /// `.claude/settings.json` `hooks` key); rejected at parse time for a
    /// harness whose hooks oneharness has no file mapping for.
    pub hooks: Option<toml::Value>,
    /// Raw settings merged verbatim into this harness's project config file by
    /// `oneharness sync` — the escape hatch for config shapes the unified
    /// fields don't model (e.g. OpenCode's `permission` policy map). Rejected
    /// at parse time for a harness with no known project config file.
    pub settings: Option<toml::Value>,
    /// Extra environment for this harness only; beats the top-level `[env]`.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// One `[[hooks]]` entry: a pre-tool gate installed into each targeted harness.
/// The `command` may contain `{harness}`, replaced with the harness id when the
/// hook is built (so one entry can route `mygate hook {harness}` to every CLI).
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HookEntry {
    /// The command the harness runs before a tool call. `{harness}` is
    /// substituted with the harness id. Required and non-empty.
    pub command: String,
    /// Tool-name matcher in the harness's dialect (most read it as a regex);
    /// applied to every targeted harness as written. Absent means match-all.
    pub matcher: Option<String>,
    /// Timeout in seconds, for the harnesses whose hook schema carries one
    /// (Goose, Crush); ignored by the others.
    pub timeout: Option<u64>,
    /// Identity for plugin-delivered harnesses (Goose, OpenCode) and Copilot's
    /// per-owner file, so two tools' hooks never collide. Defaults to
    /// `oneharness`.
    pub plugin_name: Option<String>,
    /// Restrict this entry to these harness ids; absent means every harness
    /// being synced.
    pub harnesses: Option<Vec<String>>,
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
    let hook_harnesses = config
        .hooks
        .iter()
        .flatten()
        .flat_map(|h| h.harnesses.iter().flatten());
    let named = config
        .harnesses
        .iter()
        .flatten()
        .chain(config.exclude.iter().flatten())
        .chain(hook_harnesses)
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
    // A `[[hooks]]` entry must carry a command, and any harness it explicitly
    // targets must be one oneharness can install a hook into — a hook that
    // could never land is a loud error, not a silent drop.
    for entry in config.hooks.iter().flatten() {
        if entry.command.trim().is_empty() {
            return Err("a `[[hooks]]` entry needs a non-empty `command`".to_string());
        }
        for id in entry.harnesses.iter().flatten() {
            // ids are validated above; unwrap is safe here.
            if harness::by_id(id)
                .expect("hook harness id validated")
                .hooks
                .is_none()
            {
                return Err(format!(
                    "`[[hooks]]` targets harness `{id}`, which oneharness cannot install a hook into"
                ));
            }
        }
    }
    // Sync settings scoped to a harness whose config file (if any) has no
    // place for them are rejected here, at parse time: a permission rule or
    // hook that silently would not land is worse than an error.
    for (id, h) in &config.harness {
        let spec = harness::by_id(id).expect("ids validated above");
        let sync = spec.sync.as_ref();
        let unsupported = [
            (h.allowed_tools.is_some() && sync.and_then(|s| s.allow_path).is_none())
                .then_some("allowed_tools"),
            (h.denied_tools.is_some() && sync.and_then(|s| s.deny_path).is_none())
                .then_some("denied_tools"),
            (h.hooks.is_some() && sync.and_then(|s| s.hooks_path).is_none()).then_some("hooks"),
            (h.settings.is_some() && sync.is_none()).then_some("settings"),
        ];
        if let Some(setting) = unsupported.into_iter().flatten().next() {
            return Err(format!(
                "`oneharness sync` cannot deliver `{setting}` for harness `{id}`: its \
                 config file has no mapping for it. Use `[harness.{id}.settings]` if the \
                 harness has a config file (see `sync_file` in `oneharness list`), or \
                 `[harness.{id}] args` for flag-based harnesses"
            ));
        }
        // Both are merged into a JSON object, so anything but a table would
        // corrupt the harness's config file — reject the shape up front.
        for (name, value) in [("hooks", &h.hooks), ("settings", &h.settings)] {
            if value.as_ref().is_some_and(|v| !v.is_table()) {
                return Err(format!(
                    "`{name}` for harness `{id}` must be a table (e.g. `[harness.{id}.{name}]`)"
                ));
            }
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
            allowed_tools: o.allowed_tools.or(entry.allowed_tools.take()),
            denied_tools: o.denied_tools.or(entry.denied_tools.take()),
            hooks: o.hooks.or(entry.hooks.take()),
            settings: o.settings.or(entry.settings.take()),
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
        allowed_tools: over.allowed_tools.or(base.allowed_tools),
        denied_tools: over.denied_tools.or(base.denied_tools),
        hooks: over.hooks.or(base.hooks),
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

    /// Allow rules for one harness: its `[harness.<id>]` override, else the
    /// top-level `allowed_tools`. (CLI `--allowed-tools` beats both.)
    pub fn allowed_tools_for(&self, id: &str) -> &[String] {
        self.harness
            .get(id)
            .and_then(|h| h.allowed_tools.as_deref())
            .or(self.allowed_tools.as_deref())
            .unwrap_or(&[])
    }

    /// Deny rules for one harness, resolved like [`Self::allowed_tools_for`].
    pub fn denied_tools_for(&self, id: &str) -> &[String] {
        self.harness
            .get(id)
            .and_then(|h| h.denied_tools.as_deref())
            .or(self.denied_tools.as_deref())
            .unwrap_or(&[])
    }

    /// The hooks table for one harness, if configured (per-harness only:
    /// hooks schemas are harness-specific, so there is no top-level form).
    pub fn hooks_for(&self, id: &str) -> Option<&toml::Value> {
        self.harness.get(id).and_then(|h| h.hooks.as_ref())
    }

    /// The normalized `[[hooks]]` that apply to harness `id`, ready to install:
    /// entries with no `harnesses` filter or one naming `id`, with `{harness}`
    /// substituted in the command. Order follows the config (later entries
    /// install after earlier ones).
    pub fn hook_specs_for(&self, id: &str) -> Vec<HookSpec> {
        self.hooks
            .iter()
            .flatten()
            .filter(|entry| match &entry.harnesses {
                Some(ids) => ids.iter().any(|h| h == id),
                None => true,
            })
            .map(|entry| HookSpec {
                command: entry.command.replace("{harness}", id),
                matcher: entry.matcher.clone(),
                timeout: entry.timeout,
                plugin_name: entry.plugin_name.clone(),
                description: None,
            })
            .collect()
    }

    /// The raw settings table for one harness, if configured (per-harness
    /// only: the shape is that harness's own config schema).
    pub fn settings_for(&self, id: &str) -> Option<&toml::Value> {
        self.harness.get(id).and_then(|h| h.settings.as_ref())
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

/// The `source` recorded for a value that comes from no file: the built-in
/// default. File sources are paths, so the two can't collide.
pub const DEFAULT_SOURCE: &str = "default";

/// One resolved field in the `oneharness config` report: the effective value
/// plus where it came from (a config file path, or [`DEFAULT_SOURCE`]). Both
/// are `null` when the field is unset everywhere and has no built-in default.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Field<T> {
    pub value: Option<T>,
    pub source: Option<String>,
}

impl<T> Field<T> {
    fn unset() -> Self {
        Self {
            value: None,
            source: None,
        }
    }

    fn default_value(value: T) -> Self {
        Self {
            value: Some(value),
            source: Some(DEFAULT_SOURCE.to_string()),
        }
    }

    fn or_default(self, value: T) -> Self {
        if self.value.is_some() {
            self
        } else {
            Self::default_value(value)
        }
    }
}

/// The `oneharness config` report: the fully layered configuration with the
/// provenance of every value, so a consumer can see exactly which file (or
/// default) shaped each setting of a run.
#[derive(Debug, Serialize)]
pub struct ConfigReport {
    pub schema_version: &'static str,
    /// The files consulted, in layering order (user first, project last).
    pub config_files: Vec<String>,
    pub all: Field<bool>,
    pub harnesses: Field<Vec<String>>,
    pub exclude: Field<Vec<String>>,
    pub model: Field<String>,
    pub system: Field<String>,
    pub bypass: Field<bool>,
    pub timeout: Field<u64>,
    pub output_format: Field<OutputFormat>,
    pub max_parallel: Field<usize>,
    pub require_available: Field<bool>,
    pub allowed_tools: Field<Vec<String>>,
    pub denied_tools: Field<Vec<String>>,
    pub hooks: Field<Vec<HookEntry>>,
    /// Per-key provenance for the top-level `[env]`.
    pub env: BTreeMap<String, Field<String>>,
    /// Per-harness overrides, with per-field provenance.
    pub harness: BTreeMap<String, HarnessReport>,
}

/// One harness's `[harness.<id>]` overrides, with provenance.
#[derive(Debug, Serialize)]
pub struct HarnessReport {
    pub model: Field<String>,
    pub bin: Field<String>,
    pub args: Field<Vec<String>>,
    pub allowed_tools: Field<Vec<String>>,
    pub denied_tools: Field<Vec<String>>,
    pub hooks: Field<toml::Value>,
    pub settings: Field<toml::Value>,
    pub env: BTreeMap<String, Field<String>>,
}

/// The last layer that sets the field wins — the same precedence [`merge`]
/// applies — but here the winning layer's path is recorded alongside.
fn pick<T: Clone>(
    layers: &[(String, FileConfig)],
    get: impl Fn(&FileConfig) -> Option<T>,
) -> Field<T> {
    let mut field = Field::unset();
    for (path, config) in layers {
        if let Some(value) = get(config) {
            field = Field {
                value: Some(value),
                source: Some(path.clone()),
            };
        }
    }
    field
}

/// Resolve the layers into a provenance report. Pure: `layers` are the parsed
/// files in layering order (user first, project last), exactly as loaded by
/// the io layer; built-in defaults are filled in with [`DEFAULT_SOURCE`].
pub fn explain(layers: &[(String, FileConfig)]) -> ConfigReport {
    // The selection moves as a unit (see `merge`): the last layer that states
    // any selection supplies both `all` and `harnesses`, so the report can
    // never show a contradictory mix of two files.
    let (all, harnesses) = layers
        .iter()
        .rev()
        .find(|(_, c)| c.all.is_some() || c.harnesses.is_some())
        .map(|(path, c)| {
            (
                Field {
                    source: c.all.is_some().then(|| path.clone()),
                    value: c.all,
                },
                Field {
                    source: c.harnesses.is_some().then(|| path.clone()),
                    value: c.harnesses.clone(),
                },
            )
        })
        .unwrap_or((Field::unset(), Field::unset()));

    let mut env: BTreeMap<String, Field<String>> = BTreeMap::new();
    for (path, config) in layers {
        for (key, value) in &config.env {
            env.insert(
                key.clone(),
                Field {
                    value: Some(value.clone()),
                    source: Some(path.clone()),
                },
            );
        }
    }

    let mut harness: BTreeMap<String, HarnessReport> = BTreeMap::new();
    let ids: std::collections::BTreeSet<&String> =
        layers.iter().flat_map(|(_, c)| c.harness.keys()).collect();
    for id in ids {
        let section = |c: &FileConfig| c.harness.get(id).cloned();
        let mut h_env: BTreeMap<String, Field<String>> = BTreeMap::new();
        for (path, config) in layers {
            if let Some(h) = config.harness.get(id) {
                for (key, value) in &h.env {
                    h_env.insert(
                        key.clone(),
                        Field {
                            value: Some(value.clone()),
                            source: Some(path.clone()),
                        },
                    );
                }
            }
        }
        harness.insert(
            id.clone(),
            HarnessReport {
                model: pick(layers, |c| section(c).and_then(|h| h.model)),
                bin: pick(layers, |c| section(c).and_then(|h| h.bin)),
                args: pick(layers, |c| section(c).and_then(|h| h.args)),
                allowed_tools: pick(layers, |c| section(c).and_then(|h| h.allowed_tools)),
                denied_tools: pick(layers, |c| section(c).and_then(|h| h.denied_tools)),
                hooks: pick(layers, |c| section(c).and_then(|h| h.hooks)),
                settings: pick(layers, |c| section(c).and_then(|h| h.settings)),
                env: h_env,
            },
        );
    }

    ConfigReport {
        schema_version: crate::domain::report::SCHEMA_VERSION,
        config_files: layers.iter().map(|(path, _)| path.clone()).collect(),
        all,
        harnesses,
        exclude: pick(layers, |c| c.exclude.clone()),
        model: pick(layers, |c| c.model.clone()),
        system: pick(layers, |c| c.system.clone()),
        bypass: pick(layers, |c| c.bypass).or_default(true),
        timeout: pick(layers, |c| c.timeout).or_default(120),
        output_format: pick(layers, |c| c.output_format),
        max_parallel: pick(layers, |c| c.max_parallel),
        require_available: pick(layers, |c| c.require_available).or_default(false),
        allowed_tools: pick(layers, |c| c.allowed_tools.clone()),
        denied_tools: pick(layers, |c| c.denied_tools.clone()),
        hooks: pick(layers, |c| c.hooks.clone()),
        env,
        harness,
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
    fn normalized_hooks_filter_and_substitute_per_harness() {
        let c = parsed(
            r#"
            [[hooks]]
            command = "mygate hook {harness}"
            matcher = "Bash"
            timeout = 10

            [[hooks]]
            command = "extra hook"
            harnesses = ["codex"]
            "#,
        );
        // The unfiltered entry applies everywhere with `{harness}` resolved; the
        // filtered one only adds to codex.
        let claude = c.hook_specs_for("claude-code");
        assert_eq!(claude.len(), 1);
        assert_eq!(claude[0].command, "mygate hook claude-code");
        assert_eq!(claude[0].matcher.as_deref(), Some("Bash"));
        assert_eq!(claude[0].timeout, Some(10));

        let codex = c.hook_specs_for("codex");
        assert_eq!(codex.len(), 2);
        assert_eq!(codex[0].command, "mygate hook codex");
        assert_eq!(codex[1].command, "extra hook");
    }

    #[test]
    fn hooks_merge_replaces_the_list_per_layer() {
        let base = parsed("[[hooks]]\ncommand = \"user gate\"");
        let over = parsed("[[hooks]]\ncommand = \"project gate\"");
        let merged = merge(base, over);
        let specs = merged.hook_specs_for("claude-code");
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].command, "project gate");
    }

    #[test]
    fn an_empty_hook_command_is_rejected() {
        let err = parse("[[hooks]]\ncommand = \"  \"").unwrap_err();
        assert!(err.contains("non-empty `command`"), "{err}");
    }

    #[test]
    fn a_hook_targeting_an_unknown_harness_is_rejected() {
        let err = parse("[[hooks]]\ncommand = \"x\"\nharnesses = [\"bogus\"]").unwrap_err();
        assert!(err.contains("bogus"), "{err}");
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
    fn allow_deny_and_hooks_parse_at_both_levels() {
        let c = parsed(
            r#"
            allowed_tools = ["Bash(git log:*)"]
            denied_tools = ["Bash(rm:*)"]

            [harness.cursor]
            allowed_tools = ["Shell(git)"]

            [harness.claude-code.hooks]
            PreToolUse = [{ matcher = "Bash", hooks = [{ type = "command", command = "./check.sh" }] }]
            "#,
        );
        // Per-harness beats top-level; others fall through to the top level.
        assert_eq!(c.allowed_tools_for("cursor"), ["Shell(git)"]);
        assert_eq!(c.allowed_tools_for("claude-code"), ["Bash(git log:*)"]);
        assert_eq!(c.denied_tools_for("claude-code"), ["Bash(rm:*)"]);
        assert!(c.hooks_for("claude-code").is_some());
        assert!(c.hooks_for("cursor").is_none());
    }

    #[test]
    fn sync_settings_without_a_file_mapping_are_rejected_at_parse() {
        // codex/goose/copilot have no project config file oneharness writes;
        // opencode's permission shape is a map, not a list; only claude-code
        // has a hooks mapping.
        for (text, what) in [
            ("[harness.codex]\nallowed_tools = [\"x\"]", "allowed_tools"),
            (
                "[harness.copilot]\nallowed_tools = [\"x\"]",
                "allowed_tools",
            ),
            (
                "[harness.opencode]\nallowed_tools = [\"x\"]",
                "allowed_tools",
            ),
            ("[harness.crush]\nhooks = {}", "hooks"),
            ("[harness.cursor.hooks]\nbeforeShellExecution = []", "hooks"),
            (
                "[harness.goose.settings]\nGOOSE_MODE = \"auto\"",
                "settings",
            ),
        ] {
            let err = parse(text).unwrap_err();
            assert!(err.contains(what), "{text} -> {err}");
            assert!(err.contains("cannot deliver"), "{text} -> {err}");
        }
        // The same settings parse fine where a mapping exists — including
        // qwen deny (permissions.deny) and crush deny (options.disabled_tools),
        // which file sync supports even though no CLI flag does.
        for text in [
            "[harness.qwen]\ndenied_tools = [\"Shell\"]",
            "[harness.crush]\ndenied_tools = [\"bash\"]",
            "[harness.cursor]\nallowed_tools = [\"Shell(ls)\"]",
            "[harness.claude-code.hooks]\nPreToolUse = []",
            "[harness.opencode.settings.permission]\nedit = \"deny\"",
        ] {
            assert!(parse(text).is_ok(), "{text} should parse");
        }
    }

    #[test]
    fn non_table_hooks_and_settings_are_rejected_at_parse() {
        // A scalar would be merged into the harness's JSON config as a bare
        // value, corrupting it — the shape is validated before anything else.
        for text in [
            "[harness.claude-code]\nhooks = \"oops\"",
            "[harness.claude-code]\nsettings = 3",
            "[harness.opencode]\nsettings = [\"list\"]",
        ] {
            let err = parse(text).unwrap_err();
            assert!(err.contains("must be a table"), "{text} -> {err}");
        }
    }

    #[test]
    fn merge_replaces_settings_tables_per_layer() {
        // Like hooks, a project-level settings table replaces the user-level
        // one wholesale (no deep merge across oneharness layers — the deep
        // merge happens against the harness's own file, at sync time).
        let base = parsed("[harness.opencode.settings.permission]\nedit = \"deny\"");
        let over = parsed("[harness.opencode.settings.permission.bash]\n\"git *\" = \"allow\"");
        let merged = merge(base, over);
        let settings = merged.settings_for("opencode").unwrap();
        assert!(settings.get("permission").unwrap().get("bash").is_some());
        assert!(
            settings.get("permission").unwrap().get("edit").is_none(),
            "tables replace across layers, not merge"
        );
    }

    #[test]
    fn merge_replaces_rule_lists_per_field() {
        let base = parsed("allowed_tools = [\"user-rule\"]\ndenied_tools = [\"user-deny\"]");
        let over = parsed("allowed_tools = [\"project-rule\"]");
        let merged = merge(base, over);
        assert_eq!(merged.allowed_tools.as_deref().unwrap(), ["project-rule"]);
        assert_eq!(merged.denied_tools.as_deref().unwrap(), ["user-deny"]);

        // Per-harness hooks: the project's table replaces the user's whole.
        let base = parsed("[harness.claude-code.hooks]\nPreToolUse = []");
        let over = parsed("[harness.claude-code.hooks]\nPostToolUse = []");
        let merged = merge(base, over);
        let hooks = merged.hooks_for("claude-code").unwrap();
        assert!(hooks.get("PostToolUse").is_some());
        assert!(
            hooks.get("PreToolUse").is_none(),
            "tables replace, not merge"
        );
    }

    #[test]
    fn explain_covers_rules_and_hooks() {
        let report = explain(&layers(
            "allowed_tools = [\"user-rule\"]",
            "[harness.claude-code]\nallowed_tools = [\"claude-rule\"]\n[harness.claude-code.hooks]\nPreToolUse = []",
        ));
        assert_eq!(
            report.allowed_tools.value.as_deref().unwrap(),
            ["user-rule"]
        );
        assert_eq!(report.allowed_tools.source.as_deref(), Some("/user.toml"));
        let claude = &report.harness["claude-code"];
        assert_eq!(
            claude.allowed_tools.source.as_deref(),
            Some("/project.toml")
        );
        assert_eq!(claude.hooks.source.as_deref(), Some("/project.toml"));
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

    fn layers(user: &str, project: &str) -> Vec<(String, FileConfig)> {
        vec![
            ("/user.toml".to_string(), parsed(user)),
            ("/project.toml".to_string(), parsed(project)),
        ]
    }

    #[test]
    fn explain_with_no_layers_shows_only_built_in_defaults() {
        let report = explain(&[]);
        assert!(report.config_files.is_empty());
        assert_eq!(report.model, Field::unset());
        assert_eq!(report.bypass, Field::default_value(true));
        assert_eq!(report.timeout, Field::default_value(120));
        assert_eq!(report.require_available, Field::default_value(false));
        assert!(report.env.is_empty());
        assert!(report.harness.is_empty());
    }

    #[test]
    fn explain_attributes_each_field_to_its_winning_layer() {
        let report = explain(&layers(
            "model = \"user\"\ntimeout = 30",
            "model = \"project\"",
        ));
        assert_eq!(report.config_files, ["/user.toml", "/project.toml"]);
        assert_eq!(report.model.value.as_deref(), Some("project"));
        assert_eq!(report.model.source.as_deref(), Some("/project.toml"));
        assert_eq!(report.timeout.value, Some(30));
        assert_eq!(report.timeout.source.as_deref(), Some("/user.toml"));
        // Untouched fields fall to their defaults, attributed as such.
        assert_eq!(report.bypass.source.as_deref(), Some(DEFAULT_SOURCE));
    }

    #[test]
    fn explain_moves_selection_as_a_unit_like_merge() {
        let report = explain(&layers("all = true", "harnesses = [\"codex\"]"));
        assert_eq!(report.all, Field::unset());
        assert_eq!(report.harnesses.value.as_deref().unwrap(), ["codex"]);
        assert_eq!(report.harnesses.source.as_deref(), Some("/project.toml"));
    }

    #[test]
    fn explain_tracks_env_per_key_and_harness_per_field() {
        let report = explain(&layers(
            "[env]\nA = \"user\"\nB = \"user\"\n[harness.claude-code]\nbin = \"/u/claude\"",
            "[env]\nB = \"project\"\n[harness.claude-code]\nmodel = \"sonnet\"",
        ));
        assert_eq!(report.env["A"].source.as_deref(), Some("/user.toml"));
        assert_eq!(report.env["B"].value.as_deref(), Some("project"));
        assert_eq!(report.env["B"].source.as_deref(), Some("/project.toml"));
        let claude = &report.harness["claude-code"];
        assert_eq!(claude.bin.source.as_deref(), Some("/user.toml"));
        assert_eq!(claude.model.value.as_deref(), Some("sonnet"));
        assert_eq!(claude.model.source.as_deref(), Some("/project.toml"));
        assert_eq!(claude.args, Field::unset());
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
