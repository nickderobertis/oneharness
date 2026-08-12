//! Unified configuration: the schema of `oneharness.toml`, plus the pure
//! parse / validate / merge logic. Discovery and reading of the actual files
//! is I/O and lives in `src/io/config.rs`.
//!
//! The sources layer per field: CLI flags beat the `ONEHARNESS_*` environment
//! overrides ([`from_env`]), which beat the project file, which beats the user
//! file, which beats the built-in defaults. Within one resolved config, a
//! `[harness.<id>]` value beats the top-level value for that harness. The
//! layering — and the env layer's parsing — is resolved here, with no I/O (the
//! env reader is injected), so it is unit-testable.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};

use crate::domain::fallback::RunMode;
use crate::domain::harness;
use crate::domain::history::{self, HistoryLabels};
use crate::domain::hooks::HookSpec;
use crate::domain::mode::PermissionMode;
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
    /// The single-model base: used as each harness's model when no `models` list
    /// (and no CLI `--model`) is given, and overridable per harness via
    /// `[harness.<id>].model`.
    pub model: Option<String>,
    /// A list of models to **fan out** over (like repeated `--model`). When set
    /// (more than one entry), the run multiplies by the model axis: in `parallel`
    /// mode every selected harness runs once per model (the harness × model
    /// cross-product); in `fallback` mode the (harness, model) pairs form the
    /// priority chain, harness-major then model-minor, and a per-model rejection
    /// (`model_not_found` / `rate_limit`) falls through to the next model. A CLI
    /// `--model` (repeatable) overrides this entirely. Unlike the per-harness
    /// `model`, this list applies uniformly to every selected harness.
    pub models: Option<Vec<String>>,
    /// Portable system prompt (like `--system`).
    pub system: Option<String>,
    /// Reasoning / thinking effort (like `--reasoning`), an opaque string chosen
    /// for the model and forwarded verbatim to each harness that exposes effort
    /// on the argv (Claude Code's `--effort`, Codex's `model_reasoning_effort`).
    /// Naturally set per harness via `[harness.<id>].reasoning` next to that
    /// harness's `model`, since effort values are provider-specific. A harness
    /// with no headless reasoning surface is a loud usage error, never a silent
    /// drop.
    pub reasoning: Option<String>,
    /// Request each harness's bypass mode (default true; like `--no-bypass`
    /// when false). The CLI's `--bypass` / `--no-bypass` always win. Superseded
    /// by `mode` when both are set (`mode` is the richer spelling; `bypass =
    /// false` is exactly `mode = "default"`).
    pub bypass: Option<bool>,
    /// The normalized approval mode (like `--mode`): `plan` / `default` / `edit`
    /// / `auto` / `bypass`. Beats `bypass` when both are set. The CLI's `--mode`
    /// (and `--bypass` / `--no-bypass`) always win.
    pub mode: Option<PermissionMode>,
    /// Per-harness timeout in seconds (like `--timeout`).
    pub timeout: Option<u64>,
    /// Output format override (like `--output-format`).
    pub output_format: Option<OutputFormat>,
    /// Publish normalized events to stdout as they occur (like `--stream`; off
    /// by default). Exactly the flag's semantics, including its single-outcome
    /// validation — a config that turns streaming on for a selection it cannot
    /// stream (parallel multi-harness, a batch, `--schema`) is the same loud
    /// usage error the flag raises, never a silent downgrade. Most naturally set
    /// in the config of a supervising consumer that always reads events, so a
    /// wrapper no longer has to inject the flag per invocation. The CLI's
    /// `--stream` / `--no-stream` always win.
    pub stream: Option<bool>,
    /// Path to a JSON Schema file constraining each harness's final answer
    /// (like `--schema`). Resolved relative to the working directory. Turns the
    /// run into a structured-output run: the schema is delivered to the harness
    /// (natively where supported, else via the prompt), the result is validated,
    /// and a failure is re-prompted up to `schema_max_retries` times.
    pub schema_file: Option<String>,
    /// Maximum retries per harness when a response fails schema validation
    /// (like `--schema-max-retries`; default 2). Only meaningful with a schema.
    pub schema_max_retries: Option<u32>,
    /// Concurrency cap (like `--max-parallel`).
    pub max_parallel: Option<usize>,
    /// How the selected harnesses are run (like `--run-mode`): `parallel` (the
    /// default) runs them all at once; `fallback` runs them in priority order,
    /// stopping at the first that runs and falling through only harnesses that
    /// cannot run at all. Most naturally set in a project `oneharness.toml` to
    /// declare the harnesses a repo supports, so whichever a contributor has set
    /// up is used.
    pub run_mode: Option<RunMode>,
    /// Treat a missing harness as a failure (like `--require-available`).
    pub require_available: Option<bool>,
    /// Record a normalized, cross-harness history of each run to disk (opt-in,
    /// off by default; like `--history` / `--no-history`). Most naturally set in
    /// a user-level `config.toml` so one turns it on across every project.
    pub history: Option<bool>,
    /// Directory the history is written to and read from (like `--history-dir`).
    /// Defaults to `<platform state dir>/oneharness/history` when unset.
    pub history_dir: Option<String>,
    /// Labels attached to every history record from a run. Layers merge by key;
    /// higher-precedence values replace only the same key.
    pub history_labels: Option<HistoryLabels>,
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
    /// Reasoning / thinking effort for this harness only; beats the top-level
    /// `reasoning`. Its accepted values are provider-specific, so it usually
    /// lives next to this harness's `model`.
    pub reasoning: Option<String>,
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
    #[serde(default)]
    pub variant: BTreeMap<VariantName, VariantConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, schemars::JsonSchema)]
#[serde(transparent)]
pub struct VariantName(String);

pub const VARIANT_NAME_PATTERN: &str = "[A-Za-z0-9][A-Za-z0-9_-]{0,63}";

/// Whether `name` has the portable environment-variable identifier shape.
pub fn valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first == '_' || first.is_ascii_alphabetic())
        && chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

/// Whether an `env_from` value names a **filesystem location** the identity
/// lives in (an absolute path) rather than an opaque credential.
///
/// Absolute is the whole test, and it is deliberately conservative in the safe
/// direction: a credential — an API key, a token, an account name — is never an
/// absolute path, so a secret can never be probed on disk, while every
/// home-directory indirection in practice is one (`CODEX_HOME`,
/// `CLAUDE_CONFIG_DIR`). A relative value stays untouched too: oneharness has no
/// business deciding what it would be relative to.
///
/// Pure: this asks what the string *is*, never what is on disk. The existence
/// check belongs to the caller at the I/O boundary.
pub fn names_absolute_path(value: &str) -> bool {
    std::path::Path::new(value).is_absolute()
}

impl VariantName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for VariantName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::borrow::Borrow<str> for VariantName {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for VariantName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        validate_variant_name(&value).map_err(serde::de::Error::custom)?;
        Ok(Self(value))
    }
}

