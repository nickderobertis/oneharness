//! Compacting a named session now (`oneharness compact`): the outcome shape and
//! the pure reading of each harness's own compaction, as the translation table
//! ([`crate::domain::lean`], [`Setting::CompactNow`]) records it.
//!
//! Both translations compact a **fork** of the stored session and rebind the
//! session name to the fork only once the compaction completed. A cancelled or
//! failed compaction therefore never writes to the session it was asked about:
//! the next `run --session` continues it exactly as it was.
//!
//! [`Setting::CompactNow`]: crate::domain::lean::Setting::CompactNow

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::domain::signals::Usage;

/// How a compaction ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CompactStatus {
    /// The harness compacted a fork of the session, and the session name now
    /// continues the compacted fork.
    Completed,
    /// The caller cancelled; the harness process was ended and the session is
    /// exactly as it was.
    Cancelled,
    /// The harness has no compaction oneharness can drive.
    Unsupported,
    /// The harness did not complete the compaction (its own refusal, a
    /// non-zero exit, a timeout); the session is exactly as it was and `error`
    /// says why.
    Failed,
}

impl CompactStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            CompactStatus::Completed => "completed",
            CompactStatus::Cancelled => "cancelled",
            CompactStatus::Unsupported => "unsupported",
            CompactStatus::Failed => "failed",
        }
    }
}

// llmlint: ignore-block[invalid_states_unrepresentable] The flat `status` + `context_tokens_before`/`context_tokens_after` + `usage` shape is the contract agreed with this report's consumer, and `error` with the `failed` status was added by explicit manager ruling as an additive optional beside it; a status-tagged union would change that wire shape. `compact_session` is its one producer and sets `token` and `error` per status in one place.
/// The `oneharness compact` report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CompactOutcome {
    pub schema_version: String,
    pub status: CompactStatus,
    /// The session name compacted.
    pub session: String,
    /// The harness identity the session is bound to.
    pub harness: String,
    /// The context size before compaction, where the harness reports it
    /// (claude-code's `compact_boundary.pre_tokens`); `null` otherwise.
    pub context_tokens_before: Option<u64>,
    /// The context size after compaction, where the harness reports it
    /// (claude-code's `compact_boundary.post_tokens`); `null` otherwise.
    pub context_tokens_after: Option<u64>,
    /// The compaction's own usage, every field `null` where the harness
    /// reports none.
    pub usage: Usage,
    /// The native session token the name continues after this call: the
    /// compacted fork's on `completed`, the untouched original's otherwise.
    /// `null` when the request was refused before a session was resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// Why the compaction is `unsupported` or `failed`; absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
// llmlint: ignore-end[invalid_states_unrepresentable]

/// How claude-code's `/compact` stream says the compaction ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaudeResult {
    /// The stream never reported a `compact_result`.
    Unreported,
    Succeeded,
    /// Any other result, with the harness's own `compact_error` if it gave one.
    Failed(Option<String>),
}

/// What claude-code's stream-json `/compact` output says.
#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeCompaction {
    pub result: ClaudeResult,
    pub before: Option<u64>,
    pub after: Option<u64>,
    /// The session id the compacted conversation lives under (the fork's).
    pub session_id: Option<String>,
    /// The compaction's own usage: the result's `model_usage_since_last_result`
    /// summed across models. Its top-level `usage` is all zeros for a
    /// `/compact` turn and its `total_cost_usd` is the session's running
    /// total, so neither is the compaction's.
    pub usage: Usage,
}

