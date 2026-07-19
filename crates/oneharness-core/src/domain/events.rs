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
//! Four output shapes are recognized, all sourced from real transcripts captured
//! from the live CLIs (the `explore-events` CI probe), not guessed:
//! - **OpenCode** (`run --format json`): JSONL events whose `part.type == "tool"`
//!   carry the tool `name` (`part.tool`) and a `state` object holding the call
//!   `input` and observed `output`. One part is one completed call → one
//!   `tool_call` event carrying its `output`.
//! - **Anthropic content blocks** (Claude Code `stream-json`, Qwen `stream-json`
//!   / `json`): messages whose `message.content[]` holds `tool_use` blocks
//!   (`name` + structured `input`) and `tool_result` blocks (the observation) —
//!   `tool_call` and `tool_result` events respectively.
//! - **Cursor** (`--output-format stream-json`): top-level `type:"tool_call"`
//!   events whose `tool_call` object nests a `<name>ToolCall` payload (e.g.
//!   `shellToolCall`) with `args` and, once complete, `result.success`. The tool
//!   name is the payload key minus its `ToolCall` suffix.
//! - **Codex** (`exec --json`): flat `item.started` / `item.completed` lifecycle
//!   events for command, MCP, collaboration, and web-search tool items.
//!
//! Goose, Crush, and Copilot expose no machine-readable transcript headlessly
//! (decorative TUI text, or no JSON output mode at all — confirmed by the probe),
//! and Claude Code / Cursor under their non-stream `json` mode collapse to only a
//! final result object. In those cases `events` stays `None` rather than being
//! invented. Which format yields a transcript per harness is declared by
//! `HarnessSpec.events_format` and selected by `run --events` / `--stream`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::report::{OutputFormat, OutputObservation, Status};

/// One normalized action a harness took, harness-agnostic so a single consumer
/// assertion works across harnesses. Every field is always serialized (null when
/// absent) so the shape is stable, mirroring the `usage` contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
    /// Stable call identity within the session. Present on tool calls and their
    /// matching results when the provider exposes an identity; history fills a
    /// deterministic run-local identity for providers that do not.
    pub tool_call_id: Option<String>,
    /// UTC interval bounds for tool execution, populated on history records.
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    /// Monotonic elapsed tool time. `None` means no terminal boundary was seen.
    pub duration_ms: Option<u128>,
    /// Terminal tool state, populated on history tool-call events.
    pub status: Option<ToolCallStatus>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallStatus {
    Completed,
    Failed,
    Timeout,
    Interrupted,
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
    tool_call_id: Option<String>,
}

impl PartialEvent {
    fn into_event(self, index: usize) -> ActionEvent {
        ActionEvent {
            kind: self.kind.to_string(),
            name: self.name,
            input: self.input,
            output: self.output,
            index,
            tool_call_id: self.tool_call_id,
            started_at: None,
            finished_at: None,
            duration_ms: None,
            status: None,
        }
    }
}

