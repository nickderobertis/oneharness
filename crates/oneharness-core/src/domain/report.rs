//! The serializable shapes that make up oneharness's output contract.
//!
//! The JSON report carries a `schema_version`: consumers depend on it, so fields
//! are added, never repurposed or removed, without bumping the version.

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::domain::batch::BatchStrategy;
use crate::domain::events::ActionEvent;
use crate::domain::mode::PermissionMode;
use crate::domain::session::SessionPhase;
use crate::domain::signals::{FailureKind, Usage};
use crate::domain::usage::UtcInstant;

/// Bumped when the JSON shape changes in a way consumers must notice.
///
/// Shared by every non-history report on stdout — `run`, `list`, `detect`,
/// `sync`, and `config` — so one number describes the whole surface; the history
/// records carry their own (`domain::history::SCHEMA_VERSION`).
///
/// `0.6` adds the `session_not_found` [`FailureKind`] — the refusal a harness
/// returns when asked to continue a session its identity has never seen — and,
/// with it, the `"session-not-found"` reason a fallback run reports for a
/// candidate it routed around. Purely additive: every 0.5 field keeps its name,
/// type, and meaning. The new *enum value* is why the bump matters, since a
/// consumer that exhaustively matches `failure_kind` learns from the version that
/// a sixth value now exists.
///
/// `0.5` added two things to the `run` report: the measured
/// [`ExecutionTelemetry`] on each result (previously internal, which forced a
/// consumer to re-read the history file for numbers the run already knew), and
/// the `cancelled` [`Status`] a run reaches when the caller — or a SIGINT/SIGTERM
/// on the host — tore the harness tree down before it finished. Both are
/// additive: every 0.4 field keeps its name, type, and meaning. The new *status
/// value* is why the bump matters, since a consumer that exhaustively matches
/// `status` learns from the version that a sixth value now exists.
///
/// `0.4` added the `config` report's `stream` field (the layered `--stream`
/// value, with its provenance).
pub const SCHEMA_VERSION: &str = "0.6";

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
    /// Terminated before it finished because the run was **cancelled** — a
    /// library caller flipped its [`crate::io::cancel::CancelToken`], or the host
    /// received SIGINT/SIGTERM while [`crate::io::cancel::install_signal_cancel`]
    /// was in force. Distinct from `timeout` (the harness was given its full
    /// deadline and exceeded it) and from a consumer-driven streaming stop (which
    /// stays `ok`, because the consumer already got what it asked for). Any
    /// output captured before the cancellation is still normalized, exactly as it
    /// is for a timeout.
    Cancelled,
    /// The binary was resolved but could not be executed.
    SpawnError,
    /// The harness was not run. Either the binary was not found (`available:
    /// false`), or it was found but the identity the selection points at is not
    /// provisioned — an `env_from` home directory that is not on disk, reported
    /// as `available: true` with `failure_kind: "auth"`.
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

/// The exact text [`crate::domain::history::format_rfc3339_millis`] produces:
/// `YYYY-MM-DDTHH:MM:SS.mmmZ`, always 24 characters.
const RUN_INSTANT_LEN: u32 = 24;
const RUN_INSTANT_PATTERN: &str =
    r"^\d{4}-(0[1-9]|1[0-2])-(0[1-9]|[12]\d|3[01])T([01]\d|2[0-3]):[0-5]\d:([0-5]\d|60)\.\d{3}Z$";

/// The error returned when text is not a [`RunInstant`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("must be an RFC 3339 UTC instant with milliseconds, e.g. 2026-01-01T00:00:00.000Z")]
pub struct RunInstantError;

