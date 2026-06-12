//! The harness registry: one declarative adapter per supported CLI.
//!
//! An adapter is data — a canonical id, a default binary, an install hint, an
//! output format — plus one pure function that builds the argv. Adding a harness
//! is adding an entry here; `run`, the runner, and the report shape are untouched.
//!
//! The flags encoded below mirror the known-good non-interactive invocations used
//! to drive each real CLI headlessly (deny prompts, pick the model, request a
//! parseable format). Source new flags from a working driver, not by guessing.

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
    /// Tool/permission rules, in the harness's native syntax (Claude Code's
    /// `Bash(git log:*)`, Copilot's `shell(git)`, Qwen's tool names). Only set
    /// after the command layer has verified `supports_allowed_tools` /
    /// `supports_denied_tools` — enforcement settings are never dropped
    /// silently, so an adapter that maps them can assume support.
    pub allowed_tools: &'a [String],
    pub denied_tools: &'a [String],
    /// The `[harness.<id>.hooks]` table serialized to JSON, for harnesses that
    /// accept lifecycle hooks via their invocation (Claude Code's `--settings`).
    /// Verified against `supports_hooks` by the command layer. Pure data:
    /// oneharness never interprets the hooks, it only delivers them.
    pub hooks_json: Option<&'a str>,
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
    /// Whether `allowed_tools` / `denied_tools` rules can be enforced through
    /// this harness's headless invocation. When false, a configured rule for it
    /// is a usage error — a permission rule that silently doesn't apply is a
    /// security footgun, so absence is loud (mirrors `--resume`). Sourced from
    /// each CLI's documented flags: Claude Code's `--allowedTools` /
    /// `--disallowedTools`, Copilot's `--allow-tool` / `--deny-tool`, Qwen's
    /// `--allowed-tools` (allow only). The rest gate permissions behind their
    /// own config files (opencode.json, crush.json, Cursor's cli-config.json,
    /// Codex/Goose sandbox-and-approval modes), which oneharness does not write.
    pub supports_allowed_tools: bool,
    pub supports_denied_tools: bool,
    /// Whether lifecycle hooks can be delivered through the invocation (Claude
    /// Code's `--settings` JSON). Harnesses whose hooks live only in config
    /// files on disk (Copilot, Cursor, OpenCode plugins) are `false`: oneharness
    /// wires invocations, it does not write into a project's config.
    pub supports_hooks: bool,
    /// Environment variables oneharness sets when spawning this harness, so a
    /// headless run is clean without the caller knowing the harness's quirks
    /// (e.g. silencing a startup warning that would otherwise litter `stderr`).
    /// Pure data: the registry declares them; the command/io layer injects them,
    /// and an explicit `--env` always wins over a default here. Empty for most.
    pub default_env: &'static [(&'static str, &'static str)],
    /// Builds the full argv (argv[0] is the binary). Pure.
    pub build_argv: fn(&BuildCtx) -> Vec<String>,
}

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
        supports_allowed_tools: true,
        supports_denied_tools: true,
        supports_hooks: true,
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
        supports_allowed_tools: false,
        supports_denied_tools: false,
        supports_hooks: false,
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
        supports_allowed_tools: false,
        supports_denied_tools: false,
        supports_hooks: false,
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
        supports_allowed_tools: false,
        supports_denied_tools: false,
        supports_hooks: false,
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
        supports_allowed_tools: true,
        supports_denied_tools: false,
        supports_hooks: false,
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
        supports_allowed_tools: false,
        supports_denied_tools: false,
        supports_hooks: false,
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
        supports_allowed_tools: true,
        supports_denied_tools: true,
        supports_hooks: false,
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
        supports_allowed_tools: false,
        supports_denied_tools: false,
        supports_hooks: false,
        default_env: &[],
        build_argv: argv_cursor,
    },
];

