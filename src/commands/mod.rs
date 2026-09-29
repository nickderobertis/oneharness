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

use crate::cli::StdoutFormat;

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

/// Write a verb's report to stdout in the format the caller chose: the JSON
/// document (pretty unless compact) or the human-readable view `render_text`
/// produces from the same value.
///
/// One seam for every JSON-stdout verb, so `--format` means the same thing on
/// each: the text view is a rendering of the report the JSON carries — never a
/// second computation that could disagree with it — and `--compact` is a JSON
/// rendering choice that has no bearing on text. Takes the [`StdoutFormat`]
/// clap settled, so a contradictory pair was refused before the verb worked.
pub(crate) fn print_report<T: Serialize>(
    value: &T,
    format: StdoutFormat,
    render_text: impl FnOnce(&T) -> String,
) -> Result<(), OneharnessError> {
    match format {
        StdoutFormat::Json { compact } => print_json(value, compact),
        StdoutFormat::DefaultText | StdoutFormat::Text => print_text(&render_text(value)),
    }
}

// The text views' flattening is the engine's, so a verb's view
// and the renderers an embedder calls draw a harness's bytes the same way.
pub(crate) use oneharness_core::domain::render::printable;
