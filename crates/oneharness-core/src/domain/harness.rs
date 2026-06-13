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
use crate::domain::report::OutputFormat;

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
    pub bypass: bool,
    pub output_format: OutputFormat,
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
    /// Whether this harness can continue a prior session (`run --resume`). When
    /// false, the command layer rejects `--resume` for it rather than silently
    /// starting a fresh session. Kept as data so the capability is introspectable
    /// via `oneharness list`.
    pub supports_resume: bool,
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
    /// Environment variables oneharness sets when spawning this harness, so a
    /// headless run is clean without the caller knowing the harness's quirks
    /// (e.g. silencing a startup warning that would otherwise litter `stderr`).
    /// Pure data: the registry declares them; the command/io layer injects them,
    /// and an explicit `--env` always wins over a default here. Empty for most.
    pub default_env: &'static [(&'static str, &'static str)],
    /// Builds the full argv (argv[0] is the binary). Pure.
    pub build_argv: fn(&BuildCtx) -> Vec<String>,
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
/// cwd}` on stdin, and throws to block on a `{"decision":"deny"}` reply —
/// failing open if the command cannot run. Mirrors the known-good allowlister
/// shim; pinned in the `io::hooks` tests.
const OPENCODE_PLUGIN_JS: &str = r#"// {name} OpenCode plugin — installed by oneharness.
//
// OpenCode can block a tool call only from an in-process plugin, so this shim
// bridges to an external command: before any tool runs it spawns the command,
// piping the tool name and arguments as JSON on stdin, and throws to block when
// the command replies with {"decision":"deny"}. Re-running sync regenerates it.

export const {export} = async ({ directory }) => ({
  "tool.execute.before": async (input, output) => {
    const tool_name = (input && input.tool) || "";
    if (!tool_name) return;
    const args = (output && output.args) || {};
    const cwd = args.workdir || directory || ".";
    const event = JSON.stringify({ tool_name, tool_input: args, cwd });

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
        supports_resume: true,
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
                with_timeout: false,
            },
            path: &["hooks"],
        }),
        global_hook: Some(GlobalHook {
            base: HookBase::Home,
            anchor: ".claude/settings.json",
        }),
        gate_deny: Some(DenyShape::ClaudeNested),
        default_env: &[],
        build_argv: argv_claude_code,
    },
    HarnessSpec {
        id: "codex",
        display: "OpenAI Codex CLI",
        default_bin: "codex",
        install_hint: "npm install -g @openai/codex",
        output_format: OutputFormat::Text,
        supports_resume: false,
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
        default_env: &[],
        build_argv: argv_codex,
    },
    HarnessSpec {
        id: "opencode",
        display: "OpenCode",
        default_bin: "opencode",
        install_hint: "npm install -g opencode-ai",
        output_format: OutputFormat::Json,
        supports_resume: true,
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
        default_env: &[],
        build_argv: argv_opencode,
    },
    HarnessSpec {
        id: "goose",
        display: "Goose",
        default_bin: "goose",
        install_hint: "see https://block.github.io/goose/docs/getting-started/installation",
        output_format: OutputFormat::Text,
        supports_resume: false,
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
        default_env: &[],
        build_argv: argv_goose,
    },
    HarnessSpec {
        id: "qwen",
        display: "Qwen Code",
        default_bin: "qwen",
        install_hint: "npm install -g @qwen-code/qwen-code",
        output_format: OutputFormat::Text,
        supports_resume: false,
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
        default_env: &[("QWEN_CODE_SUPPRESS_YOLO_WARNING", "1")],
        build_argv: argv_qwen,
    },
    HarnessSpec {
        id: "crush",
        display: "Crush",
        default_bin: "crush",
        install_hint: "npm install -g @charmland/crush",
        output_format: OutputFormat::Text,
        supports_resume: false,
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
        default_env: &[],
        build_argv: argv_crush,
    },
    HarnessSpec {
        id: "copilot",
        display: "GitHub Copilot CLI",
        default_bin: "copilot",
        install_hint: "npm install -g @github/copilot",
        output_format: OutputFormat::Text,
        supports_resume: false,
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
        default_env: &[],
        build_argv: argv_copilot,
    },
    HarnessSpec {
        id: "cursor",
        display: "Cursor CLI",
        default_bin: "cursor-agent",
        install_hint: "see https://docs.cursor.com/en/cli/overview",
        output_format: OutputFormat::StreamJson,
        supports_resume: true,
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
        default_env: &[],
        build_argv: argv_cursor,
    },
];

