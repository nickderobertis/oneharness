//! The harness registry: one declarative adapter per supported CLI.
//!
//! An adapter is data — a canonical id, a default binary, an install hint, an
//! output format — plus one pure function that builds the argv. Adding a harness
//! is adding an entry here; `run`, the runner, and the report shape are untouched.
//!
//! The flags encoded below mirror the known-good non-interactive invocations used
//! to drive each real CLI headlessly (deny prompts, pick the model, request a
//! parseable format). Source new flags from a working driver, not by guessing.

use crate::domain::gate::DenyShape;
use crate::domain::hooks::HookShape;
use crate::domain::mock::{MockDelivery, RewriteShape};
use crate::domain::mode::{ModeHeadless, PermissionMode};
use crate::domain::report::OutputFormat;
use crate::domain::structured::NativeSchema;

/// Everything `build_argv` needs, with no I/O: the resolved binary, the prompt,
/// the optional model, whether to request the harness's "don't prompt" mode, and
/// the effective output format (the harness default, or a `--output-format`
/// override) for harnesses that take a format flag.
pub struct BuildCtx<'a> {
    pub bin: &'a str,
    pub prompt: &'a str,
    pub model: Option<&'a str>,
    /// System prompt to apply. Adapters with a native system flag map it (Claude
    /// Code's `--append-system-prompt`, Goose's `--system`); adapters without one
    /// prepend it to the prompt via `prompt_with_system` so the instructions
    /// still reach the model, rather than dropping it.
    pub system: Option<&'a str>,
    /// Session id to continue, for harnesses that support resumption. Only set
    /// after the command layer has verified the selected harness's
    /// `supports_resume`, so an adapter that maps it can assume support.
    pub resume: Option<&'a str>,
    /// When resuming, branch a *new* session from the resumed one instead of
    /// appending to it — so the original (and its cached prefix) is untouched and
    /// can seed independent follow-ups. Only honored alongside `resume`, and only
    /// set after the command layer has verified `supports_fork`; an adapter maps it
    /// to its native fork flag (Claude Code's `--fork-session`, OpenCode's
    /// `--fork`). Ignored by adapters that cannot fork (they are never selected
    /// with it).
    pub fork: bool,
    /// The normalized approval mode to request. Each adapter maps it to its
    /// harness's native mechanism (argv flags here; any environment via the
    /// matching [`ModeSpec`]). The command layer guarantees the selected harness
    /// actually supports `mode` before calling `build_argv`, so an adapter only
    /// needs correct output for the modes in its [`HarnessSpec::modes`].
    pub mode: PermissionMode,
    pub output_format: OutputFormat,
    /// Inline JSON-Schema text to deliver through the harness's *native*
    /// structured-output flag, set only for an adapter with a
    /// [`HarnessSpec::native_schema`] when a schema run is requested. Adapters
    /// without native support ignore it — the command layer instead appends the
    /// schema instruction to the prompt — so it is never silently dropped.
    pub schema: Option<&'a str>,
}

/// The CLI token for a format, as the harnesses spell it.
fn format_flag(format: OutputFormat) -> &'static str {
    match format {
        OutputFormat::Text => "text",
        OutputFormat::Json => "json",
        OutputFormat::StreamJson => "stream-json",
    }
}

/// The prompt an adapter should send, with the system instructions prepended when
/// the harness has no native system flag. This is how `--system` reaches models
/// on harnesses like Codex/OpenCode that expose no system-prompt option — without
/// it the instructions would be silently dropped. A blank system prompt is a
/// no-op. Adapters with a native flag (claude-code, goose) pass `c.prompt`
/// directly and map `c.system` separately instead of calling this.
fn prompt_with_system(c: &BuildCtx) -> String {
    match c.system {
        Some(s) if !s.is_empty() => format!("{s}\n\n{}", c.prompt),
        _ => c.prompt.to_string(),
    }
}

/// A single harness adapter.
pub struct HarnessSpec {
    /// Canonical id used on the CLI and in JSON (e.g. `claude-code`).
    pub id: &'static str,
    /// Human-friendly name for `list`.
    pub display: &'static str,
    /// Binary name looked up on PATH unless overridden.
    pub default_bin: &'static str,
    /// How a user installs the CLI (shown when it is missing).
    pub install_hint: &'static str,
    /// The format the adapter requests, which drives text extraction.
    pub output_format: OutputFormat,
    /// The format to switch this harness to when the caller asks for tool
    /// **events** (`--events` / `--stream`) and its *default* format carries no
    /// machine-readable tool transcript — so `events` can be surfaced without the
    /// caller knowing each CLI's quirk. `None` means either the default already
    /// carries a transcript (OpenCode's `json`, Cursor's `stream-json` — no
    /// upgrade needed) or the harness exposes no events-capable format at all
    /// (the plain-text harnesses); in both cases `--events` leaves the format
    /// unchanged. When `Some`, the command layer selects it under `--events`
    /// unless the caller set an explicit `--output-format`. Claude Code needs
    /// `stream-json` (its default single-document `json` result omits the
    /// transcript). Sourced from each CLI's real output, never guessed; text/usage
    /// extraction must still work under the upgraded format (verified live).
    pub events_format: Option<OutputFormat>,
    /// Whether this harness can continue a prior session (`run --resume`). When
    /// false, the command layer rejects `--resume` for it rather than silently
    /// starting a fresh session. Kept as data so the capability is introspectable
    /// via `oneharness list`.
    pub supports_resume: bool,
    /// Whether this harness can *fork* a session when resuming (`run --resume
    /// <id> --fork`): branch a new session id from the resumed one, leaving the
    /// original untouched so its cached prefix seeds independent follow-ups. Only
    /// two CLIs expose a headless fork flag (Claude Code's `--fork-session`,
    /// OpenCode's `--fork`); the rest resume linearly (append in place). When
    /// false, `--fork` is a loud usage error for the harness, never a silent
    /// linear resume. Implies `supports_resume`. Introspectable via `oneharness
    /// list`.
    pub supports_fork: bool,
    /// Whether a forked run *reuses* the parent session's provider prompt-cache
    /// prefix — so a fork-based `min-tokens` batch (warm one prompt, fork the
    /// rest) actually *reduces* tokens. Implies `supports_fork`. This is the gate
    /// for the fork-based batch path: when false, `min-tokens` only orders the
    /// calls (no token saving) rather than forking. Measured by the live e2e
    /// (`oh_batch_fork_enforce`), never guessed: **true for Claude Code** (Anthropic
    /// prompt caching, and `--fork-session` preserves the cached session prefix);
    /// **false for OpenCode**, whose `--fork` re-sends the branched conversation
    /// cold (the fan-out reads nothing and re-writes the whole prefix — measured,
    /// so forking it would *raise* tokens, not lower them). Introspectable via
    /// `oneharness list`.
    pub fork_reuses_cache: bool,
    /// Where this harness reads project-scoped configuration, and how the
    /// unified enforcement settings (`allowed_tools` / `denied_tools` /
    /// `hooks` / `settings`) map into that file. `None` means the harness has
    /// no project-level config file oneharness knows how to write (Codex and
    /// Goose read only user-global config; Copilot takes permission rules as
    /// flags, deliverable via `[harness.copilot] args`) — configuring a sync
    /// setting for it is then a loud usage error, never a silent no-op.
    /// Consumed by `oneharness sync`; nothing here is passed on the argv.
    pub sync: Option<SyncSpec>,
    /// How a normalized pre-tool [`crate::domain::hooks::HookSpec`] is installed
    /// into this harness — the shape its config expects and where the file lands
    /// (a shared config file, a dedicated hooks file, or a plugin). `None` for a
    /// harness oneharness cannot wire a hook into. Consumed by `oneharness sync`
    /// / `src/io/hooks.rs`; nothing here is passed on the argv.
    pub hooks: Option<HookBinding>,
    /// Where this harness reads a *user-global* hook, for an `io::hooks::install`
    /// at [`crate::io::hooks::Scope::Global`] (the project path lives in
    /// [`HookBinding`]). The install strategy is identical to the project one;
    /// only the anchor moves to a `$HOME`/`$XDG_CONFIG_HOME` location. `None` for
    /// a harness with no user-global hook location oneharness knows. Sourced from
    /// the allowlister adapters, never guessed.
    pub global_hook: Option<GlobalHook>,
    /// How this harness expresses a pre-tool *deny* when its installed hook runs
    /// `oneharness gate <id>` — the runtime counterpart to [`HookBinding`]. `None`
    /// for a harness with no gateable pre-tool hook. Consumed by the `gate`
    /// command (`src/commands/gate.rs`); sourced from the allowlister adapters.
    pub gate_deny: Option<DenyShape>,
    /// How this harness expresses a pre-tool *input rewrite* when its installed
    /// hook runs `oneharness mock <id>` — allow the call with substituted
    /// arguments, the mock workhorse (swap a shell command for a stub that
    /// prints canned output). `None` for a harness whose hook protocol has no
    /// rewrite verdict (Goose), or whose support is not yet verified through
    /// `oneharness run` (Codex, Copilot, Cursor — pending the `explore-hooks`
    /// probe; see `docs/mock-spy-design.md`): a rewrite rule for it is then a
    /// loud usage error, never a silent allow. Sourced from each CLI's hook
    /// docs; honored-live is drift-alarmed by the `oh_mock_enforce` e2e phases.
    pub mock_rewrite: Option<RewriteShape>,
    /// How `run --mock-rules`/`run --spy-file` delivers the mock hook for ONE
    /// invocation (see [`MockDelivery`]): Claude Code takes it on the argv via
    /// `--settings` (zero workspace mutation); the rest get a project-scope
    /// install that is snapshotted and restored around the run, layering on top
    /// of any existing config non-destructively. `None` for a harness whose
    /// hooks cannot fire from a project-scope headless run (qwen: user scope
    /// only; copilot: probe-refuted entirely) — the flag is then a loud usage
    /// error, never a silently inert install.
    pub mock_delivery: Option<MockDelivery>,
    /// Environment variables oneharness sets when spawning this harness, so a
    /// headless run is clean without the caller knowing the harness's quirks
    /// (e.g. silencing a startup warning that would otherwise litter `stderr`).
    /// Pure data: the registry declares them; the command/io layer injects them,
    /// and an explicit `--env` always wins over a default here. Empty for most.
    pub default_env: &'static [(&'static str, &'static str)],
    /// How this harness accepts a JSON Schema *natively*, when it does — the
    /// schema is delivered through its own CLI flag and the conforming value
    /// read from a known field, rather than appended to the prompt. `None` means
    /// structured-output runs fall back to the portable prompt-based path (which
    /// works for every harness). Either way oneharness validates the result
    /// itself, so a native flag the harness ignores is still caught.
    pub native_schema: Option<NativeSchema>,
    /// The approval modes this harness can express, each with how it behaves in
    /// a headless run. A [`PermissionMode`] absent from this list is unsupported
    /// for the harness — the command layer turns a request for it into a loud
    /// usage error rather than silently downgrading. Every harness lists
    /// [`PermissionMode::Bypass`] (the headless default) and
    /// [`PermissionMode::Default`]. Sourced from each CLI's docs/behavior, never
    /// guessed (see the README support matrix and `AGENTS.md`).
    pub modes: &'static [ModeSpec],
    /// Builds the full argv (argv[0] is the binary). Pure.
    pub build_argv: fn(&BuildCtx) -> Vec<String>,
}

