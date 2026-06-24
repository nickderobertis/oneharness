//! Parsing a Windows `.cmd`/`.bat` command-shim into a direct invocation.
//!
//! Pure string logic; the file read and the spawn decision live in the runner
//! (`crate::io::runner`). The *why* lives there too: since Rust 1.77 (the
//! CVE-2024-24576 fix) `std::process::Command` runs a `.cmd`/`.bat` through
//! cmd.exe but returns `InvalidInput` for any argument it cannot escape for
//! cmd.exe — a newline is the trigger ("batch file arguments are invalid"). npm
//! installs every JS harness (claude, codex, qwen, …) as a `claude.cmd` shim, so
//! a multi-line argument (a rendered `--system`/`--prompt`) fails to spawn on
//! Windows. The fix is to bypass the batch wrapper and invoke the underlying
//! interpreter (node) and script directly: node is a real `.exe`, so std uses the
//! ordinary `CreateProcess` argument encoding, which carries newlines fine.
//!
//! npm's `cmd-shim` emits a small, stable batch file. Two layouts exist in the
//! wild and both are handled: the older one references the script through `%~dp0`
//! directly, the newer one captures `%~dp0` into a `dp0` variable and uses
//! `%dp0%`. Either way the interpreter is named by a `SET "_prog=…"` line and the
//! script (plus any interpreter flags) sits on the final `"%_prog%" … %*` line.

/// How to invoke a shim's underlying program directly, sidestepping the batch
/// wrapper. `interpreter` is the program to spawn — a bare name to resolve on
/// `PATH` (e.g. `node`) or an absolute path. `prefix_args` are the arguments the
/// shim passes ahead of the caller's own: the target script path, plus any
/// interpreter flags, with `%dp0%`/`%~dp0` already expanded to the shim's
/// directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShimTarget {
    pub interpreter: String,
    pub prefix_args: Vec<String>,
}

/// Parse a Windows command-shim's `contents`, expanding `%dp0%`/`%~dp0` to
/// `shim_dir` (the directory the `.cmd` file lives in). Returns `None` when the
/// text is not a shim layout we recognize, so the caller can fall back to
/// spawning the `.cmd` directly (preserving the exact prior behavior, error and
/// all). Pure: no I/O, so it is unit-tested on every platform.
pub fn parse_cmd_shim(contents: &str, shim_dir: &str) -> Option<ShimTarget> {
    // `%~dp0` expands to the batch file's drive+path *with* a trailing
    // separator; the shim then writes `%dp0%\script`, so stripping a trailing
    // separator here yields a single clean separator after expansion.
    let dir = shim_dir.trim_end_matches(['\\', '/']);
    let expand = |s: &str| s.replace("%~dp0", dir).replace("%dp0%", dir);

    // The interpreter: a `SET "_prog=VALUE"` line names it. Prefer a bare name
    // (resolved on PATH, the shim's own ELSE fallback) over the colocated
    // `%dp0%\node.exe` form — the bare name spawns without assuming node sits
    // beside the shim, which it does not for a normal global npm install.
    let mut bare: Option<String> = None;
    let mut colocated: Option<String> = None;
    for line in contents.lines() {
        if let Some(val) = parse_set_prog(line) {
            if val.contains("%dp0%") || val.contains("%~dp0") {
                colocated.get_or_insert_with(|| expand(&val));
            } else {
                bare.get_or_insert(val);
            }
        }
    }
    let interpreter = bare.or(colocated)?;

    // The exec line carries the script path and any interpreter flags between
    // `"%_prog%"` and the `%*` that forwards the caller's own arguments.
    let exec = contents
        .lines()
        .find(|l| l.contains("%_prog%") && l.contains("%*"))?;
    let after_prog = exec.split("%_prog%").nth(1)?;
    // Drop the closing quote of `"%_prog%"`, then take everything up to `%*`.
    let middle = after_prog
        .trim_start()
        .strip_prefix('"')
        .unwrap_or(after_prog)
        .split("%*")
        .next()?;
    let prefix_args: Vec<String> = tokenize(middle).into_iter().map(|t| expand(&t)).collect();
    if prefix_args.is_empty() {
        return None;
    }
    Some(ShimTarget {
        interpreter,
        prefix_args,
    })
}