/// `claude -p <prompt> --permission-mode <mode> [--allowedTools R…]
/// [--disallowedTools R…] [--settings {"hooks":…}] [--model M]
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
    // `--allowedTools` / `--disallowedTools` are variadic: one flag, then each
    // rule as its own argv token (the documented form). Rules never begin with
    // `-`, so the next `--flag` ends the list.
    if !c.allowed_tools.is_empty() {
        a.push("--allowedTools".into());
        a.extend(c.allowed_tools.iter().cloned());
    }
    if !c.denied_tools.is_empty() {
        a.push("--disallowedTools".into());
        a.extend(c.denied_tools.iter().cloned());
    }
    // Hooks travel as an inline session-scoped settings document.
    if let Some(hooks) = c.hooks_json {
        a.push("--settings".into());
        a.push(format!("{{\"hooks\":{hooks}}}"));
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

/// `qwen [--yolo] [--allowed-tools R,R] [-m M] -p <prompt>` (no system flag, so
/// `--system` is prepended; `--allowed-tools` takes one comma-separated list of
/// tool names that bypass the confirmation dialog — qwen has no deny flag)
fn argv_qwen(c: &BuildCtx) -> Vec<String> {
    let mut a = vec![c.bin.into()];
    if c.bypass {
        a.push("--yolo".into());
    }
    if !c.allowed_tools.is_empty() {
        a.push("--allowed-tools".into());
        a.push(c.allowed_tools.join(","));
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
/// [--allow-tool R]… [--deny-tool R]… [--model M]` (no system flag, so
/// `--system` is prepended; allow/deny flags are repeatable, one rule each)
fn argv_copilot(c: &BuildCtx) -> Vec<String> {
    let mut a = vec![c.bin.into(), "-p".into(), prompt_with_system(c)];
    if c.bypass {
        a.push("--allow-all-tools".into());
        a.push("--allow-all-paths".into());
        a.push("--no-ask-user".into());
    }
    for rule in c.allowed_tools {
        a.push("--allow-tool".into());
        a.push(rule.clone());
    }
    for rule in c.denied_tools {
        a.push("--deny-tool".into());
        a.push(rule.clone());
    }
    if let Some(m) = c.model {
        a.push("--model".into());
        a.push(m.into());
    }
    a
}

/// `cursor-agent -p <prompt> [--force] [--model M] [--resume SID]
/// --output-format stream-json` (Cursor continues a chat id with `--resume`; no
/// system flag, so `--system` is prepended to the prompt)
fn argv_cursor(c: &BuildCtx) -> Vec<String> {
    let mut a = vec![c.bin.into(), "-p".into(), prompt_with_system(c)];
    if c.bypass {
        a.push("--force".into());
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
            allowed_tools: &[],
            denied_tools: &[],
            hooks_json: None,
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
            allowed_tools: &[],
            denied_tools: &[],
            hooks_json: None,
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
            allowed_tools: &[],
            denied_tools: &[],
            hooks_json: None,
            bypass: true,
            output_format: spec.output_format,
        }
    }

    #[test]
    fn allow_deny_capability_sets_match_documented_flags() {
        // claude-code (--allowedTools/--disallowedTools), copilot
        // (--allow-tool/--deny-tool), and qwen (--allowed-tools, allow only)
        // are the harnesses with documented headless permission flags. Guard
        // the sets so a registry edit can't silently claim (or drop) support.
        let allowed: std::collections::HashSet<&str> = all()
            .iter()
            .filter(|h| h.supports_allowed_tools)
            .map(|h| h.id)
            .collect();
        assert_eq!(
            allowed,
            ["claude-code", "copilot", "qwen"].into_iter().collect()
        );
        let denied: std::collections::HashSet<&str> = all()
            .iter()
            .filter(|h| h.supports_denied_tools)
            .map(|h| h.id)
            .collect();
        assert_eq!(denied, ["claude-code", "copilot"].into_iter().collect());
        let hooks: Vec<&str> = all()
            .iter()
            .filter(|h| h.supports_hooks)
            .map(|h| h.id)
            .collect();
        assert_eq!(hooks, ["claude-code"]);
    }

    #[test]
    fn claude_maps_allow_deny_as_variadic_flags() {
        let spec = by_id("claude-code").unwrap();
        let allowed = vec!["Bash(git log:*)".to_string(), "Read".to_string()];
        let denied = vec!["Bash(rm:*)".to_string()];
        let argv = (spec.build_argv)(&BuildCtx {
            allowed_tools: &allowed,
            denied_tools: &denied,
            ..base_ctx(spec)
        });
        assert!(
            argv.windows(3)
                .any(|w| w == ["--allowedTools", "Bash(git log:*)", "Read"]),
            "{argv:?}"
        );
        assert!(
            argv.windows(2)
                .any(|w| w == ["--disallowedTools", "Bash(rm:*)"]),
            "{argv:?}"
        );
    }

    #[test]
    fn claude_maps_hooks_into_inline_settings_json() {
        let spec = by_id("claude-code").unwrap();
        let argv = (spec.build_argv)(&BuildCtx {
            hooks_json: Some(r#"{"PreToolUse":[]}"#),
            ..base_ctx(spec)
        });
        assert!(
            argv.windows(2)
                .any(|w| w == ["--settings", r#"{"hooks":{"PreToolUse":[]}}"#]),
            "{argv:?}"
        );
    }

    #[test]
    fn copilot_repeats_allow_and_deny_flags_per_rule() {
        let spec = by_id("copilot").unwrap();
        let allowed = vec!["shell(git)".to_string(), "write".to_string()];
        let denied = vec!["shell(rm)".to_string()];
        let argv = (spec.build_argv)(&BuildCtx {
            allowed_tools: &allowed,
            denied_tools: &denied,
            ..base_ctx(spec)
        });
        assert!(
            argv.windows(4)
                .any(|w| w == ["--allow-tool", "shell(git)", "--allow-tool", "write"]),
            "{argv:?}"
        );
        assert!(
            argv.windows(2).any(|w| w == ["--deny-tool", "shell(rm)"]),
            "{argv:?}"
        );
    }

    #[test]
    fn qwen_joins_allowed_tools_with_commas() {
        let spec = by_id("qwen").unwrap();
        let allowed = vec!["ShellTool(git status)".to_string(), "WebFetch".to_string()];
        let argv = (spec.build_argv)(&BuildCtx {
            allowed_tools: &allowed,
            ..base_ctx(spec)
        });
        assert!(
            argv.windows(2)
                .any(|w| w == ["--allowed-tools", "ShellTool(git status),WebFetch"]),
            "{argv:?}"
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