/// The compaction's own usage from a `/compact` result line's
/// `model_usage_since_last_result`; every field `None` when it is absent.
fn claude_compaction_usage(result: &Value) -> Usage {
    let mut usage = Usage::default();
    let Some(models) = result
        .get("model_usage_since_last_result")
        .and_then(Value::as_object)
    else {
        return usage;
    };
    let add = |total: &mut Option<u64>, value: Option<u64>| {
        if let Some(value) = value {
            *total = Some(total.unwrap_or(0) + value);
        }
    };
    for model in models.values() {
        let field = |name: &str| model.get(name).and_then(Value::as_u64);
        add(&mut usage.input_tokens, field("inputTokens"));
        add(&mut usage.output_tokens, field("outputTokens"));
        add(&mut usage.cache_read_tokens, field("cacheReadInputTokens"));
        add(
            &mut usage.cache_write_tokens,
            field("cacheCreationInputTokens"),
        );
        if let Some(cost) = model.get("costUSD").and_then(Value::as_f64) {
            usage.cost_usd = Some(usage.cost_usd.unwrap_or(0.0) + cost);
        }
    }
    usage
}

/// Read claude-code's `/compact` stream (see
/// `tests/fixtures/claude-compact-fork.jsonl`).
#[must_use]
pub fn read_claude(stdout: &str) -> ClaudeCompaction {
    let mut out = ClaudeCompaction {
        result: ClaudeResult::Unreported,
        before: None,
        after: None,
        session_id: None,
        usage: Usage::default(),
    };
    for value in stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
    {
        if out.session_id.is_none() {
            out.session_id = value
                .get("session_id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .map(str::to_string);
        }
        if let Some(result) = value.get("compact_result").and_then(Value::as_str) {
            out.result = if result == "success" {
                ClaudeResult::Succeeded
            } else {
                ClaudeResult::Failed(
                    value
                        .get("compact_error")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                )
            };
        }
        if value.get("type").and_then(Value::as_str) == Some("result") {
            out.usage = claude_compaction_usage(&value);
        }
        if let Some(meta) = value.get("compact_metadata") {
            out.before = meta.get("pre_tokens").and_then(Value::as_u64);
            out.after = meta.get("post_tokens").and_then(Value::as_u64);
        }
    }
    out
}

/// One step of the codex fork-then-compact dialogue.
#[derive(Debug, Clone, PartialEq)]
pub enum CodexStep {
    /// Write these frames and keep reading.
    Send(Vec<String>),
    /// Keep reading.
    Wait,
    /// The compaction ended.
    Done(CodexCompaction),
}

/// How codex's compaction of the fork ended.
#[derive(Debug, Clone, PartialEq)]
pub enum CodexCompaction {
    /// The fork `fork` was compacted; `usage` is what the thread's totals grew
    /// by over the compaction.
    Completed { fork: String, usage: Usage },
    /// Codex refused or did not complete it.
    Failed { error: String },
}

/// Where the dialogue is, carrying exactly what that point has learned.
#[derive(Debug, Clone)]
enum State {
    /// `initialize` sent; awaiting its result.
    Initializing,
    /// `thread/fork` sent; awaiting the new thread.
    Forking,
    /// `thread/compact/start` sent on `fork`; `turn` is the compaction turn
    /// once the fork's `turn/started` names it, and `before` the fork's token
    /// total at that point.
    Compacting {
        fork: String,
        turn: Option<(String, Option<Usage>)>,
    },
}

/// The codex app-server conversation: `initialize`, `thread/fork` of the
/// stored thread, `thread/compact/start` on the fork, until that fork's
/// compaction turn completes (see
/// `tests/fixtures/codex-app-server-fork-compact.jsonl`). The two protocol
/// methods are the [`Setting::CompactNow`] knobs of the translation table.
///
/// [`Setting::CompactNow`]: crate::domain::lean::Setting::CompactNow
#[derive(Debug, Clone)]
pub struct CodexDialogue {
    thread: String,
    model: Option<String>,
    state: State,
    /// The fork's latest token total, whatever turn it was reported for.
    latest: Option<Usage>,
}

const INITIALIZE_ID: u64 = 1;
const FORK_ID: u64 = 2;
const COMPACT_ID: u64 = 3;

/// The translation table's two codex compaction methods, in order.
fn codex_methods() -> (&'static str, &'static str) {
    let knobs =
        crate::domain::lean::translation("codex", crate::domain::lean::Setting::CompactNow).knobs;
    match (knobs.first().map(|k| k.knob), knobs.get(1).map(|k| k.knob)) {
        (
            Some(crate::domain::lean::Knob::ProtocolMethod(fork)),
            Some(crate::domain::lean::Knob::ProtocolMethod(compact)),
        ) => (fork, compact),
        _ => ("thread/fork", "thread/compact/start"),
    }
}

impl CodexDialogue {
    /// The dialogue for compacting `thread`, its fork running under `model`
    /// when one is named.
    #[must_use]
    pub fn new(thread: &str, model: Option<&str>) -> Self {
        Self {
            thread: thread.to_string(),
            model: model.map(str::to_string),
            state: State::Initializing,
            latest: None,
        }
    }

    /// The frames written before any line is read.
    #[must_use]
    pub fn open(&self) -> Vec<String> {
        vec![frame(json!({
            "jsonrpc": "2.0",
            "id": INITIALIZE_ID,
            "method": "initialize",
            "params": {"clientInfo": {
                "name": "oneharness",
                "title": "oneharness",
                "version": env!("CARGO_PKG_VERSION"),
            }},
        }))]
    }

    fn failed(error: String) -> CodexStep {
        CodexStep::Done(CodexCompaction::Failed { error })
    }

    /// React to one line the server wrote.
    pub fn on_line(&mut self, line: &str) -> CodexStep {
        let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
            return CodexStep::Wait;
        };
        let (fork_method, compact_method) = codex_methods();
        let id = value.get("id").and_then(Value::as_u64);
        if matches!(id, Some(INITIALIZE_ID | FORK_ID | COMPACT_ID)) {
            // A response to one of ours: an `error`, or anything but a
            // `result`, ends the compaction rather than advancing it.
            let Some(result) = value.get("result") else {
                return Self::failed(format!(
                    "codex refused the compaction: {}",
                    value
                        .pointer("/error/message")
                        .and_then(Value::as_str)
                        .unwrap_or("a response without a result")
                ));
            };
            return match (&self.state, id) {
                (State::Initializing, Some(INITIALIZE_ID)) => {
                    self.state = State::Forking;
                    let mut params = json!({"threadId": self.thread});
                    if let Some(model) = &self.model {
                        params["model"] = json!(model);
                    }
                    CodexStep::Send(vec![
                        frame(json!({"jsonrpc": "2.0", "method": "initialized", "params": {}})),
                        frame(json!({
                            "jsonrpc": "2.0",
                            "id": FORK_ID,
                            "method": fork_method,
                            "params": params,
                        })),
                    ])
                }
                (State::Forking, Some(FORK_ID)) => {
                    // The untouched-original guarantee rests on compacting a
                    // DIFFERENT thread, so a fork codex did not name, or named
                    // as the original, is refused before anything is compacted.
                    let fork = result
                        .pointer("/thread/id")
                        .and_then(Value::as_str)
                        .filter(|fork| !fork.is_empty() && *fork != self.thread)
                        .map(str::to_string);
                    let Some(fork) = fork else {
                        return Self::failed(format!(
                            "codex's {fork_method} named no new thread, so nothing was compacted"
                        ));
                    };
                    self.latest = None;
                    self.state = State::Compacting {
                        fork: fork.clone(),
                        turn: None,
                    };
                    CodexStep::Send(vec![frame(json!({
                        "jsonrpc": "2.0",
                        "id": COMPACT_ID,
                        "method": compact_method,
                        "params": {"threadId": fork},
                    }))])
                }
                _ => CodexStep::Wait,
            };
        }
        let State::Compacting { fork, turn } = &mut self.state else {
            return CodexStep::Wait;
        };
        if value.pointer("/params/threadId").and_then(Value::as_str) != Some(fork.as_str()) {
            return CodexStep::Wait;
        }
        let named_turn = value
            .pointer("/params/turnId")
            .or_else(|| value.pointer("/params/turn/id"))
            .and_then(Value::as_str);
        let method = value.get("method").and_then(Value::as_str);
        match (turn.as_ref(), method) {
            (_, Some("thread/tokenUsage/updated")) => {
                // Before the compaction turn any of the fork's totals is its
                // baseline; after it, only the compaction turn's own counts.
                let ours = turn
                    .as_ref()
                    .is_none_or(|(id, _)| named_turn == Some(id.as_str()));
                if let (true, Some(total)) = (ours, value.pointer("/params/tokenUsage/total")) {
                    self.latest = Some(codex_usage(total));
                }
                CodexStep::Wait
            }
            (None, Some("turn/started")) => {
                if let Some(id) = named_turn {
                    *turn = Some((id.to_string(), self.latest.clone()));
                }
                CodexStep::Wait
            }
            (Some((id, before)), Some("turn/completed")) if named_turn == Some(id.as_str()) => {
                let status = value
                    .pointer("/params/turn/status")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if status == "completed" {
                    CodexStep::Done(CodexCompaction::Completed {
                        fork: fork.clone(),
                        usage: usage_delta(before.as_ref(), self.latest.as_ref()),
                    })
                } else {
                    Self::failed(
                        value
                            .pointer("/params/turn/error/message")
                            .and_then(Value::as_str)
                            .map_or_else(
                                || format!("codex ended the compaction turn as `{status}`"),
                                str::to_string,
                            ),
                    )
                }
            }
            _ => CodexStep::Wait,
        }
    }
}

