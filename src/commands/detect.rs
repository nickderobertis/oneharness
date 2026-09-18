//! `oneharness detect` — probe which harnesses are installed (binary + version).
//!
//! The sweep is a library call ([`oneharness_core::io::detect::detect`]), so a
//! Rust consumer reads the same [`DetectReport`] without spawning anything; this
//! is the shell that prints it (JSON, or one line per harness under `--format
//! text`) and maps `--require-available` to an exit code.

use crate::cli::DetectArgs;
use crate::commands::{print_report, printable, resolve_format};
use oneharness_core::errors::OneharnessError;
use oneharness_core::io::detect::{self, DetectRequest};

// Re-exported so the schema generator and existing consumers keep one import
// path for the CLI's output contract, wherever the type is defined.
pub use oneharness_core::io::detect::{DetectInfo, DetectReport};

pub fn run(args: &DetectArgs) -> Result<i32, OneharnessError> {
    let format = resolve_format(args.format, args.compact)?;
    let report = detect::detect(&DetectRequest {
        all: args.all,
        harness: args.harness.clone(),
        exclude: args.exclude.clone(),
        bin: args.bin.clone(),
        config: args.config.clone(),
        no_config: args.no_config,
        cwd: None,
    })?;
    let any_missing = report.any_missing();
    print_report(&report, format, args.compact, render_text)?;

    if args.require_available && any_missing {
        eprintln!("oneharness: one or more requested harnesses are not installed");
        return Ok(1);
    }
    Ok(0)
}

/// One line per probed harness: whether it is installed, and — when it is —
/// the path that resolved and the version it reported. A version the binary
/// did not answer is said to be unknown, never guessed; the version string is
/// the binary's own text, so it is flattened before it is drawn.
fn render_text(report: &DetectReport) -> String {
    let mut out = String::new();
    for d in &report.detected {
        if d.available {
            out.push_str(&format!(
                "{id}: available · {path} · version {version}\n",
                id = printable(&d.id),
                path = printable(d.path.as_deref().unwrap_or(&d.bin)),
                version = d
                    .version
                    .as_deref()
                    .map_or_else(|| "unknown".to_string(), printable),
            ));
        } else {
            out.push_str(&format!(
                "{id}: not installed (looked for `{bin}`)\n",
                id = printable(&d.id),
                bin = printable(&d.bin),
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(id: &str, available: bool, path: Option<&str>, version: Option<&str>) -> DetectInfo {
        DetectInfo {
            id: id.to_string(),
            bin: format!("{id}-bin"),
            available,
            path: path.map(str::to_string),
            version: version.map(str::to_string),
        }
    }

    #[test]
    fn text_view_says_available_with_path_and_version_or_not_installed() {
        let report = DetectReport {
            schema_version: "test",
            detected: vec![
                info(
                    "codex",
                    true,
                    Some("/usr/bin/codex"),
                    Some("1.2.3\u{1b}[0m"),
                ),
                info("goose", true, Some("/opt/goose"), None),
                info("crush", false, None, None),
            ],
        };
        let text = render_text(&report);
        assert_eq!(
            text,
            "codex: available · /usr/bin/codex · version 1.2.3 [0m\n\
             goose: available · /opt/goose · version unknown\n\
             crush: not installed (looked for `crush-bin`)\n"
        );
    }
}