/// Extract `VALUE` from a `SET "_prog=VALUE"` batch line (case-insensitive `SET`,
/// optional leading `@`), or `None` if the line is not one.
fn parse_set_prog(line: &str) -> Option<String> {
    let trimmed = line.trim().strip_prefix('@').unwrap_or(line.trim());
    if !trimmed.to_ascii_lowercase().starts_with("set ") {
        return None;
    }
    let start = trimmed.find("\"_prog=")? + "\"_prog=".len();
    let rest = &trimmed[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Split a batch command fragment into arguments, honoring double quotes and
/// dropping them (cmd.exe's own argument rule). Whitespace separates tokens
/// outside quotes; quoted runs are kept verbatim.
fn tokenize(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut has_token = false;
    for ch in s.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                // An opening quote starts a token even if it ends up empty.
                has_token = true;
            }
            c if c.is_whitespace() && !in_quotes => {
                if has_token {
                    out.push(std::mem::take(&mut cur));
                    has_token = false;
                }
            }
            c => {
                cur.push(c);
                has_token = true;
            }
        }
    }
    if has_token {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // The newer npm cmd-shim layout: `%~dp0` captured into `dp0`, script via
    // `%dp0%`, both a colocated and a bare `_prog`.
    const NEWER: &str = r#"@ECHO off
GOTO start
:find_dp0
SET dp0=%~dp0
EXIT /b
:start
SETLOCAL
CALL :find_dp0

IF EXIST "%dp0%\node.exe" (
  SET "_prog=%dp0%\node.exe"
) ELSE (
  SET "_prog=node"
  SET PATHEXT=%PATHEXT:;.JS;=;%
)

endLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & "%_prog%"  "%dp0%\node_modules\@anthropic-ai\claude-code\cli.js" %*
"#;

    // The older layout: the script is referenced through `%~dp0` directly.
    const OLDER: &str = r#"@IF EXIST "%~dp0\node.exe" (
  @SET "_prog=%~dp0\node.exe"
) ELSE (
  @SET "_prog=node"
  @SET PATHEXT=%PATHEXT:;.JS;=;%
)
"%_prog%"  "%~dp0\..\@anthropic-ai\claude-code\cli.js" %*
"#;

    #[test]
    fn parses_newer_layout_to_node_and_expanded_script() {
        let t = parse_cmd_shim(NEWER, r"C:\npm").unwrap();
        // Bare `node` is preferred over the colocated `%dp0%\node.exe`.
        assert_eq!(t.interpreter, "node");
        assert_eq!(
            t.prefix_args,
            vec![r"C:\npm\node_modules\@anthropic-ai\claude-code\cli.js"]
        );
    }

    #[test]
    fn parses_older_dp0_layout() {
        let t = parse_cmd_shim(OLDER, r"C:\npm").unwrap();
        assert_eq!(t.interpreter, "node");
        assert_eq!(
            t.prefix_args,
            vec![r"C:\npm\..\@anthropic-ai\claude-code\cli.js"]
        );
    }

    #[test]
    fn trailing_separator_on_shim_dir_is_normalized() {
        // A `shim_dir` that already ends in a separator must not double it.
        let t = parse_cmd_shim(NEWER, r"C:\npm\").unwrap();
        assert_eq!(
            t.prefix_args,
            vec![r"C:\npm\node_modules\@anthropic-ai\claude-code\cli.js"]
        );
    }

    #[test]
    fn keeps_interpreter_flags_before_the_script() {
        let shim = "SET \"_prog=node\"\n\"%_prog%\" --no-warnings \"%dp0%\\cli.js\" %*\n";
        let t = parse_cmd_shim(shim, r"C:\x").unwrap();
        assert_eq!(t.interpreter, "node");
        assert_eq!(t.prefix_args, vec!["--no-warnings", r"C:\x\cli.js"]);
    }

    #[test]
    fn falls_back_to_colocated_interpreter_when_no_bare_name() {
        let shim = "SET \"_prog=%dp0%\\node.exe\"\n\"%_prog%\" \"%dp0%\\cli.js\" %*\n";
        let t = parse_cmd_shim(shim, r"C:\x").unwrap();
        assert_eq!(t.interpreter, r"C:\x\node.exe");
        assert_eq!(t.prefix_args, vec![r"C:\x\cli.js"]);
    }

    #[test]
    fn non_shim_text_returns_none() {
        // No `_prog`, no exec line: not something we can safely rewrite.
        assert!(parse_cmd_shim("echo hello\nexit /b 0\n", r"C:\x").is_none());
    }

    #[test]
    fn missing_exec_line_returns_none() {
        // Names an interpreter but never invokes it with `%*`.
        assert!(parse_cmd_shim("SET \"_prog=node\"\n", r"C:\x").is_none());
    }

    #[test]
    fn empty_prefix_args_returns_none() {
        // `"%_prog%" %*` with no script between is not a rewritable shim.
        let shim = "SET \"_prog=node\"\n\"%_prog%\" %*\n";
        assert!(parse_cmd_shim(shim, r"C:\x").is_none());
    }

    #[test]
    fn parse_set_prog_ignores_unrelated_lines() {
        assert_eq!(parse_set_prog("SET PATHEXT=%PATHEXT:;.JS;=;%"), None);
        assert_eq!(parse_set_prog("  not a set line"), None);
        assert_eq!(
            parse_set_prog("@SET \"_prog=node\""),
            Some("node".to_string())
        );
    }

    #[test]
    fn tokenize_handles_quotes_and_whitespace() {
        assert_eq!(tokenize("  \"a b\"  c "), vec!["a b", "c"]);
        assert_eq!(tokenize(""), Vec::<String>::new());
        // An empty quoted token still counts as a token.
        assert_eq!(tokenize("\"\""), vec![""]);
    }
}