/// `claude -p <prompt> --permission-mode <mode> [--model M]
/// [--append-system-prompt S] --output-format json`
fn argv_claude_code(c: &BuildCtx) -> Vec<String> {
    let mut a = vec![c.bin.into(), "-p".into(), c.prompt.into()];
    a.push("--permission-mode".into());
    a.push(
        if c.bypass {
            "bypassPermissions"
        } else {
            "default"
        }
        .into(),
    );
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
    }
    a.push("--output-format".into());
    a.push(format_flag(c.output_format).into());
    a
}

/// `codex exec [--dangerously-bypass-approvals-and-sandbox] [--model M] <prompt>`
///
/// Codex exposes no system-prompt flag, so `--system` is prepended to the prompt.
/// The single bypass flag replaces the older `--sandbox danger-full-access -a
/// never`: codex-cli >= 0.135 removed `-a`, and this flag is the supported way to
/// skip every approval prompt and the sandbox for a headless run.
fn argv_codex(c: &BuildCtx) -> Vec<String> {
    let mut a = vec![c.bin.into(), "exec".into()];
    if c.bypass {
        a.push("--dangerously-bypass-approvals-and-sandbox".into());
    }
    if let Some(m) = c.model {
        a.push("--model".into());
        a.push(m.into());
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
    if c.bypass {
        a.push("--dangerously-skip-permissions".into());
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
    }
    a.push(prompt_with_system(c));
    a
}

/// `goose run --with-builtin developer [--system S] -t <prompt>`
///
/// Goose has no headless permission prompt and selects its model from its own
/// config, so `bypass` and `model` are intentionally not mapped. It does expose a
/// native `--system` flag, so `--system` maps to it rather than being prepended.
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
    a.push("-t".into());
    a.push(c.prompt.into());
    a
}

/// `qwen [--yolo] [-m M] -p <prompt>` (no system flag, so `--system` is prepended)
fn argv_qwen(c: &BuildCtx) -> Vec<String> {
    let mut a = vec![c.bin.into()];
    if c.bypass {
        a.push("--yolo".into());
    }
    if let Some(m) = c.model {
        a.push("-m".into());
        a.push(m.into());
    }
    a.push("-p".into());
    a.push(prompt_with_system(c));
    a
}

/// `crush run -q [-m M] <prompt>` (`run` is non-interactive; `-q` quiets it; no
/// system flag, so `--system` is prepended to the prompt)
fn argv_crush(c: &BuildCtx) -> Vec<String> {
    let mut a = vec![c.bin.into(), "run".into(), "-q".into()];
    if let Some(m) = c.model {
        a.push("-m".into());
        a.push(m.into());
    }
    a.push(prompt_with_system(c));
    a
}

/// `copilot -p <prompt> [--allow-all-tools --allow-all-paths --no-ask-user]
/// [--model M]` (no system flag, so `--system` is prepended to the prompt;
/// its `--allow-tool`/`--deny-tool` permission flags are not unified — Copilot
/// has no project config file to sync, so rules go via `[harness.copilot] args`)
fn argv_copilot(c: &BuildCtx) -> Vec<String> {
    let mut a = vec![c.bin.into(), "-p".into(), prompt_with_system(c)];
    if c.bypass {
        a.push("--allow-all-tools".into());
        a.push("--allow-all-paths".into());
        a.push("--no-ask-user".into());
    }
    if let Some(m) = c.model {
        a.push("--model".into());
        a.push(m.into());
    }
    a
}

