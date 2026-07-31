//! Best-effort extraction of normalized signals — token/cost usage, the session
//! id, and a coarse failure reason — from a harness's raw output. Pure: no I/O.
//!
//! Like `text`, every signal here is a convenience: it is `null`/empty when it
//! cannot be found, is **never fabricated**, and (where there is more than one
//! possible method) records how it was found, so a consumer can tell a real
//! reading from a guess. The execution envelope stays the guaranteed contract;
//! these only enrich it. An absent signal is the honest answer, not an error.
//!
//! Coverage is keyed off each harness's real output shape, sourced from its docs,
//! not guessed:
//! - Claude Code (`--output-format json`): a terminal `result` event with a
//!   top-level `usage` block (including `cache_read_input_tokens` /
//!   `cache_creation_input_tokens` for prompt-cache reads/writes), `total_cost_usd`,
//!   and `session_id`.
//! - OpenCode (`run --format json`): JSONL `step_finish` events that report
//!   *per-step* tokens/cost under `part` (cache reads/writes under
//!   `part.tokens.cache.{read,write}`), and a camelCase `sessionID`. The run
//!   total is the sum across steps (taking the last would undercount).
//! - Cursor (`--output-format stream-json`): NDJSON whose events carry a
//!   snake_case `session_id`; it does not emit token usage today, so usage stays
//!   absent rather than fabricated.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Normalized token/cost accounting. Every field is best-effort and independently
/// nullable: a harness may report tokens but not dollar cost (cost is commonly
/// absent on subscription auth), or report nothing at all (plain-text harnesses).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Usage {
    /// Prompt/input tokens billed, when the harness reports them.
    pub input_tokens: Option<u64>,
    /// Completion/output tokens billed, when the harness reports them.
    pub output_tokens: Option<u64>,
    /// Prompt tokens served from the provider's prompt cache (a cheap read of a
    /// previously-written prefix), when the harness reports them. `None` when the
    /// harness does not surface cache counts — never `0` as a guess.
    pub cache_read_tokens: Option<u64>,
    /// Prompt tokens written to the provider's prompt cache (a.k.a. cache
    /// creation), when the harness reports them. `None` when not surfaced.
    pub cache_write_tokens: Option<u64>,
    /// Total cost in USD, when the harness reports it (often absent on
    /// subscription auth, where there is no per-call dollar figure).
    pub cost_usd: Option<f64>,
}

impl Usage {
    fn is_empty(&self) -> bool {
        self.input_tokens.is_none()
            && self.output_tokens.is_none()
            && self.cache_read_tokens.is_none()
            && self.cache_write_tokens.is_none()
            && self.cost_usd.is_none()
    }
}

/// A usage reading plus the method that produced it (e.g. `json`).
#[derive(Debug, Clone, PartialEq)]
pub struct UsageReading {
    pub usage: Usage,
    pub source: String,
}

/// The normalized, closed set of failure reasons oneharness can classify from a
/// harness's output. It is the single source for the `failure_kind` contract
/// value: serialized as the snake_case token a consumer reads in the report
/// (`auth`, `rate_limit`, `model_not_found`, `quota`, `tool_deferred`), so the
/// wire shape is unchanged — modeling it as an enum keeps a misspelled or
/// invalid kind unrepresentable and gives every producer/consumer (classifier,
/// `is_failure`, the fallback fall-through rule, the report, history) one
/// definition to share instead of scattered string literals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    /// Authentication / authorization rejected the request (401/403, missing or
    /// invalid credentials).
    Auth,
    /// Rate limited (429, too many requests) — a transient condition of an
    /// otherwise working, authenticated harness.
    RateLimit,
    /// The requested model was not found / is invalid — a configuration mistake.
    ModelNotFound,
    /// Out of quota / credits, or a billing problem — a provisioning failure.
    Quota,
    /// The harness deferred a builtin tool call instead of executing it, so a
    /// clean-exit run did no useful work (Claude Code bridge deployments; issue
    /// #1114). The only kind that can appear on a `status: ok` run.
    ToolDeferred,
}

impl FailureKind {
    /// The snake_case token this kind serializes to — for the few call sites
    /// (stderr diagnostics, the fallback reason map) that need the raw string.
    pub fn as_str(self) -> &'static str {
        match self {
            FailureKind::Auth => "auth",
            FailureKind::RateLimit => "rate_limit",
            FailureKind::ModelNotFound => "model_not_found",
            FailureKind::Quota => "quota",
            FailureKind::ToolDeferred => "tool_deferred",
        }
    }
}

/// A classified failure reason plus where it was read from (`stderr`/`stdout`).
#[derive(Debug, Clone, PartialEq)]
pub struct FailureReading {
    pub kind: FailureKind,
    pub source: String,
}

/// Adapter-specific failure vocabulary understood by the signal classifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureDialect {
    Generic,
    ClaudeCode,
    Codex,
}

