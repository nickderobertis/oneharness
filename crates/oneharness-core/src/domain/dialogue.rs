//! The pure protocol state machines behind server-backed turn control.
//!
//! Claude Code's control rides its ordinary `-p` run: one prompt frame in, a
//! `result` document out. Every other controllable harness instead speaks a
//! **JSON-RPC protocol on stdio** (`codex app-server`, `copilot --acp`,
//! `goose acp`), where driving one turn is a conversation: handshake, open a
//! thread/session, send the prompt, answer whatever the server asks, and read
//! the turn's end off the stream. The interrupt then rides that same open stdin.
//!
//! A [`Dialogue`] is that conversation as a pure state machine: it is fed one
//! stdout line at a time and answers with the lines to write next. No I/O, no
//! clock — so every protocol rule below is unit-testable against a recorded
//! exchange instead of only against a live provider.
//!
//! Two rules here are load-bearing and were learned the hard way from
//! `scripts/explore-control.sh`:
//!
//! * An ACP client **must answer `session/request_permission`**. Goose and
//!   Copilot block indefinitely and never begin work until it does, which reads
//!   exactly like a slow or broken harness.
//! * ACP cancel is a **notification**, never a request. Sent with an `id`, goose
//!   answers `-32601 Method not found` and the turn keeps running.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::domain::control::ControlShape;
use crate::domain::mode::PermissionMode;

/// What a run needs to open a turn over one of these protocols. The command
/// layer resolves each value exactly as it would for an ordinary run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogueConfig {
    /// The assembled user prompt (system prompt already folded in where the
    /// harness has no separate channel for one).
    pub prompt: String,
    /// The working directory the turn runs in, absolute.
    pub cwd: String,
    /// The model to request, when one was resolved.
    pub model: Option<String>,
    /// The approval posture, which decides how the client answers the server's
    /// permission requests.
    pub mode: PermissionMode,
}

/// What the driver should do after one line.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DialogueStep {
    /// Lines to write to the server's stdin, in order. Each is newline-terminated.
    pub send: Vec<String>,
    /// The turn is over; the driver stops reading and tears the server down.
    pub terminal: bool,
}

/// How far along one turn's conversation is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Waiting for the handshake response.
    Handshake,
    /// Waiting for the thread/session to be created.
    Opening,
    /// The prompt is out; the turn is in flight.
    InTurn,
    /// The turn ended (completed, failed, or interrupted).
    Done,
}

/// One turn driven over a JSON-RPC stdio protocol.
#[derive(Debug, Clone)]
pub struct Dialogue {
    shape: ControlShape,
    config: DialogueConfig,
    phase: Phase,
    next_id: u64,
    /// The request id whose *response* ends the turn.
    turn_request_id: Option<u64>,
    handshake_id: u64,
    open_id: u64,
    /// The harness's own session/thread identifier, once it exists.
    session_id: Option<String>,
    /// Codex's per-turn identifier, which `turn/interrupt` addresses.
    turn_id: Option<String>,
    /// Assistant text accumulated from the update stream.
    text: String,
    /// The message the accumulated text is currently coming from, so two
    /// separate assistant messages do not run together into one word.
    text_item: Option<String>,
}

impl Dialogue {
    /// A dialogue for `shape`, or `None` when the shape needs none (Claude
    /// Code's control rides its ordinary run, which is not a conversation).
    #[must_use]
    pub fn new(shape: ControlShape, config: DialogueConfig) -> Option<Self> {
        matches!(
            shape,
            ControlShape::CodexAppServer | ControlShape::AcpCancel
        )
        .then(|| Dialogue {
            shape,
            config,
            phase: Phase::Handshake,
            next_id: 1,
            turn_request_id: None,
            handshake_id: 0,
            open_id: 0,
            session_id: None,
            turn_id: None,
            text: String::new(),
            text_item: None,
        })
    }