impl HarnessSpec {
    /// The [`ModeSpec`] for `mode`, or `None` when this harness cannot express
    /// it. The lookup the command layer uses to gate a run and to inject any
    /// per-mode environment.
    pub fn mode(&self, mode: PermissionMode) -> Option<&'static ModeSpec> {
        self.modes.iter().find(|m| m.mode == mode)
    }
}

/// One approval mode a harness supports: its headless behavior and any
/// environment that delivers it. Most modes are expressed on the argv by
/// `build_argv`; a few harnesses (Goose) carry the mode in the environment
/// instead, declared here so the command layer injects it when spawning.
pub struct ModeSpec {
    pub mode: PermissionMode,
    /// Whether this mode blocks on an interactive prompt headlessly.
    pub headless: ModeHeadless,
    /// Environment variables that select this mode (Goose's `GOOSE_MODE`,
    /// OpenCode's `OPENCODE_CONFIG_CONTENT`). Empty when `build_argv` expresses
    /// the mode on the argv. Injected like the harness's `default_env` (so a
    /// config / `--env` value still wins).
    pub env: &'static [(&'static str, &'static str)],
    /// A per-run instruction prepended to the prompt to induce a *behavioral*
    /// mode the harness can't express natively, paired with the enforcement that
    /// `build_argv`/`env` provides. Used for Codex's `plan`: the read-only
    /// sandbox enforces no-mutation and this instruction induces the planning
    /// behavior (mirroring Codex's own interactive Plan-mode template). `None`
    /// for modes a harness expresses natively (their own plan/agent mode already
    /// carries the behavior). Prepended by the command layer; kept single-line.
    pub instruction: Option<&'static str>,
}

/// Shorthand for an argv-expressed mode (no environment, no instruction): the
/// common case.
const fn mode(mode: PermissionMode, headless: ModeHeadless) -> ModeSpec {
    ModeSpec {
        mode,
        headless,
        env: &[],
        instruction: None,
    }
}

/// A harness's project-scoped config file and the key paths the unified
/// settings merge into. All paths were sourced from each CLI's documentation —
/// never guessed (see the README support matrix for the references).
pub struct SyncSpec {
    /// Project-relative path of the config file to create or merge into.
    pub file: &'static str,
    /// Alternative file names the harness reads with *higher* precedence;
    /// when one exists, oneharness merges into it instead of `file` so it
    /// never creates a second, shadowed config (e.g. crush's `.crush.json`).
    pub alt_files: &'static [&'static str],
    /// Key path `allowed_tools` rules land at (e.g. `permissions.allow`);
    /// `None` rejects the unified list field for this harness (OpenCode's
    /// `permission` is a policy map, not a list — use `settings` instead).
    pub allow_path: Option<&'static [&'static str]>,
    /// Key path `denied_tools` rules land at. For crush this is
    /// `options.disabled_tools`: the tool is hidden from the agent entirely,
    /// the strongest deny it offers.
    pub deny_path: Option<&'static [&'static str]>,
    /// Key path the `hooks` table lands at (Claude Code's top-level `hooks`).
    pub hooks_path: Option<&'static [&'static str]>,
    /// JSON merged *beneath* any top-level key the fragment touches, so a
    /// partial write still satisfies the harness's schema. Cursor's
    /// `.cursor/cli.json` requires `permissions.allow` and `permissions.deny`
    /// to both exist whenever `permissions` does (its CLI rejects the file
    /// otherwise — caught by the live e2e), so writing only an allow list
    /// must seed an empty deny. Keys the fragment doesn't touch are never
    /// seeded, preserving the "only keys oneharness manages" contract.
    pub schema_seed: Option<&'static str>,
}

/// How a normalized hook reaches one harness. The [`HookShape`] (where present)
/// is the JSON layout [`crate::domain::hooks::render`] produces; the variant is
/// where that JSON — or, for OpenCode, a JS shim — is written. All eight
/// harnesses gate the same pre-tool moment; only the file and the shape differ.
pub enum HookBinding {
    /// Merge the rendered hook under `path` in the harness's *existing* config
    /// file (`SyncSpec.file`/`alt_files`) — hooks share the permissions file
    /// (Claude Code, Qwen, Crush).
    SameFile {
        shape: HookShape,
        path: &'static [&'static str],
    },
    /// Merge into a *dedicated* JSON file, created (seeded with `seed`) if
    /// absent. `file` may contain `{name}` for the plugin identity (Copilot's
    /// per-owner file). Codex, Cursor, Copilot.
    File {
        shape: HookShape,
        file: &'static str,
        path: &'static [&'static str],
        seed: Option<&'static str>,
    },
    /// Goose plugin: a one-time `<plugins_dir>/<name>/plugin.json` manifest plus
    /// the hook merged under `path` in `<plugins_dir>/<name>/hooks/hooks.json`.
    GoosePlugin {
        shape: HookShape,
        plugins_dir: &'static str,
        manifest: &'static str,
        path: &'static [&'static str],
    },
    /// OpenCode can only block from an in-process plugin, so the hook is a JS
    /// shim at `<plugin_dir>/<name>.js` that bridges to the command; rendered
    /// from `template` (not a JSON shape).
    JsPlugin {
        plugin_dir: &'static str,
        template: &'static str,
    },
}

/// The base directory a user-global hook anchors under. Resolved by the I/O
/// layer from the environment; kept abstract here so the registry stays pure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookBase {
    /// `$HOME`.
    Home,
    /// `$XDG_CONFIG_HOME`, falling back to `$HOME/.config` (Crush, OpenCode).
    ConfigHome,
}

/// Where a harness reads a *user-global* hook. The install strategy is the
/// matching [`HookBinding`] variant — only the anchor differs, because several
/// harnesses place the global hook at a different relative path than the project
/// one (Copilot's `.github/hooks` becomes `~/.copilot/hooks`; Crush and OpenCode
/// move under the XDG config dir). `{name}` is the plugin identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlobalHook {
    pub base: HookBase,
    /// Anchor relative to `base`: the settings/hooks file for the JSON-merge
    /// strategies, the `.js` shim for OpenCode, or the plugin directory for
    /// Goose — i.e. the same thing the [`HookBinding`] anchors at under a project.
    pub anchor: &'static str,
}

/// The plan instruction synthesized for Codex, whose `exec` has no native plan
/// mode: the read-only sandbox (`build_argv`) enforces no-mutation and this
/// induces the planning behavior, together reproducing Codex's own interactive
/// Plan mode (which is exactly a read-only sandbox + a plan template). Mirrors
/// the semantics of that template (`codex-rs/.../templates/plan.md`). Kept on a
/// single line — the command layer prepends it to the prompt.
const CODEX_PLAN_INSTRUCTION: &str = "PLAN MODE: research the task and produce an implementation plan only — do not edit or create files and do not run mutating commands (reading and searching files, configs, and docs is allowed); even if asked to execute, treat it as a request to plan the execution. Reply with a short intent paragraph, explicit in-scope vs out-of-scope, then a 6-10 item ordered checklist (discovery, changes, tests, rollout). The task to plan:";

/// Goose plugin manifest, `{name}` filled with the plugin identity. Written once
/// and preserved; the merge is idempotent so re-syncing changes nothing.
const GOOSE_MANIFEST: &str = r#"{
  "name": "{name}",
  "version": "0.1.0",
  "description": "Pre-tool hook installed by oneharness."
}"#;

/// OpenCode plugin shim. `{export}` is a JS-identifier-safe plugin name,
/// `{argv}` the command as a JSON argv array, `{name}` the display name. It
/// spawns the command before every tool call, pipes `{tool_name, tool_input,
/// cwd, session_id}` on stdin (the camelCase `input.sessionID` normalized to
/// snake_case, omitted when absent), throws to block on a
/// `{"decision":"deny"}` reply, and merges a reply's `updated_input` object
/// into the tool's mutable `args` (OpenCode's documented pre-execution
/// mutation point — how `oneharness mock` rewrites a call) — failing open if
/// the command cannot run. Mirrors the known-good allowlister shim; pinned in
/// the `io::hooks` tests.
const OPENCODE_PLUGIN_JS: &str = r#"// {name} OpenCode plugin — installed by oneharness.
//
// OpenCode can block a tool call only from an in-process plugin, so this shim
// bridges to an external command: before any tool runs it spawns the command,
// piping the tool name and arguments as JSON on stdin, throws to block when
// the command replies with {"decision":"deny"}, and applies a reply's
// {"updated_input":{...}} by merging it into the tool's mutable args (an input
// rewrite, for `oneharness mock`). Re-running sync regenerates it.

export const {export} = async ({ directory }) => ({
  "tool.execute.before": async (input, output) => {
    const tool_name = (input && input.tool) || "";
    if (!tool_name) return;
    const args = (output && output.args) || {};
    const cwd = args.workdir || directory || ".";
    const session_id = (input && input.sessionID) || undefined;
    const event = JSON.stringify({ tool_name, tool_input: args, cwd, session_id });

    let stdout = "";
    try {
      const proc = Bun.spawn({argv}, {
        cwd,
        stdin: new TextEncoder().encode(event),
        stdout: "pipe",
        stderr: "ignore",
      });
      stdout = await new Response(proc.stdout).text();
      await proc.exited;
    } catch (_) {
      return; // fail open: if the gate command cannot run, never block
    }

    const trimmed = stdout.trim();
    if (trimmed.length === 0) return; // no objection — let it run
    let decision;
    try {
      decision = JSON.parse(trimmed);
    } catch (_) {
      return; // unparseable output: fail open
    }
    if (decision && decision.decision === "deny") {
      throw new Error(decision.reason || "Blocked by {name}");
    }
    if (
      decision &&
      decision.updated_input &&
      typeof decision.updated_input === "object" &&
      !Array.isArray(decision.updated_input) &&
      output &&
      output.args
    ) {
      Object.assign(output.args, decision.updated_input);
    }
  },
});
"#;

/// All supported harnesses, in a stable order.
pub fn all() -> &'static [HarnessSpec] {
    REGISTRY
}

/// Look up a harness by its canonical id.
pub fn by_id(id: &str) -> Option<&'static HarnessSpec> {
    REGISTRY.iter().find(|h| h.id == id)
}

/// Comma-joined list of valid ids, for error messages and help.
pub fn valid_ids() -> String {
    REGISTRY.iter().map(|h| h.id).collect::<Vec<_>>().join(", ")
}