/// Best-effort normalized tool events from a harness's stdout. Scans every JSON
/// candidate (the whole document, each array element, or each parseable line) in
/// order, collecting events from whichever known shape each candidate matches.
/// `None` when no candidate yields an event, so the consumer can distinguish
/// "unsupported" from "used no tools" via the absent `events_source`. `fmt` only
/// labels the source's provenance prefix.
pub fn extract_events(stdout: &str, fmt: OutputFormat) -> Option<EventsReading> {
    let mut events: Vec<PartialEvent> = Vec::new();
    let mut recognizer: Option<&'static str> = None;
    for value in json_candidates(stdout) {
        if let Some((label, mut partials)) = recognize(&value) {
            recognizer.get_or_insert(label);
            if label == "opencode-parts" || label == "codex-items" {
                for partial in partials {
                    merge_tool_update(&mut events, partial);
                }
            } else {
                events.append(&mut partials);
            }
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

/// Extract normalized events from a single already-parsed JSON value (one stream
/// line or document), numbering them from `start_index`. The streaming path calls
/// this per line as output arrives; [`extract_events`] is the batch counterpart.
/// Empty when the value carries no recognizable tool event.
pub fn events_from_value(value: &Value, start_index: usize) -> Vec<ActionEvent> {
    match recognize(value) {
        Some((_, partials)) => partials
            .into_iter()
            .enumerate()
            .map(|(i, pe)| pe.into_event(start_index + i))
            .collect(),
        None => Vec::new(),
    }
}

/// Try each known harness transcript shape against one JSON value, returning the
/// recognizer label (for `events_source`) and the events it yielded, or `None`.
/// The shapes are mutually exclusive in practice (each keys off a distinct field
/// layout), so the first match wins.
fn recognize(value: &Value) -> Option<(&'static str, Vec<PartialEvent>)> {
    // OpenCode `tool` part: self-contained (name + input + output).
    if let Some(pe) = opencode_tool_event(value) {
        return Some(("opencode-parts", vec![pe]));
    }
    // Cursor `type:"tool_call"` with a nested `<name>ToolCall` payload.
    if let Some(pe) = cursor_tool_call(value) {
        return Some(("cursor-tool-calls", vec![pe]));
    }
    // Codex executable item lifecycle; repeated updates collapse by item id.
    if let Some(pe) = codex_tool_item(value) {
        return Some(("codex-items", vec![pe]));
    }
    // Anthropic content blocks (Claude Code / Qwen): tool_use + tool_result.
    let blocks = content_block_events(value);
    if !blocks.is_empty() {
        return Some(("content-blocks", blocks));
    }
    None
}

fn merge_tool_update(events: &mut Vec<PartialEvent>, update: PartialEvent) {
    let existing = update.tool_call_id.as_deref().and_then(|id| {
        events
            .iter_mut()
            .find(|event| event.kind == "tool_call" && event.tool_call_id.as_deref() == Some(id))
    });
    if let Some(existing) = existing {
        existing.name = update.name.or_else(|| existing.name.take());
        existing.input = update.input.or_else(|| existing.input.take());
        existing.output = update.output.or_else(|| existing.output.take());
    } else {
        events.push(update);
    }
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
        tool_call_id: part
            .get("callID")
            .or_else(|| part.get("id"))
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
                tool_call_id: obj.get("id").and_then(Value::as_str).map(str::to_string),
            }),
            Some("tool_result") => out.push(PartialEvent {
                kind: "tool_result",
                name: None,
                input: None,
                output: tool_result_text(obj.get("content")),
                tool_call_id: obj
                    .get("tool_use_id")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            }),
            _ => {}
        }
    }
    out
}