/// An RFC 3339 **UTC** instant at millisecond precision — a runner clock read.
///
/// Deliberately *not* [`UtcInstant`], which canonicalizes to whole seconds: the
/// invocation bounds this carries are minted in milliseconds, and a type that
/// silently truncated them would make a sub-second run report as instantaneous.
/// Wrapping the text is what keeps a timestamp that is not UTC, not
/// millisecond-precise, or not a timestamp at all out of a measurement — the same
/// bar `UtcInstant` holds its own field to.
///
/// The `FromStr` rule and the [`JsonSchema`] below state one thing, so the Rust
/// reader and every generated SDK validator accept exactly the same values. The
/// length bound is load-bearing, not decorative: it is what makes a trailing
/// newline fail even where a regex `$` would match before one.
///
/// The schema states everything a JSON Schema can: the shape, the length, and
/// each component's range (month `01`-`12`, hour `00`-`23`, a real `:60` leap
/// second). `FromStr` adds the one thing a regex cannot express — that the date
/// exists at all, so `2026-02-30` is refused. The residual slack is **one-way**
/// and harmless: a generated validator accepts a shape this parser would reject,
/// but oneharness can never emit one, because every value it writes came from a
/// real clock and through here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct RunInstant(String);

impl RunInstant {
    /// The canonical millisecond-precision UTC text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::str::FromStr for RunInstant {
    type Err = RunInstantError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let bytes = text.as_bytes();
        if bytes.len() != RUN_INSTANT_LEN as usize {
            return Err(RunInstantError);
        }
        let shaped = bytes.iter().enumerate().all(|(index, byte)| match index {
            4 | 7 => *byte == b'-',
            10 => *byte == b'T',
            13 | 16 => *byte == b':',
            19 => *byte == b'.',
            23 => *byte == b'Z',
            _ => byte.is_ascii_digit(),
        });
        if !shaped {
            return Err(RunInstantError);
        }
        // Then that it is a real instant, by the same rule the usage parser holds
        // its own timestamps to: every component in range *and* a date that exists.
        // llmlint: ignore[contracts_have_one_source_or_a_drift_gate] The generated SDK validators state this rule as far as a JSON Schema reaches — length, shape, and every component's range, per RUN_INSTANT_PATTERN; only "the date exists" is inexpressible as a regex. The resulting slack runs one way (a validator accepts what this parser rejects, never the reverse), so no consumer is ever told a value is valid that oneharness would refuse to write.
        crate::domain::usage::normalize_timestamp(text)
            .ok_or(RunInstantError)
            .map(|_| Self(text.to_string()))
    }
}

impl fmt::Display for RunInstant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<'de> Deserialize<'de> for RunInstant {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for RunInstant {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("RunInstant")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "minLength": RUN_INSTANT_LEN,
            "maxLength": RUN_INSTANT_LEN,
            "pattern": RUN_INSTANT_PATTERN,
        })
    }
}

/// Measured execution telemetry for one harness run: when the invocation ran,
/// and — when the harness's transcript supports it — how its wall clock split
/// between provider latency and tool work.
///
/// Carried from the runner/parser to the history writer *and*, since report
/// schema `0.5`, serialized on [`RunResult::telemetry`]. Exposing it there is
/// what lets a consumer read the numbers off the run it just made instead of
/// re-opening the history file the same run wrote.
///
/// Internally tagged by `source`, so the variant is a value a consumer switches
/// on rather than a shape it has to sniff. Every variant states only what was
/// actually measured — there is no variant meaning "no telemetry"; that is a
/// `null` field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum ExecutionTelemetry {
    /// The harness's own transcript carried a complete provider trace, so the
    /// invocation bounds *and* the model/tool split are measured.
    ProviderMeasured {
        started_at: RunInstant,
        finished_at: Option<RunInstant>,
        model_ms: Option<u128>,
        tool_ms: Option<u128>,
        time_to_first_token_ms: Option<u128>,
    },
    /// The invocation bounds the runner observed for a run whose provider trace
    /// never completed, with no model/tool split: a split read out of a
    /// transcript that stopped mid-turn is not a measurement, but when the run
    /// itself started is, and it is what an operator reads off a failure. Typed
    /// as a [`UtcInstant`] so the one thing this variant claims to know cannot be
    /// empty or in some other offset by the time a record renders it.
    PartialInvocation {
        #[schemars(with = "String")]
        started_at: UtcInstant,
    },
    /// The union of tool intervals observed at the stdout pipe, for a harness
    /// whose transcript carries no provider trace to split. Not provider-measured
    /// and with no model-latency counterpart, which is what the `source` tag tells
    /// a consumer; per-event provenance lives on `ActionEvent`.
    StdoutObserved { tool_ms: u128 },
}

