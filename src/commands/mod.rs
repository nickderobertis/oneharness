//! CLI verbs: each module orchestrates the domain + io layers for one command.
//! Shared helpers (harness selection, JSON/text output) live here.

pub mod config;
pub mod detect;
pub mod gate;
pub mod history;
pub mod init;
pub mod interrupt;
pub mod list;
pub mod mock;
pub mod run;
pub mod sync;
pub mod usage;

use std::io::Write;

use serde::Serialize;

use oneharness_core::errors::OneharnessError;

use crate::cli::Format;

// Selection and identity resolution live in the engine, so every entry point
// that names harnesses — these verbs and a library caller of
// `oneharness_core::io::run` alike — resolves the same selectors the same way.
pub use oneharness_core::domain::select::{dedupe_exact_ids, select_specs};
pub use oneharness_core::io::identity::{
    variant_environment, variant_unprovisioned_identity, UnprovisionedIdentity,
};

/// Write `text` to stdout verbatim, reporting a write failure rather than
/// panicking on it.
///
/// `print!`/`println!` panic when stdout cannot be written — which a reader
/// closing the pipe (`oneharness usage | head -1`) makes an ordinary event, not
/// a bug. A command whose output *is* its deliverable should say that the
/// deliverable was truncated, through the same error channel as every other I/O
/// fault, instead of dying mid-sentence with a stack trace.
pub fn print_text(text: &str) -> Result<(), OneharnessError> {
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
        .map_err(OneharnessError::StdoutWrite)
}

/// Write a value as JSON to stdout (pretty unless `compact`).
pub fn print_json<T: Serialize>(value: &T, compact: bool) -> Result<(), OneharnessError> {
    let json = if compact {
        serde_json::to_string(value)?
    } else {
        serde_json::to_string_pretty(value)?
    };
    print_text(&format!("{json}\n"))
}

/// The stdout format a verb's `--format` / `--compact` pair resolves to.
///
/// `text` is the default wherever stdout points: a person running a verb by
/// hand gets a view they can read, and a program that wants the JSON contract
/// says so. `--compact` is a JSON rendering choice, so alone it selects `json`
/// (which is what keeps every consumer that already passed it on the
/// contract), and beside an explicit `--format text` it asks for two things one
/// stdout cannot be — refused, naming both flags. Called before a verb does
/// its work, so the refusal never follows a sync that wrote, a probe that ran,
/// or a turn that was interrupted.
pub(crate) fn resolve_format(
    format: Option<Format>,
    compact: bool,
) -> Result<Format, OneharnessError> {
    match (format, compact) {
        (Some(Format::Text), true) => Err(OneharnessError::FormatConflict {
            flag: "--compact",
            why: "--compact is a JSON rendering choice (drop it, or pass --format json)",
        }),
        (Some(format), _) => Ok(format),
        (None, true) => Ok(Format::Json),
        (None, false) => Ok(Format::Text),
    }
}

/// Write a verb's report to stdout in the format the caller chose: the JSON
/// document (pretty unless `compact`) or the human-readable view `render_text`
/// produces from the same value.
///
/// One seam for every JSON-stdout verb, so `--format` means the same thing on
/// each: the text view is a rendering of the report the JSON carries — never a
/// second computation that could disagree with it — and `--compact` is a JSON
/// rendering choice that has no bearing on text. Takes the format
/// [`resolve_format`] settled, so a verb refuses a contradictory pair before
/// it works rather than after.
pub(crate) fn print_report<T: Serialize>(
    value: &T,
    format: Format,
    compact: bool,
    render_text: impl FnOnce(&T) -> String,
) -> Result<(), OneharnessError> {
    match format {
        Format::Json => print_json(value, compact),
        Format::Text => print_text(&render_text(value)),
    }
}

/// `text` with every control character except the newline flattened to a
/// space, for a text view.
///
/// Harness output reaches the text views verbatim — a result's `text`, its
/// `error`, a version string `detect` read off a binary — and an ANSI escape, a
/// carriage return or a bell inside it could move the cursor, recolour, or
/// overwrite part of a report whose whole point is to be read at a glance. The
/// JSON contract carries the bytes as they were; the text view is the one that
/// draws them, so it is the one that flattens. Newlines survive because a
/// multi-line answer is laid out by the renderer, one row per line.
pub(crate) fn printable(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() && c != '\n' { ' ' } else { c })
        .collect()
}

/// `text` as an indented block: every line prefixed with `indent`, each
/// terminated, so a multi-line value sits under its label rather than beside
/// it. Flattened through [`printable`] on the way.
pub(crate) fn indented(text: &str, indent: &str) -> String {
    let mut out = String::new();
    for line in printable(text).lines() {
        out.push_str(indent);
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// A display value for something the JSON reports as `null`.
pub(crate) fn or_null(value: Option<&str>) -> String {
    value.map_or_else(|| "null".to_string(), printable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_defaults_to_text_unless_compact_asks_for_json() {
        assert_eq!(resolve_format(None, false).unwrap(), Format::Text);
        assert_eq!(resolve_format(None, true).unwrap(), Format::Json);
        assert_eq!(
            resolve_format(Some(Format::Json), false).unwrap(),
            Format::Json
        );
        assert_eq!(
            resolve_format(Some(Format::Json), true).unwrap(),
            Format::Json
        );
        assert_eq!(
            resolve_format(Some(Format::Text), false).unwrap(),
            Format::Text
        );
        let refused = resolve_format(Some(Format::Text), true)
            .unwrap_err()
            .to_string();
        assert!(
            refused.contains("--format text") && refused.contains("--compact"),
            "{refused}"
        );
    }

    #[test]
    fn printable_keeps_newlines_and_flattens_every_other_control_character() {
        let flattened = printable("a\u{1b}[31mb\r\nc\u{7}d\u{9b}e\tf");
        assert_eq!(flattened, "a [31mb \nc d e f");
    }

    #[test]
    fn indented_lays_a_multi_line_value_out_one_row_per_line() {
        assert_eq!(indented("one\ntwo\u{1b}", "  "), "  one\n  two \n");
        assert_eq!(indented("", "  "), "");
    }

    #[test]
    fn or_null_names_an_absent_value() {
        assert_eq!(or_null(None), "null");
        assert_eq!(or_null(Some("x\u{8}")), "x ");
    }
}