    /// The frames that open the conversation, written before any line is read.
    pub fn open(&mut self) -> Vec<String> {
        let id = self.take_id();
        self.handshake_id = id;
        match self.shape {
            ControlShape::CodexAppServer => vec![request(
                id,
                "initialize",
                json!({
                    "clientInfo": {
                        "name": "oneharness",
                        "title": "oneharness",
                        "version": env!("CARGO_PKG_VERSION"),
                    }
                }),
            )],
            // ACP's `initialize` announces what the *client* can do. oneharness
            // reads and writes files itself, so it advertises neither: a server
            // that believes otherwise would wait on a filesystem request nobody
            // is listening for.
            _ => vec![request(
                id,
                "initialize",
                json!({
                    "protocolVersion": 1,
                    "clientCapabilities": {
                        "fs": {"readTextFile": false, "writeTextFile": false}
                    },
                }),
            )],
        }
    }

    /// Advance the conversation with one stdout line.
    pub fn on_line(&mut self, line: &str) -> DialogueStep {
        let trimmed = line.trim();
        if trimmed.is_empty() || self.phase == Phase::Done {
            return DialogueStep::default();
        }
        let Ok(message) = serde_json::from_str::<Value>(trimmed) else {
            return DialogueStep::default();
        };
        let method = message.get("method").and_then(Value::as_str);
        let id = message.get("id");
        match (method, id) {
            // A server->client request: answering is mandatory, or the turn
            // stalls forever waiting on a client that never replies.
            (Some(method), Some(id)) => {
                let params = message.get("params").cloned().unwrap_or(Value::Null);
                self.on_server_request(method, id.clone(), &params)
            }
            (Some(method), None) => self.on_notification(method, &message),
            (None, Some(id)) => self.on_response(id.as_u64(), &message),
            (None, None) => DialogueStep::default(),
        }
    }

    /// The frames that abort the in-flight turn, or `None` when there is no
    /// turn to abort (before the prompt, or after the turn ended).
    pub fn interrupt(&mut self) -> Option<Vec<String>> {
        if self.phase != Phase::InTurn {
            return None;
        }
        match self.shape {
            ControlShape::CodexAppServer => {
                let thread = self.session_id.clone()?;
                let turn = self.turn_id.clone()?;
                let id = self.take_id();
                Some(vec![request(
                    id,
                    "turn/interrupt",
                    json!({"threadId": thread, "turnId": turn}),
                )])
            }
            // A NOTIFICATION, not a request. Sent with an `id`, goose answers
            // `-32601 Method not found` and the work carries on.
            _ => {
                let session = self.session_id.as_ref()?;
                Some(vec![notification(
                    "session/cancel",
                    json!({"sessionId": session}),
                )])
            }
        }
    }

    /// The harness's native session identifier, once the server minted one.
    #[must_use]
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// The assistant text accumulated from the update stream, or `None` when
    /// the turn produced none. Never fabricated.
    #[must_use]
    pub fn text(&self) -> Option<String> {
        let text = self.text.trim();
        (!text.is_empty()).then(|| text.to_string())
    }

