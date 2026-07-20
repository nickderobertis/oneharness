//! The serializable shapes that make up oneharness's output contract.
//!
//! The JSON report carries a `schema_version`: consumers depend on it, so fields
//! are added, never repurposed or removed, without bumping the version.

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::domain::batch::BatchStrategy;
use crate::domain::events::ActionEvent;
use crate::domain::mode::PermissionMode;
use crate::domain::session::SessionPhase;
use crate::domain::signals::{FailureKind, Usage};

/// Bumped when the JSON shape changes in a way consumers must notice.
pub const SCHEMA_VERSION: &str = "0.1";

/// How a harness emits its result, which decides how `text` is extracted.
///
/// Also accepted as a CLI value (`--output-format`, parsed in the `oneharness`
/// binary) and a config-file value (`output_format`, via `Deserialize`). The
/// CLI parsing lives in the binary so this core crate stays free of `clap`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum OutputFormat {
    /// Plain text on stdout; `text` is the trimmed stdout.
    Text,
    /// A single JSON document on stdout.
    Json,
    /// Line-delimited JSON events on stdout.
    StreamJson,
}

impl OutputFormat {
    /// The stable CLI/config/wire token for this format.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Json => "json",
            Self::StreamJson => "stream-json",
        }
    }
}

/// The outcome of attempting to run one harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
    /// UTC invocation boundaries observed by the runner (never synthesized by
    /// history serialization).
    pub started_at: String,
    pub finished_at: Option<String>,
    /// Complete stdout chunks paired with their monotonic offset from start.
    pub stdout_observations: Vec<OutputObservation>,
}

#[derive(Debug, Clone)]
pub struct OutputObservation {
    pub offset_ms: u128,
    pub observed_at: String,
    pub bytes: Vec<u8>,
}

/// Measured execution telemetry carried internally from the runner/parser to
/// the history writer. It is deliberately not part of the run-report contract.
#[derive(Debug, Clone, Default)]
pub struct ExecutionTelemetry {
    pub started_at: String,
    pub finished_at: Option<String>,
    pub model_ms: Option<u128>,
    pub tool_ms: Option<u128>,
    pub time_to_first_token_ms: Option<u128>,
}

/// One harness's entry in the report.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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
    /// The model this result ran with (the value oneharness put on the harness's
    /// model flag), or `null` when no model was requested and the harness used its
    /// own default. On a **model fan-out** run (`RunReport::models`), this is what
    /// distinguishes results that share a harness — each entry is one (harness,
    /// model) pair. The model is also visible in `command`; this field surfaces it
    /// without parsing the argv.
    pub model: Option<String>,
    /// Process exit code; `null` when not run, timed out, or signalled.
    pub exit_code: Option<i32>,
    /// Wall-clock duration of the run; `null` when not executed.
    pub duration_ms: Option<u128>,
    #[serde(skip, default)]
    #[schemars(skip)]
    pub telemetry: Option<ExecutionTelemetry>,
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
    /// exposed. Surfaced for a consumer to thread into `--resume`, and consumed
    /// by oneharness itself when `--session` is in play (it is captured into the
    /// session store to back the uniform handle — see [`RunReport::session`]).
    pub session_id: Option<String>,
    /// Best-effort normalized tool-call / action events the harness took (shell
    /// commands, file edits, tool uses), in order — so consumers can assert on
    /// *behavior*, not just the final `text`. `null` when the harness's output
    /// exposes no machine-readable trace (a plain-text harness, or Claude Code's
    /// single-document `json` result), distinct from `[]` — an empty array is not
    /// currently emitted; absence is signalled by `null` + a null `events_source`.
    /// Never fabricated. See [`crate::domain::events`].
    pub events: Option<Vec<ActionEvent>>,
    /// How `events` was recovered (e.g. `json:opencode-parts`,
    /// `stream-json:content-blocks`), parallel to `text_source`; `null` when no
    /// events were found. Lets a consumer tell "harness doesn't support it" from
    /// "no tools were used."
    pub events_source: Option<String>,
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
    /// Best-effort failure reason; `null` when unclassified. Distinct from
    /// `status`, which records oneharness's relationship to the process. Two
    /// families: coarse reasons for a non-zero run (`auth`, `rate_limit`,
    /// `model_not_found`, `quota`), and `tool_deferred` — a run that exited
    /// *cleanly* but only deferred a builtin tool call instead of executing it
    /// (Claude Code bridge/managed deployments), so it did no useful work. The
    /// deferred case is the only `failure_kind` that can appear on a `status: ok`
    /// run, and it also marks the run as failed for exit-code purposes.
    /// Serialized as its snake_case token (see [`FailureKind`]).
    pub failure_kind: Option<FailureKind>,
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
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RunReport {
    pub schema_version: String,
    pub oneharness_version: String,
    /// The prompt sent. On an ordinary run this is *the* prompt every result
    /// shares; on a **batch** run (see `batch`) it repeats the first prompt for
    /// back-compat, and each result's own `prompt` field is authoritative.
    pub prompt: String,
    /// The effective top-level model: the first of the fan-out `models` list when
    /// one was given, else the single configured/CLI model, else `null`. Each
    /// result's own `model` is authoritative on a fan-out run.
    pub model: Option<String>,
    /// The model fan-out list this run multiplied over (repeated `--model` /
    /// config `models`), or `null` on an ordinary single-model run. Its presence
    /// is the signal a consumer keys on to read each result's own `model`: in
    /// `parallel` mode `results` holds one entry per (harness, model) pair; in
    /// `fallback` mode the pairs were tried in priority order (harness-major,
    /// model-minor).
    pub models: Option<Vec<String>>,
    /// The session id being continued, when `--resume` was passed; else `null`.
    pub resume: Option<String>,
    /// Whether the resumed session was forked (`--fork`) rather than appended to.
    /// `false` unless `--resume` was given with `--fork`.
    pub fork: bool,
    /// The uniform session handle in play (`--session <name>`), or `null` when
    /// none was requested. Lets a consumer thread one stable name across turns
    /// instead of extracting each harness's native session id. Distinct from the
    /// low-level `resume` field above, which echoes an explicit `--resume` id.
    pub session: Option<SessionReport>,
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
    /// Fallback-mode metadata when this run drove the selected harnesses in
    /// priority order, stopping at the first that ran (`--run-mode fallback`);
    /// `null` on a parallel run (and under `--print-command`, where nothing
    /// executes). Its presence tells a consumer that `results` holds only the
    /// harnesses actually *attempted* — the fallen-through ones in order, then
    /// the one that ran — not every selected harness.
    pub fallback: Option<FallbackReport>,
    /// The parsed `--mock-rules` ruleset this run was intercepted with; `null`
    /// when no mocking was requested. Present so a consumer can tell a mocked
    /// run's report from a clean one without out-of-band state.
    pub mock_rules: Option<Value>,
    /// The spy-log path the mock hook appended tool-call records to (absolute);
    /// `null` when none was requested.
    pub spy_file: Option<String>,
    /// The history session file this run streamed normalized records to
    /// (absolute); `null` when history was not enabled (or under `--print-command`,
    /// where nothing runs). The programmatic handle a consumer captures to read the
    /// session back later with `oneharness history show`.
    pub history_file: Option<String>,
    /// Config files that shaped this run, in layering order (user first,
    /// project last); empty under `--no-config` or when none exist.
    pub config_files: Vec<String>,
    pub results: Vec<RunResult>,
}