/// A deferred-tool dead-end detected in a harness's output: the harness ended a
/// turn by *deferring* a builtin tool call instead of executing it, so the run
/// exits cleanly (status `ok`) with an empty result. This is Claude Code's
/// behavior in a bridged/managed deployment (where `tengu_non_deferrable_builtins`
/// is empty), and without detection it masquerades as a schema/output failure —
/// see issue #1114.
#[derive(Debug, Clone, PartialEq)]
pub struct DeferredTool {
    /// The name of the tool the harness deferred (e.g. `Read`), when the output
    /// named it; `None` when the shape signalled a deferral without a name.
    pub tool: Option<String>,
}

/// Best-effort token/cost usage from a harness's stdout. Tries the two known
/// shapes in order: a single terminal event whose totals are self-contained
/// (Claude Code), else a sum across per-step events (OpenCode). `None` when
/// neither yields anything.
pub fn extract_usage(stdout: &str) -> Option<UsageReading> {
    let candidates = json_candidates(stdout);
    // Single-object shape: a terminal event carrying the whole-run `usage` block
    // and/or a top-level dollar cost. Scan from the end so the final event wins
    // for stream output.
    if let Some(reading) = candidates.iter().rev().find_map(single_object_usage) {
        return Some(reading);
    }
    // Per-step shape: each event reports only its own step's tokens/cost, so the
    // run total is their sum.
    summed_step_usage(&candidates)
}

/// Usage from one self-contained event: a top-level `usage` object
/// (`input_tokens`/`output_tokens`) plus a top-level `total_cost_usd`/`cost_usd`.
/// `None` when the event carries no usage. This is Claude Code's `result` shape.
fn single_object_usage(value: &Value) -> Option<UsageReading> {
    let obj = value.as_object()?;
    let mut usage = Usage::default();
    if let Some(u) = obj.get("usage").and_then(Value::as_object) {
        usage.input_tokens = u.get("input_tokens").and_then(Value::as_u64);
        usage.output_tokens = u.get("output_tokens").and_then(Value::as_u64);
        usage.cache_read_tokens = u
            .get("cache_read_input_tokens")
            .or_else(|| u.get("cached_input_tokens"))
            .and_then(Value::as_u64);
        usage.cache_write_tokens = u.get("cache_creation_input_tokens").and_then(Value::as_u64);
    }
    usage.cost_usd = obj
        .get("total_cost_usd")
        .or_else(|| obj.get("cost_usd"))
        .and_then(Value::as_f64);
    (!usage.is_empty()).then(|| UsageReading {
        usage,
        source: "json".to_string(),
    })
}

/// Version of the deliberately small fallback price table. Prices are USD per
/// million tokens and only apply to exact documented model families; an unknown
/// alias stays unpriced rather than being guessed.
pub const MODEL_PRICE_TABLE_VERSION: &str = "openai-2026-07-19";

pub fn apply_model_price(usage: &mut Usage, harness: &str, model: Option<&str>) {
    if usage.cost_usd.is_some() {
        return;
    }
    // Harness identity is the provider boundary available in the normalized
    // result. Never apply an OpenAI table to another provider's coincidentally
    // identical model alias.
    if harness != "codex" {
        return;
    }
    let Some(model) = model else { return };
    let (input_rate, cached_rate, output_rate) = match model {
        // OpenAI API pricing snapshot named by MODEL_PRICE_TABLE_VERSION.
        "gpt-5-codex" => (1.25, 0.125, 10.0),
        _ => return,
    };
    let (Some(input), Some(output)) = (usage.input_tokens, usage.output_tokens) else {
        return;
    };
    let cached = usage.cache_read_tokens.unwrap_or(0).min(input);
    let uncached = input - cached;
    usage.cost_usd = Some(
        (uncached as f64 * input_rate + cached as f64 * cached_rate + output as f64 * output_rate)
            / 1_000_000.0,
    );
}

/// Usage summed across OpenCode `step_finish` events, each of which reports only
/// its own step under `part` (`part.tokens.{input,output}`, `part.cost`). A
/// consumer that wants the run total needs the sum; taking the last step would
/// silently undercount. `None` when no step carried usage.
fn summed_step_usage(candidates: &[Value]) -> Option<UsageReading> {
    let mut usage = Usage::default();
    let mut saw_usage = false;
    for value in candidates {
        let Some(obj) = value.as_object() else {
            continue;
        };
        if obj.get("type").and_then(Value::as_str) != Some("step_finish") {
            continue;
        }
        let Some(part) = obj.get("part").and_then(Value::as_object) else {
            continue;
        };
        if let Some(tokens) = part.get("tokens").and_then(Value::as_object) {
            saw_usage |= add_u64(&mut usage.input_tokens, tokens.get("input"));
            saw_usage |= add_u64(&mut usage.output_tokens, tokens.get("output"));
            if let Some(cache) = tokens.get("cache").and_then(Value::as_object) {
                saw_usage |= add_u64(&mut usage.cache_read_tokens, cache.get("read"));
                saw_usage |= add_u64(&mut usage.cache_write_tokens, cache.get("write"));
            }
        }
        saw_usage |= add_f64(&mut usage.cost_usd, part.get("cost"));
    }
    saw_usage.then(|| UsageReading {
        usage,
        source: "json:summed-steps".to_string(),
    })
}