impl<'de> Deserialize<'de> for ExecutionTelemetry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Serde's internally-tagged buffer cannot hold a u128, and these durations
        // legitimately are u128 — the same constraint `RunStreamEnvelope` below
        // documents. So the tag is read off a `Value` and each variant's body is
        // then handed to serde_json's number-aware deserializer, whole. Deriving
        // the bodies (rather than picking fields out of the `Value` by hand) is
        // what keeps this reader exactly as strict as the schema generated from
        // the same types: a missing or wrong-typed field is an error, never a
        // silent `None`, and an additive field from a newer producer is ignored.
        let value = Value::deserialize(deserializer)?;
        let source = value.get("source").and_then(Value::as_str).ok_or_else(|| {
            serde::de::Error::custom("execution telemetry is missing string field `source`")
        })?;
        let variant = |name: &str| -> Result<Self, D::Error> {
            match name {
                "provider_measured" => {
                    serde_json::from_value::<ProviderMeasuredWire>(value.clone())
                        .map(|wire| Self::ProviderMeasured {
                            started_at: wire.started_at,
                            finished_at: wire.finished_at,
                            model_ms: wire.model_ms,
                            tool_ms: wire.tool_ms,
                            time_to_first_token_ms: wire.time_to_first_token_ms,
                        })
                        .map_err(serde::de::Error::custom)
                }
                "partial_invocation" => {
                    serde_json::from_value::<PartialInvocationWire>(value.clone())
                        .map(|wire| Self::PartialInvocation {
                            started_at: wire.started_at,
                        })
                        .map_err(serde::de::Error::custom)
                }
                "stdout_observed" => serde_json::from_value::<StdoutObservedWire>(value.clone())
                    .map(|wire| Self::StdoutObserved {
                        tool_ms: wire.tool_ms,
                    })
                    .map_err(serde::de::Error::custom),
                other => Err(serde::de::Error::custom(format!(
                    "unknown execution telemetry source `{other}`"
                ))),
            }
        };
        variant(source)
    }
}

/// The `provider_measured` body, minus its tag. Every field is required and
/// independently nullable, exactly as the generated schema states it.
///
/// `required_nullable` is what makes that true: serde otherwise fills a missing
/// `Option` field with `None`, which would read an absent measurement as "not
/// measured" — a claim the producer never made, and one every generated SDK
/// validator refuses.
#[derive(Deserialize)]
struct ProviderMeasuredWire {
    started_at: RunInstant,
    #[serde(deserialize_with = "required_nullable")]
    finished_at: Option<RunInstant>,
    #[serde(deserialize_with = "required_nullable")]
    model_ms: Option<u128>,
    #[serde(deserialize_with = "required_nullable")]
    tool_ms: Option<u128>,
    #[serde(deserialize_with = "required_nullable")]
    time_to_first_token_ms: Option<u128>,
}

/// Read an `Option<T>` that must be *present* (as a value or an explicit
/// `null`). Naming a `deserialize_with` is what turns off serde's implicit
/// missing-field default for an `Option`; the body is the ordinary decode.
fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

#[derive(Deserialize)]
struct PartialInvocationWire {
    started_at: UtcInstant,
}

#[derive(Deserialize)]
struct StdoutObservedWire {
    tool_ms: u128,
}