    /// How [`Self::text`] was obtained, for the report's `text_source`.
    #[must_use]
    pub fn text_source(&self) -> &'static str {
        match self.shape {
            ControlShape::CodexAppServer => "jsonrpc:codex-app-server",
            _ => "jsonrpc:acp-session-update",
        }
    }

    fn take_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Whether this run's approval posture allows the server to act. The
    /// permissive modes are exactly the ones that mean "act without asking";
    /// everything else declines, which is the same deny-and-continue posture
    /// `--mode default` gives an ordinary run.
    fn permits_action(&self) -> bool {
        matches!(
            self.config.mode,
            PermissionMode::Bypass | PermissionMode::Auto | PermissionMode::Edit
        )
    }

    fn on_server_request(&mut self, method: &str, id: Value, params: &Value) -> DialogueStep {
        let allow = self.permits_action();
        let result = match self.shape {
            ControlShape::CodexAppServer => {
                json!({"decision": if allow { "approved" } else { "denied" }})
            }
            // ACP: select one of the server's OWN options — the protocol offers
            // no way to narrow a grant, so the choice is which option, not how
            // much of it.
            _ if method == "session/request_permission" => {
                let chosen = allow
                    .then(|| acp_allow_option(params.get("options").unwrap_or(&Value::Null)))
                    .flatten();
                match chosen {
                    Some(option_id) => {
                        json!({"outcome": {"outcome": "selected", "optionId": option_id}})
                    }
                    None => json!({"outcome": {"outcome": "cancelled"}}),
                }
            }
            // An unrecognized request still gets an answer: both ACP harnesses
            // block indefinitely on an unanswered one, so silence is the one
            // reply that guarantees the turn never starts.
            _ => json!({}),
        };
        DialogueStep {
            send: vec![response(id, result)],
            terminal: false,
        }
    }

    fn on_notification(&mut self, method: &str, message: &Value) -> DialogueStep {
        let params = message.get("params").unwrap_or(&Value::Null);
        match (self.shape, method) {
            (ControlShape::CodexAppServer, "turn/started") => {
                self.turn_id = params
                    .get("turn")
                    .and_then(|turn| turn.get("id"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
                DialogueStep::default()
            }
            (ControlShape::CodexAppServer, "item/agentMessage/delta") => {
                if let Some(delta) = params.get("delta").and_then(Value::as_str) {
                    let item = params
                        .get("itemId")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    self.push_text(item, delta);
                }
                DialogueStep::default()
            }
            (ControlShape::CodexAppServer, "turn/completed") => self.finish(),
            (ControlShape::AcpCancel, "session/update") => {
                self.absorb_acp_update(params);
                DialogueStep::default()
            }
            _ => DialogueStep::default(),
        }
    }

    /// Append `delta`, separating it from the previous message when it belongs
    /// to a different one — codex streams a running commentary and a final
    /// answer as distinct items, which otherwise concatenate mid-sentence.
    fn push_text(&mut self, item: Option<String>, delta: &str) {
        if item.is_some() && self.text_item.is_some() && item != self.text_item {
            self.text.push('\n');
        }
        if item.is_some() {
            self.text_item = item;
        }
        self.text.push_str(delta);
    }

    fn absorb_acp_update(&mut self, params: &Value) {
        let update = params.get("update").unwrap_or(&Value::Null);
        if update.get("sessionUpdate").and_then(Value::as_str) != Some("agent_message_chunk") {
            return;
        }
        if let Some(text) = update
            .get("content")
            .and_then(|content| content.get("text"))
            .and_then(Value::as_str)
        {
            self.text.push_str(text);
        }
    }

    fn on_response(&mut self, id: Option<u64>, message: &Value) -> DialogueStep {
        // An error at any stage ends the turn: there is nothing further to
        // drive, and the run's envelope already carries the server's own output.
        if message.get("error").is_some() {
            return self.finish();
        }
        let result = message.get("result").unwrap_or(&Value::Null);
        match id {
            Some(id) if id == self.handshake_id && self.phase == Phase::Handshake => {
                self.phase = Phase::Opening;
                let open_id = self.take_id();
                self.open_id = open_id;
                match self.shape {
                    ControlShape::CodexAppServer => DialogueStep {
                        send: vec![
                            notification("initialized", json!({})),
                            request(open_id, "thread/start", self.thread_start_params()),
                        ],
                        terminal: false,
                    },
                    _ => DialogueStep {
                        send: vec![request(
                            open_id,
                            "session/new",
                            json!({"cwd": self.config.cwd, "mcpServers": []}),
                        )],
                        terminal: false,
                    },
                }
            }
            Some(id) if id == self.open_id && self.phase == Phase::Opening => {
                self.session_id = match self.shape {
                    ControlShape::CodexAppServer => result
                        .get("thread")
                        .and_then(|thread| thread.get("id"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    _ => result
                        .get("sessionId")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                };
                let Some(session) = self.session_id.clone() else {
                    // No identifier means no turn to address and no handle to
                    // resume; stop rather than send a prompt nobody can interrupt.
                    return self.finish();
                };
                self.phase = Phase::InTurn;
                let turn_id = self.take_id();
                self.turn_request_id = Some(turn_id);
                DialogueStep {
                    send: vec![request(
                        turn_id,
                        self.turn_method(),
                        self.turn_params(&session),
                    )],
                    terminal: false,
                }
            }
            Some(id) if Some(id) == self.turn_request_id => match self.shape {
                // Codex answers `turn/start` IMMEDIATELY with the new turn's id
                // and `status: "inProgress"` — the response is the turn's
                // *acknowledgement*, not its end. Treating it as terminal ended
                // every controlled run in under half a second, before the agent
                // did anything. The turn ends on `turn/completed`.
                ControlShape::CodexAppServer => {
                    if let Some(turn) = result
                        .get("turn")
                        .and_then(|turn| turn.get("id"))
                        .and_then(Value::as_str)
                    {
                        self.turn_id = Some(turn.to_string());
                    }
                    DialogueStep::default()
                }
                // ACP's `session/prompt` response carries the stop reason, so it
                // really is the end of the turn.
                _ => self.finish(),
            },
            _ => DialogueStep::default(),
        }
    }

    fn finish(&mut self) -> DialogueStep {
        self.phase = Phase::Done;
        DialogueStep {
            send: Vec::new(),
            terminal: true,
        }
    }

    fn turn_method(&self) -> &'static str {
        match self.shape {
            ControlShape::CodexAppServer => "turn/start",
            _ => "session/prompt",
        }
    }

    fn thread_start_params(&self) -> Value {
        let mut params = json!({
            "cwd": self.config.cwd,
            "approvalPolicy": self.codex_approval_policy(),
        });
        if let Some(model) = &self.config.model {
            params["model"] = json!(model);
        }
        params
    }

    fn turn_params(&self, session: &str) -> Value {
        match self.shape {
            ControlShape::CodexAppServer => json!({
                "threadId": session,
                "cwd": self.config.cwd,
                "input": [{"type": "text", "text": self.config.prompt}],
                "approvalPolicy": self.codex_approval_policy(),
                "sandboxPolicy": self.codex_sandbox_policy(),
            }),
            _ => json!({
                "sessionId": session,
                "prompt": [{"type": "text", "text": self.config.prompt}],
            }),
        }
    }

    /// Codex's own approval spelling for this run's mode. `never` is the
    /// headless posture: the client still answers any request that arrives, but
    /// a turn should not be *waiting* on approvals it cannot show anyone.
    fn codex_approval_policy(&self) -> &'static str {
        "never"
    }

    /// The narrowest sandbox that still lets the mode do what it promises:
    /// writes confined to the working directory under a permissive mode,
    /// read-only otherwise.
    fn codex_sandbox_policy(&self) -> Value {
        if self.permits_action() {
            json!({
                "type": "workspaceWrite",
                "writableRoots": [self.config.cwd],
                "networkAccess": false,
            })
        } else {
            json!({"type": "readOnly"})
        }
    }
}

/// An ACP permission option the client may select, as the server offers it.
#[derive(Debug, Deserialize)]
struct PermissionOption {
    #[serde(rename = "optionId")]
    option_id: String,
    #[serde(default)]
    kind: String,
}

/// The option to select for a `session/request_permission` under a permissive
/// mode: the server's own "allow" option, preferring a one-shot grant over a
/// standing one. `None` when the server offered nothing selectable.
#[must_use]
pub fn acp_allow_option(options: &Value) -> Option<String> {
    let options: Vec<PermissionOption> = serde_json::from_value(options.clone()).ok()?;
    let once = options
        .iter()
        .find(|option| option.kind.contains("allow") && option.kind.contains("once"));
    let any = options.iter().find(|option| option.kind.contains("allow"));
    once.or(any)
        .or_else(|| options.first())
        .map(|option| option.option_id.clone())
}

fn request(id: u64, method: &str, params: Value) -> String {
    line(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
}

fn notification(method: &str, params: Value) -> String {
    line(json!({"jsonrpc": "2.0", "method": method, "params": params}))
}

fn response(id: Value, result: Value) -> String {
    line(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

/// One newline-terminated wire line. The shapes are closed, so encoding cannot
/// fail; an empty object would be a protocol violation either way, so a failure
/// is surfaced as one rather than silently dropped.
fn line(value: Value) -> String {
    format!("{value}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> DialogueConfig {
        DialogueConfig {
            prompt: "do the thing".to_string(),
            cwd: "/work".to_string(),
            model: Some("gpt-5-codex".to_string()),
            mode: PermissionMode::Bypass,
        }
    }

    fn parse(frame: &str) -> Value {
        assert!(frame.ends_with('\n'), "frames are newline-terminated");
        serde_json::from_str(frame).expect("frames are JSON")
    }

    #[test]
    fn claude_control_needs_no_dialogue() {
        assert!(Dialogue::new(ControlShape::ClaudeControlRequest, config()).is_none());
        assert!(Dialogue::new(ControlShape::CodexAppServer, config()).is_some());
        assert!(Dialogue::new(ControlShape::AcpCancel, config()).is_some());
    }

    /// Drive a whole codex turn the way the app-server really answers, and
    /// assert every frame oneharness sends back.
    #[test]
    fn codex_handshakes_opens_a_thread_and_sends_the_prompt() {
        let mut d = Dialogue::new(ControlShape::CodexAppServer, config()).unwrap();
        let open = d.open();
        assert_eq!(open.len(), 1);
        let init = parse(&open[0]);
        assert_eq!(init["method"], "initialize");
        assert_eq!(init["id"], 1);

        // Nothing to interrupt before the prompt is out.
        assert!(d.interrupt().is_none());

        let step = d.on_line(r#"{"jsonrpc":"2.0","id":1,"result":{}}"#);
        assert!(!step.terminal);
        assert_eq!(step.send.len(), 2);
        assert_eq!(parse(&step.send[0])["method"], "initialized");
        let start = parse(&step.send[1]);
        assert_eq!(start["method"], "thread/start");
        assert_eq!(start["params"]["cwd"], "/work");
        assert_eq!(start["params"]["approvalPolicy"], "never");
        assert_eq!(start["params"]["model"], "gpt-5-codex");

        let step = d.on_line(r#"{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"th-1"}}}"#);
        assert!(!step.terminal);
        assert_eq!(d.session_id(), Some("th-1"));
        let turn = parse(&step.send[0]);
        assert_eq!(turn["method"], "turn/start");
        assert_eq!(turn["params"]["threadId"], "th-1");
        assert_eq!(turn["params"]["input"][0]["text"], "do the thing");
        // A permissive mode confines writes to the working directory rather
        // than handing over full access.
        assert_eq!(turn["params"]["sandboxPolicy"]["type"], "workspaceWrite");
        assert_eq!(turn["params"]["sandboxPolicy"]["writableRoots"][0], "/work");

        // The turn id arrives on the event stream and is what interrupt addresses.
        d.on_line(r#"{"jsonrpc":"2.0","method":"turn/started","params":{"threadId":"th-1","turn":{"id":"tu-9"}}}"#);
        let frames = d.interrupt().expect("a turn is in flight");
        let abort = parse(&frames[0]);
        assert_eq!(abort["method"], "turn/interrupt");
        assert_eq!(abort["params"]["threadId"], "th-1");
        assert_eq!(abort["params"]["turnId"], "tu-9");

        // Assistant text is accumulated from the delta stream.
        d.on_line(
            r#"{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"itemId":"m1","delta":"all "}}"#,
        );
        d.on_line(
            r#"{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"itemId":"m1","delta":"done"}}"#,
        );
        // A second message is separated rather than run together mid-sentence.
        d.on_line(
            r#"{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"itemId":"m2","delta":"and here is why"}}"#,
        );
        assert_eq!(d.text().as_deref(), Some("all done\nand here is why"));

        // The turn/start response ACKNOWLEDGES the turn; it does not end it.
        let step = d.on_line(
            r#"{"jsonrpc":"2.0","id":3,"result":{"turn":{"id":"tu-9","status":"inProgress"}}}"#,
        );
        assert!(
            !step.terminal,
            "codex acknowledges turn/start immediately — ending here would end every run instantly"
        );
        assert!(d.interrupt().is_some(), "the turn is still in flight");

        // `turn/completed` is what ends it.
        let step = d.on_line(r#"{"jsonrpc":"2.0","method":"turn/completed","params":{}}"#);
        assert!(step.terminal);
        assert!(d.interrupt().is_none(), "no turn left to interrupt");
    }

    #[test]
    fn codex_takes_the_turn_id_from_the_turn_start_response_too() {
        // The id arrives on whichever comes first — the response or the
        // `turn/started` notification — and interrupt needs it either way.
        let mut d = Dialogue::new(ControlShape::CodexAppServer, config()).unwrap();
        d.open();
        d.on_line(r#"{"jsonrpc":"2.0","id":1,"result":{}}"#);
        d.on_line(r#"{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"th-1"}}}"#);
        assert!(d.interrupt().is_none(), "no turn id yet");
        d.on_line(r#"{"jsonrpc":"2.0","id":3,"result":{"turn":{"id":"tu-3"}}}"#);
        let abort = parse(&d.interrupt().expect("a turn is in flight")[0]);
        assert_eq!(abort["params"]["turnId"], "tu-3");
    }

    #[test]
    fn codex_turn_completed_notification_also_ends_the_turn() {
        let mut d = Dialogue::new(ControlShape::CodexAppServer, config()).unwrap();
        d.open();
        d.on_line(r#"{"jsonrpc":"2.0","id":1,"result":{}}"#);
        d.on_line(r#"{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"th-1"}}}"#);
        let step = d
            .on_line(r#"{"jsonrpc":"2.0","method":"turn/completed","params":{"threadId":"th-1"}}"#);
        assert!(step.terminal);
    }

    #[test]
    fn acp_handshakes_opens_a_session_and_prompts() {
        let mut d = Dialogue::new(ControlShape::AcpCancel, config()).unwrap();
        let init = parse(&d.open()[0]);
        assert_eq!(init["method"], "initialize");
        assert_eq!(init["params"]["protocolVersion"], 1);
        // oneharness does not serve filesystem requests, so it says so.
        assert_eq!(
            init["params"]["clientCapabilities"]["fs"]["readTextFile"],
            false
        );

        let step = d.on_line(r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1}}"#);
        let new = parse(&step.send[0]);
        assert_eq!(new["method"], "session/new");
        assert_eq!(new["params"]["cwd"], "/work");

        let step = d.on_line(r#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"sess-7"}}"#);
        assert_eq!(d.session_id(), Some("sess-7"));
        let prompt = parse(&step.send[0]);
        assert_eq!(prompt["method"], "session/prompt");
        assert_eq!(prompt["params"]["sessionId"], "sess-7");
        assert_eq!(prompt["params"]["prompt"][0]["text"], "do the thing");

        // Cancel is a NOTIFICATION: with an `id`, goose answers -32601 and the
        // work carries on.
        let frames = d.interrupt().expect("a turn is in flight");
        let cancel = parse(&frames[0]);
        assert_eq!(cancel["method"], "session/cancel");
        assert_eq!(cancel["params"]["sessionId"], "sess-7");
        assert!(cancel.get("id").is_none(), "cancel must carry no id");

        // Text comes off the update stream.
        d.on_line(
            r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"sess-7","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"hi"}}}}"#,
        );
        assert_eq!(d.text().as_deref(), Some("hi"));
        // An unrelated update is not text.
        d.on_line(
            r#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"tool_call","title":"bash"}}}"#,
        );
        assert_eq!(d.text().as_deref(), Some("hi"));

        // The prompt response ends the turn — even reporting `end_turn` after a
        // real cancellation, which is exactly why oneharness records the
        // interrupt from its own side instead of reading this.
        let step = d.on_line(r#"{"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn"}}"#);
        assert!(step.terminal);
    }

    #[test]
    fn an_acp_permission_request_is_always_answered() {
        // Goose and Copilot block forever until the client replies, so a
        // request must never fall through unanswered.
        let mut d = Dialogue::new(ControlShape::AcpCancel, config()).unwrap();
        let step = d.on_line(
            r#"{"jsonrpc":"2.0","id":41,"method":"session/request_permission","params":{"options":[{"optionId":"a","kind":"allow_once"},{"optionId":"r","kind":"reject_once"}]}}"#,
        );
        assert_eq!(step.send.len(), 1, "a request must be answered");
        let reply = parse(&step.send[0]);
        assert_eq!(reply["id"], 41);
        assert_eq!(reply["result"]["outcome"]["outcome"], "selected");
        assert_eq!(reply["result"]["outcome"]["optionId"], "a");
    }

    #[test]
    fn a_restrictive_mode_declines_instead_of_granting() {
        let mut d = Dialogue::new(
            ControlShape::AcpCancel,
            DialogueConfig {
                mode: PermissionMode::Default,
                ..config()
            },
        )
        .unwrap();
        let reply = parse(
            &d.on_line(
                r#"{"jsonrpc":"2.0","id":7,"method":"session/request_permission","params":{"options":[{"optionId":"a","kind":"allow_once"}]}}"#,
            )
            .send[0],
        );
        assert_eq!(reply["result"]["outcome"]["outcome"], "cancelled");

        let mut codex = Dialogue::new(
            ControlShape::CodexAppServer,
            DialogueConfig {
                mode: PermissionMode::ReadOnly,
                ..config()
            },
        )
        .unwrap();
        let reply = parse(
            &codex
                .on_line(r#"{"jsonrpc":"2.0","id":5,"method":"execCommandApproval","params":{}}"#)
                .send[0],
        );
        assert_eq!(reply["result"]["decision"], "denied");
        assert_eq!(codex.codex_sandbox_policy()["type"], "readOnly");
    }

    #[test]
    fn allow_option_prefers_a_one_shot_grant() {
        let options = json!([
            {"optionId": "always", "kind": "allow_always"},
            {"optionId": "once", "kind": "allow_once"},
            {"optionId": "no", "kind": "reject_once"},
        ]);
        assert_eq!(acp_allow_option(&options).as_deref(), Some("once"));
        let only_always = json!([{"optionId": "always", "kind": "allow_always"}]);
        assert_eq!(acp_allow_option(&only_always).as_deref(), Some("always"));
        assert_eq!(acp_allow_option(&json!([])), None);
    }

    #[test]
    fn a_server_error_ends_the_turn_rather_than_hanging() {
        let mut d = Dialogue::new(ControlShape::AcpCancel, config()).unwrap();
        d.open();
        let step = d.on_line(
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32603,"message":"Internal error"}}"#,
        );
        assert!(step.terminal, "there is nothing further to drive");
        assert_eq!(d.session_id(), None);
        assert_eq!(d.text(), None);
    }

    #[test]
    fn a_session_the_server_never_named_ends_the_turn() {
        // No identifier means no turn to address and no handle to resume, so
        // sending a prompt would create work nobody could interrupt.
        let mut d = Dialogue::new(ControlShape::AcpCancel, config()).unwrap();
        d.open();
        d.on_line(r#"{"jsonrpc":"2.0","id":1,"result":{}}"#);
        let step = d.on_line(r#"{"jsonrpc":"2.0","id":2,"result":{}}"#);
        assert!(step.terminal);
        assert!(step.send.is_empty());
    }

    #[test]
    fn noise_and_partial_lines_are_ignored() {
        let mut d = Dialogue::new(ControlShape::CodexAppServer, config()).unwrap();
        d.open();
        for noise in ["", "   ", "not json", "{}", r#"{"jsonrpc":"2.0"}"#] {
            assert_eq!(d.on_line(noise), DialogueStep::default(), "{noise}");
        }
    }

    #[test]
    fn text_is_never_fabricated() {
        let mut d = Dialogue::new(ControlShape::CodexAppServer, config()).unwrap();
        assert_eq!(d.text(), None);
        d.on_line(
            r#"{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"delta":"   "}}"#,
        );
        assert_eq!(d.text(), None, "whitespace is not an answer");
        assert_eq!(d.text_source(), "jsonrpc:codex-app-server");
    }
}