fn frame(value: Value) -> String {
    format!("{value}\n")
}

fn codex_usage(breakdown: &Value) -> Usage {
    let field = |name: &str| breakdown.get(name).and_then(Value::as_u64);
    Usage {
        input_tokens: field("inputTokens"),
        output_tokens: field("outputTokens"),
        cache_read_tokens: field("cachedInputTokens"),
        cache_write_tokens: field("cacheWriteInputTokens"),
        cost_usd: None,
    }
}

/// What the thread's totals grew by over the compaction. A total that did
/// not grow reports nothing (`null`), never a guessed zero: codex 0.159.2's
/// own compaction leaves the thread totals as they were.
fn usage_delta(before: Option<&Usage>, after: Option<&Usage>) -> Usage {
    let (Some(before), Some(after)) = (before, after) else {
        return Usage::default();
    };
    let grew = |b: Option<u64>, a: Option<u64>| match (b, a) {
        (Some(b), Some(a)) if a > b => Some(a - b),
        _ => None,
    };
    Usage {
        input_tokens: grew(before.input_tokens, after.input_tokens),
        output_tokens: grew(before.output_tokens, after.output_tokens),
        cache_read_tokens: grew(before.cache_read_tokens, after.cache_read_tokens),
        cache_write_tokens: grew(before.cache_write_tokens, after.cache_write_tokens),
        cost_usd: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!(
            "{}/../../tests/fixtures/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    #[test]
    fn the_recorded_claude_compaction_reads_as_a_fork_that_succeeded() {
        let read = read_claude(&fixture("claude-compact-fork.jsonl"));
        assert_eq!(read.result, ClaudeResult::Succeeded);
        assert_eq!((read.before, read.after), (Some(6_809), Some(7_405)));
        assert_eq!(
            read.usage,
            Usage {
                input_tokens: Some(58_305),
                output_tokens: Some(1_538),
                cache_read_tokens: Some(0),
                cache_write_tokens: Some(13_711),
                cost_usd: Some(0.008_313_375),
            },
            "the compaction's own figures, not the session's running total"
        );
        assert_eq!(
            read.session_id.as_deref(),
            Some("9e02ec47-3a9e-4737-a89a-ce9ed05abf43")
        );
        let refused = read_claude(
            r#"{"type":"system","subtype":"status","compact_result":"failed","compact_error":"Not enough messages to compact."}"#,
        );
        assert_eq!(
            refused.result,
            ClaudeResult::Failed(Some("Not enough messages to compact.".to_string()))
        );
    }

    #[test]
    fn the_recorded_codex_dialogue_forks_then_compacts_the_fork() {
        let original = "01a19db2-ce98-7aa2-8bc0-f060559d130d";
        let mut d = CodexDialogue::new(original, Some("gpt-test"));
        let open: Value = serde_json::from_str(&d.open()[0]).unwrap();
        assert_eq!(open["method"], "initialize");
        let CodexStep::Send(frames) = d.on_line(r#"{"id":1,"result":{}}"#) else {
            panic!("initialize answered")
        };
        let fork: Value = serde_json::from_str(&frames[1]).unwrap();
        assert_eq!(fork["method"], "thread/fork");
        assert_eq!(fork["params"]["threadId"], original);
        assert_eq!(fork["params"]["model"], "gpt-test");
        let mut done = None;
        for line in fixture("codex-app-server-fork-compact.jsonl").lines() {
            match d.on_line(line) {
                CodexStep::Send(frames) => {
                    let compact: Value = serde_json::from_str(&frames[0]).unwrap();
                    assert_eq!(compact["method"], "thread/compact/start");
                    assert_eq!(
                        compact["params"]["threadId"],
                        "01a19db5-ae1c-77d2-ad64-b3e9aa7b861e"
                    );
                }
                CodexStep::Done(end) => {
                    done = Some(end);
                    break;
                }
                CodexStep::Wait => {}
            }
        }
        assert_eq!(
            done.expect("the recorded turn completed"),
            CodexCompaction::Completed {
                fork: "01a19db5-ae1c-77d2-ad64-b3e9aa7b861e".to_string(),
                usage: Usage::default(),
            },
            "the recorded totals did not grow"
        );
    }

    #[test]
    fn a_codex_refusal_ends_the_dialogue_as_failed() {
        let mut d = CodexDialogue::new("t", None);
        d.on_line(r#"{"id":1,"result":{}}"#);
        let CodexStep::Done(CodexCompaction::Failed { error }) =
            d.on_line(r#"{"id":2,"error":{"code":-32600,"message":"thread not found"}}"#)
        else {
            panic!("refusal ends it")
        };
        assert!(error.contains("thread not found"));

        // A response with no result advances nothing.
        let mut d = CodexDialogue::new("t", None);
        assert!(matches!(
            d.on_line(r#"{"id":1}"#),
            CodexStep::Done(CodexCompaction::Failed { .. })
        ));

        // A fork that is the original thread is no fork.
        let mut d = CodexDialogue::new("t", None);
        d.on_line(r#"{"id":1,"result":{}}"#);
        let CodexStep::Done(CodexCompaction::Failed { .. }) =
            d.on_line(r#"{"id":2,"result":{"thread":{"id":"t"}}}"#)
        else {
            panic!("a fork that is the original is refused")
        };

        // Another thread's turn neither starts nor ends this compaction.
        let mut d = CodexDialogue::new("t", None);
        d.on_line(r#"{"id":1,"result":{}}"#);
        d.on_line(r#"{"id":2,"result":{"thread":{"id":"f"}}}"#);
        assert_eq!(
            d.on_line(
                r#"{"method":"turn/started","params":{"threadId":"other","turn":{"id":"x"}}}"#
            ),
            CodexStep::Wait
        );
        d.on_line(r#"{"method":"turn/started","params":{"threadId":"f","turn":{"id":"c"}}}"#);
        assert_eq!(
            d.on_line(r#"{"method":"turn/completed","params":{"threadId":"f","turn":{"id":"z","status":"completed"}}}"#),
            CodexStep::Wait,
            "a different turn of the fork is not the compaction"
        );
        assert_eq!(
            d.on_line(r#"{"method":"turn/completed","params":{"threadId":"f","turn":{"status":"completed"}}}"#),
            CodexStep::Wait,
            "a completion naming no turn is not the compaction's"
        );
        let CodexStep::Done(CodexCompaction::Failed { error }) = d.on_line(
            r#"{"method":"turn/completed","params":{"threadId":"f","turn":{"id":"c","status":"failed","error":{"message":"context too small"}}}}"#,
        ) else {
            panic!("a failed compaction turn ends it")
        };
        assert_eq!(error, "context too small");
    }
}