/// `cursor-agent -p <prompt> [--force|--trust] [--model M] [--resume SID]
/// --output-format stream-json` (Cursor continues a chat id with `--resume`; no
/// system flag, so `--system` is prepended to the prompt)
fn argv_cursor(c: &BuildCtx) -> Vec<String> {
    let mut a = vec![c.bin.into(), "-p".into(), prompt_with_system(c)];
    if c.bypass {
        a.push("--force".into());
    } else {
        // A headless run cannot answer Cursor's interactive workspace-trust
        // prompt, and without trust the CLI refuses to run at all ("Workspace
        // Trust Required", observed live). `--trust` trusts the directory the
        // caller pointed oneharness at while leaving the permission system
        // active — so --no-bypass still means "normal permission flow", not
        // "cannot run".
        a.push("--trust".into());
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

    fn ctx<'a>(bin: &'a str, model: Option<&'a str>, bypass: bool) -> BuildCtx<'a> {
        ctx_fmt(bin, model, bypass, OutputFormat::Json)
    }

    fn ctx_fmt<'a>(
        bin: &'a str,
        model: Option<&'a str>,
        bypass: bool,
        output_format: OutputFormat,
    ) -> BuildCtx<'a> {
        BuildCtx {
            bin,
            prompt: "hi",
            model,
            system: None,
            resume: None,
            bypass,
            output_format,
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

    #[test]
    fn claude_argv_bypass_on() {
        let spec = by_id("claude-code").unwrap();
        let argv = (spec.build_argv)(&ctx("claude", None, true));
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
    fn claude_argv_no_bypass_uses_default_mode() {
        let spec = by_id("claude-code").unwrap();
        let argv = (spec.build_argv)(&ctx("claude", Some("haiku"), false));
        assert_eq!(
            argv,
            vec![
                "claude",
                "-p",
                "hi",
                "--permission-mode",
                "default",
                "--model",
                "haiku",
                "--output-format",
                "json"
            ]
        );
    }

    #[test]
    fn codex_argv_uses_exec_and_bypass_flag() {
        let spec = by_id("codex").unwrap();
        let argv = (spec.build_argv)(&ctx("codex", None, true));
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
    fn goose_ignores_model_and_bypass() {
        let spec = by_id("goose").unwrap();
        let with = (spec.build_argv)(&ctx("goose", Some("gpt"), true));
        let without = (spec.build_argv)(&ctx("goose", None, false));
        assert_eq!(with, without);
        assert_eq!(
            with,
            vec!["goose", "run", "--with-builtin", "developer", "-t", "hi"]
        );
    }

    #[test]
    fn output_format_override_changes_the_emitted_flag() {
        let spec = by_id("claude-code").unwrap();
        let argv = (spec.build_argv)(&ctx_fmt("claude", None, true, OutputFormat::StreamJson));
        assert!(
            argv.windows(2)
                .any(|w| w == ["--output-format", "stream-json"]),
            "{argv:?}"
        );
        // opencode spells its flag `--format`.
        let oc = by_id("opencode").unwrap();
        let argv = (oc.build_argv)(&ctx_fmt("opencode", None, true, OutputFormat::Text));
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
            bypass: true,
            output_format: OutputFormat::Json,
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
            bypass: true,
            output_format: spec.output_format,
        }
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
    fn resume_supported_set_is_claude_opencode_cursor() {
        let supported: std::collections::HashSet<&str> = all()
            .iter()
            .filter(|h| h.supports_resume)
            .map(|h| h.id)
            .collect();
        assert_eq!(
            supported,
            ["claude-code", "opencode", "cursor"].into_iter().collect(),
            "supports_resume set drifted"
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
            bypass: false,
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
            let argv = (h.build_argv)(&ctx("/custom/bin", None, true));
            assert_eq!(argv[0], "/custom/bin", "harness {}", h.id);
        }
    }
}