impl std::str::FromStr for VariantName {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        validate_variant_name(value)?;
        Ok(Self(value.to_string()))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariantConfig {
    pub model: Option<String>,
    pub reasoning: Option<String>,
    pub bin: Option<String>,
    pub args: Option<Vec<String>>,
    pub allowed_tools: Option<Vec<String>>,
    pub denied_tools: Option<Vec<String>>,
    pub hooks: Option<toml::Value>,
    pub settings: Option<toml::Value>,
    #[serde(default)]
    // llmlint: ignore[invalid_states_unrepresentable] This map shares the established TOML `[env]` string-key contract; `validate` checks every key before FileConfig is exposed, preserving backward-compatible merging without a second map-key representation.
    pub env: BTreeMap<String, String>,
    // llmlint: ignore[invalid_states_unrepresentable] The TOML contract intentionally stores a portable path string; deserialization rejects empty/NUL values and the I/O boundary resolves it, verifies it is a regular mode-private file, and reports the concrete path.
    pub env_file: Option<String>,
    #[serde(default)]
    // llmlint: ignore[invalid_states_unrepresentable] Environment names are TOML map keys/values that must remain strings for backward-compatible config merging; `validate` checks every target/source before FileConfig is exposed and the env-file parser independently validates external names.
    pub env_from: BTreeMap<String, String>,
    #[serde(default)]
    // llmlint: ignore[invalid_states_unrepresentable] These names share the established string-list config shape; `validate` checks the complete list before FileConfig is exposed, and Command receives only validated values.
    pub unset_env: Vec<String>,
}

/// One `[[hooks]]` entry: a pre-tool gate installed into each targeted harness.
/// The `command` may contain `{harness}`, replaced with the harness id when the
/// hook is built (so one entry can route `mygate hook {harness}` to every CLI).
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize, schemars::JsonSchema)]
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
    // llmlint: ignore[invalid_states_unrepresentable] HookEntry is the deny-unknown-fields TOML boundary DTO, so it must retain raw strings long enough to issue precise errors for unknown ids and variant selectors; `validate` rejects both before hooks are normalized or exposed to sync.
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
    for harness in config.harness.values() {
        for variant in harness.variant.values() {
            if variant
                .env_file
                .as_ref()
                .is_some_and(|path| path.is_empty() || path.contains('\0'))
            {
                return Err("variant `env_file` must be a non-empty path without NUL".to_string());
            }
        }
    }
    if config.all == Some(true) && config.harnesses.is_some() {
        return Err("`all = true` and `harnesses` are mutually exclusive".to_string());
    }
    let named = config
        .harnesses
        .iter()
        .flatten()
        .chain(config.exclude.iter().flatten())
        .filter_map(|id| {
            id.split_once(':')
                .map_or(Some(id.as_str()), |(base, _)| Some(base))
        })
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
            if id.contains(':') {
                return Err(format!(
                    "`[[hooks]].harnesses` accepts base harness ids, not variant selector `{id}`; variants share one synced harness config"
                ));
            }
            let Some(spec) = harness::by_id(id) else {
                return Err(format!(
                    "unknown harness id `{id}`. valid ids: {}",
                    harness::valid_ids()
                ));
            };
            if spec.hooks.is_none() {
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
        for name in h.variant.keys() {
            validate_variant_name(name.as_str())?;
        }
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
        for (name, variant) in &h.variant {
            let unsupported = [
                (variant.allowed_tools.is_some()
                    && sync.and_then(|value| value.allow_path).is_none())
                .then_some("allowed_tools"),
                (variant.denied_tools.is_some()
                    && sync.and_then(|value| value.deny_path).is_none())
                .then_some("denied_tools"),
                (variant.hooks.is_some() && sync.and_then(|value| value.hooks_path).is_none())
                    .then_some("hooks"),
                (variant.settings.is_some() && sync.is_none()).then_some("settings"),
            ];
            if let Some(setting) = unsupported.into_iter().flatten().next() {
                return Err(format!(
                    "`oneharness sync` cannot deliver `{setting}` for harness variant \
                     `{id}:{name}`"
                ));
            }
            for (field, value) in [("hooks", &variant.hooks), ("settings", &variant.settings)] {
                if value.as_ref().is_some_and(|value| !value.is_table()) {
                    return Err(format!(
                        "`{field}` for harness variant `{id}:{name}` must be a table"
                    ));
                }
            }
        }
    }
    for key in config
        .env
        .keys()
        .chain(config.harness.values().flat_map(|h| h.env.keys()))
        .chain(
            config
                .harness
                .values()
                .flat_map(|h| h.variant.values())
                .flat_map(|variant| {
                    variant
                        .env
                        .keys()
                        .chain(variant.env_from.keys())
                        .chain(variant.env_from.values())
                        .chain(variant.unset_env.iter())
                }),
        )
    {
        if !valid_env_name(key) {
            return Err("environment variable names must match [A-Za-z_][A-Za-z0-9_]*".to_string());
        }
    }
    for composed in config
        .harnesses
        .iter()
        .flatten()
        .chain(config.exclude.iter().flatten())
    {
        if let Some((_, name)) = composed.split_once(':') {
            validate_variant_name(name)?;
        }
    }
    Ok(())
}

/// Whether `name` is a legal `[harness.<id>.variant.<name>]` key — the one
/// spelling of [`VARIANT_NAME_PATTERN`], shared so every place that admits a
/// variant (config validation here, [`super::harness::HarnessIdentity`] parsing a
/// composed id) accepts exactly the same set rather than drifting apart.
#[must_use]
pub(crate) fn is_valid_variant_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .enumerate()
            .all(|(i, b)| b.is_ascii_alphanumeric() || (i > 0 && matches!(b, b'-' | b'_')))
}

fn validate_variant_name(name: &str) -> Result<(), String> {
    is_valid_variant_name(name).then_some(()).ok_or_else(|| {
        format!("invalid harness variant name `{name}`; expected {VARIANT_NAME_PATTERN}")
    })
}

/// The `source` recorded for a value that comes from an `ONEHARNESS_*`
/// environment override rather than a config file or a built-in default. It is
/// not a path, so it never collides with a file source or [`DEFAULT_SOURCE`].
pub const ENV_SOURCE: &str = "environment";

