//! Best-effort extraction of a harness's **normalized tool-call / action events**
//! from its raw stdout. Pure: no I/O.
//!
//! Like `text` and `usage`, this is a convenience layered over the guaranteed
//! execution envelope: the array is `None` when a harness's output carries no
//! recognizable tool trace, is **never fabricated**, and records how it was
//! recovered (`events_source`, parallel to `text_source`) so a consumer can tell
//! "this harness doesn't expose tool events" from "no tools were used." An absent
//! trace is the honest answer, not an error.
//!
//! The motivation is behavioral skill-testing (the `skilltest` consumer, issue
//! #1096): asserting what a harness actually *did* ("ran `git commit`", "edited
//! exactly one file", "used ≤ 3 tool calls") — none of which the text-only
//! envelope could express. Normalizing here, where the per-harness output shapes
//! are already known, spares every consumer from per-harness stdout parsing.
//!
//! Two output shapes are recognized, both sourced from real transcripts, not
//! guessed:
//! - **OpenCode** (`run --format json`): JSONL events whose `part.type == "tool"`
//!   carry the tool `name`, and a `state` object holding the call `input` and the
//!   observed `output`. One part is one completed call, so it maps to a single
//!   `tool_call` event that also carries its `output`.
//! - **Anthropic content blocks** (Claude Code / Cursor under `stream-json`):
//!   assistant messages whose `message.content[]` holds `tool_use` blocks
//!   (`name` + structured `input`), and `user` messages whose `content[]` holds
//!   `tool_result` blocks (the observation). These become `tool_call` and
//!   `tool_result` events respectively.
//!
//! A harness whose oneharness output format is plain `text` (Codex, Goose, Qwen,
//! Crush, Copilot), or Claude Code under its default single-document `json`
//! result (which omits the intermediate transcript), exposes no machine-readable
//! trace, so `events` stays `None` for it rather than being invented.

use serde::Serialize;
use serde_json::Value;

use crate::domain::report::OutputFormat;

/// One normalized action a harness took, harness-agnostic so a single consumer
/// assertion works across harnesses. Every field is always serialized (null when
/// absent) so the shape is stable, mirroring the `usage` contract.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ActionEvent {
    /// The kind of event: `tool_call` (the model invoked a tool) or
    /// `tool_result` (the observation returned to the model). Left open for
    /// future kinds rather than an enum, so a new shape never breaks the field.
    pub kind: String,
    /// Normalized tool name where knowable (e.g. `bash`, `Edit`); `null` for a
    /// `tool_result`, or when the harness did not name the tool.
    pub name: Option<String>,
    /// Structured, tool-shaped arguments (the command string, the file path),
    /// so a consumer asserts on specific args without re-parsing; `null` when the
    /// event carries none (e.g. a `tool_result`).
    pub input: Option<Value>,
    /// The result/observation text, when the trace exposes it; `null` otherwise.
    pub output: Option<String>,
    /// Position of this event within the run, so "≤ N tool calls" and "did X
    /// before Y" are expressible from a stable ordering (also array order).
    pub index: usize,
}

/// A recovered event list plus the method that produced it (e.g.
/// `json:opencode-parts`, `stream-json:content-blocks`), parallel to
/// [`crate::domain::normalize::Extracted::source`].
#[derive(Debug, Clone, PartialEq)]
pub struct EventsReading {
    pub events: Vec<ActionEvent>,
    pub source: String,
}

/// One event before it is assigned its ordering index; the recognizers produce
/// these and [`extract_events`] numbers them in document order.
struct PartialEvent {
    kind: &'static str,
    name: Option<String>,
    input: Option<Value>,
    output: Option<String>,
}

impl PartialEvent {
    fn into_event(self, index: usize) -> ActionEvent {
        ActionEvent {
            kind: self.kind.to_string(),
            name: self.name,
            input: self.input,
            output: self.output,
            index,
        }
    }
}