/// Cursor's `stream-json` tool event: a top-level `{"type":"tool_call", ...}`
/// whose `tool_call` object holds a nested `<name>ToolCall` payload (e.g.
/// `shellToolCall`) with `args` (the input) and, once complete, a
/// `result.success` (the observation). The tool identity is the payload *key*,
/// not a string field — so the name is that key with its `ToolCall` suffix
/// stripped (`shellToolCall` → `shell`). Emitted only on the `completed` subtype
/// (which carries the result); the paired `started` event is skipped so a call
/// is counted once. `None` for any other line. Sourced from a real cursor-agent
/// transcript, not guessed.
fn cursor_tool_call(value: &Value) -> Option<PartialEvent> {
    let obj = value.as_object()?;
    if obj.get("type").and_then(Value::as_str) != Some("tool_call") {
        return None;
    }
    // Only the terminal event carries the result; skip `started` to avoid dupes.
    if obj.get("subtype").and_then(Value::as_str) != Some("completed") {
        return None;
    }
    let tool_call = obj.get("tool_call").and_then(Value::as_object)?;
    // The payload key ends in `ToolCall` (e.g. `shellToolCall`), distinct from the
    // sibling `toolCallId` metadata; its value is the tool object.
    let (key, payload) = tool_call
        .iter()
        .find(|(k, v)| k.ends_with("ToolCall") && v.is_object())?;
    let name = key.strip_suffix("ToolCall").unwrap_or(key).to_string();
    let payload = payload.as_object()?;
    Some(PartialEvent {
        kind: "tool_call",
        name: Some(name),
        input: payload.get("args").cloned(),
        // The observation lives under result.success (shape varies per tool); pull
        // a stdout string when present, else leave null rather than fabricating.
        output: payload
            .get("result")
            .and_then(|r| r.get("success"))
            .and_then(|s| s.get("stdout"))
            .and_then(Value::as_str)
            .map(str::to_string),
        tool_call_id: obj
            .get("call_id")
            .or_else(|| tool_call.get("toolCallId"))
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

/// Codex `exec --json` executable items. Started, updated, and completed records
/// share `item.id`; [`extract_events`] folds them into one normalized call. The
/// allow-list follows Codex's typed exec interface and deliberately excludes
/// completion-only `file_change` items because they expose no execution start.
fn codex_tool_item(value: &Value) -> Option<PartialEvent> {
    let obj = value.as_object()?;
    if !matches!(
        obj.get("type").and_then(Value::as_str),
        Some("item.started" | "item.updated" | "item.completed")
    ) {
        return None;
    }
    let item = obj.get("item").and_then(Value::as_object)?;
    let item_type = item.get("type").and_then(Value::as_str)?;
    if !is_codex_tool_type(item_type) {
        return None;
    }
    let (name, input, output) = match item_type {
        "command_execution" => (
            "command_execution".to_string(),
            item.get("command")
                .map(|command| serde_json::json!({ "command": command.clone() })),
            item.get("aggregated_output")
                .and_then(Value::as_str)
                .map(str::to_string),
        ),
        "mcp_tool_call" => (
            item.get("tool")
                .and_then(Value::as_str)
                .unwrap_or("mcp_tool_call")
                .to_string(),
            item.get("arguments").cloned(),
            item.get("error")
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| item.get("result").map(Value::to_string)),
        ),
        "collab_tool_call" => (
            item.get("tool")
                .and_then(Value::as_str)
                .unwrap_or("collab_tool_call")
                .to_string(),
            Some(Value::Object(
                item.iter()
                    .filter(|(key, _)| {
                        matches!(
                            key.as_str(),
                            "prompt" | "receiver_thread_ids" | "sender_thread_id"
                        )
                    })
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect(),
            )),
            None,
        ),
        "web_search" => (
            "web_search".to_string(),
            item.get("query")
                .map(|query| serde_json::json!({ "query": query.clone() })),
            None,
        ),
        _ => return None,
    };
    Some(PartialEvent {
        kind: "tool_call",
        name: Some(name),
        input,
        output,
        tool_call_id: item.get("id").and_then(Value::as_str).map(str::to_string),
    })
}

fn is_codex_tool_type(item_type: &str) -> bool {
    matches!(
        item_type,
        "command_execution" | "mcp_tool_call" | "collab_tool_call" | "web_search"
    )
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

/// Candidate JSON objects in `stdout`: the whole document when it parses (a
/// top-level array is flattened to its elements — Qwen's `json` mode emits one
/// JSON array of message objects), else each parseable line (stream-json /
/// JSONL). Document order preserved so event ordering reflects the transcript.
fn json_candidates(stdout: &str) -> Vec<Value> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return match value {
            Value::Array(items) => items,
            other => vec![other],
        };
    }
    stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
        .collect()
}

/// Timing derived from provider events as they crossed the stdout pipe. The
/// monotonic offsets are runner observations; UTC strings are labels captured
/// from the same boundaries. Unknown boundaries remain absent.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TimingReading {
    pub model_ms: Option<u128>,
    pub tool_ms: Option<u128>,
    pub time_to_first_token_ms: Option<u128>,
    pub trace_complete: bool,
}

/// Provider trace grammar advertised by a harness adapter. A format alone is
/// insufficient: several CLIs use `json` for a compact terminal object that has
/// no provider request boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelemetryTrace {
    CodexJson,
    OpenCodeJson,
}

#[derive(Debug, Clone)]
struct Boundary {
    id: String,
    start: Option<(u128, String)>,
    finish: Option<(u128, String)>,
    status: Option<ToolCallStatus>,
}

