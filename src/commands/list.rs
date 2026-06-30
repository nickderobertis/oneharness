//! `oneharness list` — describe the supported harnesses as JSON.

use serde::Serialize;

use crate::cli::ListArgs;
use crate::commands::print_json;
use oneharness_core::domain::harness::{self, BuildCtx};
use oneharness_core::domain::mode::{ModeHeadless, PermissionMode};
use oneharness_core::domain::report::OutputFormat;
use oneharness_core::errors::OneharnessError;

/// One supported approval mode for a harness, with its headless behavior, in
/// `oneharness list`. A [`PermissionMode`] absent from a harness's array is
/// unsupported for it (a `--mode` request would be refused).
#[derive(Serialize)]
struct ModeInfo {
    mode: &'static str,
    /// `"clean"` (never blocks headless) or `"hangs"` (would block on an
    /// approval prompt; refused without --permit-prompts).
    headless: &'static str,
}

#[derive(Serialize)]
struct HarnessInfo {
    id: &'static str,
    display: &'static str,
    default_bin: &'static str,
    install_hint: &'static str,
    output_format: OutputFormat,
    /// Whether `run --resume <session>` is supported for this harness.
    supports_resume: bool,
    /// Whether `run --resume <session> --fork` is supported — branching a new
    /// session from the resumed one. `false` means it resumes linearly only.
    supports_fork: bool,
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
    /// The project-scoped config file `oneharness sync` writes for this
    /// harness; `null` when it has none (sync settings are then rejected).
    sync_file: Option<&'static str>,
    /// Whether the unified allow/deny rule lists and hooks table can be synced
    /// into that file (see the README support matrix).
    supports_allowed_tools: bool,
    supports_denied_tools: bool,
    supports_hooks: bool,
    /// The argv oneharness would build, with placeholders, so the adapter's
    /// shape is visible without running anything.
    example_command: Vec<String>,
}

#[derive(Serialize)]
struct ListReport {
    schema_version: &'static str,
    harnesses: Vec<HarnessInfo>,
}

pub fn run(args: &ListArgs) -> Result<i32, OneharnessError> {
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
            };
            let sync = spec.sync.as_ref();
            HarnessInfo {
                id: spec.id,
                display: spec.display,
                default_bin: spec.default_bin,
                install_hint: spec.install_hint,
                output_format: spec.output_format,
                supports_resume: spec.supports_resume,
                supports_fork: spec.supports_fork,
                fork_reuses_cache: spec.fork_reuses_cache,
                modes: spec
                    .modes
                    .iter()
                    .map(|m| ModeInfo {
                        mode: m.mode.as_str(),
                        headless: match m.headless {
                            ModeHeadless::Clean => "clean",
                            ModeHeadless::Hangs => "hangs",
                        },
                    })
                    .collect(),
                supports_native_schema: spec.native_schema.is_some(),
                sync_file: sync.map(|s| s.file),
                supports_allowed_tools: sync.is_some_and(|s| s.allow_path.is_some()),
                supports_denied_tools: sync.is_some_and(|s| s.deny_path.is_some()),
                supports_hooks: sync.is_some_and(|s| s.hooks_path.is_some()),
                example_command: (spec.build_argv)(&ctx),
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
