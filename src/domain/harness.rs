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
        build_argv: argv_claude_code,
    },
    HarnessSpec {
        id: "codex",
        display: "OpenAI Codex CLI",
        default_bin: "codex",
        install_hint: "npm install -g @openai/codex",
        output_format: OutputFormat::Text,
        build_argv: argv_codex,
    },
    HarnessSpec {
        id: "opencode",
        display: "OpenCode",
        default_bin: "opencode",
        install_hint: "npm install -g opencode-ai",
        output_format: OutputFormat::Json,
        build_argv: argv_opencode,
    },
    HarnessSpec {
        id: "goose",
        display: "Goose",
        default_bin: "goose",
        install_hint: "see https://block.github.io/goose/docs/getting-started/installation",
        output_format: OutputFormat::Text,
        build_argv: argv_goose,
    },
    HarnessSpec {
        id: "qwen",
        display: "Qwen Code",
        default_bin: "qwen",
        install_hint: "npm install -g @qwen-code/qwen-code",
        output_format: OutputFormat::Text,
        build_argv: argv_qwen,
    },
    HarnessSpec {
        id: "crush",
        display: "Crush",
        default_bin: "crush",
        install_hint: "npm install -g @charmland/crush",
        output_format: OutputFormat::Text,
        build_argv: argv_crush,
    },
    HarnessSpec {
        id: "copilot",
        display: "GitHub Copilot CLI",
        default_bin: "copilot",
        install_hint: "npm install -g @github/copilot",
        output_format: OutputFormat::Text,
        build_argv: argv_copilot,
    },
    HarnessSpec {
        id: "cursor",
        display: "Cursor CLI",
        default_bin: "cursor-agent",
        install_hint: "see https://docs.cursor.com/en/cli/overview",
        output_format: OutputFormat::StreamJson,
        build_argv: argv_cursor,
    },
];

/// `claude -p <prompt> --permission-mode <mode> [--model M] --output-format json`
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
    a.push("--output-format".into());
    a.push(format_flag(c.output_format).into());
    a
}

/// `codex exec [--sandbox danger-full-access -a never] [--model M] <prompt>`
fn argv_codex(c: &BuildCtx) -> Vec<String> {
    let mut a = vec![c.bin.into(), "exec".into()];
    if c.bypass {
        a.push("--sandbox".into());
        a.push("danger-full-access".into());
        a.push("-a".into());
        a.push("never".into());
    }
    if let Some(m) = c.model {
        a.push("--model".into());
        a.push(m.into());
    }
    a.push(c.prompt.into());
    a
}

/// `opencode run [--dangerously-skip-permissions] --format json [-m M] <prompt>`
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
    a.push(c.prompt.into());
    a
}

/// `goose run --with-builtin developer -t <prompt>`
///
/// Goose has no headless permission prompt and selects its model from its own
/// config, so `bypass` and `model` are intentionally not mapped.
fn argv_goose(c: &BuildCtx) -> Vec<String> {
    vec![
        c.bin.into(),
        "run".into(),
        "--with-builtin".into(),
        "developer".into(),
        "-t".into(),
        c.prompt.into(),
    ]
}

/// `qwen [--yolo] [-m M] -p <prompt>`
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
    a.push(c.prompt.into());
    a
}

/// `crush run -q [-m M] <prompt>` (`run` is non-interactive; `-q` quiets it)
fn argv_crush(c: &BuildCtx) -> Vec<String> {
    let mut a = vec![c.bin.into(), "run".into(), "-q".into()];
    if let Some(m) = c.model {
        a.push("-m".into());
        a.push(m.into());
    }
    a.push(c.prompt.into());
    a
}

/// `copilot -p <prompt> [--allow-all-tools --allow-all-paths --no-ask-user] [--model M]`
fn argv_copilot(c: &BuildCtx) -> Vec<String> {
    let mut a = vec![c.bin.into(), "-p".into(), c.prompt.into()];
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

/// `cursor-agent -p <prompt> [--force] [--model M] --output-format stream-json`
fn argv_cursor(c: &BuildCtx) -> Vec<String> {
    let mut a = vec![c.bin.into(), "-p".into(), c.prompt.into()];
    if c.bypass {
        a.push("--force".into());
    }
    if let Some(m) = c.model {
        a.push("--model".into());
        a.push(m.into());
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
    fn codex_argv_uses_exec_and_sandbox() {
        let spec = by_id("codex").unwrap();
        let argv = (spec.build_argv)(&ctx("codex", None, true));
        assert_eq!(
            argv,
            vec![
                "codex",
                "exec",
                "--sandbox",
                "danger-full-access",
                "-a",
                "never",
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
    fn bin_override_lands_at_argv0_for_every_harness() {
        for h in all() {
            let argv = (h.build_argv)(&ctx("/custom/bin", None, true));
            assert_eq!(argv[0], "/custom/bin", "harness {}", h.id);
        }
    }
}