/// One harness's entry in the report.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RunResult {
    // llmlint: ignore[invalid_states_unrepresentable] These three additive wire fields must remain directly addressable for the stable JSON/SDK contract; command construction always derives all three together from one validated composed selection in `apply_result_identity`, and round-trip integration tests pin their consistency.
    /// Canonical harness id (e.g. `claude-code`).
    pub harness: String,
    /// Named preset, when this result came from a composed harness id.
    // llmlint: ignore[invalid_states_unrepresentable] This additive public wire field must remain an Option<String> for generated SDK compatibility; all production constructors set it together with harness/harness_id via `apply_result_identity`, and subprocess tests assert the triplet.
    pub variant: Option<String>,
    /// Base id or `<base>:<variant>`, suitable for selecting the same candidate.
    // llmlint: ignore[invalid_states_unrepresentable] This stable serialized selector remains a String so existing consumers can round-trip it; production code derives it from the validated selection alongside base/variant and integration tests pin consistency.
    pub harness_id: String,
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
    /// Measured execution telemetry for this run: the invocation bounds and, when
    /// the harness's transcript carried a trace to read them from, the
    /// model/tool split. `null` when nothing was measured — never estimated.
    /// Added in report schema `0.5`; before that a consumer had to re-read the
    /// history file for numbers the run itself already had.
    #[serde(default)]
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
    /// `model_not_found`, `quota`, `session_not_found`), and `tool_deferred` — a run that exited
    /// *cleanly* but only deferred a builtin tool call instead of executing it
    /// (Claude Code bridge/managed deployments), so it did no useful work. The
    /// deferred case is the only `failure_kind` that can appear on a `status: ok`
    /// run, and it also marks the run as failed for exit-code purposes.
    /// Serialized as its snake_case token (see [`FailureKind`]).
    pub failure_kind: Option<FailureKind>,
    /// Where `failure_kind` was read (`stderr`/`stdout`, or `config:env_from`
    /// for a candidate refused before spawning); `null` when absent.
    // llmlint: ignore[invalid_states_unrepresentable] This is a stable serialized string in the JSON/SDK contract, deliberately open so a new reading site is an additive value rather than a generated-type change for every consumer; the values are produced only by `domain::signals` and the pre-spawn refusal, and the report round-trip tests pin them.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(telemetry: ExecutionTelemetry) -> Value {
        let wire = serde_json::to_value(&telemetry).expect("telemetry serializes");
        let back: ExecutionTelemetry =
            serde_json::from_value(wire.clone()).expect("telemetry round-trips");
        assert_eq!(back, telemetry);
        wire
    }

    #[test]
    fn provider_measured_telemetry_round_trips_its_millisecond_splits() {
        // The u128 durations are the reason this enum hand-rolls `Deserialize`:
        // serde's internally-tagged buffer cannot hold one, so a derived impl
        // would serialize fine and then refuse to read its own output back.
        let wire = round_trip(ExecutionTelemetry::ProviderMeasured {
            started_at: "2026-01-01T00:00:00.000Z".parse().unwrap(),
            finished_at: Some("2026-01-01T00:00:12.500Z".parse().unwrap()),
            model_ms: Some(9_000),
            tool_ms: Some(3_500),
            time_to_first_token_ms: Some(420),
        });
        assert_eq!(wire["source"], "provider_measured");
        assert_eq!(wire["model_ms"], 9_000);
        assert_eq!(wire["time_to_first_token_ms"], 420);
    }

    #[test]
    fn the_other_telemetry_variants_round_trip_under_their_own_source() {
        let partial = round_trip(ExecutionTelemetry::PartialInvocation {
            started_at: "2026-01-01T00:00:00Z".parse().expect("canonical instant"),
        });
        assert_eq!(partial["source"], "partial_invocation");
        assert_eq!(partial["started_at"], "2026-01-01T00:00:00Z");

        let observed = round_trip(ExecutionTelemetry::StdoutObserved { tool_ms: 77 });
        assert_eq!(observed["source"], "stdout_observed");
        assert_eq!(observed["tool_ms"], 77);
    }

    #[test]
    fn a_run_instant_accepts_only_millisecond_precision_utc() {
        // The one rule the JsonSchema above also states. Milliseconds are the
        // point: `UtcInstant` would canonicalize these to whole seconds and make
        // a sub-second run read as instantaneous.
        let ok: RunInstant = "2026-01-01T00:00:00.000Z".parse().unwrap();
        assert_eq!(ok.as_str(), "2026-01-01T00:00:00.000Z");
        assert_eq!(ok.to_string(), "2026-01-01T00:00:00.000Z");

        for bad in [
            "",
            "2026-01-01T00:00:00Z",          // no milliseconds
            "2026-01-01T00:00:00.000000Z",   // microseconds: not this shape
            "2026-01-01T00:00:00.000+00:00", // an equivalent offset is still not `Z`
            "2026-01-01T00:00:00.000-05:00", // and a real offset is not UTC at all
            "2026-13-01T00:00:00.000Z",      // shaped like a time, is not one
            "2026-01-01T25:00:00.000Z",
            "2026-01-01 00:00:00.000Z", // the space form loses the length bound
            "2026-01-01T00:00:00.000Z\n", // the trailing newline a `$` would admit
            "2026-01-32T00:00:00.000Z",
            "2026-01-01T00:61:00.000Z",
        ] {
            assert!(
                bad.parse::<RunInstant>().is_err(),
                "accepted {bad:?} as a run instant"
            );
        }
        // A real leap second is a real instant; a date that does not exist is not.
        assert!("2026-06-30T23:59:60.000Z".parse::<RunInstant>().is_ok());
        assert!("2026-02-30T00:00:00.000Z".parse::<RunInstant>().is_err());
        // And the same rule on the deserialization boundary, not just FromStr.
        assert!(serde_json::from_value::<RunInstant>(serde_json::json!("nope")).is_err());
    }

    #[test]
    fn unreadable_telemetry_is_an_error_not_a_guess() {
        // A missing/unknown tag or a variant's own missing field must fail loudly:
        // inventing a source would put a measurement in the report that nothing
        // measured.
        for bad in [
            serde_json::json!({"tool_ms": 5}),
            serde_json::json!({"source": "wall_clock", "tool_ms": 5}),
            serde_json::json!({"source": "provider_measured"}),
            serde_json::json!({"source": "stdout_observed"}),
            serde_json::json!({"source": "partial_invocation", "started_at": "not a time"}),
            serde_json::json!({
                "source": "provider_measured", "started_at": "2026-01-01T00:00:00Z"
            }),
            serde_json::json!({
                "source": "provider_measured",
                "started_at": "2026-01-01T00:00:00.000Z",
                "finished_at": "whenever"
            }),
            // Required-and-nullable, not optional: the schema generated from the
            // same type says so, and a reader that filled these in with `None`
            // would accept telemetry the SDK validators refuse.
            serde_json::json!({
                "source": "provider_measured",
                "started_at": "2026-01-01T00:00:00.000Z",
                "finished_at": null,
                "model_ms": 1
            }),
            serde_json::json!({
                "source": "provider_measured",
                "started_at": "2026-01-01T00:00:00.000Z",
                "finished_at": null,
                "model_ms": "soon",
                "tool_ms": null,
                "time_to_first_token_ms": null
            }),
            serde_json::json!({"source": "stdout_observed", "tool_ms": null}),
        ] {
            assert!(
                serde_json::from_value::<ExecutionTelemetry>(bad.clone()).is_err(),
                "accepted {bad}"
            );
        }
    }

    #[test]
    fn a_result_without_telemetry_still_deserializes() {
        // The field was absent from the wire before schema 0.5, so a report
        // captured by an older producer must still read back.
        let mut wire = serde_json::to_value(sample_result()).expect("result serializes");
        assert!(wire.get("telemetry").is_some(), "0.5 always writes the key");
        wire.as_object_mut().unwrap().remove("telemetry");
        let back: RunResult = serde_json::from_value(wire).expect("pre-0.5 result round-trips");
        assert!(back.telemetry.is_none());
    }

    #[test]
    fn the_cancelled_status_is_spelled_kebab_case_on_the_wire() {
        assert_eq!(
            serde_json::to_value(Status::Cancelled).unwrap(),
            Value::String("cancelled".to_string())
        );
        assert_eq!(
            serde_json::from_value::<Status>(Value::String("cancelled".to_string())).unwrap(),
            Status::Cancelled
        );
    }

    fn sample_result() -> RunResult {
        RunResult {
            harness: "claude-code".to_string(),
            variant: None,
            harness_id: "claude-code".to_string(),
            bin: "claude".to_string(),
            available: true,
            status: Status::Cancelled,
            prompt: None,
            model: None,
            exit_code: None,
            duration_ms: Some(1_200),
            telemetry: Some(ExecutionTelemetry::StdoutObserved { tool_ms: 12 }),
            command: vec!["claude".to_string()],
            output_format: OutputFormat::Json,
            text: None,
            text_source: None,
            usage: Usage::default(),
            usage_source: None,
            session_id: None,
            events: None,
            events_source: None,
            structured: None,
            schema_valid: None,
            schema_attempts: None,
            schema_error: None,
            failure_kind: None,
            failure_kind_source: None,
            stdout: String::new(),
            stderr: String::new(),
            error: Some("cancelled".to_string()),
        }
    }
}
