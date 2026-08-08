//! `oneharness list` — describe the supported harnesses as JSON.

use schemars::JsonSchema;
use serde::Serialize;

use crate::cli::ListArgs;
use crate::commands::print_json;
use oneharness_core::domain::harness::{self, BuildCtx};
use oneharness_core::domain::mode::{ModeHeadless, PermissionMode};
use oneharness_core::domain::report::OutputFormat;
use oneharness_core::errors::OneharnessError;
use oneharness_core::io::config as config_io;

/// One supported approval mode for a harness, with its headless behavior, in
/// `oneharness list`. A [`PermissionMode`] absent from a harness's array is
/// unsupported for it (a `--mode` request would be refused).
#[derive(JsonSchema, Serialize)]
pub struct ModeInfo {
    mode: PermissionMode,
    /// `"clean"` (never blocks headless) or `"hangs"` (would block on an
    /// approval prompt; refused without --permit-prompts).
    headless: ModeHeadless,
}

#[derive(JsonSchema, Serialize)]
pub struct HarnessInfo {
    id: &'static str,
    display: &'static str,
    default_bin: &'static str,
    install_hint: &'static str,
    output_format: OutputFormat,
    /// Whether `run --resume <session>` is supported for this harness.
    supports_resume: bool,
    /// Whether `run --session <name>` is supported — a uniform, caller-owned
    /// handle oneharness maps to the harness's native session id. `true` only for
    /// harnesses that expose a session id headlessly; `false` means `--session` is
    /// a loud usage error (there is no id to bind a name to).
    session_capable: bool,
    /// Whether `run --resume <session> --fork` is supported — branching a new
    /// session from the resumed one. `false` means it resumes linearly only.
    supports_fork: bool,
    /// How `run --control` + `oneharness interrupt` abort an in-flight turn on
    /// this harness (the mechanism id), or `null` when it has no out-of-band
    /// turn control at all — in which case `--control` is a loud usage error for
    /// it, never a socket that reports success while the turn keeps running.
    control: Option<&'static str>,
    /// Whether a forked run reuses the parent session's prompt-cache prefix, so a
    /// fork-based `--batch-strategy min-tokens` run actually reduces tokens. `true`
    /// only for Claude Code today; `false` (incl. OpenCode, whose fork re-sends the
    /// prefix cold) means `min-tokens` only orders the calls (no saving).
    fork_reuses_cache: bool,
    /// The approval modes (`--mode`) this harness can express, each with its
    /// headless behavior. Modes not listed are unsupported for the harness.
    modes: Vec<ModeInfo>,
    /// Whether `run --schema` is delivered through a native structured-output
    /// flag for this harness (Claude Code's `--json-schema`). `false` means the
    /// portable prompt-based path is used — structured output works either way;
    /// oneharness always validates and retries.
    supports_native_schema: bool,
    /// Whether `run --reasoning <effort>` can be delivered on the argv for this
    /// harness (Claude Code's `--effort`, Codex's `model_reasoning_effort`).
    /// `false` means it has no headless reasoning flag — a reasoning request is
    /// then a loud usage error (effort is provider/model config there).
    supports_reasoning: bool,
    /// The project-scoped config file `oneharness sync` writes for this
    /// harness; `null` when it has none (sync settings are then rejected).
    sync_file: Option<&'static str>,
    /// Whether the unified allow/deny rule lists and hooks table can be synced
    /// into that file (see the README support matrix).
    supports_allowed_tools: bool,
    supports_denied_tools: bool,
    supports_hooks: bool,
    /// Whether `oneharness mock <id>` can express a pre-tool *deny* for this
    /// harness (the same protocol `oneharness gate` speaks).
    supports_mock_deny: bool,
    /// The input-rewrite verdict shape `oneharness mock <id>` speaks for this
    /// harness (`claude-nested`, `crush-flat`, `opencode-shim`); `null` when
    /// the harness has no verified rewrite — a rewrite rule for it is then a
    /// loud usage error (see the README mock support matrix).
    mock_rewrite: Option<&'static str>,
    /// Whether a large user prompt can be delivered off the argv (piped to the
    /// harness's stdin) so it never trips the OS argument ceiling (`E2BIG`).
    supports_prompt_stdin: bool,
    /// Whether a large system prompt can be delivered off the argv via a file
    /// flag (Claude Code's `--append-system-prompt-file`). `false` does not mean a
    /// large system is unhandled — for a harness whose system rides the prompt it
    /// travels on stdin with the prompt; see the README large-prompt matrix.
    supports_system_file: bool,
    /// The argv oneharness would build, with placeholders, so the adapter's
    /// shape is visible without running anything.
    example_command: Vec<String>,
    variants: Vec<VariantInfo>,
}