/// Enrich normalized calls using paired provider identities and the runner's
/// actual output observations. Returns measured model/tool totals; overlapping
/// tool intervals are merged before summing.
pub fn apply_observed_timing(
    events: &mut [ActionEvent],
    observations: &[OutputObservation],
    run_status: Status,
    duration_ms: Option<u128>,
    trace: TelemetryTrace,
) -> TimingReading {
    let lines = observed_json_lines(observations);
    let mut boundaries: Vec<Boundary> = Vec::new();
    let mut request_start = None;
    let mut first_token = None;
    let mut model_start = None;
    let mut last_model_byte = None;
    let mut saw_model_boundary = false;
    let mut model_intervals = Vec::new();
    for (value, offset, utc) in &lines {
        if is_provider_start(value, trace) {
            request_start.get_or_insert(*offset);
            model_start.get_or_insert(*offset);
        }
        if is_model_token(value) {
            first_token.get_or_insert(*offset);
            last_model_byte = Some(*offset);
            saw_model_boundary = true;
        }
        if is_tool_start(value) {
            if let Some(start) = model_start.take() {
                model_intervals.push((start, *offset));
            }
            last_model_byte = None;
            saw_model_boundary = true;
        }
        if is_tool_finish(value) && request_start.is_some() {
            model_start = Some(*offset);
            last_model_byte = None;
        }
        if is_provider_finish(value, trace) {
            if let (Some(start), Some(finish)) = (model_start.take(), last_model_byte.take()) {
                model_intervals.push((start, finish.max(start)));
            }
        }
        observe_boundaries(value, *offset, utc, &mut boundaries);
    }
    if !matches!(run_status, Status::Ok | Status::Nonzero) {
        if let (Some(start), Some(finish)) = (model_start.take(), last_model_byte.take()) {
            model_intervals.push((start, finish.max(start)));
        }
    }
    for event in events.iter_mut().filter(|event| event.kind == "tool_call") {
        let Some(id) = event.tool_call_id.as_deref() else {
            continue;
        };
        let Some(boundary) = boundaries.iter().find(|boundary| boundary.id == id) else {
            continue;
        };
        event.started_at = boundary.start.as_ref().map(|(_, utc)| utc.clone());
        event.finished_at = boundary.finish.as_ref().map(|(_, utc)| utc.clone());
        event.duration_ms = boundary
            .start
            .as_ref()
            .zip(boundary.finish.as_ref())
            .map(|((start, _), (finish, _))| finish.saturating_sub(*start));
        event.status = boundary.status.or_else(|| {
            boundary.start.as_ref().map(|_| {
                if run_status == Status::Timeout {
                    ToolCallStatus::Timeout
                } else {
                    ToolCallStatus::Interrupted
                }
            })
        });
    }
    let mut intervals = boundaries
        .iter()
        .filter_map(|boundary| {
            let start = boundary.start.as_ref()?.0;
            let finish = boundary
                .finish
                .as_ref()
                .map(|finish| finish.0)
                .or(duration_ms)?;
            Some((start, finish.max(start)))
        })
        .collect::<Vec<_>>();
    intervals.sort_unstable();
    let mut union = 0;
    let mut current: Option<(u128, u128)> = None;
    for (start, finish) in intervals {
        current = match current {
            Some((left, right)) if start <= right => Some((left, right.max(finish))),
            Some((left, right)) => {
                union += right.saturating_sub(left);
                Some((start, finish))
            }
            None => Some((start, finish)),
        };
    }
    if let Some((left, right)) = current {
        union += right.saturating_sub(left);
    }
    TimingReading {
        model_ms: request_start.map(|_| interval_union(&mut model_intervals)),
        tool_ms: request_start.map(|_| union),
        time_to_first_token_ms: request_start
            .zip(first_token)
            .map(|(start, token)| token.saturating_sub(start)),
        trace_complete: request_start.is_some()
            && (lines
                .iter()
                .any(|(value, _, _)| is_provider_finish(value, trace))
                || !matches!(run_status, Status::Ok | Status::Nonzero))
            && saw_model_boundary
            && events
                .iter()
                .filter(|event| event.kind == "tool_call")
                .all(|event| {
                    event.tool_call_id.is_some()
                        && event.started_at.is_some()
                        && event.status.is_some()
                        && (!matches!(
                            event.status,
                            Some(ToolCallStatus::Completed | ToolCallStatus::Failed)
                        ) || (event.finished_at.is_some() && event.duration_ms.is_some()))
                }),
    }
}

fn interval_union(intervals: &mut [(u128, u128)]) -> u128 {
    intervals.sort_unstable();
    let mut total = 0;
    let mut current: Option<(u128, u128)> = None;
    for &(start, finish) in intervals.iter() {
        current = match current {
            Some((left, right)) if start <= right => Some((left, right.max(finish))),
            Some((left, right)) => {
                total += right.saturating_sub(left);
                Some((start, finish))
            }
            None => Some((start, finish)),
        };
    }
    if let Some((left, right)) = current {
        total += right.saturating_sub(left);
    }
    total
}

