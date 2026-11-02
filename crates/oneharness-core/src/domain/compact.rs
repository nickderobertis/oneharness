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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Initialize,
    Fork,
    Compact,
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
    phase: Phase,
    fork: Option<String>,
    /// The compaction turn's id, once the fork's `turn/started` names it.
    turn: Option<String>,
    /// The fork's last token total before the compaction turn, and the last
    /// within it: their difference is the compaction's own usage.
    before: Option<Usage>,
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
            phase: Phase::Initialize,
            fork: None,
            turn: None,
            before: None,
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

    /// Whether a notification is about the fork (and, once known, its
    /// compaction turn) rather than any other thread the server reports on.
    fn about_fork(&self, value: &Value) -> bool {
        let thread = value.pointer("/params/threadId").and_then(Value::as_str);
        let turn = value
            .pointer("/params/turnId")
            .or_else(|| value.pointer("/params/turn/id"))
            .and_then(Value::as_str);
        self.fork.is_some()
            && thread == self.fork.as_deref()
            && (self.turn.is_none() || turn.is_none() || turn == self.turn.as_deref())
    }

    /// React to one line the server wrote.
    pub fn on_line(&mut self, line: &str) -> CodexStep {
        let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
            return CodexStep::Wait;
        };
        let (fork_method, compact_method) = codex_methods();
        let id = value.get("id").and_then(Value::as_u64);
        if let Some(error) = value.get("error") {
            if matches!(id, Some(FORK_ID | COMPACT_ID | INITIALIZE_ID)) {
                return Self::failed(format!(
                    "codex refused the compaction: {}",
                    error
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("no message")
                ));
            }
        }
        match (self.phase, id) {
            (Phase::Initialize, Some(INITIALIZE_ID)) => {
                self.phase = Phase::Fork;
                let mut params = json!({"threadId": self.thread});
                if let Some(model) = &self.model {
                    params["model"] = json!(model);
                }
                return CodexStep::Send(vec![
                    frame(json!({"jsonrpc": "2.0", "method": "initialized", "params": {}})),
                    frame(json!({
                        "jsonrpc": "2.0",
                        "id": FORK_ID,
                        "method": fork_method,
                        "params": params,
                    })),
                ]);
            }
            (Phase::Fork, Some(FORK_ID)) => {
                // The untouched-original guarantee rests on compacting a
                // DIFFERENT thread, so a fork codex did not name, or named as
                // the original, is refused before anything is compacted.
                let fork = value
                    .pointer("/result/thread/id")
                    .and_then(Value::as_str)
                    .filter(|fork| !fork.is_empty() && *fork != self.thread)
                    .map(str::to_string);
                let Some(fork) = fork else {
                    return Self::failed(format!(
                        "codex's {fork_method} named no new thread, so nothing was compacted"
                    ));
                };
                self.phase = Phase::Compact;
                self.fork = Some(fork.clone());
                return CodexStep::Send(vec![frame(json!({
                    "jsonrpc": "2.0",
                    "id": COMPACT_ID,
                    "method": compact_method,
                    "params": {"threadId": fork},
                }))]);
            }
            _ => {}
        }
        if self.phase != Phase::Compact || !self.about_fork(&value) {
            return CodexStep::Wait;
        }
        match value.get("method").and_then(Value::as_str) {
            Some("thread/tokenUsage/updated") => {
                if let Some(total) = value.pointer("/params/tokenUsage/total") {
                    self.latest = Some(codex_usage(total));
                }
                CodexStep::Wait
            }
            Some("turn/started") if self.turn.is_none() => {
                self.turn = value
                    .pointer("/params/turn/id")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                self.before = self.latest.clone();
                CodexStep::Wait
            }
            Some("turn/completed") if self.turn.is_some() => {
                let status = value
                    .pointer("/params/turn/status")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if status == "completed" {
                    CodexStep::Done(CodexCompaction::Completed {
                        fork: self.fork.clone().unwrap_or_default(),
                        usage: usage_delta(self.before.as_ref(), self.latest.as_ref()),
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
        let CodexStep::Done(CodexCompaction::Failed { error }) = d.on_line(
            r#"{"method":"turn/completed","params":{"threadId":"f","turn":{"id":"c","status":"failed","error":{"message":"context too small"}}}}"#,
        ) else {
            panic!("a failed compaction turn ends it")
        };
        assert_eq!(error, "context too small");
    }
}