/// Build a config layer from the `ONEHARNESS_*` environment overrides, reading
/// each variable through `get` (kept a parameter so this stays pure and
/// unit-testable — the io layer passes `std::env::var`). Returns `Ok(None)` when
/// no override is set (so no layer is added), `Ok(Some(_))` otherwise, and an
/// `Err` for a value that cannot be parsed (a bad boolean/integer/output format,
/// or a selection that names an unknown harness or sets both `all` and
/// `harnesses`) — a malformed override fails loudly, exactly like a malformed
/// file.
///
/// Every name mirrors its config field and CLI flag: `ONEHARNESS_<FIELD>` in
/// upper snake case (`model` → `ONEHARNESS_MODEL`, `schema_max_retries` →
/// `ONEHARNESS_SCHEMA_MAX_RETRIES`). List fields are comma-separated like the
/// repeatable `--harness` / `--exclude` flags. An empty value counts as unset,
/// matching `ONEHARNESS_CONFIG` / `ONEHARNESS_BIN_<ID>`. Only the top-level
/// fields with a `run` flag are mapped; the sync-policy fields
/// (`allowed_tools` / `denied_tools` / `hooks` / `settings`), the `[env]` table,
/// and the `[harness.<id>]` overrides have no env form by design.
pub fn from_env(get: impl Fn(&str) -> Option<String>) -> Result<Option<FileConfig>, String> {
    let read = |name: &str| get(name).filter(|v| !v.is_empty());

    let config = FileConfig {
        all: env_bool(&read, "ONEHARNESS_ALL")?,
        harnesses: env_list(&read, "ONEHARNESS_HARNESSES"),
        exclude: env_list(&read, "ONEHARNESS_EXCLUDE"),
        model: read("ONEHARNESS_MODEL"),
        models: env_list(&read, "ONEHARNESS_MODELS"),
        system: read("ONEHARNESS_SYSTEM"),
        reasoning: read("ONEHARNESS_REASONING"),
        bypass: env_bool(&read, "ONEHARNESS_BYPASS")?,
        mode: env_mode(&read)?,
        timeout: env_num(&read, "ONEHARNESS_TIMEOUT", "a non-negative integer")?,
        output_format: env_output_format(&read)?,
        stream: env_bool(&read, "ONEHARNESS_STREAM")?,
        schema_file: read("ONEHARNESS_SCHEMA_FILE"),
        schema_max_retries: env_num(
            &read,
            "ONEHARNESS_SCHEMA_MAX_RETRIES",
            "a non-negative integer",
        )?,
        max_parallel: env_num(&read, "ONEHARNESS_MAX_PARALLEL", "a non-negative integer")?,
        run_mode: env_run_mode(&read)?,
        require_available: env_bool(&read, "ONEHARNESS_REQUIRE_AVAILABLE")?,
        history: env_bool(&read, "ONEHARNESS_HISTORY")?,
        history_dir: read("ONEHARNESS_HISTORY_DIR"),
        history_labels: read("ONEHARNESS_HISTORY_LABELS")
            .map(|value| history::parse_labels(value.split(',').map(str::trim)))
            .transpose()?,
        ..FileConfig::default()
    };

    // No override touched anything: contribute no layer at all, so the report's
    // `config_files` and provenance only mention `environment` when it matters.
    if config == FileConfig::default() {
        return Ok(None);
    }
    validate(&config)?;
    Ok(Some(config))
}

/// Read an `ONEHARNESS_*` boolean: `true`/`false` (case-insensitive) or `1`/`0`.
fn env_bool<F: Fn(&str) -> Option<String>>(read: &F, name: &str) -> Result<Option<bool>, String> {
    match read(name) {
        None => Ok(None),
        Some(v) => match v.to_ascii_lowercase().as_str() {
            "true" | "1" => Ok(Some(true)),
            "false" | "0" => Ok(Some(false)),
            _ => Err(format!("`{name}` must be `true` or `false`, got `{v}`")),
        },
    }
}

/// Read an `ONEHARNESS_*` value that parses via [`std::str::FromStr`] (the
/// numeric fields), attaching the variable name to any parse failure.
fn env_num<T, F>(read: &F, name: &str, what: &str) -> Result<Option<T>, String>
where
    T: std::str::FromStr,
    F: Fn(&str) -> Option<String>,
{
    match read(name) {
        None => Ok(None),
        Some(v) => v
            .parse::<T>()
            .map(Some)
            .map_err(|_| format!("`{name}` must be {what}, got `{v}`")),
    }
}

/// Read a comma-separated `ONEHARNESS_*` list (the selection fields), trimming
/// each entry and dropping empties, mirroring the `--harness` / `--exclude`
/// delimiter behavior.
fn env_list<F: Fn(&str) -> Option<String>>(read: &F, name: &str) -> Option<Vec<String>> {
    read(name).map(|v| {
        v.split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect()
    })
}

/// Read `ONEHARNESS_MODE`, accepting the same tokens as the `--mode` flag.
fn env_mode<F: Fn(&str) -> Option<String>>(read: &F) -> Result<Option<PermissionMode>, String> {
    let name = "ONEHARNESS_MODE";
    match read(name) {
        None => Ok(None),
        Some(v) => PermissionMode::parse(&v)
            .map(Some)
            .map_err(|e| format!("`{name}` {e}")),
    }
}

/// Read `ONEHARNESS_RUN_MODE`, accepting the same tokens as the `--run-mode` flag.
fn env_run_mode<F: Fn(&str) -> Option<String>>(read: &F) -> Result<Option<RunMode>, String> {
    let name = "ONEHARNESS_RUN_MODE";
    match read(name) {
        None => Ok(None),
        Some(v) => RunMode::parse(&v)
            .map(Some)
            .ok_or_else(|| format!("`{name}` must be one of `parallel`, `fallback`, got `{v}`")),
    }
}

/// Read `ONEHARNESS_OUTPUT_FORMAT`, accepting the same tokens as the CLI flag.
fn env_output_format<F: Fn(&str) -> Option<String>>(
    read: &F,
) -> Result<Option<OutputFormat>, String> {
    let name = "ONEHARNESS_OUTPUT_FORMAT";
    match read(name) {
        None => Ok(None),
        Some(v) => match v.as_str() {
            "text" => Ok(Some(OutputFormat::Text)),
            "json" => Ok(Some(OutputFormat::Json)),
            "stream-json" => Ok(Some(OutputFormat::StreamJson)),
            _ => Err(format!(
                "`{name}` must be one of `text`, `json`, `stream-json`, got `{v}`"
            )),
        },
    }
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

    let history_labels = match (base.history_labels.clone(), over.history_labels.clone()) {
        (None, None) => None,
        (base, higher) => {
            let mut labels = base.unwrap_or_default();
            if let Some(higher) = higher {
                labels.extend(&higher);
            }
            Some(labels)
        }
    };

    let mut harness = base.harness;
    for (id, o) in over.harness {
        let entry = harness.entry(id).or_default();
        let mut merged_env = std::mem::take(&mut entry.env);
        merged_env.extend(o.env);
        *entry = HarnessConfig {
            model: o.model.or(entry.model.take()),
            reasoning: o.reasoning.or(entry.reasoning.take()),
            bin: o.bin.or(entry.bin.take()),
            args: o.args.or(entry.args.take()),
            allowed_tools: o.allowed_tools.or(entry.allowed_tools.take()),
            denied_tools: o.denied_tools.or(entry.denied_tools.take()),
            hooks: o.hooks.or(entry.hooks.take()),
            settings: o.settings.or(entry.settings.take()),
            env: merged_env,
            variant: merge_variant_maps(std::mem::take(&mut entry.variant), o.variant),
        };
    }

    FileConfig {
        all,
        harnesses,
        exclude: over.exclude.or(base.exclude),
        model: over.model.or(base.model),
        models: over.models.or(base.models),
        system: over.system.or(base.system),
        reasoning: over.reasoning.or(base.reasoning),
        bypass: over.bypass.or(base.bypass),
        mode: over.mode.or(base.mode),
        timeout: over.timeout.or(base.timeout),
        output_format: over.output_format.or(base.output_format),
        stream: over.stream.or(base.stream),
        schema_file: over.schema_file.or(base.schema_file),
        schema_max_retries: over.schema_max_retries.or(base.schema_max_retries),
        max_parallel: over.max_parallel.or(base.max_parallel),
        run_mode: over.run_mode.or(base.run_mode),
        require_available: over.require_available.or(base.require_available),
        history: over.history.or(base.history),
        history_dir: over.history_dir.or(base.history_dir),
        history_labels,
        allowed_tools: over.allowed_tools.or(base.allowed_tools),
        denied_tools: over.denied_tools.or(base.denied_tools),
        hooks: over.hooks.or(base.hooks),
        env,
        harness,
    }
}