fn observed_json_lines(observations: &[OutputObservation]) -> Vec<(Value, u128, String)> {
    let mut pending = Vec::new();
    let mut out = Vec::new();
    for observation in observations {
        pending.extend_from_slice(&observation.bytes);
        while let Some(newline) = pending.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = pending.drain(..=newline).collect();
            if let Ok(value) = serde_json::from_slice::<Value>(&line) {
                out.push((
                    value,
                    observation.offset_ms,
                    observation.observed_at.clone(),
                ));
            }
        }
    }
    if !pending.is_empty() {
        if let (Some(last), Ok(value)) = (
            observations.last(),
            serde_json::from_slice::<Value>(&pending),
        ) {
            out.push((value, last.offset_ms, last.observed_at.clone()));
        }
    }
    out
}

fn observe_boundaries(value: &Value, offset: u128, utc: &str, out: &mut Vec<Boundary>) {
    if let Some(item) = value.get("item").and_then(Value::as_object) {
        if item
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(is_codex_tool_type)
        {
            let Some(id) = item.get("id").and_then(Value::as_str) else {
                return;
            };
            let boundary = boundary(out, id);
            match value.get("type").and_then(Value::as_str) {
                Some("item.started") => boundary.start = Some((offset, utc.to_string())),
                Some("item.completed") => {
                    boundary.finish = Some((offset, utc.to_string()));
                    boundary.status = Some(codex_tool_status(item));
                }
                _ => {}
            }
        }
    }
    if value.get("type").and_then(Value::as_str) == Some("tool_call") {
        if let Some(id) = value
            .get("call_id")
            .or_else(|| value.pointer("/tool_call/toolCallId"))
            .and_then(Value::as_str)
        {
            let boundary = boundary(out, id);
            match value.get("subtype").and_then(Value::as_str) {
                Some("started") => boundary.start = Some((offset, utc.to_string())),
                Some("completed") => {
                    boundary.finish = Some((offset, utc.to_string()));
                    boundary.status = Some(if value.pointer("/tool_call/error").is_some() {
                        ToolCallStatus::Failed
                    } else {
                        ToolCallStatus::Completed
                    });
                }
                _ => {}
            }
        }
    }
    let blocks = value
        .pointer("/message/content")
        .or_else(|| value.get("content"));
    if let Some(blocks) = blocks.and_then(Value::as_array) {
        for block_value in blocks {
            let Some(block) = block_value.as_object() else {
                continue;
            };
            match block.get("type").and_then(Value::as_str) {
                Some("tool_use") => {
                    if let Some(id) = block.get("id").and_then(Value::as_str) {
                        boundary(out, id).start = Some((offset, utc.to_string()));
                    }
                }
                Some("tool_result") => {
                    if let Some(id) = block.get("tool_use_id").and_then(Value::as_str) {
                        let boundary = boundary(out, id);
                        boundary.finish = Some((offset, utc.to_string()));
                        boundary.status = Some(
                            if block.get("is_error").and_then(Value::as_bool) == Some(true) {
                                ToolCallStatus::Failed
                            } else {
                                ToolCallStatus::Completed
                            },
                        );
                    }
                }
                _ => {}
            }
        }
    }
    if let Some(part) = value.get("part").and_then(Value::as_object) {
        if part.get("type").and_then(Value::as_str) == Some("tool") {
            if let Some(id) = part
                .get("callID")
                .or_else(|| part.get("id"))
                .and_then(Value::as_str)
            {
                let state = part.get("state").and_then(Value::as_object);
                let boundary = boundary(out, id);
                let start_epoch = state
                    .and_then(|state| state.get("time"))
                    .and_then(|time| time.get("start"))
                    .and_then(Value::as_u64)
                    .map(u128::from);
                let end_epoch = state
                    .and_then(|state| state.get("time"))
                    .and_then(|time| time.get("end"))
                    .and_then(Value::as_u64)
                    .map(u128::from);
                if let Some(start_epoch) = start_epoch {
                    let start_offset = end_epoch
                        .map(|end| offset.saturating_sub(end.saturating_sub(start_epoch)))
                        .unwrap_or(offset);
                    boundary.start = Some((
                        start_offset,
                        crate::domain::history::format_rfc3339_millis(start_epoch),
                    ));
                }
                if let Some(end_epoch) = end_epoch {
                    boundary.finish = Some((
                        offset,
                        crate::domain::history::format_rfc3339_millis(end_epoch),
                    ));
                    boundary.status = Some(
                        match state
                            .and_then(|state| state.get("status"))
                            .and_then(Value::as_str)
                        {
                            Some("error" | "failed") => ToolCallStatus::Failed,
                            _ => ToolCallStatus::Completed,
                        },
                    );
                }
            }
        }
    }
}