/// The uniform session handle for a run (`--session`). Present on
/// [`RunReport::session`] only when `--session <name>` was requested.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionReport {
    /// The caller's stable handle (`--session <name>`, sanitized for the store).
    pub name: String,
    /// Whether this run created the named session (no prior token) or continued
    /// an existing one.
    pub phase: SessionPhase,
    /// The harness native token now bound to the name: the id resumed on a
    /// continue, or the id captured on a create. `null` only when a create run
    /// exposed no session id (the handle then cannot be continued — a warning is
    /// emitted), or under `--print-command` on a create (nothing ran).
    pub token: Option<String>,
    /// The session store file backing the handle (absolute); the programmatic
    /// handle to the persisted state.
    pub store_file: Option<String>,
}

/// Metadata for a fallback run (harnesses tried in priority order until one
/// runs). Present on [`RunReport::fallback`] only in that mode. The per-harness
/// detail lives in `results`; this block summarizes the outcome so a consumer
/// need not re-derive it from statuses.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FallbackReport {
    /// The harness that actually ran the task (the run stopped there), or `null`
    /// when no candidate could run at all — every one was a startup failure.
    pub ran: Option<String>,
    /// The candidates fallen through because they could not run the task at all,
    /// in priority order, each with why (`not-installed`, `spawn-error`, `auth`,
    /// `quota`, and — on a model fan-out — `model-not-found` / `rate-limit`; see
    /// [`crate::domain::fallback::startup_failure_reason`]).
    pub fell_through: Vec<FallThrough>,
}

/// One candidate a fallback run fell through, with the reason it could not run.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FallThrough {
    /// Canonical harness id.
    pub harness: String,
    /// Short reason token (`not-installed` / `spawn-error` / `auth` / `quota` /
    /// `model-not-found` / `rate-limit`).
    pub reason: String,
}

/// Metadata for a same-prefix batch run (one harness, N prompts sharing a
/// cacheable prefix). Present on [`RunReport::batch`] only in that mode.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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

/// One line of `oneharness run --stream` output.
///
/// Event lines carry normalized actions as they arrive. Exactly one terminal
/// result line carries the complete report unless the consumer closes the
/// stream early. This is an output contract, so deserialization deliberately
/// tolerates additive fields from newer producers.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunStreamEnvelope {
    /// A normalized action observed while the harness is still running.
    Event { event: ActionEvent },
    /// The complete report that terminates a normally consumed stream.
    Result { report: RunReport },
}

impl<'de> Deserialize<'de> for RunStreamEnvelope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Serde's internally-tagged-enum buffer does not support u128, while a
        // RunReport legitimately carries millisecond durations as u128. Decode
        // the small JSON envelope first so the nested report is deserialized by
        // serde_json's number-aware value deserializer instead.
        let value = Value::deserialize(deserializer)?;
        let kind = value.get("type").and_then(Value::as_str).ok_or_else(|| {
            serde::de::Error::custom("run stream envelope is missing string field `type`")
        })?;
        match kind {
            "event" => serde_json::from_value(
                value
                    .get("event")
                    .cloned()
                    .ok_or_else(|| serde::de::Error::custom("event envelope is missing `event`"))?,
            )
            .map(|event| Self::Event { event })
            .map_err(serde::de::Error::custom),
            "result" => {
                serde_json::from_value(value.get("report").cloned().ok_or_else(|| {
                    serde::de::Error::custom("result envelope is missing `report`")
                })?)
                .map(|report| Self::Result { report })
                .map_err(serde::de::Error::custom)
            }
            other => Err(serde::de::Error::custom(format!(
                "unknown run stream envelope type `{other}`"
            ))),
        }
    }
}