#[derive(JsonSchema, Serialize)]
pub struct VariantInfo {
    // llmlint: ignore[invalid_states_unrepresentable] The JSON/SDK field is a string sourced exclusively from validated VariantName map keys, not arbitrary caller input.
    name: String,
    // llmlint: ignore[invalid_states_unrepresentable] This selector is constructed only from a registry base id plus that validated VariantName, and integration assertions pin the composition.
    harness_id: String,
    model: Option<String>,
    bin: Option<String>,
    args: Vec<String>,
    // llmlint: ignore[invalid_states_unrepresentable] This read-only JSON/SDK projection is populated only from a FileConfig that has passed environment-name validation; strings preserve the public list contract without duplicating the input boundary's validated representation.
    env_keys: Vec<String>,
    // llmlint: ignore[invalid_states_unrepresentable] This is the portable display value of an already-validated config path, not a path used for I/O; retaining a string preserves the established JSON/SDK contract.
    env_file: Option<String>,
    // llmlint: ignore[invalid_states_unrepresentable] Both sides are serialized environment names copied only after FileConfig validation; introducing output-only newtypes would duplicate the validated input boundary and change generated SDK types.
    env_from: std::collections::BTreeMap<String, String>,
    // llmlint: ignore[invalid_states_unrepresentable] These serialized names come only from the validated config and are never accepted back as process inputs through this DTO; strings are the intentional stable output shape.
    unset_env: Vec<String>,
}

#[derive(JsonSchema, Serialize)]
pub struct ListReport {
    schema_version: &'static str,
    harnesses: Vec<HarnessInfo>,
}

pub fn run(args: &ListArgs) -> Result<i32, OneharnessError> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let loaded = config_io::load(None, false, &cwd)?;
    let cfg = &loaded.config;
    let harnesses = harness::all()
        .iter()
        .map(|spec| {
            let ctx = BuildCtx {
                bin: spec.default_bin,
                prompt: "<PROMPT>",
                model: None,
                system: None,
                resume: None,
                fork: false,
                mode: PermissionMode::Bypass,
                output_format: spec.output_format,
                schema: None,
                system_file: None,
                delivery: harness::PromptDelivery::Argv,
            };
            let sync = spec.sync.as_ref();
            HarnessInfo {
                id: spec.id,
                display: spec.display,
                default_bin: spec.default_bin,
                install_hint: spec.install_hint,
                output_format: spec.output_format,
                supports_resume: spec.supports_resume,
                session_capable: spec.session_capable(),
                supports_fork: spec.supports_fork,
                control: spec.control.map(|shape| shape.as_str()),
                fork_reuses_cache: spec.fork_reuses_cache,
                modes: spec
                    .modes
                    .iter()
                    .map(|m| ModeInfo {
                        mode: m.mode,
                        headless: m.headless,
                    })
                    .collect(),
                supports_native_schema: spec.native_schema.is_some(),
                supports_reasoning: spec.reasoning.is_some(),
                sync_file: sync.map(|s| s.file),
                supports_allowed_tools: sync.is_some_and(|s| s.allow_path.is_some()),
                supports_denied_tools: sync.is_some_and(|s| s.deny_path.is_some()),
                supports_hooks: sync.is_some_and(|s| s.hooks_path.is_some()),
                supports_mock_deny: spec.gate_deny.is_some(),
                mock_rewrite: spec.mock_rewrite.map(|s| s.as_str()),
                supports_prompt_stdin: spec.large_input.prompt_stdin,
                supports_system_file: spec.large_input.system_file_flag.is_some(),
                example_command: (spec.build_argv)(&ctx),
                variants: cfg
                    .harness
                    .get(spec.id)
                    .into_iter()
                    .flat_map(|harness| &harness.variant)
                    .map(|(name, variant)| {
                        let id = format!("{}:{name}", spec.id);
                        VariantInfo {
                            name: name.to_string(),
                            harness_id: id.clone(),
                            model: cfg.model_for(&id).map(str::to_string),
                            bin: cfg.bin_for(&id).map(str::to_string),
                            args: cfg.args_for(&id).to_vec(),
                            env_keys: variant.env.keys().cloned().collect(),
                            env_file: variant.env_file.clone(),
                            env_from: variant.env_from.clone(),
                            unset_env: variant.unset_env.clone(),
                        }
                    })
                    .collect(),
            }
        })
        .collect();

    let report = ListReport {
        schema_version: oneharness_core::domain::report::SCHEMA_VERSION,
        harnesses,
    };
    print_json(&report, args.compact)?;
    Ok(0)
}