fn boundary<'a>(boundaries: &'a mut Vec<Boundary>, id: &str) -> &'a mut Boundary {
    if let Some(index) = boundaries.iter().position(|boundary| boundary.id == id) {
        return &mut boundaries[index];
    }
    boundaries.push(Boundary {
        id: id.to_string(),
        start: None,
        finish: None,
        status: None,
    });
    boundaries.last_mut().expect("just pushed")
}

fn codex_tool_status(item: &serde_json::Map<String, Value>) -> ToolCallStatus {
    match item.get("status").and_then(Value::as_str) {
        Some("failed" | "declined" | "cancelled") => ToolCallStatus::Failed,
        Some("completed") => ToolCallStatus::Completed,
        _ if item.get("type").and_then(Value::as_str) == Some("command_execution") => {
            if item.get("exit_code").and_then(Value::as_i64) == Some(0) {
                ToolCallStatus::Completed
            } else {
                ToolCallStatus::Failed
            }
        }
        _ => ToolCallStatus::Completed,
    }
}

fn is_model_token(value: &Value) -> bool {
    let assistant_content = value
        .get("type")
        .and_then(Value::as_str)
        .filter(|kind| *kind == "assistant")
        .and_then(|_| {
            value
                .pointer("/message/content")
                .or_else(|| value.get("content"))
        })
        .and_then(Value::as_array)
        .is_some_and(|blocks| {
            blocks.iter().any(|block| {
                matches!(
                    block.get("type").and_then(Value::as_str),
                    Some("text" | "reasoning" | "thinking")
                ) && block
                    .get("text")
                    .or_else(|| block.get("thinking"))
                    .and_then(Value::as_str)
                    .is_some_and(|text| !text.is_empty())
            })
        });
    assistant_content
        || value
            .pointer("/part/type")
            .and_then(Value::as_str)
            .is_some_and(|kind| matches!(kind, "text" | "reasoning"))
            && value
                .pointer("/part/text")
                .and_then(Value::as_str)
                .is_some_and(|text| !text.is_empty())
        || value.pointer("/item/type").and_then(Value::as_str) == Some("agent_message")
            && value
                .pointer("/item/text")
                .and_then(Value::as_str)
                .is_some_and(|text| !text.is_empty())
        || value.pointer("/item/type").and_then(Value::as_str) == Some("reasoning")
            && value
                .pointer("/item/text")
                .and_then(Value::as_str)
                .is_some_and(|text| !text.is_empty())
}

fn is_provider_start(value: &Value, trace: TelemetryTrace) -> bool {
    match trace {
        TelemetryTrace::CodexJson => {
            value.get("type").and_then(Value::as_str) == Some("turn.started")
        }
        TelemetryTrace::OpenCodeJson => {
            value.get("type").and_then(Value::as_str) == Some("step_start")
                || value.pointer("/part/type").and_then(Value::as_str) == Some("step-start")
        }
    }
}

fn is_provider_finish(value: &Value, trace: TelemetryTrace) -> bool {
    match trace {
        TelemetryTrace::CodexJson => {
            value.get("type").and_then(Value::as_str) == Some("turn.completed")
        }
        TelemetryTrace::OpenCodeJson => {
            value.get("type").and_then(Value::as_str) == Some("step_finish")
                || value.pointer("/part/type").and_then(Value::as_str) == Some("step-finish")
        }
    }
}

fn is_tool_start(value: &Value) -> bool {
    value.get("type").and_then(Value::as_str) == Some("tool_call")
        && value.get("subtype").and_then(Value::as_str) == Some("started")
        || value.get("type").and_then(Value::as_str) == Some("item.started")
            && value
                .pointer("/item/type")
                .and_then(Value::as_str)
                .is_some_and(is_codex_tool_type)
        || value
            .pointer("/message/content")
            .or_else(|| value.get("content"))
            .and_then(Value::as_array)
            .is_some_and(|blocks| {
                blocks
                    .iter()
                    .any(|block| block.get("type").and_then(Value::as_str) == Some("tool_use"))
            })
        || value.pointer("/part/type").and_then(Value::as_str) == Some("tool")
            && value.pointer("/part/state/time/start").is_some()
            && value.pointer("/part/state/time/end").is_none()
}

