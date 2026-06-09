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
//!   top-level `usage` block, `total_cost_usd`, and `session_id`.
//! - OpenCode (`run --format json`): JSONL `step_finish` events that report
//!   *per-step* tokens/cost under `part`, and a camelCase `sessionID`. The run
//!   total is the sum across steps (taking the last would undercount).
//! - Cursor (`--output-format stream-json`): NDJSON whose events carry a
//!   snake_case `session_id`; it does not emit token usage today, so usage stays
//!   absent rather than fabricated.

use serde::Serialize;
use serde_json::Value;

/// Normalized token/cost accounting. Every field is best-effort and independently
/// nullable: a harness may report tokens but not dollar cost (cost is commonly
/// absent on subscription auth), or report nothing at all (plain-text harnesses).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Usage {
    /// Prompt/input tokens billed, when the harness reports them.
    pub input_tokens: Option<u64>,
    /// Completion/output tokens billed, when the harness reports them.
    pub output_tokens: Option<u64>,
    /// Total cost in USD, when the harness reports it (often absent on
    /// subscription auth, where there is no per-call dollar figure).
    pub cost_usd: Option<f64>,
}

impl Usage {
    fn is_empty(&self) -> bool {
        self.input_tokens.is_none() && self.output_tokens.is_none() && self.cost_usd.is_none()
    }
}

/// A usage reading plus the method that produced it (e.g. `json`).
#[derive(Debug, Clone, PartialEq)]
pub struct UsageReading {
    pub usage: Usage,
    pub source: String,
}

/// A coarse failure reason plus where it was read from (`stderr`/`stdout`).
#[derive(Debug, Clone, PartialEq)]
pub struct FailureReading {
    pub kind: String,
    pub source: String,
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
/// found in the emitted JSON, accepting both the snake_case `session_id` (Claude
/// Code, Cursor) and camelCase `sessionID` (OpenCode); `None` when absent.
pub fn extract_session(stdout: &str) -> Option<String> {
    json_candidates(stdout).into_iter().find_map(|value| {
        let obj = value.as_object()?;
        obj.get("session_id")
            .or_else(|| obj.get("sessionID"))
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
                kind: kind.to_string(),
                source: source.to_string(),
            });
        }
    }
    None
}

/// Match the first known failure signal in `text` (case-insensitive). Ordered
/// most-specific first so a 429 reads as `rate_limit`, not `auth`.
fn match_failure(text: &str) -> Option<&'static str> {
    let h = text.to_lowercase();
    const SIGNALS: &[(&str, &[&str])] = &[
        (
            "model_not_found",
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
            "rate_limit",
            &[
                "rate limit",
                "rate-limit",
                "ratelimit",
                "too many requests",
                "429",
            ],
        ),
        (
            "quota",
            &[
                "insufficient_quota",
                "quota",
                "credit balance",
                "out of credits",
                "billing",
            ],
        ),
        (
            "auth",
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
            "usage":{"input_tokens":1234,"output_tokens":56,"cache_read_input_tokens":7}}"#;
        let got = extract_usage(raw).unwrap();
        assert_eq!(got.usage.input_tokens, Some(1234));
        assert_eq!(got.usage.output_tokens, Some(56));
        assert_eq!(got.usage.cost_usd, Some(0.0095));
        assert_eq!(got.source, "json");
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
    fn usage_summed_across_opencode_step_finish_events() {
        // OpenCode JSONL: per-step tokens/cost under `part`; the run total is the
        // sum, and the method is recorded distinctly from the single-event shape.
        let raw = concat!(
            "{\"type\":\"step_start\",\"sessionID\":\"ses_1\",\"part\":{}}\n",
            "{\"type\":\"step_finish\",\"sessionID\":\"ses_1\",\"part\":{\"cost\":0.001,\
             \"tokens\":{\"input\":671,\"output\":8,\"reasoning\":0,\"cache\":{\"read\":21415}}}}\n",
            "{\"type\":\"step_finish\",\"sessionID\":\"ses_1\",\"part\":{\"cost\":0.002,\
             \"tokens\":{\"input\":12,\"output\":34}}}\n",
        );
        let got = extract_usage(raw).unwrap();
        assert_eq!(got.usage.input_tokens, Some(683));
        assert_eq!(got.usage.output_tokens, Some(42));
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
            "auth"
        );
        assert_eq!(
            classify_failure("", "HTTP 429: rate limit exceeded")
                .unwrap()
                .kind,
            "rate_limit"
        );
        assert_eq!(
            classify_failure("", "model not found: gpt-9").unwrap().kind,
            "model_not_found"
        );
        assert_eq!(
            classify_failure("", "insufficient_quota; check billing")
                .unwrap()
                .kind,
            "quota"
        );
    }

    #[test]
    fn classify_records_source_and_prefers_stderr() {
        let got = classify_failure("rate limit in stdout", "unauthorized in stderr").unwrap();
        assert_eq!(got.kind, "auth");
        assert_eq!(got.source, "stderr");
        let got = classify_failure("model not found", "").unwrap();
        assert_eq!(got.source, "stdout");
    }

    #[test]
    fn classify_none_when_no_signal() {
        assert!(classify_failure("just some output", "a normal error").is_none());
    }
}
