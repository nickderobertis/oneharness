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

/// What claude-code's stream-json `/compact` output says.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClaudeCompaction {
    /// `Some(true)` on `compact_result: success`, `Some(false)` on any other
    /// result, `None` when the stream never reported one.
    pub succeeded: Option<bool>,
    pub error: Option<String>,
    pub before: Option<u64>,
    pub after: Option<u64>,
    /// The session id the compacted conversation lives under (the fork's).
    pub session_id: Option<String>,
}

/// Read claude-code's `/compact` stream (see
/// `tests/fixtures/claude-compact-fork.jsonl`).
#[must_use]
pub fn read_claude(stdout: &str) -> ClaudeCompaction {
    let mut out = ClaudeCompaction::default();
    for value in stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
    {
        if out.session_id.is_none() {
            out.session_id = value
                .get("session_id")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        if let Some(result) = value.get("compact_result").and_then(Value::as_str) {
            out.succeeded = Some(result == "success");
            out.error = value
                .get("compact_error")
                .and_then(Value::as_str)
                .map(str::to_string);
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
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CodexCompaction {
    pub completed: bool,
    pub error: Option<String>,
    /// The fork's thread id, once `thread/fork` answered.
    pub fork: Option<String>,
    pub usage: Usage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Initialize,
    Fork,
    Compact,
}

/// The codex app-server conversation: `initialize`, `thread/fork` of the
/// stored thread, `thread/compact/start` on the fork, until `turn/completed`
/// (see `tests/fixtures/codex-app-server-fork-compact.jsonl`).
#[derive(Debug, Clone)]
pub struct CodexDialogue {
    thread: String,
    phase: Phase,
    fork: Option<String>,
    /// The last thread token total before the compaction turn, and the last
    /// within it: their difference is the compaction's own usage.
    before: Option<Usage>,
    latest: Option<Usage>,
    compact_turn_started: bool,
}

const INITIALIZE_ID: u64 = 1;
const FORK_ID: u64 = 2;
const COMPACT_ID: u64 = 3;

impl CodexDialogue {
    #[must_use]
    pub fn new(thread: &str) -> Self {
        Self {
            thread: thread.to_string(),
            phase: Phase::Initialize,
            fork: None,
            before: None,
            latest: None,
            compact_turn_started: false,
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

    /// React to one line the server wrote.
    pub fn on_line(&mut self, line: &str) -> CodexStep {
        let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
            return CodexStep::Wait;
        };
        if let Some(error) = value.get("error") {
            let id = value.get("id").and_then(Value::as_u64);
            if matches!(id, Some(FORK_ID | COMPACT_ID | INITIALIZE_ID)) {
                return CodexStep::Done(self.ended(
                    false,
                    Some(format!(
                        "codex refused the compaction: {}",
                        error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("no message")
                    )),
                ));
            }
        }
        match (self.phase, value.get("id").and_then(Value::as_u64)) {
            (Phase::Initialize, Some(INITIALIZE_ID)) => {
                self.phase = Phase::Fork;
                return CodexStep::Send(vec![
                    frame(json!({"jsonrpc": "2.0", "method": "initialized", "params": {}})),
                    frame(json!({
                        "jsonrpc": "2.0",
                        "id": FORK_ID,
                        "method": "thread/fork",
                        "params": {"threadId": self.thread},
                    })),
                ]);
            }
            (Phase::Fork, Some(FORK_ID)) => {
                let Some(fork) = value
                    .pointer("/result/thread/id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                else {
                    return CodexStep::Done(
                        self.ended(false, Some("codex's thread/fork named no thread".into())),
                    );
                };
                self.phase = Phase::Compact;
                self.fork = Some(fork.clone());
                self.before = self.latest.clone();
                return CodexStep::Send(vec![frame(json!({
                    "jsonrpc": "2.0",
                    "id": COMPACT_ID,
                    "method": "thread/compact/start",
                    "params": {"threadId": fork},
                }))]);
            }
            _ => {}
        }
        match value.get("method").and_then(Value::as_str) {
            Some("thread/tokenUsage/updated") => {
                if let Some(total) = value.pointer("/params/tokenUsage/total") {
                    self.latest = Some(codex_usage(total));
                }
                CodexStep::Wait
            }
            Some("turn/started") if self.phase == Phase::Compact => {
                self.compact_turn_started = true;
                if self.before.is_none() {
                    self.before = self.latest.clone();
                }
                CodexStep::Wait
            }
            Some("turn/completed") if self.compact_turn_started => {
                let status = value
                    .pointer("/params/turn/status")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let error = value
                    .pointer("/params/turn/error/message")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                if status == "completed" {
                    CodexStep::Done(self.ended(true, None))
                } else {
                    CodexStep::Done(self.ended(
                        false,
                        Some(error.unwrap_or_else(|| {
                            format!("codex ended the compaction turn as `{status}`")
                        })),
                    ))
                }
            }
            _ => CodexStep::Wait,
        }
    }

    fn ended(&self, completed: bool, error: Option<String>) -> CodexCompaction {
        CodexCompaction {
            completed,
            error,
            fork: self.fork.clone(),
            usage: usage_delta(self.before.as_ref(), self.latest.as_ref()),
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
        assert_eq!(read.succeeded, Some(true));
        assert_eq!((read.before, read.after), (Some(6_809), Some(7_405)));
        assert_eq!(
            read.session_id.as_deref(),
            Some("9e02ec47-3a9e-4737-a89a-ce9ed05abf43")
        );
        let refused = read_claude(
            r#"{"type":"system","subtype":"status","compact_result":"failed","compact_error":"Not enough messages to compact."}"#,
        );
        assert_eq!(refused.succeeded, Some(false));
        assert_eq!(
            refused.error.as_deref(),
            Some("Not enough messages to compact.")
        );
    }

    #[test]
    fn the_recorded_codex_dialogue_forks_then_compacts_the_fork() {
        let original = "01a19db2-ce98-7aa2-8bc0-f060559d130d";
        let mut d = CodexDialogue::new(original);
        let open: Value = serde_json::from_str(&d.open()[0]).unwrap();
        assert_eq!(open["method"], "initialize");
        let CodexStep::Send(frames) = d.on_line(r#"{"id":1,"result":{}}"#) else {
            panic!("initialize answered")
        };
        let fork: Value = serde_json::from_str(&frames[1]).unwrap();
        assert_eq!(fork["method"], "thread/fork");
        assert_eq!(fork["params"]["threadId"], original);
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
        let done = done.expect("the recorded turn completed");
        assert!(done.completed, "{done:?}");
        assert_eq!(
            done.fork.as_deref(),
            Some("01a19db5-ae1c-77d2-ad64-b3e9aa7b861e")
        );
        assert_eq!(
            done.usage,
            Usage::default(),
            "the recorded totals did not grow"
        );
    }

    #[test]
    fn a_codex_refusal_ends_the_dialogue_as_failed() {
        let mut d = CodexDialogue::new("t");
        d.on_line(r#"{"id":1,"result":{}}"#);
        let CodexStep::Done(end) =
            d.on_line(r#"{"id":2,"error":{"code":-32600,"message":"thread not found"}}"#)
        else {
            panic!("refusal ends it")
        };
        assert!(!end.completed);
        assert!(end.error.unwrap().contains("thread not found"));
    }
}