/// Best-effort normalized tool events from a harness's stdout. Scans every JSON
/// candidate (the whole document, else each parseable line) in order, collecting
/// events from the first shape each candidate matches — OpenCode tool parts or
/// Anthropic content blocks. `None` when no candidate yields an event, so the
/// consumer can distinguish "unsupported" from "used no tools" via the absent
/// `events_source`. `fmt` only labels the source's provenance prefix.
pub fn extract_events(stdout: &str, fmt: OutputFormat) -> Option<EventsReading> {
    let mut events: Vec<PartialEvent> = Vec::new();
    let mut recognizer: Option<&'static str> = None;
    for value in json_candidates(stdout) {
        // An OpenCode `tool` part is self-contained (name + input + output); its
        // shape never overlaps a content-block message, so try it first.
        if let Some(pe) = opencode_tool_event(&value) {
            recognizer.get_or_insert("opencode-parts");
            events.push(pe);
            continue;
        }
        let blocks = content_block_events(&value);
        if !blocks.is_empty() {
            recognizer.get_or_insert("content-blocks");
            events.extend(blocks);
        }
    }
    let recognizer = recognizer?;
    Some(EventsReading {
        source: format!("{}:{recognizer}", format_prefix(fmt)),
        events: events
            .into_iter()
            .enumerate()
            .map(|(i, pe)| pe.into_event(i))
            .collect(),
    })
}