static REGISTRY: &[HarnessSpec] = &[
    HarnessSpec {
        id: "claude-code",
        display: "Claude Code",
        default_bin: "claude",
        install_hint: "npm install -g @anthropic-ai/claude-code",
        output_format: OutputFormat::Json,
        // The default `json` result carries no transcript; `stream-json` emits the
        // Anthropic content-block stream oneharness normalizes into `events`.
        events_format: Some(OutputFormat::StreamJson),
        supports_resume: true,
        supports_fork: true,
        fork_reuses_cache: true,
        sync: Some(SyncSpec {
            file: ".claude/settings.json",
            alt_files: &[],
            allow_path: Some(&["permissions", "allow"]),
            deny_path: Some(&["permissions", "deny"]),
            hooks_path: Some(&["hooks"]),
            schema_seed: None,
        }),
        hooks: Some(HookBinding::SameFile {
            shape: HookShape::Nested {
                event: "PreToolUse",
                // Claude Code's PreToolUse hook object accepts a per-hook
                // `timeout` (sourced from the allowlister adapter, which sets
                // one); emit it when a `[[hooks]]` entry provides one.
                with_timeout: true,
            },
            path: &["hooks"],
        }),
        global_hook: Some(GlobalHook {
            base: HookBase::Home,
            anchor: ".claude/settings.json",
        }),
        gate_deny: Some(DenyShape::ClaudeNested),
        // PreToolUse output: `permissionDecision: "allow"` + `updatedInput`
        // (the documented input-rewrite verdict; docs/mock-spy-design.md).
        mock_rewrite: Some(RewriteShape::ClaudeNested),
        // Probe-verified: hooks load from a per-run `--settings <file>` in `-p`
        // mode — the zero-mutation delivery (existing config still applies).
        mock_delivery: Some(MockDelivery::SettingsFlag { flag: "--settings" }),
        default_env: &[],
        native_schema: Some(NativeSchema::ClaudeJsonSchema),
        // `--permission-mode` covers the whole spectrum, all honored under `-p`.
        // `default` maps to `dontAsk` (deny-and-continue), not `default` (which
        // aborts on an un-allowed tool), so the ask flow never hangs headless.
        // `read-only` is `bypassPermissions` with the mutating tools denied (deny
        // rules win even under bypass), distinct from `plan`'s plan workflow.
        modes: &[
            mode(PermissionMode::ReadOnly, ModeHeadless::Clean),
            mode(PermissionMode::Plan, ModeHeadless::Clean),
            mode(PermissionMode::Default, ModeHeadless::Clean),
            mode(PermissionMode::Edit, ModeHeadless::Clean),
            mode(PermissionMode::Auto, ModeHeadless::Clean),
            mode(PermissionMode::Bypass, ModeHeadless::Clean),
        ],
        build_argv: argv_claude_code,
    },
    HarnessSpec {
        id: "codex",
        display: "OpenAI Codex CLI",
        default_bin: "codex",
        install_hint: "npm install -g @openai/codex",
        output_format: OutputFormat::Text,
        // `codex exec --json` emits a JSONL event stream whose `command_execution`
        // items oneharness normalizes into `events` (the plain default has no
        // transcript). A non-text format maps to `--json` in `argv_codex`.
        events_format: Some(OutputFormat::Json),
        supports_resume: true,
        supports_fork: false,
        fork_reuses_cache: false,
        sync: None,
        hooks: Some(HookBinding::File {
            shape: HookShape::Nested {
                event: "PreToolUse",
                with_timeout: false,
            },
            file: ".codex/hooks.json",
            path: &["hooks"],
            seed: None,
        }),
        global_hook: Some(GlobalHook {
            base: HookBase::Home,
            anchor: ".codex/hooks.json",
        }),
        gate_deny: Some(DenyShape::ClaudeNested),
        // Probe-verified (2026-07-06, codex v0.142.5): `codex exec` DOES load
        // project `.codex/hooks.json` and honors the claude-nested
        // `updatedInput` rewrite — but ONLY when the invocation carries
        // `-c features.hooks=true --dangerously-bypass-hook-trust` (the
        // `projects.<dir>.trust_level="trusted"` config route yields zero hook
        // events). oneharness does not add those flags itself; the caller
        // passes them per run (config `args` / `--` passthrough), as the
        // `oh_mock_enforce codex` phase does.
        mock_rewrite: Some(RewriteShape::ClaudeNested),
        // Project .codex/hooks.json, restored after the run; the opt-in flags
        // the probe proved necessary are auto-appended to the argv.
        mock_delivery: Some(MockDelivery::ProjectHooks {
            extra_args: &[
                "-c",
                "features.hooks=true",
                "--dangerously-bypass-hook-trust",
            ],
        }),
        default_env: &[],
        // Codex `exec` *does* have a native schema flag (`--output-schema <file>`),
        // but it takes a schema FILE (not inline) and is reportedly ignored once
        // the agent uses tools: https://github.com/openai/codex/issues/15451 — so
        // structured output uses the more reliable prompt-based path for it today.
        // To wire it up once that's resolved: add a `CodexOutputSchema` variant to
        // `structured::NativeSchema`, set it here, add a `--output-schema` arm to
        // `argv_codex` (the command layer must materialize the schema to a temp
        // file and pass its path via `BuildCtx.schema`), and teach
        // `structured::extract_value` where Codex reports the conforming value.
        native_schema: None,
        // `codex exec` gates by sandbox, not by op-type, and downgrades approval
        // to `never` (it never hangs — out-of-sandbox actions fail closed and the
        // agent continues). `read-only` is the (OS-enforced) read-only sandbox,
        // `auto` is `workspace-write`. Codex has no *native* plan mode in `exec`
        // (its TUI Plan mode = read-only sandbox + a plan instruction, both
        // reproducible here), so `plan` is synthesized: same read-only sandbox
        // plus the `instruction` below. No edit-vs-shell split, so no `edit`.
        modes: &[
            mode(PermissionMode::ReadOnly, ModeHeadless::Clean),
            ModeSpec {
                mode: PermissionMode::Plan,
                headless: ModeHeadless::Clean,
                env: &[],
                instruction: Some(CODEX_PLAN_INSTRUCTION),
            },
            mode(PermissionMode::Default, ModeHeadless::Clean),
            mode(PermissionMode::Auto, ModeHeadless::Clean),
            mode(PermissionMode::Bypass, ModeHeadless::Clean),
        ],
        build_argv: argv_codex,
    },
    HarnessSpec {
        id: "opencode",
        display: "OpenCode",
        default_bin: "opencode",
        install_hint: "npm install -g opencode-ai",
        output_format: OutputFormat::Json,
        // Default `json` (JSONL) already carries the `tool` parts, so no upgrade.
        events_format: None,
        supports_resume: true,
        supports_fork: true,
        // OpenCode can fork, but its fork re-sends the branched conversation cold
        // (measured: the fan-out reads no cache and re-writes the whole prefix),
        // so a fork-based min-tokens would raise tokens, not lower them.
        fork_reuses_cache: false,
        sync: Some(SyncSpec {
            file: "opencode.json",
            alt_files: &[],
            allow_path: None,
            deny_path: None,
            hooks_path: None,
            schema_seed: None,
        }),
        hooks: Some(HookBinding::JsPlugin {
            plugin_dir: ".opencode/plugin",
            template: OPENCODE_PLUGIN_JS,
        }),
        global_hook: Some(GlobalHook {
            base: HookBase::ConfigHome,
            anchor: "opencode/plugin/{name}.js",
        }),
        gate_deny: Some(DenyShape::Decision("deny")),
        // The oneharness plugin shim applies `updated_input` by merging it into
        // the tool's mutable `args` at `tool.execute.before` (the officially
        // documented mutation point of OpenCode's plugin API).
        mock_rewrite: Some(RewriteShape::OpencodeShim),
        mock_delivery: Some(MockDelivery::ProjectHooks { extra_args: &[] }),
        default_env: &[],
        native_schema: None,
        // The built-in `plan` agent is read-only, so `plan` and `read-only` both
        // map to it. `default` is clean headless: OpenCode's out-of-box default
        // is `allow`, and even an `ask` permission *auto-rejects* (deny-and-
        // continue) under `opencode run` rather than blocking — it never hangs.
        // `edit` (auto-approve edits, gate bash) is delivered per-run through the
        // inline-config env var `OPENCODE_CONFIG_CONTENT` (highest-precedence
        // config, set without touching `opencode.json`) — no argv flag exists.
        // Bypass auto-approves all but explicit denies. There is no classifier
        // `auto`.
        modes: &[
            mode(PermissionMode::ReadOnly, ModeHeadless::Clean),
            mode(PermissionMode::Plan, ModeHeadless::Clean),
            mode(PermissionMode::Default, ModeHeadless::Clean),
            ModeSpec {
                mode: PermissionMode::Edit,
                headless: ModeHeadless::Clean,
                env: &[(
                    "OPENCODE_CONFIG_CONTENT",
                    r#"{"permission":{"edit":"allow","bash":"deny"}}"#,
                )],
                instruction: None,
            },
            mode(PermissionMode::Bypass, ModeHeadless::Clean),
        ],
        build_argv: argv_opencode,
    },
    HarnessSpec {
        id: "goose",
        display: "Goose",
        default_bin: "goose",
        install_hint: "see https://block.github.io/goose/docs/getting-started/installation",
        output_format: OutputFormat::Text,
        // Events pending investigation (see the events matrix).
        events_format: None,
        supports_resume: true,
        supports_fork: false,
        fork_reuses_cache: false,
        sync: None,
        hooks: Some(HookBinding::GoosePlugin {
            shape: HookShape::Nested {
                event: "PreToolUse",
                with_timeout: true,
            },
            plugins_dir: ".agents/plugins",
            manifest: GOOSE_MANIFEST,
            path: &["hooks"],
        }),
        global_hook: Some(GlobalHook {
            base: HookBase::Home,
            anchor: ".agents/plugins/{name}",
        }),
        gate_deny: Some(DenyShape::Decision("block")),
        // Goose hooks can only block (exit 2 / `decision: "block"`); its
        // protocol has no input-rewrite verdict — deny is its mock ceiling.
        mock_rewrite: None,
        // Deny/spy rules still deliver (its project plugin hooks fire live).
        mock_delivery: Some(MockDelivery::ProjectHooks { extra_args: &[] }),
        default_env: &[],
        native_schema: None,
        // Goose has no mode flag on `goose run`; the mode is the `GOOSE_MODE`
        // environment variable (highest precedence over its config.yaml). None of
        // these hang headlessly: `approve`/`smart_approve` fail *closed* with an
        // error when a tool needs approval (exit non-zero rather than block),
        // `auto` approves everything. Goose has no headless plan workflow and no
        // per-run read-only-with-reads (its `chat` mode disables *all* tools,
        // reads included, so it is neither) — `plan`/`read-only`/`edit` are
        // absent. Bypass MUST set `GOOSE_MODE=auto` explicitly: leaving it unset
        // would inherit the user's config, which may be a fail-closed mode.
        modes: &[
            ModeSpec {
                mode: PermissionMode::Default,
                headless: ModeHeadless::Clean,
                env: &[("GOOSE_MODE", "approve")],
                instruction: None,
            },
            ModeSpec {
                mode: PermissionMode::Auto,
                headless: ModeHeadless::Clean,
                env: &[("GOOSE_MODE", "smart_approve")],
                instruction: None,
            },
            ModeSpec {
                mode: PermissionMode::Bypass,
                headless: ModeHeadless::Clean,
                env: &[("GOOSE_MODE", "auto")],
                instruction: None,
            },
        ],
        build_argv: argv_goose,
    },
    HarnessSpec {
        id: "qwen",
        display: "Qwen Code",
        default_bin: "qwen",
        install_hint: "npm install -g @qwen-code/qwen-code",
        output_format: OutputFormat::Text,
        // Qwen's `--output-format stream-json` emits the Anthropic content-block
        // stream oneharness normalizes into `events` (its default text has no
        // transcript). Mapped to `--output-format stream-json` in `argv_qwen`.
        events_format: Some(OutputFormat::StreamJson),
        supports_resume: true,
        supports_fork: false,
        fork_reuses_cache: false,
        sync: Some(SyncSpec {
            file: ".qwen/settings.json",
            alt_files: &[],
            allow_path: Some(&["permissions", "allow"]),
            deny_path: Some(&["permissions", "deny"]),
            hooks_path: None,
            schema_seed: None,
        }),
        hooks: Some(HookBinding::SameFile {
            shape: HookShape::Nested {
                event: "PreToolUse",
                with_timeout: false,
            },
            path: &["hooks"],
        }),
        global_hook: Some(GlobalHook {
            base: HookBase::Home,
            anchor: ".qwen/settings.json",
        }),
        gate_deny: Some(DenyShape::ClaudeNested),
        // Qwen's docs describe the same `updatedInput` rewrite as Claude Code,
        // but it is NOT honored live: with the hook demonstrably firing (its
        // gate deny passed at the same global scope) and the allow+updatedInput
        // verdict emitted, the ORIGINAL command still ran — measured by
        // oh_mock_enforce on all three OSes (2026-07-06, `--yolo`). Absent per
        // the measured-not-guessed rule until the explore-hooks probe sources a
        // shape qwen actually applies; `oneharness mock qwen` is deny-only.
        mock_rewrite: None,
        // Qwen fires only *user*-scoped hooks headlessly (project hooks sit
        // behind folder trust), so a project-scope one-shot install would be
        // silently inert — refused loudly; use `sync --global` into a
        // redirected HOME instead (the oh_hook_enforce qwen pattern).
        mock_delivery: None,
        default_env: &[("QWEN_CODE_SUPPRESS_YOLO_WARNING", "1")],
        native_schema: None,
        // `--approval-mode` spans the whole spectrum, all clean headless: current
        // qwen-code *deny-and-continues* a gated tool in non-interactive mode
        // (auto-deny + the agent loop proceeds, process exits 0) rather than
        // hanging. So `default` denies gated tools and continues; `auto-edit`
        // genuinely auto-applies edits while denying shell; `auto` runs the
        // classifier; none block. Qwen's only read-only mechanism is its plan
        // mode, so `read-only` coincides with `plan` (both → `--approval-mode
        // plan`).
        modes: &[
            mode(PermissionMode::ReadOnly, ModeHeadless::Clean),
            mode(PermissionMode::Plan, ModeHeadless::Clean),
            mode(PermissionMode::Default, ModeHeadless::Clean),
            mode(PermissionMode::Edit, ModeHeadless::Clean),
            mode(PermissionMode::Auto, ModeHeadless::Clean),
            mode(PermissionMode::Bypass, ModeHeadless::Clean),
        ],
        build_argv: argv_qwen,
    },
    HarnessSpec {
        id: "crush",
        display: "Crush",
        default_bin: "crush",
        install_hint: "npm install -g @charmland/crush",
        output_format: OutputFormat::Text,
        // Events pending investigation (see the events matrix).
        events_format: None,
        supports_resume: true,
        supports_fork: false,
        fork_reuses_cache: false,
        sync: Some(SyncSpec {
            file: "crush.json",
            alt_files: &[".crush.json"],
            allow_path: Some(&["permissions", "allowed_tools"]),
            deny_path: Some(&["options", "disabled_tools"]),
            hooks_path: None,
            schema_seed: None,
        }),
        hooks: Some(HookBinding::SameFile {
            shape: HookShape::Flat {
                event: "PreToolUse",
            },
            path: &["hooks"],
        }),
        global_hook: Some(GlobalHook {
            base: HookBase::ConfigHome,
            anchor: "crush/crush.json",
        }),
        gate_deny: Some(DenyShape::Decision("deny")),
        // Crush's PreToolUse stdout documents `updated_input` (a shallow-merge
        // patch of the tool input) beside `decision: "allow"`.
        mock_rewrite: Some(RewriteShape::CrushFlat),
        mock_delivery: Some(MockDelivery::ProjectHooks { extra_args: &[] }),
        default_env: &[],
        native_schema: None,
        // `crush run` auto-approves the whole session, so it never hangs — but it
        // also cannot gate, so `default` and `bypass` behave the same (bypass
        // adds the explicit `--yolo`). There is no plan/edit/auto mode on `run`.
        modes: &[
            mode(PermissionMode::Default, ModeHeadless::Clean),
            mode(PermissionMode::Bypass, ModeHeadless::Clean),
        ],
        build_argv: argv_crush,
    },
    HarnessSpec {
        id: "copilot",
        display: "GitHub Copilot CLI",
        default_bin: "copilot",
        install_hint: "npm install -g @github/copilot",
        output_format: OutputFormat::Text,
        // Events pending investigation (see the events matrix).
        events_format: None,
        supports_resume: true,
        supports_fork: false,
        fork_reuses_cache: false,
        sync: None,
        hooks: Some(HookBinding::File {
            shape: HookShape::CrossShell {
                event: "preToolUse",
            },
            file: ".github/hooks/{name}.json",
            path: &["hooks"],
            seed: Some(r#"{"version":1}"#),
        }),
        global_hook: Some(GlobalHook {
            base: HookBase::Home,
            anchor: ".copilot/hooks/{name}.json",
        }),
        gate_deny: Some(DenyShape::CopilotFlat),
        // Probe-REFUTED (2026-07-06): copilot's repo `.github/hooks/*.json`
        // hooks produced ZERO events across every headless `-p` experiment,
        // even though the agent demonstrably used its shell tool — despite its
        // docs demonstrating `-p` hooks. With no hook firing there is nothing
        // to rewrite; absent until a live run shows its hooks loading at all.
        mock_rewrite: None,
        // No hook has ever fired headlessly (probe: zero events) — nothing any
        // delivery could make the CLI run, so the flag is refused loudly.
        mock_delivery: None,
        default_env: &[],
        native_schema: None,
        // `--mode plan` is a real read-only plan mode; `read-only` is allow-all
        // with `write`/`shell` denied (deny beats allow); `edit` allows
        // `write`/`read` but not `shell`, so edits run and shell is auto-denied
        // (the headless form of "gate shell"); bypass is the allow-all trio.
        // Without any, `-p` auto-denies gated tools and continues (never hangs),
        // so `default` is `Clean`. No classifier `auto`, so it is absent.
        modes: &[
            mode(PermissionMode::ReadOnly, ModeHeadless::Clean),
            mode(PermissionMode::Plan, ModeHeadless::Clean),
            mode(PermissionMode::Default, ModeHeadless::Clean),
            mode(PermissionMode::Edit, ModeHeadless::Clean),
            mode(PermissionMode::Bypass, ModeHeadless::Clean),
        ],
        build_argv: argv_copilot,
    },
    HarnessSpec {
        id: "cursor",
        display: "Cursor CLI",
        default_bin: "cursor-agent",
        install_hint: "see https://docs.cursor.com/en/cli/overview",
        output_format: OutputFormat::StreamJson,
        // Default `stream-json` already carries the tool transcript, so no upgrade.
        events_format: None,
        supports_resume: true,
        supports_fork: false,
        fork_reuses_cache: false,
        sync: Some(SyncSpec {
            file: ".cursor/cli.json",
            alt_files: &[],
            allow_path: Some(&["permissions", "allow"]),
            deny_path: Some(&["permissions", "deny"]),
            hooks_path: None,
            schema_seed: Some(r#"{"permissions":{"allow":[],"deny":[]}}"#),
        }),
        hooks: Some(HookBinding::File {
            shape: HookShape::CommandOnly {
                events: &[
                    "beforeShellExecution",
                    "beforeReadFile",
                    "beforeMCPExecution",
                    // `preToolUse` is the ONLY cursor event whose reply can
                    // rewrite the tool's input (`updated_input`, probe-verified
                    // headlessly 2026-07-06); the three `before*` events are
                    // allow/deny-only. Wiring it alongside them means a shell
                    // call invokes the hook command more than once — the gate's
                    // verdicts are idempotent, and the mock's rewrite rides
                    // this event.
                    "preToolUse",
                ],
            },
            file: ".cursor/hooks.json",
            path: &["hooks"],
            seed: Some(r#"{"version":1}"#),
        }),
        global_hook: Some(GlobalHook {
            base: HookBase::Home,
            anchor: ".cursor/hooks.json",
        }),
        gate_deny: Some(DenyShape::CursorPermission),
        // Probe-verified (2026-07-06): cursor honors a `preToolUse` reply of
        // `{"permission":"allow","updated_input":{…}}` headlessly — the
        // original command in the preToolUse event, the rewritten one in the
        // subsequent before/afterShellExecution events. The `preToolUse` event
        // is wired into the hook binding above for exactly this.
        mock_rewrite: Some(RewriteShape::CursorPermission),
        mock_delivery: Some(MockDelivery::ProjectHooks { extra_args: &[] }),
        default_env: &[],
        native_schema: None,
        // `--mode plan` is the read-only plan mode; `--mode ask` is read-only
        // Q&A (no plan workflow) → `read-only`; `--force` is bypass. Without
        // `--force` a gated tool stalls (Cursor proposes-not-applies, with no
        // fail-fast deny flag), so `default` (`--trust` only) is `Hangs`.
        // Edit/shell gating is a `permissions` config concern (synced).
        modes: &[
            mode(PermissionMode::ReadOnly, ModeHeadless::Clean),
            mode(PermissionMode::Plan, ModeHeadless::Clean),
            mode(PermissionMode::Default, ModeHeadless::Hangs),
            mode(PermissionMode::Bypass, ModeHeadless::Clean),
        ],
        build_argv: argv_cursor,
    },
];

/// Claude Code's `--permission-mode` token for each normalized mode. `Default`
/// maps to `dontAsk` (deny any un-allowed tool and continue) rather than
/// `default` (which *aborts* the `-p` run on an un-allowed tool): the ask flow
/// then completes headlessly instead of failing on the first prompt. `ReadOnly`
/// rides `bypassPermissions` (allow-all, no prompts) with the mutating tools
/// denied separately — deny rules take precedence even under bypass.
fn claude_permission_mode(mode: PermissionMode) -> &'static str {
    match mode {
        PermissionMode::Plan => "plan",
        PermissionMode::ReadOnly => "bypassPermissions",
        PermissionMode::Default => "dontAsk",
        PermissionMode::Edit => "acceptEdits",
        PermissionMode::Auto => "auto",
        PermissionMode::Bypass => "bypassPermissions",
    }
}

/// `claude -p <prompt> --permission-mode <mode> [--disallowedTools …] [--model M]
/// [--append-system-prompt S] [--resume <id> [--fork-session]] --output-format json`
/// (`--resume` continues a session by id; `--fork-session` branches a new session
/// from it instead of appending — the session id is read from the result JSON's
/// `session_id`).
fn argv_claude_code(c: &BuildCtx) -> Vec<String> {
    let mut a = vec![c.bin.into(), "-p".into(), c.prompt.into()];
    a.push("--permission-mode".into());
    a.push(claude_permission_mode(c.mode).into());
    // read-only: deny the mutating tools (Bash covers destructive shell; reads
    // still run via Read/Grep/Glob). A bare name removes the tool entirely.
    if c.mode == PermissionMode::ReadOnly {
        a.push("--disallowedTools".into());
        for tool in ["Bash", "Edit", "Write", "NotebookEdit"] {
            a.push(tool.into());
        }
    }
    if let Some(m) = c.model {
        a.push("--model".into());
        a.push(m.into());
    }
    if let Some(s) = c.system {
        a.push("--append-system-prompt".into());
        a.push(s.into());
    }
    if let Some(sid) = c.resume {
        a.push("--resume".into());
        a.push(sid.into());
        // Fork instead of appending: a new session id branches off `sid`, leaving
        // the original (and its cached prefix) untouched. Only meaningful with
        // `--resume`; the command layer only sets `fork` for this verified-capable
        // adapter.
        if c.fork {
            a.push("--fork-session".into());
        }
    }
    a.push("--output-format".into());
    a.push(format_flag(c.output_format).into());
    // Claude Code refuses `-p --output-format stream-json` without `--verbose`
    // ("--print with --output-format=stream-json requires --verbose"). Emit it so
    // the streaming path actually runs — it is what surfaces the Anthropic
    // content-block transcript oneharness normalizes into `events` (the default
    // single-document `json` result carries no transcript). Sourced from the CLI's
    // own error, not guessed.
    if c.output_format == OutputFormat::StreamJson {
        a.push("--verbose".into());
    }
    // Native structured output: `--json-schema <inline>` makes Claude Code return
    // the conforming value in the result document's `structured_output` field
    // (it requires `--output-format json`, already emitted above). Sourced from
    // the headless docs; only set when a schema run selected this adapter.
    if let Some(schema) = c.schema {
        a.push("--json-schema".into());
        a.push(schema.into());
    }
    a
}

/// `codex exec [resume <id>] [--dangerously-bypass-approvals-and-sandbox]
/// [--model M] <prompt>`
///
/// Codex exposes no system-prompt flag, so `--system` is prepended to the prompt.
/// The single bypass flag replaces the older `--sandbox danger-full-access -a
/// never`: codex-cli >= 0.135 removed `-a`, and this flag is the supported way to
/// skip every approval prompt and the sandbox for a headless run.
///
/// Continuation is a *subcommand*, not a flag: `codex exec resume <SESSION_ID>
/// <prompt>` replays the stored thread and appends the new turn (linear; Codex's
/// `exec` has no headless fork — `codex fork` is TUI-only, openai/codex#11750). The
/// session handle is the `thread_id` Codex emits under `--json`; oneharness reads
/// it via [`crate::domain::signals::extract_session`].
fn argv_codex(c: &BuildCtx) -> Vec<String> {
    let mut a = vec![c.bin.into(), "exec".into()];
    if c.resume.is_some() {
        a.push("resume".into());
    }
    // The sandbox is the real control surface under `exec` (approval downgrades
    // to `never`). `Default` keeps the exec default (read-only). `Edit` is not a
    // supported mode for codex, so it is never reached.
    match c.mode {
        PermissionMode::Bypass => {
            a.push("--dangerously-bypass-approvals-and-sandbox".into());
        }
        // `plan` is the read-only sandbox too (enforcement half); its plan
        // instruction is prepended to the prompt by the command layer.
        PermissionMode::ReadOnly | PermissionMode::Plan => {
            a.push("--sandbox".into());
            a.push("read-only".into());
        }
        PermissionMode::Auto => {
            a.push("--sandbox".into());
            a.push("workspace-write".into());
        }
        // `default` keeps the exec default; `edit` is unsupported for codex and
        // never reaches here.
        PermissionMode::Default | PermissionMode::Edit => {}
    }
    if let Some(m) = c.model {
        a.push("--model".into());
        a.push(m.into());
    }
    // `--events`/`--stream` upgrades codex to its JSON event stream (`--json`),
    // whose `command_execution` items become normalized `events` and whose
    // `agent_message` item carries the final text. The default (`Text`) stays
    // plain. Codex has no `stream-json`; `--json` IS its JSONL stream, so both
    // non-text formats map to it. Sourced from `codex exec --help`.
    if c.output_format != OutputFormat::Text {
        a.push("--json".into());
    }
    // The resumed thread's id is the positional that precedes the prompt for
    // `codex exec resume <id> <prompt>` (the `resume` token was pushed above).
    if let Some(sid) = c.resume {
        a.push(sid.into());
    }
    a.push(prompt_with_system(c));
    a
}

/// `opencode run [--dangerously-skip-permissions] --format json [-m M]
/// [--session SID] <prompt>` (OpenCode continues a session id with `--session`)
///
/// OpenCode's `run` has no system-prompt flag, so `--system` is prepended to the
/// prompt.
fn argv_opencode(c: &BuildCtx) -> Vec<String> {
    let mut a = vec![c.bin.into(), "run".into()];
    // `plan`/`read-only` both select the built-in read-only `plan` agent; bypass
    // auto-approves. Other modes are unsupported and never reach here.
    match c.mode {
        PermissionMode::Bypass => a.push("--dangerously-skip-permissions".into()),
        PermissionMode::Plan | PermissionMode::ReadOnly => {
            a.push("--agent".into());
            a.push("plan".into());
        }
        _ => {}
    }
    a.push("--format".into());
    a.push(format_flag(c.output_format).into());
    if let Some(m) = c.model {
        a.push("-m".into());
        a.push(m.into());
    }
    if let Some(sid) = c.resume {
        a.push("--session".into());
        a.push(sid.into());
        // Branch a new session from `sid` rather than appending in place, so the
        // original's cached prefix can seed independent follow-ups. Only set for
        // this verified-capable adapter, and only alongside `--session`.
        if c.fork {
            a.push("--fork".into());
        }
    }
    a.push(prompt_with_system(c));
    a
}

/// `goose run --with-builtin developer [--system S] [--resume --name <name>]
/// -t <prompt>`
///
/// Goose selects its model from its own config, so `model` is not mapped, and the
/// approval mode is delivered through the `GOOSE_MODE` environment variable (the
/// matching [`ModeSpec::env`]), not the argv — so `c.mode` is intentionally not
/// read here. It does expose a native `--system` flag, so `--system` maps to it
/// rather than being prepended.
fn argv_goose(c: &BuildCtx) -> Vec<String> {
    let mut a = vec![
        c.bin.into(),
        "run".into(),
        "--with-builtin".into(),
        "developer".into(),
    ];
    if let Some(s) = c.system {
        a.push("--system".into());
        a.push(s.into());
    }
    // Goose emits no session id to stdout headlessly, so continuation rides a
    // caller-chosen *name*: `--resume --name <name>` resumes that named session
    // (and a fresh `--name <name>` run creates it — create-or-resume). The
    // `--resume` value oneharness forwards is therefore the name. `session_id`
    // stays null for Goose (nothing to extract); the caller owns the handle.
    if let Some(name) = c.resume {
        a.push("--resume".into());
        a.push("--name".into());
        a.push(name.into());
    }
    a.push("-t".into());
    a.push(c.prompt.into());
    a
}

/// `qwen [--yolo | --approval-mode <m>] [-m M] [--resume <id>] -p <prompt>` (no
/// system flag, so `--system` is prepended). Bypass uses the dedicated `--yolo`;
/// the other modes use `--approval-mode` (only `plan` and `bypass` run cleanly
/// headless — see the `modes` table — but the flag is mapped for every supported
/// mode). `--resume <id>` continues a prior session by UUID (linear append; no
/// headless fork). The id is the `session_id` Qwen reports under
/// `--output-format json`.
fn argv_qwen(c: &BuildCtx) -> Vec<String> {
    let mut a = vec![c.bin.into()];
    match c.mode {
        PermissionMode::Bypass => a.push("--yolo".into()),
        PermissionMode::Plan | PermissionMode::ReadOnly => {
            a.push("--approval-mode".into());
            a.push("plan".into());
        }
        PermissionMode::Default => {
            a.push("--approval-mode".into());
            a.push("default".into());
        }
        PermissionMode::Edit => {
            a.push("--approval-mode".into());
            a.push("auto-edit".into());
        }
        PermissionMode::Auto => {
            a.push("--approval-mode".into());
            a.push("auto".into());
        }
    }
    if let Some(m) = c.model {
        a.push("-m".into());
        a.push(m.into());
    }
    // `--events`/`--stream` upgrades qwen to `--output-format stream-json` (its
    // NDJSON Anthropic content-block stream), which oneharness normalizes into
    // `events` and from which it recovers the final text. The default stays plain
    // text. Sourced from `qwen --help` (`-o, --output-format text|json|stream-json`).
    if c.output_format != OutputFormat::Text {
        a.push("--output-format".into());
        a.push(format_flag(c.output_format).into());
    }
    if let Some(sid) = c.resume {
        a.push("--resume".into());
        a.push(sid.into());
    }
    a.push("-p".into());
    a.push(prompt_with_system(c));
    a
}

/// `crush run -q [--session <id>] [-m M] <prompt>` (`run` is non-interactive; `-q`
/// quiets it; no system flag, so `--system` is prepended). `crush run` already
/// auto-approves the whole session (verified live), so `default` and `bypass` are
/// identical — crush has no per-run permission flag (`--yolo` is rejected on `run`
/// as of v0.80.0), so the mode is not expressed on the argv. `--session <id>`
/// continues a stored session by id (linear append; no headless fork). The id is
/// the `session_id` crush reports under `--format json`.
fn argv_crush(c: &BuildCtx) -> Vec<String> {
    let mut a = vec![c.bin.into(), "run".into(), "-q".into()];
    if let Some(sid) = c.resume {
        a.push("--session".into());
        a.push(sid.into());
    }
    if let Some(m) = c.model {
        a.push("-m".into());
        a.push(m.into());
    }
    a.push(prompt_with_system(c));
    a
}

/// `copilot -p <prompt> [--allow-all-tools --allow-all-paths --no-ask-user]
/// [--model M] [--resume <id>]` (no system flag, so `--system` is prepended to the
/// prompt; its `--allow-tool`/`--deny-tool` permission flags are not unified —
/// Copilot has no project config file to sync, so rules go via `[harness.copilot]
/// args`). `--resume <id>` continues a session by UUID (linear append; no headless
/// fork). Copilot emits no session id headlessly, and `--resume <uuid>` *creates*
/// the session when the id is new (create-or-resume) — so the caller mints and
/// reuses a UUID; `session_id` stays null (nothing to extract).
fn argv_copilot(c: &BuildCtx) -> Vec<String> {
    let mut a = vec![c.bin.into(), "-p".into(), prompt_with_system(c)];
    // Bypass is the allow-all trio; `plan` is the read-only plan mode;
    // `read-only` is allow-all with `write`/`shell` denied (deny beats allow).
    // Without any, `-p` auto-denies gated tools and continues. Unsupported modes
    // never reach here.
    match c.mode {
        PermissionMode::Bypass => {
            a.push("--allow-all-tools".into());
            a.push("--allow-all-paths".into());
            a.push("--no-ask-user".into());
        }
        PermissionMode::ReadOnly => {
            a.push("--allow-all-tools".into());
            a.push("--allow-all-paths".into());
            a.push("--deny-tool".into());
            a.push("shell".into());
            a.push("--deny-tool".into());
            a.push("write".into());
            a.push("--no-ask-user".into());
        }
        PermissionMode::Edit => {
            // Allow file edits and reads, but not shell — an un-allowed `shell`
            // is auto-denied under `-p`, so edits run and commands are gated.
            a.push("--allow-tool".into());
            a.push("write".into());
            a.push("--allow-tool".into());
            a.push("read".into());
            a.push("--allow-all-paths".into());
            a.push("--no-ask-user".into());
        }
        PermissionMode::Plan => {
            a.push("--mode".into());
            a.push("plan".into());
        }
        _ => {}
    }
    if let Some(m) = c.model {
        a.push("--model".into());
        a.push(m.into());
    }
    if let Some(sid) = c.resume {
        a.push("--resume".into());
        a.push(sid.into());
    }
    a
}

/// `cursor-agent -p <prompt> [--force|--trust] [--model M] [--resume SID]
/// --output-format stream-json` (Cursor continues a chat id with `--resume`; no
/// system flag, so `--system` is prepended to the prompt)
fn argv_cursor(c: &BuildCtx) -> Vec<String> {
    let mut a = vec![c.bin.into(), "-p".into(), prompt_with_system(c)];
    // `--force` is bypass (it also implies trust). Otherwise a headless run still
    // needs `--trust` — Cursor refuses to run an untrusted workspace ("Workspace
    // Trust Required", observed live) — while leaving the permission system
    // active. `plan` additionally selects the read-only `--mode plan`.
    if c.mode.is_bypass() {
        a.push("--force".into());
    } else {
        a.push("--trust".into());
        match c.mode {
            PermissionMode::Plan => {
                a.push("--mode".into());
                a.push("plan".into());
            }
            PermissionMode::ReadOnly => {
                a.push("--mode".into());
                a.push("ask".into());
            }
            _ => {}
        }
    }
    if let Some(m) = c.model {
        a.push("--model".into());
        a.push(m.into());
    }
    if let Some(sid) = c.resume {
        a.push("--resume".into());
        a.push(sid.into());
    }
    a.push("--output-format".into());
    a.push(format_flag(c.output_format).into());
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a>(bin: &'a str, model: Option<&'a str>, mode: PermissionMode) -> BuildCtx<'a> {
        ctx_fmt(bin, model, mode, OutputFormat::Json)
    }

    fn ctx_fmt<'a>(
        bin: &'a str,
        model: Option<&'a str>,
        mode: PermissionMode,
        output_format: OutputFormat,
    ) -> BuildCtx<'a> {
        BuildCtx {
            bin,
            prompt: "hi",
            model,
            system: None,
            resume: None,
            fork: false,
            mode,
            output_format,
            schema: None,
        }
    }

    #[test]
    fn registry_ids_are_unique_and_nonempty() {
        let mut seen = std::collections::HashSet::new();
        for h in all() {
            assert!(!h.id.is_empty());
            assert!(!h.default_bin.is_empty());
            assert!(seen.insert(h.id), "duplicate id {}", h.id);
        }
        assert_eq!(all().len(), 8);
    }

    /// Pin each harness's mock input-rewrite capability: the live-verified
    /// shapes for the five that honor one (oh_mock_enforce + the explore-hooks
    /// probe, 2026-07-06), and — as deliberately as the presences — the
    /// absences: Goose's protocol has no rewrite verdict; Copilot's hooks
    /// never fired headlessly (probe: zero events); and Qwen's documented
    /// `updatedInput` was live-REFUTED (verdict emitted, original command
    /// still ran — see its registry comment), so it stays absent despite its
    /// docs. A rewrite also requires an installable hook and a deny shape
    /// (the mock responder's other verb), so those must accompany it.
    #[test]
    fn registry_mock_rewrite_capability_is_pinned() {
        let shape = |id: &str| by_id(id).unwrap().mock_rewrite;
        assert_eq!(shape("claude-code"), Some(RewriteShape::ClaudeNested));
        assert_eq!(shape("codex"), Some(RewriteShape::ClaudeNested));
        assert_eq!(shape("crush"), Some(RewriteShape::CrushFlat));
        assert_eq!(shape("opencode"), Some(RewriteShape::OpencodeShim));
        assert_eq!(shape("cursor"), Some(RewriteShape::CursorPermission));
        for id in ["goose", "qwen", "copilot"] {
            assert_eq!(shape(id), None, "{id} must stay absent until verified");
        }
        // The one-shot delivery (`run --mock-rules`): claude rides the argv,
        // qwen (user-scope-only hooks) and copilot (hooks never fire) are
        // refused, codex auto-appends its probe-proven opt-in flags, the rest
        // are plain project installs.
        let delivery = |id: &str| by_id(id).unwrap().mock_delivery;
        assert_eq!(
            delivery("claude-code"),
            Some(MockDelivery::SettingsFlag { flag: "--settings" })
        );
        assert_eq!(
            delivery("codex"),
            Some(MockDelivery::ProjectHooks {
                extra_args: &[
                    "-c",
                    "features.hooks=true",
                    "--dangerously-bypass-hook-trust",
                ],
            })
        );
        for id in ["opencode", "goose", "crush", "cursor"] {
            assert_eq!(
                delivery(id),
                Some(MockDelivery::ProjectHooks { extra_args: &[] }),
                "{id}"
            );
        }
        for id in ["qwen", "copilot"] {
            assert_eq!(delivery(id), None, "{id} must be refused loudly");
        }
        for h in all() {
            if h.mock_rewrite.is_some() {
                assert!(
                    h.hooks.is_some() && h.gate_deny.is_some(),
                    "{}: a rewrite shape needs an installable hook and a deny shape",
                    h.id
                );
            }
        }
    }

    /// The OpenCode shim must keep both verdict paths: throw-to-block on a
    /// deny, and merge `updated_input` into the tool's mutable args (the
    /// rewrite `oneharness mock` emits for `RewriteShape::OpencodeShim`).
    #[test]
    fn opencode_shim_applies_updated_input_and_still_blocks_on_deny() {
        assert!(OPENCODE_PLUGIN_JS.contains(r#"decision.decision === "deny""#));
        assert!(OPENCODE_PLUGIN_JS.contains("Object.assign(output.args, decision.updated_input)"));
        // The deny check runs first, so a malformed reply carrying both never
        // rewrites a call that should have been blocked.
        let deny = OPENCODE_PLUGIN_JS
            .find(r#"decision.decision === "deny""#)
            .unwrap();
        let rewrite = OPENCODE_PLUGIN_JS.find("decision.updated_input").unwrap();
        assert!(deny < rewrite, "deny must be evaluated before the rewrite");
    }

    #[test]
    fn claude_argv_bypass_on() {
        let spec = by_id("claude-code").unwrap();
        let argv = (spec.build_argv)(&ctx("claude", None, PermissionMode::Bypass));
        assert_eq!(
            argv,
            vec![
                "claude",
                "-p",
                "hi",
                "--permission-mode",
                "bypassPermissions",
                "--output-format",
                "json"
            ]
        );
    }

    #[test]
    fn claude_argv_default_mode_maps_to_dont_ask() {
        // The normalized `default` ask flow maps to `dontAsk` (deny un-allowed
        // tools and continue), not `--permission-mode default` (which aborts the
        // `-p` run on the first un-allowed tool) — so it never hangs/aborts.
        let spec = by_id("claude-code").unwrap();
        let argv = (spec.build_argv)(&ctx("claude", Some("haiku"), PermissionMode::Default));
        assert_eq!(
            argv,
            vec![
                "claude",
                "-p",
                "hi",
                "--permission-mode",
                "dontAsk",
                "--model",
                "haiku",
                "--output-format",
                "json"
            ]
        );
    }

    #[test]
    fn claude_maps_each_mode_to_its_permission_mode_token() {
        let spec = by_id("claude-code").unwrap();
        for (mode, token) in [
            (PermissionMode::Plan, "plan"),
            (PermissionMode::ReadOnly, "bypassPermissions"),
            (PermissionMode::Default, "dontAsk"),
            (PermissionMode::Edit, "acceptEdits"),
            (PermissionMode::Auto, "auto"),
            (PermissionMode::Bypass, "bypassPermissions"),
        ] {
            let argv = (spec.build_argv)(&ctx("claude", None, mode));
            assert!(
                argv.windows(2).any(|w| w == ["--permission-mode", token]),
                "mode {mode:?} should emit {token}: {argv:?}"
            );
        }
        // read-only additionally denies the mutating tools (and only read-only
        // does — plan/bypass leave them available to the permission system).
        let ro = (spec.build_argv)(&ctx("claude", None, PermissionMode::ReadOnly));
        assert!(
            ro.windows(2).any(|w| w == ["--disallowedTools", "Bash"]),
            "read-only should deny Bash: {ro:?}"
        );
        for tool in ["Edit", "Write", "NotebookEdit"] {
            assert!(
                ro.iter().any(|t| t == tool),
                "read-only denies {tool}: {ro:?}"
            );
        }
        assert!(
            !(spec.build_argv)(&ctx("claude", None, PermissionMode::Plan))
                .iter()
                .any(|t| t == "--disallowedTools"),
            "plan should not deny tools"
        );
    }

    #[test]
    fn mode_native_flags_per_harness() {
        // Pin the mode→flag mapping for the harnesses that express it on the argv
        // (Goose carries it in the environment, asserted via `modes` env below).
        let cases: &[(&str, PermissionMode, &[&str])] = &[
            // read-only: enforced where possible (codex sandbox, copilot/cursor
            // native), coinciding with plan where it's the only mechanism.
            (
                "codex",
                PermissionMode::ReadOnly,
                &["--sandbox", "read-only"],
            ),
            (
                "codex",
                PermissionMode::Auto,
                &["--sandbox", "workspace-write"],
            ),
            ("opencode", PermissionMode::Plan, &["--agent", "plan"]),
            ("opencode", PermissionMode::ReadOnly, &["--agent", "plan"]),
            ("qwen", PermissionMode::Plan, &["--approval-mode", "plan"]),
            (
                "qwen",
                PermissionMode::ReadOnly,
                &["--approval-mode", "plan"],
            ),
            (
                "qwen",
                PermissionMode::Edit,
                &["--approval-mode", "auto-edit"],
            ),
            ("qwen", PermissionMode::Auto, &["--approval-mode", "auto"]),
            ("copilot", PermissionMode::Plan, &["--mode", "plan"]),
            (
                "copilot",
                PermissionMode::ReadOnly,
                &["--deny-tool", "shell"],
            ),
            (
                "copilot",
                PermissionMode::ReadOnly,
                &["--deny-tool", "write"],
            ),
            ("copilot", PermissionMode::Edit, &["--allow-tool", "write"]),
            ("cursor", PermissionMode::Plan, &["--mode", "plan"]),
            ("cursor", PermissionMode::ReadOnly, &["--mode", "ask"]),
        ];
        for (id, mode, want) in cases {
            let spec = by_id(id).unwrap();
            let argv = (spec.build_argv)(&ctx(spec.default_bin, None, *mode));
            assert!(
                argv.windows(want.len()).any(|w| w == *want),
                "harness {id} mode {mode:?} should emit {want:?}; got {argv:?}"
            );
        }
        // copilot `edit` must NOT blanket-allow tools, or shell wouldn't be
        // gated — it allows only write/read, leaving shell to be auto-denied.
        let copilot_edit =
            (by_id("copilot").unwrap().build_argv)(&ctx("copilot", None, PermissionMode::Edit));
        assert!(
            !copilot_edit.iter().any(|t| t == "--allow-all-tools"),
            "copilot edit must gate shell, not allow-all: {copilot_edit:?}"
        );
        // Codex `plan` is synthesized: read-only sandbox (enforcement) + a plan
        // instruction prepended to the prompt (no native exec plan mode).
        let codex_plan = by_id("codex").unwrap().mode(PermissionMode::Plan);
        assert!(
            codex_plan.is_some(),
            "codex should support synthesized plan"
        );
        assert!(
            codex_plan.unwrap().instruction.is_some(),
            "codex plan must carry a plan instruction"
        );
        let codex_plan_argv =
            (by_id("codex").unwrap().build_argv)(&ctx("codex", None, PermissionMode::Plan));
        assert!(
            codex_plan_argv
                .windows(2)
                .any(|w| w == ["--sandbox", "read-only"]),
            "codex plan must enforce read-only: {codex_plan_argv:?}"
        );
        // Goose rejects plan (no plan workflow, and no read-only to enforce it).
        assert!(by_id("goose").unwrap().mode(PermissionMode::Plan).is_none());
        assert!(by_id("goose")
            .unwrap()
            .mode(PermissionMode::ReadOnly)
            .is_none());
        // Crush supports neither plan nor read-only, and has no per-run
        // permission flag (`crush run` auto-approves), so bypass == default.
        let crush = by_id("crush").unwrap();
        assert!(crush.mode(PermissionMode::Plan).is_none());
        assert!(crush.mode(PermissionMode::ReadOnly).is_none());
        let bypass = (crush.build_argv)(&ctx("crush", None, PermissionMode::Bypass));
        let default = (crush.build_argv)(&ctx("crush", None, PermissionMode::Default));
        assert_eq!(bypass, default, "crush has no per-run mode flag");
        assert!(!bypass.iter().any(|t| t == "--yolo"), "{bypass:?}");
    }

    #[test]
    fn every_harness_supports_bypass_and_default_and_goose_carries_mode_env() {
        for h in all() {
            assert!(
                h.mode(PermissionMode::Bypass).is_some(),
                "harness {} must support bypass (the headless default)",
                h.id
            );
            assert!(
                h.mode(PermissionMode::Default).is_some(),
                "harness {} must support the default ask flow",
                h.id
            );
        }
        // Goose delivers the mode via GOOSE_MODE, so every supported mode carries
        // an env mapping (and no other harness does).
        let goose = by_id("goose").unwrap();
        assert_eq!(
            goose.mode(PermissionMode::Bypass).unwrap().env,
            &[("GOOSE_MODE", "auto")]
        );
        assert_eq!(
            goose.mode(PermissionMode::Default).unwrap().env,
            &[("GOOSE_MODE", "approve")]
        );
        // OpenCode delivers `edit` through the inline-config env var (no argv
        // flag exists for its per-tool permission map).
        assert_eq!(
            by_id("opencode")
                .unwrap()
                .mode(PermissionMode::Edit)
                .unwrap()
                .env,
            &[(
                "OPENCODE_CONFIG_CONTENT",
                r#"{"permission":{"edit":"allow","bash":"deny"}}"#
            )]
        );
        // Every other (harness, mode) expresses itself on the argv, not env.
        for h in all() {
            for m in h.modes {
                let env_ok = h.id == "goose"
                    || (h.id == "opencode" && m.mode == PermissionMode::Edit)
                    || m.env.is_empty();
                assert!(
                    env_ok,
                    "harness {} mode {:?} unexpectedly carries env",
                    h.id, m.mode
                );
            }
        }
    }

    #[test]
    fn codex_argv_uses_exec_and_bypass_flag() {
        // Codex's default format is Text (no transcript), so no `--json`.
        let spec = by_id("codex").unwrap();
        let argv = (spec.build_argv)(&ctx_fmt(
            "codex",
            None,
            PermissionMode::Bypass,
            OutputFormat::Text,
        ));
        assert_eq!(
            argv,
            vec![
                "codex",
                "exec",
                "--dangerously-bypass-approvals-and-sandbox",
                "hi"
            ]
        );
    }

    #[test]
    fn codex_events_format_adds_json_flag() {
        // Under `--events`/`--stream` the command layer selects codex's
        // events_format (Json), which maps to `--json` — its JSONL event stream.
        let spec = by_id("codex").unwrap();
        assert_eq!(spec.events_format, Some(OutputFormat::Json));
        let argv = (spec.build_argv)(&ctx_fmt(
            "codex",
            None,
            PermissionMode::Bypass,
            OutputFormat::Json,
        ));
        assert!(argv.iter().any(|t| t == "--json"), "{argv:?}");
    }

    #[test]
    fn qwen_events_format_adds_stream_json_flag() {
        // Qwen's events_format is stream-json → `--output-format stream-json`; the
        // default (text) emits no format flag.
        let spec = by_id("qwen").unwrap();
        assert_eq!(spec.events_format, Some(OutputFormat::StreamJson));
        let stream = (spec.build_argv)(&ctx_fmt(
            "qwen",
            None,
            PermissionMode::Bypass,
            OutputFormat::StreamJson,
        ));
        assert!(
            stream
                .windows(2)
                .any(|w| w == ["--output-format", "stream-json"]),
            "{stream:?}"
        );
        let text = (spec.build_argv)(&ctx_fmt(
            "qwen",
            None,
            PermissionMode::Bypass,
            OutputFormat::Text,
        ));
        assert!(
            !text.iter().any(|t| t == "--output-format"),
            "default text must not add a format flag: {text:?}"
        );
    }

    #[test]
    fn goose_ignores_model_and_bypass() {
        let spec = by_id("goose").unwrap();
        let with = (spec.build_argv)(&ctx("goose", Some("gpt"), PermissionMode::Bypass));
        let without = (spec.build_argv)(&ctx("goose", None, PermissionMode::Default));
        assert_eq!(with, without);
        assert_eq!(
            with,
            vec!["goose", "run", "--with-builtin", "developer", "-t", "hi"]
        );
    }

    #[test]
    fn output_format_override_changes_the_emitted_flag() {
        let spec = by_id("claude-code").unwrap();
        let argv = (spec.build_argv)(&ctx_fmt(
            "claude",
            None,
            PermissionMode::Bypass,
            OutputFormat::StreamJson,
        ));
        assert!(
            argv.windows(2)
                .any(|w| w == ["--output-format", "stream-json"]),
            "{argv:?}"
        );
        // opencode spells its flag `--format`.
        let oc = by_id("opencode").unwrap();
        let argv = (oc.build_argv)(&ctx_fmt(
            "opencode",
            None,
            PermissionMode::Bypass,
            OutputFormat::Text,
        ));
        assert!(
            argv.windows(2).any(|w| w == ["--format", "text"]),
            "{argv:?}"
        );
    }

    #[test]
    fn claude_maps_system_to_append_system_prompt() {
        let spec = by_id("claude-code").unwrap();
        let ctx = BuildCtx {
            bin: "claude",
            prompt: "hi",
            model: None,
            system: Some("be terse"),
            resume: None,
            fork: false,
            mode: PermissionMode::Bypass,
            output_format: OutputFormat::Json,
            schema: None,
        };
        let argv = (spec.build_argv)(&ctx);
        assert!(
            argv.windows(2)
                .any(|w| w == ["--append-system-prompt", "be terse"]),
            "{argv:?}"
        );
    }

    #[test]
    fn prompt_with_system_prefixes_only_when_present() {
        let spec = by_id("codex").unwrap();
        let none = BuildCtx {
            system: None,
            ..base_ctx(spec)
        };
        assert_eq!(prompt_with_system(&none), "hi");
        let some = BuildCtx {
            system: Some("rules"),
            ..base_ctx(spec)
        };
        assert_eq!(prompt_with_system(&some), "rules\n\nhi");
        // A blank system prompt is a no-op (no stray leading newlines).
        let empty = BuildCtx {
            system: Some(""),
            ..base_ctx(spec)
        };
        assert_eq!(prompt_with_system(&empty), "hi");
    }

    #[test]
    fn goose_maps_system_to_its_native_flag() {
        let spec = by_id("goose").unwrap();
        let argv = (spec.build_argv)(&BuildCtx {
            system: Some("be terse"),
            ..base_ctx(spec)
        });
        assert!(
            argv.windows(2).any(|w| w == ["--system", "be terse"]),
            "{argv:?}"
        );
        // The prompt is delivered via -t and left untouched (not prepended).
        assert!(argv.windows(2).any(|w| w == ["-t", "hi"]), "{argv:?}");
    }

    #[test]
    fn harnesses_without_a_system_flag_prepend_it_to_the_prompt() {
        // Codex/OpenCode/Qwen/Crush/Copilot/Cursor expose no system-prompt flag,
        // so `--system` must be prepended to the prompt — never silently dropped.
        for id in ["codex", "opencode", "qwen", "crush", "copilot", "cursor"] {
            let spec = by_id(id).unwrap();
            let argv = (spec.build_argv)(&BuildCtx {
                system: Some("be terse"),
                ..base_ctx(spec)
            });
            assert!(
                argv.iter().any(|t| t == "be terse\n\nhi"),
                "harness {id} should carry the prepended prompt; got {argv:?}"
            );
            // The un-prefixed prompt must not also be sent on its own.
            assert!(
                !argv.iter().any(|t| t == "hi"),
                "harness {id} should not also send the bare prompt; got {argv:?}"
            );
        }
    }

    fn base_ctx(spec: &'static HarnessSpec) -> BuildCtx<'static> {
        BuildCtx {
            bin: spec.default_bin,
            prompt: "hi",
            model: None,
            system: None,
            resume: None,
            fork: false,
            mode: PermissionMode::Bypass,
            output_format: spec.output_format,
            schema: None,
        }
    }

    #[test]
    fn claude_native_schema_appends_json_schema_flag() {
        // The native structured-output path: claude-code carries the inline
        // schema on `--json-schema`, after `--output-format json`. Only this
        // adapter declares native support today.
        let spec = by_id("claude-code").unwrap();
        assert_eq!(spec.native_schema, Some(NativeSchema::ClaudeJsonSchema));
        let argv = (spec.build_argv)(&BuildCtx {
            schema: Some(r#"{"type":"object"}"#),
            ..base_ctx(spec)
        });
        assert!(
            argv.windows(2)
                .any(|w| w == ["--json-schema", r#"{"type":"object"}"#]),
            "{argv:?}"
        );
        assert!(
            argv.windows(2).any(|w| w == ["--output-format", "json"]),
            "native schema requires json output: {argv:?}"
        );
        // Without a schema the flag is absent.
        let argv = (spec.build_argv)(&base_ctx(spec));
        assert!(!argv.iter().any(|t| t == "--json-schema"), "{argv:?}");
    }

    #[test]
    fn claude_stream_json_adds_verbose_but_default_json_does_not() {
        // `-p --output-format stream-json` requires `--verbose` (Claude Code
        // errors otherwise); it is what surfaces the content-block transcript
        // oneharness normalizes into `events`. The default `json` result carries
        // no transcript and must NOT get `--verbose`.
        let spec = by_id("claude-code").unwrap();
        let stream = (spec.build_argv)(&BuildCtx {
            output_format: OutputFormat::StreamJson,
            ..base_ctx(spec)
        });
        assert!(
            stream
                .windows(2)
                .any(|w| w == ["--output-format", "stream-json"]),
            "{stream:?}"
        );
        assert!(
            stream.iter().any(|t| t == "--verbose"),
            "stream-json needs --verbose: {stream:?}"
        );
        let json = (spec.build_argv)(&base_ctx(spec));
        assert!(
            !json.iter().any(|t| t == "--verbose"),
            "default json must not add --verbose: {json:?}"
        );
    }

    #[test]
    fn claude_maps_resume_to_resume_flag() {
        let spec = by_id("claude-code").unwrap();
        assert!(spec.supports_resume);
        let argv = (spec.build_argv)(&BuildCtx {
            resume: Some("sess-123"),
            ..base_ctx(spec)
        });
        assert!(
            argv.windows(2).any(|w| w == ["--resume", "sess-123"]),
            "{argv:?}"
        );
    }

    #[test]
    fn every_harness_supports_resume() {
        // All eight CLIs expose a headless continuation flag (sourced per-adapter
        // from their docs); a new harness without one must flip this expectation
        // deliberately rather than silently start a fresh session.
        let unsupported: Vec<&str> = all()
            .iter()
            .filter(|h| !h.supports_resume)
            .map(|h| h.id)
            .collect();
        assert!(
            unsupported.is_empty(),
            "resume gaps drifted: {unsupported:?}"
        );
    }

    #[test]
    fn fork_supported_set_is_claude_and_opencode() {
        // Only Claude Code (`--fork-session`) and OpenCode (`--fork`) expose a
        // headless session fork; the rest resume linearly. A drift alarm for the
        // capability the fork feature depends on.
        let supported: std::collections::HashSet<&str> = all()
            .iter()
            .filter(|h| h.supports_fork)
            .map(|h| h.id)
            .collect();
        assert_eq!(
            supported,
            ["claude-code", "opencode"].into_iter().collect(),
            "supports_fork set drifted"
        );
        // Fork implies resume: nothing forks that cannot also resume.
        assert!(all().iter().all(|h| !h.supports_fork || h.supports_resume));
    }

    #[test]
    fn claude_maps_fork_to_fork_session_flag() {
        let spec = by_id("claude-code").unwrap();
        assert!(spec.supports_fork);
        let argv = (spec.build_argv)(&BuildCtx {
            resume: Some("sess-123"),
            fork: true,
            ..base_ctx(spec)
        });
        assert!(
            argv.windows(2).any(|w| w == ["--resume", "sess-123"]),
            "{argv:?}"
        );
        assert!(argv.iter().any(|t| t == "--fork-session"), "{argv:?}");
        // Without --fork the flag is absent (plain resume appends in place).
        let argv = (spec.build_argv)(&BuildCtx {
            resume: Some("sess-123"),
            ..base_ctx(spec)
        });
        assert!(!argv.iter().any(|t| t == "--fork-session"), "{argv:?}");
    }

    #[test]
    fn opencode_maps_fork_to_fork_flag() {
        let spec = by_id("opencode").unwrap();
        assert!(spec.supports_fork);
        let argv = (spec.build_argv)(&BuildCtx {
            resume: Some("ses_abc"),
            fork: true,
            ..base_ctx(spec)
        });
        assert!(
            argv.windows(2).any(|w| w == ["--session", "ses_abc"]),
            "{argv:?}"
        );
        assert!(argv.iter().any(|t| t == "--fork"), "{argv:?}");
    }

    #[test]
    fn codex_maps_resume_to_resume_subcommand_before_prompt() {
        // `codex exec resume <id> <prompt>`: the `resume` token follows `exec`,
        // and the id is the positional immediately before the prompt.
        let spec = by_id("codex").unwrap();
        assert!(spec.supports_resume && !spec.supports_fork);
        let argv = (spec.build_argv)(&BuildCtx {
            resume: Some("0199-thread"),
            ..base_ctx(spec)
        });
        assert!(
            argv.windows(2).any(|w| w == ["exec", "resume"]),
            "resume is a subcommand after exec: {argv:?}"
        );
        // id directly precedes the prompt positional.
        assert!(
            argv.windows(2).any(|w| w == ["0199-thread", "hi"]),
            "{argv:?}"
        );
        // No fork token for codex.
        assert!(!argv.iter().any(|t| t == "--fork"), "{argv:?}");
    }

    #[test]
    fn goose_maps_resume_to_named_session() {
        // Goose emits no id headlessly; continuation rides a caller-chosen name:
        // `--resume --name <name>`.
        let spec = by_id("goose").unwrap();
        assert!(spec.supports_resume);
        let argv = (spec.build_argv)(&BuildCtx {
            resume: Some("my-session"),
            ..base_ctx(spec)
        });
        assert!(argv.iter().any(|t| t == "--resume"), "{argv:?}");
        assert!(
            argv.windows(2).any(|w| w == ["--name", "my-session"]),
            "{argv:?}"
        );
    }

    #[test]
    fn qwen_maps_resume_to_resume_flag() {
        let spec = by_id("qwen").unwrap();
        assert!(spec.supports_resume);
        let argv = (spec.build_argv)(&BuildCtx {
            resume: Some("uuid-1"),
            ..base_ctx(spec)
        });
        assert!(
            argv.windows(2).any(|w| w == ["--resume", "uuid-1"]),
            "{argv:?}"
        );
    }

    #[test]
    fn crush_maps_resume_to_session_flag() {
        let spec = by_id("crush").unwrap();
        assert!(spec.supports_resume);
        let argv = (spec.build_argv)(&BuildCtx {
            resume: Some("sess-9"),
            ..base_ctx(spec)
        });
        assert!(
            argv.windows(2).any(|w| w == ["--session", "sess-9"]),
            "{argv:?}"
        );
    }

    #[test]
    fn copilot_maps_resume_to_resume_flag() {
        let spec = by_id("copilot").unwrap();
        assert!(spec.supports_resume);
        let argv = (spec.build_argv)(&BuildCtx {
            resume: Some("uuid-c"),
            ..base_ctx(spec)
        });
        assert!(
            argv.windows(2).any(|w| w == ["--resume", "uuid-c"]),
            "{argv:?}"
        );
    }

    #[test]
    fn opencode_maps_resume_to_session_flag() {
        let spec = by_id("opencode").unwrap();
        assert!(spec.supports_resume);
        let argv = (spec.build_argv)(&BuildCtx {
            resume: Some("ses_abc"),
            ..base_ctx(spec)
        });
        assert!(
            argv.windows(2).any(|w| w == ["--session", "ses_abc"]),
            "{argv:?}"
        );
    }

    #[test]
    fn cursor_maps_resume_to_resume_flag() {
        let spec = by_id("cursor").unwrap();
        assert!(spec.supports_resume);
        let argv = (spec.build_argv)(&BuildCtx {
            resume: Some("chat-9"),
            ..base_ctx(spec)
        });
        assert!(
            argv.windows(2).any(|w| w == ["--resume", "chat-9"]),
            "{argv:?}"
        );
    }

    #[test]
    fn cursor_no_bypass_trusts_the_workspace_without_force() {
        let spec = by_id("cursor").unwrap();
        let argv = (spec.build_argv)(&BuildCtx {
            mode: PermissionMode::Default,
            ..base_ctx(spec)
        });
        assert!(argv.iter().any(|t| t == "--trust"), "{argv:?}");
        assert!(!argv.iter().any(|t| t == "--force"), "{argv:?}");
        // Bypass mode keeps the plain --force (which implies trust).
        let argv = (spec.build_argv)(&base_ctx(spec));
        assert!(argv.iter().any(|t| t == "--force"), "{argv:?}");
        assert!(!argv.iter().any(|t| t == "--trust"), "{argv:?}");
    }

    #[test]
    fn qwen_alone_declares_the_yolo_suppression_default_env() {
        // Qwen prints a one-line YOLO/no-sandbox warning to stderr under `--yolo`;
        // oneharness silences it so headless `stderr` stays clean. No other
        // harness needs a default env today — guard that the set hasn't drifted.
        for h in all() {
            if h.id == "qwen" {
                assert_eq!(
                    h.default_env,
                    &[("QWEN_CODE_SUPPRESS_YOLO_WARNING", "1")],
                    "qwen should suppress its YOLO warning"
                );
            } else {
                assert!(
                    h.default_env.is_empty(),
                    "harness {} unexpectedly declares default env",
                    h.id
                );
            }
        }
    }

    #[test]
    fn bin_override_lands_at_argv0_for_every_harness() {
        for h in all() {
            let argv = (h.build_argv)(&ctx("/custom/bin", None, PermissionMode::Bypass));
            assert_eq!(argv[0], "/custom/bin", "harness {}", h.id);
        }
    }

    #[test]
    fn model_flag_is_emitted_for_every_model_aware_harness() {
        // Each harness that accepts a model spells the flag its own way; `--model`
        // must reach the child via that spelling, never be dropped. Goose is the
        // sole exception — it selects its model from its own config — so its argv
        // is identical with and without a model (asserted separately).
        let expected: &[(&str, &[&str])] = &[
            ("claude-code", &["--model", "m"]),
            ("codex", &["--model", "m"]),
            ("opencode", &["-m", "m"]),
            ("qwen", &["-m", "m"]),
            ("crush", &["-m", "m"]),
            ("copilot", &["--model", "m"]),
            ("cursor", &["--model", "m"]),
        ];
        for (id, want) in expected {
            let spec = by_id(id).unwrap();
            let argv = (spec.build_argv)(&ctx(spec.default_bin, Some("m"), PermissionMode::Bypass));
            assert!(
                argv.windows(2).any(|w| w == *want),
                "harness {id} should carry {want:?}; got {argv:?}"
            );
        }
        // Goose deliberately ignores the model: argv is unchanged when one is set.
        let goose = by_id("goose").unwrap();
        let with = (goose.build_argv)(&ctx("goose", Some("m"), PermissionMode::Bypass));
        let without = (goose.build_argv)(&ctx("goose", None, PermissionMode::Bypass));
        assert_eq!(with, without, "goose should ignore --model");
    }
}