fn merge_variant_maps(
    mut base: BTreeMap<VariantName, VariantConfig>,
    over: BTreeMap<VariantName, VariantConfig>,
) -> BTreeMap<VariantName, VariantConfig> {
    for (name, o) in over {
        let e = base.entry(name).or_default();
        let mut env = std::mem::take(&mut e.env);
        env.extend(o.env);
        let mut env_from = std::mem::take(&mut e.env_from);
        env_from.extend(o.env_from);
        *e = VariantConfig {
            model: o.model.or(e.model.take()),
            reasoning: o.reasoning.or(e.reasoning.take()),
            bin: o.bin.or(e.bin.take()),
            args: o.args.or(e.args.take()),
            allowed_tools: o.allowed_tools.or(e.allowed_tools.take()),
            denied_tools: o.denied_tools.or(e.denied_tools.take()),
            hooks: o.hooks.or(e.hooks.take()),
            settings: o.settings.or(e.settings.take()),
            env,
            env_file: o.env_file.or(e.env_file.take()),
            env_from,
            unset_env: if o.unset_env.is_empty() {
                std::mem::take(&mut e.unset_env)
            } else {
                o.unset_env
            },
        };
    }
    base
}

impl FileConfig {
    // llmlint: ignore[invalid_states_unrepresentable] This public helper preserves the existing serialized-selector API used across the core/binary crate boundary; config validation and command selection reject malformed/unknown variants before resolution, while this function only borrows the base/name slices without asserting validity.
    pub fn split_harness_id<'a>(&self, composed: &'a str) -> (&'a str, Option<&'a str>) {
        composed
            .split_once(':')
            .map_or((composed, None), |(id, variant)| (id, Some(variant)))
    }

    pub fn variant_for(&self, composed: &str) -> Option<&VariantConfig> {
        let (id, name) = self.split_harness_id(composed);
        self.harness.get(id)?.variant.get(name?)
    }

    /// The model for one harness: its `[harness.<id>]` override, else the
    /// top-level `model`. (A CLI `--model` beats both; the caller applies it.)
    pub fn model_for(&self, id: &str) -> Option<&str> {
        let (base, _) = self.split_harness_id(id);
        if let Some(value) = self.variant_for(id).and_then(|v| v.model.as_deref()) {
            return Some(value);
        }
        self.harness
            .get(base)
            .and_then(|h| h.model.as_deref())
            .or(self.model.as_deref())
    }

    /// The reasoning/effort for one harness: its `[harness.<id>]` override, else
    /// the top-level `reasoning`. (A CLI `--reasoning` beats both; the caller
    /// applies it.)
    pub fn reasoning_for(&self, id: &str) -> Option<&str> {
        let (base, _) = self.split_harness_id(id);
        if let Some(value) = self.variant_for(id).and_then(|v| v.reasoning.as_deref()) {
            return Some(value);
        }
        self.harness
            .get(base)
            .and_then(|h| h.reasoning.as_deref())
            .or(self.reasoning.as_deref())
    }

    /// The configured binary override for one harness, if any.
    pub fn bin_for(&self, id: &str) -> Option<&str> {
        let (base, _) = self.split_harness_id(id);
        self.variant_for(id)
            .and_then(|v| v.bin.as_deref())
            .or_else(|| self.harness.get(base).and_then(|h| h.bin.as_deref()))
    }

    /// Extra args appended to one harness's command (before CLI passthrough).
    pub fn args_for(&self, id: &str) -> &[String] {
        let (base, _) = self.split_harness_id(id);
        if let Some(args) = self.variant_for(id).and_then(|v| v.args.as_deref()) {
            return args;
        }
        self.harness
            .get(base)
            .and_then(|h| h.args.as_deref())
            .unwrap_or(&[])
    }

    /// Allow rules for one harness: its `[harness.<id>]` override, else the
    /// top-level `allowed_tools`. (CLI `--allowed-tools` beats both.)
    pub fn allowed_tools_for(&self, id: &str) -> &[String] {
        let (base, _) = self.split_harness_id(id);
        if let Some(value) = self
            .variant_for(id)
            .and_then(|variant| variant.allowed_tools.as_deref())
        {
            return value;
        }
        self.harness
            .get(base)
            .and_then(|h| h.allowed_tools.as_deref())
            .or(self.allowed_tools.as_deref())
            .unwrap_or(&[])
    }

    /// Deny rules for one harness, resolved like [`Self::allowed_tools_for`].
    pub fn denied_tools_for(&self, id: &str) -> &[String] {
        let (base, _) = self.split_harness_id(id);
        if let Some(value) = self
            .variant_for(id)
            .and_then(|variant| variant.denied_tools.as_deref())
        {
            return value;
        }
        self.harness
            .get(base)
            .and_then(|h| h.denied_tools.as_deref())
            .or(self.denied_tools.as_deref())
            .unwrap_or(&[])
    }

    /// The hooks table for one harness, if configured (per-harness only:
    /// hooks schemas are harness-specific, so there is no top-level form).
    pub fn hooks_for(&self, id: &str) -> Option<&toml::Value> {
        let (base, _) = self.split_harness_id(id);
        self.variant_for(id)
            .and_then(|variant| variant.hooks.as_ref())
            .or_else(|| self.harness.get(base).and_then(|h| h.hooks.as_ref()))
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
        let (base, _) = self.split_harness_id(id);
        self.variant_for(id)
            .and_then(|variant| variant.settings.as_ref())
            .or_else(|| self.harness.get(base).and_then(|h| h.settings.as_ref()))
    }

    /// The configured environment for one harness, in application order:
    /// top-level `[env]` first, then `[harness.<id>.env]` so it wins on a key
    /// collision. (The harness's own `default_env` goes before these, and CLI
    /// `--env` after; the runner applies env last-write-wins.)
    pub fn env_for(&self, id: &str) -> Vec<(String, String)> {
        let (base, _) = self.split_harness_id(id);
        let mut env: Vec<(String, String)> = self
            .env
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if let Some(h) = self.harness.get(base) {
            env.extend(h.env.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        if let Some(v) = self.variant_for(id) {
            env.extend(v.env.iter().map(|(k, v)| (k.clone(), v.clone())));
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
#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
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
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ConfigReport {
    pub schema_version: &'static str,
    /// The files consulted, in layering order (user first, project last).
    pub config_files: Vec<String>,
    pub all: Field<bool>,
    pub harnesses: Field<Vec<String>>,
    pub exclude: Field<Vec<String>>,
    pub model: Field<String>,
    /// The model fan-out list (`models` / repeated `--model` / `ONEHARNESS_MODELS`),
    /// if any. Unset when the run uses a single per-harness model.
    pub models: Field<Vec<String>>,
    pub system: Field<String>,
    /// Reasoning / thinking effort (`--reasoning` / `ONEHARNESS_REASONING`), if
    /// any. Forwarded to reasoning-capable harnesses only.
    pub reasoning: Field<String>,
    pub bypass: Field<bool>,
    /// The configured `mode`, if any. Unset when only the legacy `bypass` field
    /// (or neither) is set — the effective mode then derives from `bypass`.
    pub mode: Field<PermissionMode>,
    pub timeout: Field<u64>,
    pub output_format: Field<OutputFormat>,
    /// Whether config asks every `run` to stream (`--stream` / ONEHARNESS_STREAM);
    /// defaults to false, the built-in buffered report.
    pub stream: Field<bool>,
    pub schema_file: Field<String>,
    pub schema_max_retries: Field<u32>,
    pub max_parallel: Field<usize>,
    /// The configured run mode, if any. Unset falls back to `parallel` (the
    /// built-in default) at run time.
    pub run_mode: Field<RunMode>,
    pub require_available: Field<bool>,
    pub history: Field<bool>,
    pub history_dir: Field<String>,
    /// Per-key provenance for history labels.
    pub history_labels: BTreeMap<String, Field<String>>,
    pub allowed_tools: Field<Vec<String>>,
    pub denied_tools: Field<Vec<String>>,
    pub hooks: Field<Vec<HookEntry>>,
    /// Per-key provenance for the top-level `[env]`.
    pub env: BTreeMap<String, Field<String>>,
    /// Per-harness overrides, with per-field provenance.
    pub harness: BTreeMap<String, HarnessReport>,
}

/// One harness's `[harness.<id>]` overrides, with provenance.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct HarnessReport {
    pub model: Field<String>,
    pub reasoning: Field<String>,
    pub bin: Field<String>,
    pub args: Field<Vec<String>>,
    pub allowed_tools: Field<Vec<String>>,
    pub denied_tools: Field<Vec<String>>,
    // llmlint: ignore[invalid_states_unrepresentable] These two fields are deliberately raw passthrough TOML — the whole point is that oneharness does not interpret a harness's own settings table — so the schema says "an arbitrary object", which is the honest description of an uninterpreted value.
    #[schemars(with = "serde_json::Map<String, serde_json::Value>")]
    pub hooks: Field<toml::Value>,
    #[schemars(with = "serde_json::Map<String, serde_json::Value>")]
    pub settings: Field<toml::Value>,
    pub env: BTreeMap<String, Field<String>>,
    pub variant: BTreeMap<String, VariantReport>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct VariantReport {
    pub model: Field<String>,
    pub reasoning: Field<String>,
    pub bin: Field<String>,
    pub args: Field<Vec<String>>,
    pub allowed_tools: Field<Vec<String>>,
    pub denied_tools: Field<Vec<String>>,
    #[schemars(with = "serde_json::Map<String, serde_json::Value>")]
    pub hooks: Field<toml::Value>,
    #[schemars(with = "serde_json::Map<String, serde_json::Value>")]
    pub settings: Field<toml::Value>,
    pub env_file: Field<String>,
    pub env_from: BTreeMap<String, Field<String>>,
    pub unset_env: Field<Vec<String>>,
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

    let mut history_labels: BTreeMap<String, Field<String>> = BTreeMap::new();
    for (path, config) in layers {
        for (key, value) in config.history_labels.iter().flat_map(HistoryLabels::as_map) {
            history_labels.insert(
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
        let mut variants = BTreeMap::new();
        let names: std::collections::BTreeSet<&VariantName> = layers
            .iter()
            .filter_map(|(_, config)| config.harness.get(id))
            .flat_map(|harness| harness.variant.keys())
            .collect();
        for name in names {
            let variant =
                |config: &FileConfig| config.harness.get(id)?.variant.get(name.as_str()).cloned();
            let mut v_env = BTreeMap::new();
            let mut env_from = BTreeMap::new();
            for (path, config) in layers {
                if let Some(value) = variant(config) {
                    for (key, value) in value.env {
                        v_env.insert(
                            key,
                            Field {
                                value: Some(value),
                                source: Some(path.clone()),
                            },
                        );
                    }
                    for (key, value) in value.env_from {
                        env_from.insert(
                            key,
                            Field {
                                value: Some(value),
                                source: Some(path.clone()),
                            },
                        );
                    }
                }
            }
            variants.insert(
                name.to_string(),
                VariantReport {
                    model: pick(layers, |c| variant(c).and_then(|v| v.model)),
                    reasoning: pick(layers, |c| variant(c).and_then(|v| v.reasoning)),
                    bin: pick(layers, |c| variant(c).and_then(|v| v.bin)),
                    args: pick(layers, |c| variant(c).and_then(|v| v.args)),
                    allowed_tools: pick(layers, |c| variant(c).and_then(|v| v.allowed_tools)),
                    denied_tools: pick(layers, |c| variant(c).and_then(|v| v.denied_tools)),
                    hooks: pick(layers, |c| variant(c).and_then(|v| v.hooks)),
                    settings: pick(layers, |c| variant(c).and_then(|v| v.settings)),
                    env_file: pick(layers, |c| variant(c).and_then(|v| v.env_file)),
                    unset_env: pick(layers, |c| variant(c).map(|v| v.unset_env)),
                    env: v_env,
                    env_from,
                },
            );
        }
        harness.insert(
            id.clone(),
            HarnessReport {
                model: pick(layers, |c| section(c).and_then(|h| h.model)),
                reasoning: pick(layers, |c| section(c).and_then(|h| h.reasoning)),
                bin: pick(layers, |c| section(c).and_then(|h| h.bin)),
                args: pick(layers, |c| section(c).and_then(|h| h.args)),
                allowed_tools: pick(layers, |c| section(c).and_then(|h| h.allowed_tools)),
                denied_tools: pick(layers, |c| section(c).and_then(|h| h.denied_tools)),
                hooks: pick(layers, |c| section(c).and_then(|h| h.hooks)),
                settings: pick(layers, |c| section(c).and_then(|h| h.settings)),
                env: h_env,
                variant: variants,
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
        models: pick(layers, |c| c.models.clone()),
        system: pick(layers, |c| c.system.clone()),
        reasoning: pick(layers, |c| c.reasoning.clone()),
        // Legacy `bypass` is opt-in now (the default mode is `default`), so its
        // built-in default is false; `mode` is the richer field.
        bypass: pick(layers, |c| c.bypass).or_default(false),
        mode: pick(layers, |c| c.mode),
        timeout: pick(layers, |c| c.timeout).or_default(120),
        output_format: pick(layers, |c| c.output_format),
        stream: pick(layers, |c| c.stream).or_default(false),
        schema_file: pick(layers, |c| c.schema_file.clone()),
        schema_max_retries: pick(layers, |c| c.schema_max_retries),
        max_parallel: pick(layers, |c| c.max_parallel),
        run_mode: pick(layers, |c| c.run_mode),
        require_available: pick(layers, |c| c.require_available).or_default(false),
        history: pick(layers, |c| c.history).or_default(false),
        history_dir: pick(layers, |c| c.history_dir.clone()),
        history_labels,
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
    fn variant_fields_layer_over_harness_and_top_level() {
        let base = parsed(
            r#"
model = "top"
[harness.claude-code]
model = "base"
args = ["base"]
[harness.claude-code.variant.work]
model = "work"
env_file = "/outside/work.env"
unset_env = ["ANTHROPIC_API_KEY"]
[harness.claude-code.variant.work.env_from]
ANTHROPIC_API_KEY = "ANTHROPIC_API_KEY_WORK"
"#,
        );
        let higher = parsed("[harness.claude-code.variant.work]\nargs = [\"higher\"]\n");
        let config = merge(base, higher);
        assert_eq!(config.model_for("claude-code:work"), Some("work"));
        assert_eq!(config.args_for("claude-code:work"), ["higher"]);
        assert_eq!(
            config.variant_for("claude-code:work").unwrap().env_from["ANTHROPIC_API_KEY"],
            "ANTHROPIC_API_KEY_WORK"
        );
    }

    #[test]
    fn variant_resolvers_cover_every_supported_override_and_base_fallback() {
        let config = parsed(
            r#"
allowed_tools = ["top-allow"]
denied_tools = ["top-deny"]
[env]
SHARED = "top"
[harness.claude-code]
reasoning = "base-reasoning"
bin = "base-bin"
args = ["base-arg"]
[harness.claude-code.env]
SHARED = "base"
BASE_ONLY = "yes"
[harness.claude-code.hooks]
base = true
[harness.claude-code.settings]
base = true
[harness.claude-code.variant.work]
reasoning = "variant-reasoning"
bin = "variant-bin"
allowed_tools = ["variant-allow"]
denied_tools = ["variant-deny"]
[harness.claude-code.variant.work.env]
SHARED = "variant"
[harness.claude-code.variant.work.hooks]
variant = true
[harness.claude-code.variant.work.settings]
variant = true
"#,
        );
        assert_eq!(
            config.reasoning_for("claude-code:work"),
            Some("variant-reasoning")
        );
        assert_eq!(config.bin_for("claude-code:work"), Some("variant-bin"));
        assert_eq!(
            config.allowed_tools_for("claude-code:work"),
            ["variant-allow"]
        );
        assert_eq!(
            config.denied_tools_for("claude-code:work"),
            ["variant-deny"]
        );
        assert_eq!(
            config.hooks_for("claude-code:work").unwrap()["variant"].as_bool(),
            Some(true)
        );
        assert_eq!(
            config.settings_for("claude-code:work").unwrap()["variant"].as_bool(),
            Some(true)
        );
        assert_eq!(
            config.env_for("claude-code:work"),
            [
                ("SHARED".to_string(), "top".to_string()),
                ("BASE_ONLY".to_string(), "yes".to_string()),
                ("SHARED".to_string(), "base".to_string()),
                ("SHARED".to_string(), "variant".to_string()),
            ]
        );
        assert_eq!(config.reasoning_for("claude-code"), Some("base-reasoning"));
        assert_eq!(config.bin_for("claude-code"), Some("base-bin"));
        assert_eq!(config.args_for("claude-code"), ["base-arg"]);
        assert_eq!(config.allowed_tools_for("claude-code"), ["top-allow"]);
        assert_eq!(config.denied_tools_for("claude-code"), ["top-deny"]);
        assert_eq!(
            config.hooks_for("claude-code").unwrap()["base"].as_bool(),
            Some(true)
        );
        assert_eq!(
            config.settings_for("claude-code").unwrap()["base"].as_bool(),
            Some(true)
        );
        assert!(config.variant_for("claude-code:missing").is_none());
    }

    #[test]
    fn malformed_variant_names_fail_loudly() {
        for name in ["-work", "work.personal", "work:personal"] {
            assert!(parse(&format!(
                "[harness.claude-code.variant.\"{name}\"]\nmodel = \"x\""
            ))
            .is_err());
        }
    }

    #[test]
    fn documented_variant_name_grammar_tracks_the_parser_constant() {
        assert!(include_str!("../../../../README.md").contains(VARIANT_NAME_PATTERN));
    }

    #[test]
    fn malformed_variant_boundaries_fail_at_parse_time() {
        let too_long = "a".repeat(65);
        assert!(parse(&format!(
            "[harness.claude-code.variant.{too_long}]\nmodel = \"x\""
        ))
        .unwrap_err()
        .to_string()
        .contains("invalid harness variant name"));
        for text in [
            "[harness.claude-code.variant.work]\nhooks = \"not-a-table\"",
            "[harness.claude-code.variant.work]\nsettings = [\"not-a-table\"]",
            "[harness.claude-code.variant.work]\nunset_env = [\"\"]",
            "[harness.claude-code.variant.work]\nunset_env = [\"BAD-NAME\"]",
            "[harness.claude-code.variant.work.env_from]\nANTHROPIC_API_KEY = \"\"",
        ] {
            assert!(parse(text).is_err(), "{text}");
        }
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
            mode = "plan"
            timeout = 90
            output_format = "stream-json"
            schema_file = "schema.json"
            schema_max_retries = 4
            max_parallel = 2
            run_mode = "fallback"
            require_available = true
            history = true
            history_dir = "/var/hist"

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
        assert_eq!(c.mode, Some(PermissionMode::Plan));
        assert_eq!(c.timeout, Some(90));
        assert_eq!(c.output_format, Some(OutputFormat::StreamJson));
        assert_eq!(c.schema_file.as_deref(), Some("schema.json"));
        assert_eq!(c.schema_max_retries, Some(4));
        assert_eq!(c.max_parallel, Some(2));
        assert_eq!(c.run_mode, Some(RunMode::Fallback));
        assert_eq!(c.require_available, Some(true));
        assert_eq!(c.history, Some(true));
        assert_eq!(c.history_dir.as_deref(), Some("/var/hist"));
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
    fn a_hook_targeting_a_variant_is_rejected() {
        let err =
            parse("[[hooks]]\ncommand = \"x\"\nharnesses = [\"claude-code:-bad\"]").unwrap_err();
        assert!(err.contains("accepts base harness ids"), "{err}");
        assert!(err.contains("claude-code:-bad"), "{err}");
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
            (
                "[harness.copilot.variant.work]\nallowed_tools = [\"x\"]",
                "allowed_tools",
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
    fn schema_fields_layer_and_explain_per_field() {
        // schema_file/schema_max_retries layer like any scalar, and `explain`
        // attributes each to its winning file.
        let merged = merge(
            parsed("schema_file = \"user.json\"\nschema_max_retries = 1"),
            parsed("schema_max_retries = 5"),
        );
        assert_eq!(merged.schema_file.as_deref(), Some("user.json"));
        assert_eq!(merged.schema_max_retries, Some(5));

        let report = explain(&layers(
            "schema_file = \"user.json\"",
            "schema_file = \"proj.json\"\nschema_max_retries = 3",
        ));
        assert_eq!(report.schema_file.value.as_deref(), Some("proj.json"));
        assert_eq!(report.schema_file.source.as_deref(), Some("/project.toml"));
        assert_eq!(report.schema_max_retries.value, Some(3));
        // Unset everywhere stays null (no built-in default).
        let report = explain(&[]);
        assert_eq!(report.schema_max_retries, Field::unset());
        assert_eq!(report.schema_file, Field::unset());
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
        assert_eq!(report.bypass, Field::default_value(false));
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

    /// Build a getter over a fixed set of name→value pairs, the pure stand-in
    /// for the process environment.
    fn env_get<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name: &str| {
            pairs
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn from_env_unset_contributes_no_layer() {
        assert_eq!(from_env(env_get(&[])).unwrap(), None);
        // An empty value counts as unset, like ONEHARNESS_CONFIG / _BIN_*.
        assert_eq!(
            from_env(env_get(&[("ONEHARNESS_MODEL", "")])).unwrap(),
            None
        );
    }

    #[test]
    fn from_env_maps_every_field_with_standard_names() {
        let c = from_env(env_get(&[
            ("ONEHARNESS_HARNESSES", "claude-code, codex"),
            ("ONEHARNESS_EXCLUDE", "cursor"),
            ("ONEHARNESS_MODEL", "haiku"),
            ("ONEHARNESS_SYSTEM", "be terse"),
            ("ONEHARNESS_REASONING", "high"),
            ("ONEHARNESS_BYPASS", "false"),
            ("ONEHARNESS_MODE", "plan"),
            ("ONEHARNESS_TIMEOUT", "90"),
            ("ONEHARNESS_OUTPUT_FORMAT", "stream-json"),
            ("ONEHARNESS_SCHEMA_FILE", "schema.json"),
            ("ONEHARNESS_SCHEMA_MAX_RETRIES", "4"),
            ("ONEHARNESS_MAX_PARALLEL", "2"),
            ("ONEHARNESS_RUN_MODE", "fallback"),
            ("ONEHARNESS_REQUIRE_AVAILABLE", "1"),
            ("ONEHARNESS_HISTORY", "true"),
            ("ONEHARNESS_HISTORY_DIR", "/var/hist"),
            ("ONEHARNESS_HISTORY_LABELS", "graph=release,task=test"),
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(c.harnesses.as_deref().unwrap(), ["claude-code", "codex"]);
        assert_eq!(c.exclude.as_deref().unwrap(), ["cursor"]);
        assert_eq!(c.model.as_deref(), Some("haiku"));
        assert_eq!(c.system.as_deref(), Some("be terse"));
        assert_eq!(c.reasoning.as_deref(), Some("high"));
        assert_eq!(c.bypass, Some(false));
        assert_eq!(c.mode, Some(PermissionMode::Plan));
        assert_eq!(c.timeout, Some(90));
        assert_eq!(c.output_format, Some(OutputFormat::StreamJson));
        assert_eq!(c.schema_file.as_deref(), Some("schema.json"));
        assert_eq!(c.schema_max_retries, Some(4));
        assert_eq!(c.max_parallel, Some(2));
        assert_eq!(c.run_mode, Some(RunMode::Fallback));
        assert_eq!(c.require_available, Some(true));
        assert_eq!(c.history, Some(true));
        assert_eq!(c.history_dir.as_deref(), Some("/var/hist"));
        assert_eq!(
            c.history_labels.unwrap().as_map(),
            &BTreeMap::from([
                ("graph".to_string(), "release".to_string()),
                ("task".to_string(), "test".to_string()),
            ])
        );
    }

    #[test]
    fn run_mode_layers_env_and_explains() {
        // Layers like any scalar: a project value beats the user one.
        let merged = merge(
            parsed("run_mode = \"parallel\""),
            parsed("run_mode = \"fallback\""),
        );
        assert_eq!(merged.run_mode, Some(RunMode::Fallback));

        // `explain` attributes it to the winning file; unset stays null (the
        // effective mode then defaults to `parallel` at run time).
        let report = explain(&layers(
            "run_mode = \"fallback\"",
            "run_mode = \"parallel\"",
        ));
        assert_eq!(report.run_mode.value, Some(RunMode::Parallel));
        assert_eq!(report.run_mode.source.as_deref(), Some("/project.toml"));
        assert_eq!(explain(&[]).run_mode, Field::unset());

        // The env layer parses the same tokens and rejects a bad one loudly.
        let c = from_env(env_get(&[("ONEHARNESS_RUN_MODE", "fallback")]))
            .unwrap()
            .unwrap();
        assert_eq!(c.run_mode, Some(RunMode::Fallback));
        let err = from_env(env_get(&[("ONEHARNESS_RUN_MODE", "nope")])).unwrap_err();
        assert!(err.contains("parallel"), "{err}");
    }

    #[test]
    fn models_list_layers_env_and_explains() {
        // The fan-out list parses, layers like any list (project beats user),
        // and coexists with the single-model `model` base.
        let c = parsed("model = \"base\"\nmodels = [\"opus\", \"sonnet\"]");
        assert_eq!(c.model.as_deref(), Some("base"));
        assert_eq!(c.models.as_deref().unwrap(), ["opus", "sonnet"]);

        let merged = merge(
            parsed("models = [\"user-a\", \"user-b\"]"),
            parsed("models = [\"proj\"]"),
        );
        assert_eq!(merged.models.as_deref().unwrap(), ["proj"]);

        // `explain` attributes the list to its winning file; unset stays null.
        let report = explain(&layers(
            "models = [\"user\"]",
            "models = [\"opus\", \"sonnet\"]",
        ));
        assert_eq!(report.models.value.as_deref().unwrap(), ["opus", "sonnet"]);
        assert_eq!(report.models.source.as_deref(), Some("/project.toml"));
        assert_eq!(explain(&[]).models, Field::unset());

        // The env layer reads a comma-separated list, trimming entries.
        let c = from_env(env_get(&[("ONEHARNESS_MODELS", "opus, sonnet")]))
            .unwrap()
            .unwrap();
        assert_eq!(c.models.as_deref().unwrap(), ["opus", "sonnet"]);
    }

    #[test]
    fn history_layers_and_explains_with_a_default() {
        // `history`/`history_dir` layer like any scalar; a project value beats
        // the user one and `explain` attributes it, defaulting `history` to false.
        let merged = merge(
            parsed("history = false\nhistory_dir = \"/user/h\""),
            parsed("history = true"),
        );
        assert_eq!(merged.history, Some(true));
        assert_eq!(merged.history_dir.as_deref(), Some("/user/h"));

        let report = explain(&layers(
            "history_dir = \"/user/h\"",
            "history = true\nhistory_dir = \"/proj/h\"",
        ));
        assert_eq!(report.history.value, Some(true));
        assert_eq!(report.history.source.as_deref(), Some("/project.toml"));
        assert_eq!(report.history_dir.value.as_deref(), Some("/proj/h"));
        assert_eq!(report.history_dir.source.as_deref(), Some("/project.toml"));
        // Unset everywhere: history defaults to false, dir stays null.
        let report = explain(&[]);
        assert_eq!(report.history, Field::default_value(false));
        assert_eq!(report.history_dir, Field::unset());
    }

    #[test]
    fn only_absolute_values_read_as_a_filesystem_identity() {
        // The home-directory indirections a variant maps are absolute; a
        // credential never is, so a secret is never probed on disk.
        let absolute = std::env::temp_dir().join("oneharness-identity");
        assert!(names_absolute_path(&absolute.display().to_string()));
        for opaque in [
            "sk-ant-api03-secret",
            "ghp_token",
            ".codex-alt",
            "relative/home",
            "",
        ] {
            assert!(!names_absolute_path(opaque), "{opaque} read as a path");
        }
    }

    #[test]
    fn stream_layers_from_file_and_environment_with_a_default() {
        // `stream` is an ordinary layered boolean: a project value beats the user
        // one, the environment beats both, and it defaults to off.
        let merged = merge(parsed("stream = false"), parsed("stream = true"));
        assert_eq!(merged.stream, Some(true));

        let report = explain(&layers("stream = true", "stream = false"));
        assert_eq!(report.stream.value, Some(false));
        assert_eq!(report.stream.source.as_deref(), Some("/project.toml"));
        assert_eq!(explain(&[]).stream, Field::default_value(false));

        let c = from_env(env_get(&[("ONEHARNESS_STREAM", "1")]))
            .unwrap()
            .unwrap();
        assert_eq!(c.stream, Some(true));
        assert!(from_env(env_get(&[("ONEHARNESS_STREAM", "yes")])).is_err());
    }

    #[test]
    fn history_labels_merge_by_key_and_validate_every_boundary() {
        let merged = merge(
            parsed("history_labels = { graph = \"release\", owner = \"config\" }"),
            parsed("history_labels = { owner = \"environment\", task = \"test\" }"),
        );
        assert_eq!(
            merged.history_labels.unwrap().as_map(),
            &BTreeMap::from([
                ("graph".to_string(), "release".to_string()),
                ("owner".to_string(), "environment".to_string()),
                ("task".to_string(), "test".to_string()),
            ])
        );

        let report = explain(&layers(
            "history_labels = { graph = \"release\", owner = \"user\" }",
            "history_labels = { owner = \"project\" }",
        ));
        assert_eq!(
            report.history_labels["graph"].value.as_deref(),
            Some("release")
        );
        assert_eq!(
            report.history_labels["graph"].source.as_deref(),
            Some("/user.toml")
        );
        assert_eq!(
            report.history_labels["owner"].value.as_deref(),
            Some("project")
        );
        assert_eq!(
            report.history_labels["owner"].source.as_deref(),
            Some("/project.toml")
        );

        assert!(parse("history_labels = { \"bad/key\" = \"value\" }").is_err());
        assert!(from_env(env_get(&[("ONEHARNESS_HISTORY_LABELS", "missing")])).is_err());
        assert!(from_env(env_get(&[("ONEHARNESS_HISTORY_LABELS", "key=")])).is_err());
    }

    #[test]
    fn from_env_all_true_selects_everything() {
        let c = from_env(env_get(&[("ONEHARNESS_ALL", "true")]))
            .unwrap()
            .unwrap();
        assert_eq!(c.all, Some(true));
    }

    #[test]
    fn from_env_rejects_malformed_values() {
        for (pairs, needle) in [
            (vec![("ONEHARNESS_BYPASS", "maybe")], "`true` or `false`"),
            (vec![("ONEHARNESS_TIMEOUT", "soon")], "non-negative integer"),
            (
                vec![("ONEHARNESS_MAX_PARALLEL", "-1")],
                "non-negative integer",
            ),
            (vec![("ONEHARNESS_OUTPUT_FORMAT", "yaml")], "text"),
            (vec![("ONEHARNESS_MODE", "yolo")], "bypass"),
        ] {
            let err = from_env(env_get(&pairs)).unwrap_err();
            assert!(err.contains(needle), "{pairs:?} -> {err}");
        }
    }

    #[test]
    fn mode_layers_and_explains_per_field() {
        // `mode` layers like any scalar; a project value beats the user one and
        // `explain` attributes it to the winning file. Unset stays null (the
        // effective mode then derives from `bypass`).
        let merged = merge(parsed("mode = \"plan\""), parsed("mode = \"bypass\""));
        assert_eq!(merged.mode, Some(PermissionMode::Bypass));
        let report = explain(&layers("mode = \"plan\"", "mode = \"edit\""));
        assert_eq!(report.mode.value, Some(PermissionMode::Edit));
        assert_eq!(report.mode.source.as_deref(), Some("/project.toml"));
        assert_eq!(explain(&[]).mode, Field::unset());
    }

    #[test]
    fn from_env_runs_the_same_validation_as_a_file() {
        // Unknown harness id and the all/harnesses conflict are rejected here too.
        let err = from_env(env_get(&[("ONEHARNESS_HARNESSES", "bogus")])).unwrap_err();
        assert!(err.contains("bogus"), "{err}");
        let err = from_env(env_get(&[
            ("ONEHARNESS_ALL", "true"),
            ("ONEHARNESS_HARNESSES", "codex"),
        ]))
        .unwrap_err();
        assert!(err.contains("mutually exclusive"), "{err}");
    }

    #[test]
    fn from_env_layer_explains_with_environment_source() {
        // The env layer flows through `explain` like any other, attributed to
        // ENV_SOURCE, and a later env layer beats an earlier file layer.
        let env = from_env(env_get(&[("ONEHARNESS_MODEL", "env-model")]))
            .unwrap()
            .unwrap();
        let report = explain(&[
            (
                "/project.toml".to_string(),
                parsed("model = \"file-model\""),
            ),
            (ENV_SOURCE.to_string(), env),
        ]);
        assert_eq!(report.model.value.as_deref(), Some("env-model"));
        assert_eq!(report.model.source.as_deref(), Some(ENV_SOURCE));
    }
}