/// One OpenCode `tool` part → a single `tool_call` event carrying its input and
/// output. `None` for any other part (`text`, `step-start`, `reasoning`, …).
fn opencode_tool_event(value: &Value) -> Option<PartialEvent> {
    let part = value.get("part").and_then(Value::as_object)?;
    if part.get("type").and_then(Value::as_str) != Some("tool") {
        return None;
    }
    let state = part.get("state").and_then(Value::as_object);
    Some(PartialEvent {
        kind: "tool_call",
        name: part.get("tool").and_then(Value::as_str).map(str::to_string),
        input: state.and_then(|s| s.get("input").cloned()),
        output: state
            .and_then(|s| s.get("output"))
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

/// Anthropic-style content blocks (Claude Code / Cursor `stream-json`): a message
/// whose `content` array (under `message.content`, or top-level `content`) holds
/// `tool_use` blocks (→ `tool_call`) and `tool_result` blocks (→ `tool_result`).
/// Empty when the value is not such a message or carries no tool block.
fn content_block_events(value: &Value) -> Vec<PartialEvent> {
    let blocks = value
        .get("message")
        .and_then(|m| m.get("content"))
        .or_else(|| value.get("content"))
        .and_then(Value::as_array);
    let Some(blocks) = blocks else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for block in blocks {
        let Some(obj) = block.as_object() else {
            continue;
        };
        match obj.get("type").and_then(Value::as_str) {
            Some("tool_use") => out.push(PartialEvent {
                kind: "tool_call",
                name: obj.get("name").and_then(Value::as_str).map(str::to_string),
                input: obj.get("input").cloned(),
                output: None,
            }),
            Some("tool_result") => out.push(PartialEvent {
                kind: "tool_result",
                name: None,
                input: None,
                output: tool_result_text(obj.get("content")),
            }),
            _ => {}
        }
    }
    out
}

/// Normalize a `tool_result` block's `content` (a bare string, or an array of
/// `{type:"text","text":…}` blocks — the two Anthropic shapes) to a single
/// string; `None` when neither yields text.
fn tool_result_text(content: Option<&Value>) -> Option<String> {
    match content? {
        Value::String(s) => Some(s.clone()),
        Value::Array(items) => {
            let joined = items
                .iter()
                .filter_map(|it| it.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n");
            (!joined.is_empty()).then_some(joined)
        }
        _ => None,
    }
}

/// The provenance prefix for the source string, matching `text_source`'s scheme.
fn format_prefix(fmt: OutputFormat) -> &'static str {
    match fmt {
        OutputFormat::Text => "text",
        OutputFormat::Json => "json",
        OutputFormat::StreamJson => "stream-json",
    }
}

/// Candidate JSON objects in `stdout`: the whole document when it parses, else
/// each parseable line (stream-json / JSONL). Document order preserved so event
/// ordering reflects the transcript.
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
    use serde_json::json;

    #[test]
    fn opencode_tool_parts_become_ordered_tool_calls() {
        // A real `opencode run --format json` tool-using turn: a text part, a
        // `tool` part carrying input+output, then a step-finish. Only the tool
        // part is an event, and it carries the normalized name/input/output.
        let raw = concat!(
            r#"{"type":"text","part":{"type":"text","text":"I'll run that."}}"#,
            "\n",
            r#"{"type":"tool_use","part":{"id":"p2","type":"tool","tool":"bash","state":{"status":"completed","input":{"command":"git commit -m x"},"output":"HELLO-FROM-TOOL"}}}"#,
            "\n",
            r#"{"type":"step_finish","part":{"type":"step-finish","cost":0.01}}"#,
            "\n",
        );
        let got = extract_events(raw, OutputFormat::Json).unwrap();
        assert_eq!(got.source, "json:opencode-parts");
        assert_eq!(got.events.len(), 1);
        let ev = &got.events[0];
        assert_eq!(ev.kind, "tool_call");
        assert_eq!(ev.name.as_deref(), Some("bash"));
        assert_eq!(ev.input, Some(json!({"command": "git commit -m x"})));
        assert_eq!(ev.output.as_deref(), Some("HELLO-FROM-TOOL"));
        assert_eq!(ev.index, 0);
    }

    #[test]
    fn opencode_multiple_tool_parts_are_indexed_in_order() {
        let raw = concat!(
            r#"{"part":{"type":"tool","tool":"read","state":{"input":{"path":"a.rs"}}}}"#,
            "\n",
            r#"{"part":{"type":"tool","tool":"bash","state":{"input":{"command":"ls"},"output":"a.rs"}}}"#,
            "\n",
        );
        let got = extract_events(raw, OutputFormat::Json).unwrap();
        assert_eq!(got.events.len(), 2);
        assert_eq!(got.events[0].name.as_deref(), Some("read"));
        assert_eq!(got.events[0].index, 0);
        assert_eq!(got.events[0].output, None); // no output field → null, not ""
        assert_eq!(got.events[1].name.as_deref(), Some("bash"));
        assert_eq!(got.events[1].index, 1);
        assert_eq!(got.events[1].output.as_deref(), Some("a.rs"));
    }

    #[test]
    fn anthropic_content_blocks_yield_call_and_result_events() {
        // Claude Code / Cursor stream-json: an assistant message with a `tool_use`
        // block, then a user message with the `tool_result` observation.
        let raw = concat!(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"ok"},{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"echo hi"}}]}}"#,
            "\n",
            r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"hi\n"}]}}"#,
            "\n",
            r#"{"type":"result","result":"done"}"#,
            "\n",
        );
        let got = extract_events(raw, OutputFormat::StreamJson).unwrap();
        assert_eq!(got.source, "stream-json:content-blocks");
        assert_eq!(got.events.len(), 2);
        assert_eq!(got.events[0].kind, "tool_call");
        assert_eq!(got.events[0].name.as_deref(), Some("Bash"));
        assert_eq!(got.events[0].input, Some(json!({"command": "echo hi"})));
        assert_eq!(got.events[0].output, None);
        assert_eq!(got.events[1].kind, "tool_result");
        assert_eq!(got.events[1].name, None);
        assert_eq!(got.events[1].input, None);
        assert_eq!(got.events[1].output.as_deref(), Some("hi\n"));
        assert_eq!(got.events[1].index, 1);
    }

    #[test]
    fn tool_result_content_array_is_joined() {
        // The other Anthropic `tool_result` shape: `content` is an array of text
        // blocks rather than a bare string; they are joined into one observation.
        let raw = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t","content":[{"type":"text","text":"line one"},{"type":"text","text":"line two"}]}]}}"#;
        let got = extract_events(raw, OutputFormat::StreamJson).unwrap();
        assert_eq!(got.events.len(), 1);
        assert_eq!(got.events[0].output.as_deref(), Some("line one\nline two"));
    }

    #[test]
    fn top_level_content_array_without_message_wrapper() {
        // Some emitters put the content array at the top level rather than under
        // `message`; both are accepted.
        let raw = r#"{"content":[{"type":"tool_use","name":"read","input":{"path":"x"}}]}"#;
        let got = extract_events(raw, OutputFormat::StreamJson).unwrap();
        assert_eq!(got.events.len(), 1);
        assert_eq!(got.events[0].name.as_deref(), Some("read"));
    }

    #[test]
    fn no_tool_events_yields_none() {
        // Claude Code's single-document `json` result carries no transcript, so
        // there is nothing to extract — events stays absent, never fabricated.
        assert!(extract_events(r#"{"type":"result","result":"hi"}"#, OutputFormat::Json).is_none());
        // A text-only turn (no tool parts/blocks) is likewise empty.
        let text_only = concat!(
            r#"{"type":"text","part":{"type":"text","text":"just prose"}}"#,
            "\n",
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hi"}]}}"#,
            "\n",
        );
        assert!(extract_events(text_only, OutputFormat::Json).is_none());
        assert!(extract_events("not json", OutputFormat::Text).is_none());
        assert!(extract_events("", OutputFormat::Json).is_none());
    }

    #[test]
    fn skips_noise_lines_and_still_recovers_events() {
        // Blank, non-JSON, and irrelevant lines are ignored without crashing or
        // mis-numbering the real events that follow.
        let raw = concat!(
            "\n",
            "   \n",
            "not json at all\n",
            r#"{"type":"step_start","part":{"type":"step-start"}}"#,
            "\n",
            r#"{"part":{"type":"tool","tool":"bash","state":{"input":{"command":"ls"}}}}"#,
            "\n",
        );
        let got = extract_events(raw, OutputFormat::Json).unwrap();
        assert_eq!(got.events.len(), 1);
        assert_eq!(got.events[0].index, 0);
        assert_eq!(got.events[0].name.as_deref(), Some("bash"));
    }

    #[test]
    fn tool_part_without_state_still_emits_call_with_null_fields() {
        // A tool part missing its `state` (e.g. captured mid-call) is still a
        // real call: the name surfaces; input/output stay null rather than faked.
        let raw = r#"{"part":{"type":"tool","tool":"webfetch"}}"#;
        let got = extract_events(raw, OutputFormat::Json).unwrap();
        assert_eq!(got.events[0].name.as_deref(), Some("webfetch"));
        assert_eq!(got.events[0].input, None);
        assert_eq!(got.events[0].output, None);
    }

    #[test]
    fn tool_result_with_non_text_content_yields_null_output() {
        // A `tool_result` whose content is neither a string nor text blocks (an
        // object, or an array of non-text items) has no recoverable observation.
        let raw = r#"{"message":{"content":[{"type":"tool_result","content":{"weird":true}}]}}"#;
        let got = extract_events(raw, OutputFormat::StreamJson).unwrap();
        assert_eq!(got.events[0].kind, "tool_result");
        assert_eq!(got.events[0].output, None);
    }

    #[test]
    fn source_prefix_tracks_output_format() {
        // The provenance prefix mirrors `text_source`: same shape, different
        // format label. OpenCode parts arrive under Json; content blocks under
        // stream-json — the prefix reflects whichever format the run used.
        let oc = r#"{"part":{"type":"tool","tool":"bash","state":{"input":{}}}}"#;
        assert_eq!(
            extract_events(oc, OutputFormat::Json).unwrap().source,
            "json:opencode-parts"
        );
        let cb = r#"{"message":{"content":[{"type":"tool_use","name":"x","input":{}}]}}"#;
        assert_eq!(
            extract_events(cb, OutputFormat::Text).unwrap().source,
            "text:content-blocks"
        );
    }
}
