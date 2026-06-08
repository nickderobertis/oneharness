//! The serializable shapes that make up oneharness's output contract.
//!
//! The JSON report carries a `schema_version`: consumers depend on it, so fields
//! are added, never repurposed or removed, without bumping the version.

use serde::Serialize;

/// Bumped when the JSON shape changes in a way consumers must notice.
pub const SCHEMA_VERSION: &str = "0.1";

/// How a harness emits its result, which decides how `text` is extracted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputFormat {
    /// Plain text on stdout; `text` is the trimmed stdout.
    Text,
    /// A single JSON document on stdout.
    Json,
    /// Line-delimited JSON events on stdout.
    StreamJson,
}

/// The outcome of attempting to run one harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    /// Spawned and exited 0.
    Ok,
    /// Spawned and exited non-zero.
    Nonzero,
    /// Killed after exceeding the per-harness timeout.
    Timeout,
    /// The binary was resolved but could not be executed.
    SpawnError,
    /// The binary was not found, so the harness was not run.
    Skipped,
    /// `--print-command`: the command was built but not executed.
    Planned,
}

/// The raw capture from running a subprocess — produced by the io layer,
/// consumed (with extraction) by the command layer. Carries no extraction so
/// the spawn path and the parse path stay independently testable.
#[derive(Debug, Clone)]
pub struct Capture {
    pub status: Status,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<u128>,
    pub stdout: String,
    pub stderr: String,
    pub error: Option<String>,
}

/// One harness's entry in the report.
#[derive(Debug, Clone, Serialize)]
pub struct RunResult {
    /// Canonical harness id (e.g. `claude-code`).
    pub harness: String,
    /// The binary name or path oneharness resolved and would invoke.
    pub bin: String,
    /// Whether that binary was found.
    pub available: bool,
    pub status: Status,
    /// Process exit code; `null` when not run, timed out, or signalled.
    pub exit_code: Option<i32>,
    /// Wall-clock duration of the run; `null` when not executed.
    pub duration_ms: Option<u128>,
    /// The exact argv oneharness built (argv[0] is the binary).
    pub command: Vec<String>,
    pub output_format: OutputFormat,
    /// Best-effort final assistant text; `null` when extraction is impossible.
    pub text: Option<String>,
    /// How `text` was extracted (e.g. `json:result`, `raw`); `null` when absent.
    pub text_source: Option<String>,
    /// Raw captured stdout (empty for skipped/planned).
    pub stdout: String,
    /// Raw captured stderr (empty for skipped/planned).
    pub stderr: String,
    /// Human-readable problem + suggested action; `null` on success.
    pub error: Option<String>,
}

/// The top-level `run` report written to stdout.
#[derive(Debug, Clone, Serialize)]
pub struct RunReport {
    pub schema_version: &'static str,
    pub oneharness_version: &'static str,
    pub prompt: String,
    pub model: Option<String>,
    pub bypass_permissions: bool,
    pub dry_run: bool,
    pub results: Vec<RunResult>,
}