fn is_tool_finish(value: &Value) -> bool {
    value.get("type").and_then(Value::as_str) == Some("tool_call")
        && value.get("subtype").and_then(Value::as_str) == Some("completed")
        || value.get("type").and_then(Value::as_str) == Some("item.completed")
            && value
                .pointer("/item/type")
                .and_then(Value::as_str)
                .is_some_and(is_codex_tool_type)
        || value
            .pointer("/message/content")
            .or_else(|| value.get("content"))
            .and_then(Value::as_array)
            .is_some_and(|blocks| {
                blocks
                    .iter()
                    .any(|block| block.get("type").and_then(Value::as_str) == Some("tool_result"))
            })
        || value.pointer("/part/type").and_then(Value::as_str) == Some("tool")
            && value.pointer("/part/state/time/end").is_some()
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
    fn opencode_running_and_completed_updates_collapse_by_call_id() {
        // Captured `opencode run --format json` behavior: the same callID is
        // emitted first with a running state and later as completed.
        let raw = concat!(
            r#"{"type":"tool_use","part":{"id":"part_1","callID":"call_1","type":"tool","tool":"bash","state":{"status":"running","input":{"command":"pwd"},"time":{"start":1773878400000}}}}"#,
            "\n",
            r#"{"type":"tool_use","part":{"id":"part_1","callID":"call_1","type":"tool","tool":"bash","state":{"status":"completed","input":{"command":"pwd"},"output":"/repo\n","time":{"start":1773878400000,"end":1773878400100}}}}"#,
            "\n",
        );
        let got = extract_events(raw, OutputFormat::Json).unwrap();
        assert_eq!(got.events.len(), 1);
        assert_eq!(got.events[0].tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(got.events[0].input, Some(json!({"command": "pwd"})));
        assert_eq!(got.events[0].output.as_deref(), Some("/repo\n"));
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
    fn cursor_tool_call_completed_event() {
        // Real cursor-agent stream-json: a `tool_call` `started` then `completed`.
        // The tool name is the `shellToolCall` key minus its `ToolCall` suffix;
        // input is `.args`, output is `.result.success.stdout`. Only the completed
        // event yields a normalized event (started is skipped to avoid a dupe).
        let raw = concat!(
            r#"{"type":"tool_call","subtype":"started","call_id":"c1","tool_call":{"shellToolCall":{"args":{"command":"echo hi"}},"toolCallId":"c1","startedAtMs":"1"}}"#,
            "\n",
            r#"{"type":"tool_call","subtype":"completed","call_id":"c1","tool_call":{"shellToolCall":{"args":{"command":"echo hi"},"result":{"success":{"command":"echo hi","exitCode":0,"stdout":"hi\n","stderr":""}}},"toolCallId":"c1","completedAtMs":"2"}}"#,
            "\n",
            r#"{"type":"result","subtype":"success","result":"done"}"#,
            "\n",
        );
        let got = extract_events(raw, OutputFormat::StreamJson).unwrap();
        assert_eq!(got.source, "stream-json:cursor-tool-calls");
        assert_eq!(got.events.len(), 1);
        assert_eq!(got.events[0].kind, "tool_call");
        assert_eq!(got.events[0].name.as_deref(), Some("shell"));
        assert_eq!(got.events[0].input, Some(json!({"command": "echo hi"})));
        assert_eq!(got.events[0].output.as_deref(), Some("hi\n"));
    }

    #[test]
    fn codex_command_execution_item_event() {
        // Real codex `exec --json`: an `item.started` then `item.completed` for a
        // `command_execution`. The completed item folds in the result; only it
        // yields an event. Name is the item type (codex has no tool-name field),
        // input is the run command, output is the aggregated output. A trailing
        // `agent_message` item is the final text, not a tool call.
        let raw = concat!(
            r#"{"type":"thread.started","thread_id":"th_1"}"#,
            "\n",
            r#"{"type":"item.started","item":{"id":"item_0","type":"command_execution","command":"/bin/bash -lc 'echo hi'","aggregated_output":"","exit_code":null,"status":"in_progress"}}"#,
            "\n",
            r#"{"type":"item.completed","item":{"id":"item_0","type":"command_execution","command":"/bin/bash -lc 'echo hi'","aggregated_output":"hi\n","exit_code":0,"status":"completed"}}"#,
            "\n",
            r#"{"type":"item.completed","item":{"id":"item_1","type":"agent_message","text":"I ran it."}}"#,
            "\n",
        );
        let got = extract_events(raw, OutputFormat::Json).unwrap();
        assert_eq!(got.source, "json:codex-items");
        assert_eq!(got.events.len(), 1);
        assert_eq!(got.events[0].kind, "tool_call");
        assert_eq!(got.events[0].name.as_deref(), Some("command_execution"));
        assert_eq!(
            got.events[0].input,
            Some(json!({"command": "/bin/bash -lc 'echo hi'"}))
        );
        assert_eq!(got.events[0].output.as_deref(), Some("hi\n"));
    }

    #[test]
    fn codex_mcp_item_updates_collapse_and_preserve_failure() {
        // `codex exec --json`'s official exec item shape uses the same id for
        // the started and completed MCP lifecycle records.
        let raw = concat!(
            r#"{"type":"item.started","item":{"id":"item_mcp","type":"mcp_tool_call","server":"minimal","tool":"count","arguments":{"n":2},"status":"in_progress"}}"#,
            "\n",
            r#"{"type":"item.completed","item":{"id":"item_mcp","type":"mcp_tool_call","server":"minimal","tool":"count","arguments":{"n":2},"result":null,"error":{"message":"user cancelled MCP tool call"},"status":"failed"}}"#,
            "\n",
        );
        let got = extract_events(raw, OutputFormat::Json).unwrap();
        assert_eq!(got.events.len(), 1);
        assert_eq!(got.events[0].tool_call_id.as_deref(), Some("item_mcp"));
        assert_eq!(got.events[0].name.as_deref(), Some("count"));
        assert_eq!(got.events[0].input, Some(json!({"n": 2})));
        assert_eq!(
            got.events[0].output.as_deref(),
            Some("user cancelled MCP tool call")
        );
    }

    #[test]
    fn qwen_content_blocks_stream_and_json_array() {
        // Qwen uses the Anthropic content-block shape. Under stream-json it is
        // NDLJSON (one message per line); under json it is a single JSON *array*
        // of the same message objects — json_candidates flattens the array so both
        // yield the same normalized events.
        let call = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"call_1","name":"run_shell_command","input":{"command":"echo hi"}}]}}"#;
        let result = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"call_1","is_error":false,"content":"hi"}]}}"#;
        let ndjson = format!("{call}\n{result}\n");
        let got = extract_events(&ndjson, OutputFormat::StreamJson).unwrap();
        assert_eq!(got.source, "stream-json:content-blocks");
        assert_eq!(got.events.len(), 2);
        assert_eq!(got.events[0].name.as_deref(), Some("run_shell_command"));
        assert_eq!(got.events[1].output.as_deref(), Some("hi"));

        let array = format!("[{call},{result}]");
        let got = extract_events(&array, OutputFormat::Json).unwrap();
        assert_eq!(got.source, "json:content-blocks");
        assert_eq!(got.events.len(), 2);
        assert_eq!(got.events[0].name.as_deref(), Some("run_shell_command"));
    }

    #[test]
    fn events_from_value_numbers_from_start_index() {
        // The streaming entry point: per-line extraction that numbers events from
        // a running offset, so the incremental stream matches the batch indices.
        let line: Value = serde_json::from_str(
            r#"{"part":{"type":"tool","tool":"bash","state":{"input":{"command":"ls"}}}}"#,
        )
        .unwrap();
        let evs = events_from_value(&line, 5);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].index, 5);
        assert_eq!(evs[0].name.as_deref(), Some("bash"));
        // A non-event line yields nothing.
        let noise: Value = serde_json::from_str(r#"{"type":"step_start"}"#).unwrap();
        assert!(events_from_value(&noise, 0).is_empty());
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

    #[test]
    fn anthropic_tool_use_is_not_a_first_content_token() {
        // Captured Claude/Qwen/Cursor stream-json content blocks use the same
        // assistant envelope. A tool-only message is model activity, but the
        // telemetry contract's TTFT boundary is the first non-empty user-visible
        // text/reasoning block.
        assert!(!is_model_token(&json!({
            "type": "assistant",
            "message": {"content": [{
                "type": "tool_use", "id": "toolu_1", "name": "Bash", "input": {}
            }]}
        })));
        assert!(!is_model_token(&json!({
            "type": "assistant",
            "message": {"content": [{"type": "text", "text": ""}]}
        })));
        assert!(is_model_token(&json!({
            "type": "assistant",
            "message": {"content": [{"type": "thinking", "thinking": "inspect"}]}
        })));
        assert!(is_model_token(&json!({
            "type": "assistant",
            "message": {"content": [{"type": "text", "text": "done"}]}
        })));
    }
}
