//! The serializable shapes that make up oneharness's output contract.
//!
//! The JSON report carries a `schema_version`: consumers depend on it, so fields
//! are added, never repurposed or removed, without bumping the version.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::batch::BatchStrategy;
use crate::domain::mode::PermissionMode;
use crate::domain::signals::Usage;

/// Bumped when the JSON shape changes in a way consumers must notice.
pub const SCHEMA_VERSION: &str = "0.1";

/// How a harness emits its result, which decides how `text` is extracted.
///
/// Also accepted as a CLI value (`--output-format`, parsed in the `oneharness`
/// binary) and a config-file value (`output_format`, via `Deserialize`). The
/// CLI parsing lives in the binary so this core crate stays free of `clap`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    /// The prompt this result ran, set only on a **batch** run (one harness
    /// fanned over N prompts), where each result has its own prompt. `null` on an
    /// ordinary run, where the single top-level `prompt` applies to every result.
    pub prompt: Option<String>,
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
    /// Best-effort token/cost accounting; every field is `null` when the harness
    /// does not report it. Always present so consumers can read a stable shape.
    pub usage: Usage,
    /// How `usage` was read (e.g. `json`); `null` when nothing was found.
    pub usage_source: Option<String>,
    /// Best-effort harness session id for continuation; `null` when none is
    /// exposed. (Surfaced only; oneharness does not yet consume it.)
    pub session_id: Option<String>,
    /// Structured-output run only: the JSON value extracted from the final
    /// answer and validated against the requested schema. `null` when no schema
    /// was requested, or when no JSON value could be extracted. Carries the
    /// last-attempted value even when it failed validation, so a consumer can
    /// see what the harness produced.
    pub structured: Option<Value>,
    /// Structured-output run only: whether `structured` conformed to the schema
    /// on the final attempt. `null` when no schema was requested (or the harness
    /// did not run); `false` when a schema was requested but the result never
    /// conformed (including "no JSON found").
    pub schema_valid: Option<bool>,
    /// Structured-output run only: how many times this harness was invoked under
    /// the validate/retry loop (1 + retries). `null` when no schema was
    /// requested or the harness did not run.
    pub schema_attempts: Option<u32>,
    /// Structured-output run only: the validation errors from the final attempt,
    /// joined for display; `null` when valid or no schema was requested.
    pub schema_error: Option<String>,
    /// Best-effort failure reason (`auth`, `rate_limit`, `model_not_found`,
    /// `quota`) for a non-zero run; `null` when unclassified. Distinct from
    /// `status`, which records oneharness's relationship to the process.
    pub failure_kind: Option<String>,
    /// Where `failure_kind` was read (`stderr`/`stdout`); `null` when absent.
    pub failure_kind_source: Option<String>,
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
    /// The prompt sent. On an ordinary run this is *the* prompt every result
    /// shares; on a **batch** run (see `batch`) it repeats the first prompt for
    /// back-compat, and each result's own `prompt` field is authoritative.
    pub prompt: String,
    pub model: Option<String>,
    /// The session id being continued, when `--resume` was passed; else `null`.
    pub resume: Option<String>,
    /// Whether the resumed session was forked (`--fork`) rather than appended to.
    /// `false` unless `--resume` was given with `--fork`.
    pub fork: bool,
    /// The normalized approval mode requested for this run (see the README
    /// support matrix). Each harness maps it to its own mechanism.
    pub permission_mode: PermissionMode,
    /// Back-compat convenience: `true` exactly when `permission_mode` is
    /// `bypass`. Retained so existing consumers keep working; new consumers
    /// should read `permission_mode`.
    pub bypass_permissions: bool,
    pub dry_run: bool,
    /// The JSON Schema applied to this run (structured output), or `null` when
    /// none was requested. Echoed so a consumer sees the exact constraint each
    /// result was validated against.
    pub schema: Option<Value>,
    /// Maximum retries allowed per harness under the validate/retry loop; `null`
    /// when no schema was requested.
    pub schema_max_retries: Option<u32>,
    /// Same-prefix batch metadata when this run fanned **one** harness over more
    /// than one prompt; `null` on an ordinary run. Its presence is the signal a
    /// consumer keys on to read each result's own `prompt`.
    pub batch: Option<BatchReport>,
    /// Config files that shaped this run, in layering order (user first,
    /// project last); empty under `--no-config` or when none exist.
    pub config_files: Vec<String>,
    pub results: Vec<RunResult>,
}

/// Metadata for a same-prefix batch run (one harness, N prompts sharing a
/// cacheable prefix). Present on [`RunReport::batch`] only in that mode.
#[derive(Debug, Clone, Serialize)]
pub struct BatchReport {
    /// How the prompts were scheduled across the parallel runner.
    pub strategy: BatchStrategy,
    /// How many prompts were run (equals `results.len()`).
    pub prompt_count: usize,
    /// Whether the fan-out actually **forked** the warm-up's session to reuse its
    /// cached prefix (`min-tokens` on a fork-capable harness whose warm-up exposed
    /// a session id). `false` for `speed`, for `min-tokens` on a harness that
    /// cannot fork, or when the warm-up exposed no session to fork. When `true`,
    /// the fan-out results' `command` carries the resume/fork flags and their
    /// `usage.cache_read_tokens` reflect the reused prefix.
    pub forked: bool,
}