/// Add `value` (as a u64) into `acc`, returning whether a number was present.
fn add_u64(acc: &mut Option<u64>, value: Option<&Value>) -> bool {
    match value.and_then(Value::as_u64) {
        Some(v) => {
            *acc = Some(acc.unwrap_or(0) + v);
            true
        }
        None => false,
    }
}

/// Add `value` (as an f64) into `acc`, returning whether a number was present.
fn add_f64(acc: &mut Option<f64>, value: Option<&Value>) -> bool {
    match value.and_then(Value::as_f64) {
        Some(v) => {
            *acc = Some(acc.unwrap_or(0.0) + v);
            true
        }
        None => false,
    }
}

/// Best-effort harness session id from stdout (the handle a harness exposes for
/// `--resume`-style continuation). Reads the first non-empty session id string
/// found in the emitted JSON, accepting the snake_case `session_id` (Claude Code,
/// Cursor, Qwen), the camelCase `sessionID` (OpenCode), and Codex's `thread_id`
/// (emitted on its `thread.started` event under `--json`); `None` when absent. A
/// harness that emits no id headlessly (Goose, Copilot) yields `None` — its
/// continuation handle is caller-supplied, never scraped.
pub fn extract_session(stdout: &str) -> Option<String> {
    json_candidates(stdout).into_iter().find_map(|value| {
        let obj = value.as_object()?;
        obj.get("session_id")
            .or_else(|| obj.get("sessionID"))
            .or_else(|| obj.get("thread_id"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    })
}

/// Coarse, best-effort reason a run failed, scanned from its stderr then stdout.
///
/// This is deliberately distinct from `Status`: `Status` describes oneharness's
/// relationship to the process (it exited non-zero), while this hints at *why* —
/// so a consumer can separate retryable conditions (auth, rate limit) from a
/// broken request (unknown model). It never changes exit-code semantics and is
/// `None` when no known signal matches.
pub fn classify_failure(stdout: &str, stderr: &str) -> Option<FailureReading> {
    for (source, text) in [("stderr", stderr), ("stdout", stdout)] {
        if let Some(kind) = match_failure(text) {
            return Some(FailureReading {
                kind,
                source: source.to_string(),
            });
        }
    }
    None
}

/// Harness-aware failure classification for adapter-specific text surfaces.
///
/// Claude Code's subscription exhaustion does not carry the generic `quota`
/// vocabulary: headless runs report a session/weekly limit message instead.
/// Keep those phrases scoped to that adapter so another harness mentioning a
/// "session limit" in an unrelated error is not silently treated as exhausted.
/// Codex is deliberately absent: it declares its usage limit in a structured
/// `turn.failed` event (see [`detect_harness_provider_failure`]), so scanning its
/// whole transcript would risk reading an agent's own mention of a usage limit
/// as exhaustion and silently re-running the task on another account.
pub fn classify_harness_failure(
    dialect: FailureDialect,
    stdout: &str,
    stderr: &str,
) -> Option<FailureReading> {
    classify_failure(stdout, stderr).or_else(|| {
        (dialect == FailureDialect::ClaudeCode).then_some(())?;
        for (source, text) in [("stderr", stderr), ("stdout", stdout)] {
            if claude_subscription_limit(text) {
                return Some(FailureReading {
                    kind: FailureKind::Quota,
                    source: source.to_string(),
                });
            }
        }
        None
    })
}

/// Classify a provider-declared failed result even when its CLI exits zero.
///
/// Some harnesses, including Claude Code on Windows, report an API rejection in
/// a terminal JSON record with `is_error: true` but still exit successfully.
/// Restricting this check to those explicit error records avoids treating
/// incidental warning text in an otherwise successful transcript as failure.
pub fn detect_provider_failure(stdout: &str) -> Option<FailureReading> {
    detect_harness_provider_failure(FailureDialect::Generic, stdout)
}

/// Provider-declared failure classification with adapter-specific quota
/// surfaces. The terminal `is_error` record is the machine signal; matching its
/// complete JSON record captures provider metadata as well as result text while
/// avoiding unstructured output outside that explicit failure record. Codex
/// declares its failure differently, so it gets its own record shape below.
pub fn detect_harness_provider_failure(
    dialect: FailureDialect,
    stdout: &str,
) -> Option<FailureReading> {
    json_candidates(stdout).into_iter().rev().find_map(|value| {
        let kind = error_record_failure(dialect, &value)
            .or_else(|| codex_turn_failure(dialect, &value))?;
        Some(FailureReading {
            kind,
            source: "stdout".to_string(),
        })
    })
}

fn error_record_failure(dialect: FailureDialect, value: &Value) -> Option<FailureKind> {
    if value.get("is_error").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let serialized = value.to_string();
    match_failure(&serialized).or_else(|| {
        (dialect == FailureDialect::ClaudeCode && claude_subscription_limit(&serialized))
            .then_some(FailureKind::Quota)
    })
}

/// Codex reports an exhausted account as a `turn.failed` event inside its stdout
/// event stream — after `turn.started`, never on stderr — and the process can
/// still exit zero. Only a message carrying the usage-limit signature is a quota
/// rejection: an ordinary `turn.failed` is a real task failure, and classifying
/// it here would let a fallback chain silently re-run the task on another
/// account. The turn started, but no work was done, so the rejection is a
/// provisioning failure like `auth` — the event ordering is a reporting detail
/// of the Codex CLI.
fn codex_turn_failure(dialect: FailureDialect, value: &Value) -> Option<FailureKind> {
    (dialect == FailureDialect::Codex).then_some(())?;
    (value.get("type").and_then(Value::as_str) == Some("turn.failed")).then_some(())?;
    let message = value.get("error")?.get("message").and_then(Value::as_str)?;
    codex_usage_limit(message).then_some(FailureKind::Quota)
}

/// Codex's usage-limit signature, matched on the stable phrasing only: the reset
/// date and the purchase URL in the real message both vary.
fn codex_usage_limit(text: &str) -> bool {
    let text = text.to_lowercase();
    [
        "hit your usage limit",
        "reached your usage limit",
        "usage limit reached",
        "usage limit exceeded",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

fn claude_subscription_limit(text: &str) -> bool {
    let text = text.to_lowercase();
    [
        "you've hit your session limit",
        "you’ve hit your session limit",
        "you have hit your session limit",
        "you've hit your limit",
        "you’ve hit your limit",
        "you have hit your limit",
        "weekly limit reached",
        "usage limit reached",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

/// Match the first known failure signal in `text` (case-insensitive). Ordered
/// most-specific first so a 429 reads as `rate_limit`, not `auth`.
fn match_failure(text: &str) -> Option<FailureKind> {
    let h = text.to_lowercase();
    const SIGNALS: &[(FailureKind, &[&str])] = &[
        (
            FailureKind::ModelNotFound,
            &[
                "model not found",
                "model_not_found",
                "unknown model",
                "no such model",
                "invalid model",
                "is not a valid model",
            ],
        ),
        (
            FailureKind::RateLimit,
            &[
                "rate limit",
                "rate-limit",
                "ratelimit",
                "too many requests",
                "429",
            ],
        ),
        (
            FailureKind::Quota,
            &[
                "insufficient_quota",
                "quota",
                "credit balance",
                "out of credits",
                "billing",
            ],
        ),
        (
            FailureKind::Auth,
            &[
                "unauthorized",
                "authentication",
                "not authenticated",
                "invalid api key",
                "missing api key",
                "no api key",
                "credentials",
                "forbidden",
                "401",
                "403",
            ],
        ),
    ];
    SIGNALS
        .iter()
        .find(|(_, needles)| needles.iter().any(|n| h.contains(n)))
        .map(|(kind, _)| *kind)
}

/// Best-effort detection of a **deferred-tool dead-end** in `stdout`: a run that
/// ended a turn by deferring a builtin tool call rather than executing it, so it
/// completed with an empty result and no useful work done. This is distinct from
/// [`classify_failure`] because a deferred run exits *cleanly* (status `ok`), so
/// it needs its own detection independent of the exit code.
///
/// Two equivalent signals of the same Claude Code shape (bridged/managed
/// deployments), scanned terminal-event-first so a stream's final event wins:
/// a `stop_reason`/`terminal_reason` of `"tool_deferred"`, or a non-empty
/// `deferred_tool_use` object alongside an empty/absent `result`. The tool name
/// is lifted from `deferred_tool_use.name` when present, never fabricated.
/// `None` when neither signal is found (the ordinary case for every harness).
pub fn detect_deferred_tool(stdout: &str) -> Option<DeferredTool> {
    json_candidates(stdout)
        .iter()
        .rev()
        .find_map(deferred_from_object)
}

/// Detect the deferred-tool shape in one JSON object. `None` unless the object
/// carries a `"tool_deferred"` reason or a `deferred_tool_use` with an empty
/// result — so a normal `result` document (which has neither) never matches.
fn deferred_from_object(value: &Value) -> Option<DeferredTool> {
    let obj = value.as_object()?;
    let reason_deferred = ["stop_reason", "terminal_reason"]
        .iter()
        .any(|k| obj.get(*k).and_then(Value::as_str) == Some("tool_deferred"));
    let deferred_use = obj.get("deferred_tool_use").and_then(Value::as_object);
    // A `deferred_tool_use` present with an empty/absent `result` is the same
    // dead-end even when the reason field is absent; a normal answer carries a
    // non-empty `result` and no `deferred_tool_use`, so it never trips this.
    let result_empty = obj
        .get("result")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty);
    if !(reason_deferred || (deferred_use.is_some() && result_empty)) {
        return None;
    }
    let tool = deferred_use
        .and_then(|d| d.get("name"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    Some(DeferredTool { tool })
}

/// Candidate JSON objects in `stdout`: the whole document when it parses, else
/// each parseable line (stream-json). In document order, so a caller can prefer
/// the terminal event by scanning from the end.
fn json_candidates(stdout: &str) -> Vec<Value> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return vec![value];
    }
    stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_from_claude_shaped_json() {
        let raw = r#"{"type":"result","result":"hi","session_id":"abc","total_cost_usd":0.0095,
            "usage":{"input_tokens":1234,"output_tokens":56,"cache_read_input_tokens":7,
            "cache_creation_input_tokens":89}}"#;
        let got = extract_usage(raw).unwrap();
        assert_eq!(got.usage.input_tokens, Some(1234));
        assert_eq!(got.usage.output_tokens, Some(56));
        assert_eq!(got.usage.cache_read_tokens, Some(7));
        assert_eq!(got.usage.cache_write_tokens, Some(89));
        assert_eq!(got.usage.cost_usd, Some(0.0095));
        assert_eq!(got.source, "json");
    }

    #[test]
    fn usage_cache_only_counts_as_usage() {
        // A run that reports only cache tokens (no fresh input/output, no cost) is
        // still a real reading — cache fields count toward "has usage".
        let raw = r#"{"type":"result","usage":{"cache_read_input_tokens":42}}"#;
        let got = extract_usage(raw).unwrap();
        assert_eq!(got.usage.cache_read_tokens, Some(42));
        assert_eq!(got.usage.input_tokens, None);
        assert_eq!(got.usage.cache_write_tokens, None);
    }

    #[test]
    fn usage_without_cache_fields_yields_none_cache() {
        // Harnesses/shapes that don't report cache counts leave the cache fields
        // null — never fabricated as 0.
        let raw = r#"{"type":"result","usage":{"input_tokens":9,"output_tokens":3}}"#;
        let got = extract_usage(raw).unwrap();
        assert_eq!(got.usage.input_tokens, Some(9));
        assert_eq!(got.usage.cache_read_tokens, None);
        assert_eq!(got.usage.cache_write_tokens, None);
    }

    #[test]
    fn codex_turn_usage_and_versioned_price_are_populated_without_guessing_unknown_models() {
        let raw = r#"{"type":"turn.completed","usage":{"input_tokens":1000,"cached_input_tokens":400,"output_tokens":100}}"#;
        let mut known = extract_usage(raw).unwrap().usage;
        apply_model_price(&mut known, "codex", Some("gpt-5-codex"));
        assert_eq!(known.input_tokens, Some(1000));
        assert_eq!(known.cache_read_tokens, Some(400));
        assert!((known.cost_usd.unwrap() - 0.0018).abs() < 1e-12);

        let mut unknown = extract_usage(raw).unwrap().usage;
        apply_model_price(&mut unknown, "codex", Some("provider-alias"));
        assert_eq!(unknown.cost_usd, None);

        let mut other_provider = extract_usage(raw).unwrap().usage;
        apply_model_price(&mut other_provider, "claude-code", Some("gpt-5-codex"));
        assert_eq!(other_provider.cost_usd, None);

        let mut native = extract_usage(raw).unwrap().usage;
        native.cost_usd = Some(0.42);
        apply_model_price(&mut native, "codex", Some("gpt-5-codex"));
        assert_eq!(native.cost_usd, Some(0.42));
    }

    #[test]
    fn malformed_usage_values_are_unknown_not_zero() {
        let got = extract_usage(
            r#"{"type":"turn.completed","usage":{"input_tokens":-1,"output_tokens":"4","cached_input_tokens":null}}"#,
        );
        assert!(got.is_none());
    }

    #[test]
    fn usage_cost_only_still_reported() {
        // Tokens absent but a cost present (or vice versa) is a non-empty reading.
        let got = extract_usage(r#"{"cost_usd":0.5}"#).unwrap();
        assert_eq!(got.usage.cost_usd, Some(0.5));
        assert_eq!(got.usage.input_tokens, None);
    }

    #[test]
    fn usage_absent_yields_none() {
        assert!(extract_usage(r#"{"result":"hi"}"#).is_none());
        assert!(extract_usage("not json").is_none());
        assert!(extract_usage("").is_none());
    }

    #[test]
    fn usage_prefers_terminal_stream_event() {
        let raw = concat!(
            "{\"type\":\"system\",\"session_id\":\"s\"}\n",
            "{\"type\":\"result\",\"usage\":{\"input_tokens\":9},\"total_cost_usd\":0.01}\n",
        );
        let got = extract_usage(raw).unwrap();
        assert_eq!(got.usage.input_tokens, Some(9));
        assert_eq!(got.usage.cost_usd, Some(0.01));
    }

    #[test]
    fn session_id_extracted_when_present() {
        assert_eq!(
            extract_session(r#"{"session_id":"sess-123","result":"x"}"#),
            Some("sess-123".to_string())
        );
        assert_eq!(extract_session(r#"{"result":"x"}"#), None);
        assert_eq!(extract_session(r#"{"session_id":""}"#), None);
    }

    #[test]
    fn session_id_read_from_camelcase_and_codex_thread_id() {
        // OpenCode's camelCase handle.
        assert_eq!(
            extract_session(r#"{"sessionID":"ses_abc","type":"text"}"#),
            Some("ses_abc".to_string())
        );
        // Codex's `thread.started` event under `--json` carries `thread_id`.
        assert_eq!(
            extract_session(r#"{"type":"thread.started","thread_id":"0199-xyz"}"#),
            Some("0199-xyz".to_string())
        );
    }

    #[test]
    fn usage_summed_across_opencode_step_finish_events() {
        // OpenCode JSONL: per-step tokens/cost under `part`; the run total is the
        // sum, and the method is recorded distinctly from the single-event shape.
        let raw = concat!(
            "{\"type\":\"step_start\",\"sessionID\":\"ses_1\",\"part\":{}}\n",
            "{\"type\":\"step_finish\",\"sessionID\":\"ses_1\",\"part\":{\"cost\":0.001,\
             \"tokens\":{\"input\":671,\"output\":8,\"reasoning\":0,\
             \"cache\":{\"read\":21415,\"write\":100}}}}\n",
            "{\"type\":\"step_finish\",\"sessionID\":\"ses_1\",\"part\":{\"cost\":0.002,\
             \"tokens\":{\"input\":12,\"output\":34,\"cache\":{\"read\":5,\"write\":3}}}}\n",
        );
        let got = extract_usage(raw).unwrap();
        assert_eq!(got.usage.input_tokens, Some(683));
        assert_eq!(got.usage.output_tokens, Some(42));
        assert_eq!(got.usage.cache_read_tokens, Some(21420));
        assert_eq!(got.usage.cache_write_tokens, Some(103));
        assert!((got.usage.cost_usd.unwrap() - 0.003).abs() < 1e-9);
        assert_eq!(got.source, "json:summed-steps");
    }

    #[test]
    fn session_from_opencode_camelcase_sessionid() {
        let raw = r#"{"type":"step_start","sessionID":"ses_494719016ffe85","part":{}}"#;
        assert_eq!(extract_session(raw), Some("ses_494719016ffe85".to_string()));
    }

    #[test]
    fn cursor_session_from_stream_json_lines_without_usage() {
        // cursor-agent stream-json: snake_case session_id in NDJSON events; it does
        // not emit token usage today, so usage is honestly absent (not fabricated).
        let raw = concat!(
            "{\"type\":\"system\",\"subtype\":\"init\",\
             \"session_id\":\"11111111-2222-3333-4444-555555555555\",\"model\":\"x\"}\n",
            "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\
             \"result\":\"pong\",\"session_id\":\"11111111-2222-3333-4444-555555555555\"}\n",
        );
        assert_eq!(
            extract_session(raw),
            Some("11111111-2222-3333-4444-555555555555".to_string())
        );
        assert!(extract_usage(raw).is_none());
    }

    #[test]
    fn classify_distinguishes_common_failures() {
        assert_eq!(
            classify_failure("", "Error: 401 Unauthorized")
                .unwrap()
                .kind,
            FailureKind::Auth
        );
        assert_eq!(
            classify_failure("", "HTTP 429: rate limit exceeded")
                .unwrap()
                .kind,
            FailureKind::RateLimit
        );
        assert_eq!(
            classify_failure("", "model not found: gpt-9").unwrap().kind,
            FailureKind::ModelNotFound
        );
        assert_eq!(
            classify_failure("", "insufficient_quota; check billing")
                .unwrap()
                .kind,
            FailureKind::Quota
        );
    }

    #[test]
    fn failure_kind_serializes_to_its_snake_case_token() {
        // The wire contract: the enum must serialize to exactly the strings a
        // consumer reads in `failure_kind`, and `as_str` must agree with serde.
        for kind in [
            FailureKind::Auth,
            FailureKind::RateLimit,
            FailureKind::ModelNotFound,
            FailureKind::Quota,
            FailureKind::ToolDeferred,
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(json, format!("\"{}\"", kind.as_str()));
        }
        assert_eq!(FailureKind::ToolDeferred.as_str(), "tool_deferred");
    }

    #[test]
    fn classify_records_source_and_prefers_stderr() {
        let got = classify_failure("rate limit in stdout", "unauthorized in stderr").unwrap();
        assert_eq!(got.kind, FailureKind::Auth);
        assert_eq!(got.source, "stderr");
        let got = classify_failure("model not found", "").unwrap();
        assert_eq!(got.source, "stdout");
    }

    #[test]
    fn classify_none_when_no_signal() {
        assert!(classify_failure("just some output", "a normal error").is_none());
    }

    #[test]
    fn claude_subscription_limit_fixtures_classify_as_quota() {
        let session_json = include_str!("../../../../tests/fixtures/claude-session-limit.json");
        let weekly_json = include_str!("../../../../tests/fixtures/claude-weekly-limit.json");
        let session_text = include_str!("../../../../tests/fixtures/claude-session-limit.txt");

        for captured in [session_json, weekly_json] {
            let got =
                detect_harness_provider_failure(FailureDialect::ClaudeCode, captured).unwrap();
            assert_eq!(got.kind, FailureKind::Quota);
            assert_eq!(got.source, "stdout");
        }
        let got = classify_harness_failure(FailureDialect::ClaudeCode, "", session_text).unwrap();
        assert_eq!(got.kind, FailureKind::Quota);
        assert_eq!(got.source, "stderr");
    }

    #[test]
    fn claude_limit_language_is_adapter_scoped_and_requires_a_failure_surface() {
        let captured = include_str!("../../../../tests/fixtures/claude-session-limit.json");
        assert!(detect_harness_provider_failure(FailureDialect::Generic, captured).is_none());
        assert!(classify_harness_failure(
            FailureDialect::Generic,
            "",
            "tool crashed while calculating a session limit"
        )
        .is_none());
        assert!(detect_harness_provider_failure(
            FailureDialect::ClaudeCode,
            r#"{"type":"result","is_error":false,"result":"You've hit your session limit"}"#
        )
        .is_none());
    }

    #[test]
    fn codex_usage_limit_capture_classifies_as_quota() {
        // The real capture: the limit arrives as a `turn.failed` event on stdout,
        // after `turn.started` — never on stderr — so the event stream is the
        // only surface carrying it.
        let captured = include_str!("../../../../tests/fixtures/codex-usage-limit.jsonl");
        let got = detect_harness_provider_failure(FailureDialect::Codex, captured).unwrap();
        assert_eq!(got.kind, FailureKind::Quota);
        assert_eq!(got.source, "stdout");
        // The reset date and the purchase URL vary, so neither is part of the
        // signature: the phrasing alone still reads as exhausted.
        let other_reset = r#"{"type":"turn.failed","error":{"message":"You’ve hit your usage limit. Try again at Dec 31st, 2027 6:00 AM."}}"#;
        assert_eq!(
            detect_harness_provider_failure(FailureDialect::Codex, other_reset)
                .unwrap()
                .kind,
            FailureKind::Quota
        );
    }

    #[test]
    fn codex_ordinary_turn_failure_is_never_a_quota_rejection() {
        // A real task failure must stay unclassified: treating it as quota would
        // silently re-run the task on another account under `--run-mode fallback`.
        let captured = include_str!("../../../../tests/fixtures/codex-turn-failed.jsonl");
        assert!(detect_harness_provider_failure(FailureDialect::Codex, captured).is_none());
        assert!(classify_harness_failure(FailureDialect::Codex, captured, "").is_none());
        // Codex's limit phrasing is adapter-scoped, and only its own failure
        // event counts: an agent message quoting the limit is not a rejection.
        let limit = include_str!("../../../../tests/fixtures/codex-usage-limit.jsonl");
        assert!(detect_harness_provider_failure(FailureDialect::Generic, limit).is_none());
        assert!(classify_harness_failure(FailureDialect::Codex, limit, "").is_none());
        assert!(detect_harness_provider_failure(
            FailureDialect::Codex,
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"You've hit your usage limit"}}"#
        )
        .is_none());
    }

    #[test]
    fn provider_error_record_is_classified_on_a_clean_process_exit() {
        let stdout = concat!(
            r#"{"type":"system","subtype":"init"}"#,
            "\n",
            r#"{"type":"result","subtype":"success","is_error":true,"api_error_status":401,"result":"Invalid API key · Fix external API key"}"#
        );
        let got = detect_provider_failure(stdout).unwrap();
        assert_eq!(got.kind, FailureKind::Auth);
        assert_eq!(got.source, "stdout");
        assert!(detect_provider_failure(
            r#"{"type":"result","is_error":false,"result":"mentions a 401 example"}"#
        )
        .is_none());
    }

    #[test]
    fn detects_deferred_tool_with_named_tool() {
        // Claude Code's bridge-deployment shape: a clean (exit 0) result whose
        // `stop_reason` is `tool_deferred`, empty `result`, and a named
        // `deferred_tool_use`. The tool name is lifted, never fabricated.
        let raw = r#"{"type":"result","num_turns":1,"stop_reason":"tool_deferred",
            "terminal_reason":"tool_deferred","result":"","permission_denials":[],
            "deferred_tool_use":{"name":"Read","input":{"file_path":"/x/usage.rs"}}}"#;
        let got = detect_deferred_tool(raw).unwrap();
        assert_eq!(got.tool.as_deref(), Some("Read"));
    }

    #[test]
    fn detects_deferred_tool_from_deferred_use_without_reason() {
        // Even without a reason field, a `deferred_tool_use` alongside an empty
        // result is the same dead-end.
        let raw = r#"{"result":"","deferred_tool_use":{"name":"Bash"}}"#;
        assert_eq!(
            detect_deferred_tool(raw).unwrap().tool.as_deref(),
            Some("Bash")
        );
    }

    #[test]
    fn detects_deferred_tool_without_a_name() {
        // A deferral signalled only by the reason, with no tool named, still
        // detects — the name stays `None` rather than being invented.
        let raw = r#"{"stop_reason":"tool_deferred","result":""}"#;
        assert_eq!(detect_deferred_tool(raw), Some(DeferredTool { tool: None }));
    }

    #[test]
    fn no_deferred_tool_on_a_normal_result() {
        // A normal answer (non-empty `result`, no `deferred_tool_use`) never trips
        // the detector, and neither does non-JSON or empty output.
        assert!(detect_deferred_tool(r#"{"type":"result","result":"pong"}"#).is_none());
        assert!(detect_deferred_tool(r#"{"stop_reason":"end_turn","result":"hi"}"#).is_none());
        assert!(detect_deferred_tool("not json").is_none());
        assert!(detect_deferred_tool("").is_none());
    }

    #[test]
    fn deferred_tool_prefers_terminal_stream_event() {
        // In a stream, the terminal event carries the deferral; scanning from the
        // end finds it even after ordinary earlier events.
        let raw = concat!(
            "{\"type\":\"system\",\"session_id\":\"s\"}\n",
            "{\"type\":\"result\",\"stop_reason\":\"tool_deferred\",\"result\":\"\",\
             \"deferred_tool_use\":{\"name\":\"Read\"}}\n",
        );
        assert_eq!(
            detect_deferred_tool(raw).unwrap().tool.as_deref(),
            Some("Read")
        );
    }

    #[test]
    fn summed_usage_skips_partless_and_usageless_step_events() {
        // The per-step sum must ignore events that carry no usage without
        // mis-reading them: a bare JSON value (no object), a `step_finish` with no
        // `part`, and a `step_finish` whose `part` has neither tokens nor cost.
        // Only the final event contributes, so the sum equals exactly that event.
        let raw = concat!(
            "42\n",
            "{\"type\":\"step_finish\",\"sessionID\":\"ses_1\"}\n",
            "{\"type\":\"step_finish\",\"sessionID\":\"ses_1\",\"part\":{}}\n",
            "{\"type\":\"step_finish\",\"sessionID\":\"ses_1\",\"part\":\
             {\"cost\":0.005,\"tokens\":{\"input\":10,\"output\":2}}}\n",
        );
        let got = extract_usage(raw).unwrap();
        assert_eq!(got.usage.input_tokens, Some(10));
        assert_eq!(got.usage.output_tokens, Some(2));
        assert!((got.usage.cost_usd.unwrap() - 0.005).abs() < 1e-9);
        assert_eq!(got.source, "json:summed-steps");
    }

    #[test]
    fn summed_usage_partial_step_reports_only_present_fields() {
        // A `step_finish` whose `part` has a cost but no `tokens` object (and
        // vice versa) yields a reading with the present field set and the absent
        // one left null — `add_u64`/`add_f64` report "no number present" for the
        // missing side rather than defaulting it to zero.
        let cost_only = "{\"type\":\"step_finish\",\"part\":{\"cost\":0.02}}\n";
        let got = extract_usage(cost_only).unwrap();
        assert_eq!(got.usage.cost_usd, Some(0.02));
        assert_eq!(got.usage.input_tokens, None);
        assert_eq!(got.usage.output_tokens, None);

        let tokens_only = "{\"type\":\"step_finish\",\"part\":{\"tokens\":{\"input\":5}}}\n";
        let got = extract_usage(tokens_only).unwrap();
        assert_eq!(got.usage.input_tokens, Some(5));
        assert_eq!(got.usage.output_tokens, None);
        assert_eq!(got.usage.cost_usd, None);
    }

    #[test]
    fn summed_usage_read_only_cache_hit_leaves_write_null() {
        // The headline scenario: a pure cache *hit* reads a cached prefix and
        // writes nothing, so OpenCode's `cache` block carries `read` but no
        // `write`. The read surfaces; `write` stays null rather than defaulting
        // to zero (never-fabricate, even for the half that's absent).
        let raw = "{\"type\":\"step_finish\",\"part\":\
            {\"tokens\":{\"input\":2,\"output\":1,\"cache\":{\"read\":9000}}}}\n";
        let got = extract_usage(raw).unwrap();
        assert_eq!(got.usage.cache_read_tokens, Some(9000));
        assert_eq!(got.usage.cache_write_tokens, None);
        assert_eq!(got.source, "json:summed-steps");
    }
}
