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

use crate::domain::control::{AbsolutePath, ControlShape, RedirectInput};
use crate::domain::mode::PermissionMode;

/// What a run needs to open a turn over one of these protocols. The command
/// layer resolves each value exactly as it would for an ordinary run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogueConfig {
    /// The assembled user prompt (system prompt already folded in where the
    /// harness has no separate channel for one).
    pub prompt: String,
    /// The working directory the turn runs in. Absolute by type: the server is
    /// a separate process that would resolve a relative path against its own
    /// directory, which is silently a different place.
    pub cwd: AbsolutePath,
    /// The model to request, when one was resolved.
    pub model: Option<String>,
    /// The approval posture, which decides how the client answers the server's
    /// permission requests.
    pub mode: PermissionMode,
}

/// What the driver should do after one line.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum DialogueStep {
    /// Keep driving the turn, writing these newline-terminated frames first.
    #[default]
    Continue,
    /// Keep driving the turn after writing these newline-terminated frames.
    Send(Vec<String>),
    /// The turn is over; the driver stops reading and tears the server down.
    Terminal,
}

impl DialogueStep {
    #[cfg(test)]
    fn frames(&self) -> &[String] {
        match self {
            Self::Send(frames) => frames,
            Self::Continue | Self::Terminal => &[],
        }
    }

    #[cfg(test)]
    fn is_terminal(&self) -> bool {
        *self == Self::Terminal
    }
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
    /// The prompt request id, whose response ends ACP turns but only acknowledges
    /// Codex turn creation; Codex ends on its later `turn/completed` notification.
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
    /// The redirection an interrupt committed, waiting for the turn it aborted
    /// to actually end.
    ///
    /// Neither protocol takes a second turn on a thread that already has one —
    /// codex answers `turn/start` with an error and ACP's `session/prompt`
    /// queues behind the live one — so the message is held here from the moment
    /// the interrupt is served until the aborted turn's own end, which is the
    /// first point a new turn can be opened. Held, never re-sent: it is taken
    /// once, so a turn that ends badly costs one attempt rather than looping.
    pending_redirect: Option<RedirectInput>,
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
            pending_redirect: None,
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
    ///
    /// `redirect` is the message to deliver with the abort. It is committed only
    /// when the abort itself could be written — a `None` here means nothing was
    /// aborted, so nothing may be queued behind it either.
    pub fn interrupt(&mut self, redirect: Option<&RedirectInput>) -> Option<Vec<String>> {
        if self.phase != Phase::InTurn {
            return None;
        }
        let frames = match self.shape {
            ControlShape::CodexAppServer => {
                let thread = self.session_id.clone()?;
                let turn = self.turn_id.clone()?;
                let id = self.take_id();
                vec![request(
                    id,
                    "turn/interrupt",
                    json!({"threadId": thread, "turnId": turn}),
                )]
            }
            // A NOTIFICATION, not a request. Sent with an `id`, goose answers
            // `-32601 Method not found` and the work carries on.
            _ => {
                let session = self.session_id.as_ref()?;
                vec![notification(
                    "session/cancel",
                    json!({"sessionId": session}),
                )]
            }
        };
        // Taken over only now that the abort exists: a caller whose interrupt
        // was refused above still holds its message, rather than having handed
        // it to a turn that was never stopped.
        if let Some(redirect) = redirect {
            self.pending_redirect = Some(redirect.clone());
        }
        Some(frames)
    }

    /// Whether a redirection is committed and waiting for the aborted turn to
    /// end. The run reads this to tell a redirect that is still in flight from
    /// one the conversation has already opened its new turn for.
    #[must_use]
    pub fn has_pending_redirect(&self) -> bool {
        self.pending_redirect.is_some()
    }

    /// Give back a redirection whose abort could not be written after all, so
    /// the supervisor still owns a message no turn is going to carry.
    pub fn abandon_redirect(&mut self) {
        self.pending_redirect = None;
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
    /// permissive modes are exactly the ones that mean "act without asking" for
    /// *any* action; everything else declines, which is the same
    /// deny-and-continue posture `--mode default` gives an ordinary run.
    ///
    /// `Edit` is deliberately absent rather than grouped with them: it promises
    /// auto-approved edits with shell still gated, and a permission request
    /// here carries no sourced way to tell the two apart, so allowing it would
    /// grant shell authority the mode denies. The command layer refuses `edit`
    /// on a driven turn up front ([`OneharnessError::ControlModeUnsupported`]),
    /// so it never reaches this decision — and if a later change lets it
    /// through, declining is the safe way to be wrong.
    fn permits_action(&self) -> bool {
        matches!(
            self.config.mode,
            PermissionMode::Bypass | PermissionMode::Auto
        )
    }

    fn on_server_request(&mut self, method: &str, id: Value, params: &Value) -> DialogueStep {
        let allow = self.permits_action();
        let result = match self.shape {
            // Approve only what is actually an approval request. A permissive
            // mode means "act without asking", not "say yes to any method the
            // server invents" — and a method this build does not recognize is
            // still *answered*, because an unanswered request stalls the turn.
            ControlShape::CodexAppServer if is_codex_approval(method) => {
                json!({"decision": if allow { "approved" } else { "denied" }})
            }
            ControlShape::CodexAppServer => json!({}),
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
        DialogueStep::Send(vec![response(id, result)])
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
        // Only an error answering one of this dialogue's active requests ends
        // it. A pooled/server stream may carry unrelated JSON-RPC responses;
        // terminating on an uncorrelated error would abort the wrong turn.
        let active_id = id.is_some_and(|id| {
            (self.phase == Phase::Handshake && id == self.handshake_id)
                || (self.phase == Phase::Opening && id == self.open_id)
                || (self.phase == Phase::InTurn && Some(id) == self.turn_request_id)
        });
        if active_id && message.get("error").is_some() {
            return self.finish();
        }
        let result = message.get("result").unwrap_or(&Value::Null);
        match id {
            Some(id) if id == self.handshake_id && self.phase == Phase::Handshake => {
                self.phase = Phase::Opening;
                let open_id = self.take_id();
                self.open_id = open_id;
                match self.shape {
                    ControlShape::CodexAppServer => DialogueStep::Send(vec![
                        notification("initialized", json!({})),
                        request(open_id, "thread/start", self.thread_start_params()),
                    ]),
                    _ => DialogueStep::Send(vec![request(
                        open_id,
                        "session/new",
                        json!({"cwd": self.config.cwd.to_string(), "mcpServers": []}),
                    )]),
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
                let prompt = self.config.prompt.clone();
                DialogueStep::Send(vec![request(
                    turn_id,
                    self.turn_method(),
                    self.turn_params(&session, &prompt),
                )])
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

    /// The turn is over. If an interrupt committed a redirection, this is the
    /// first moment a new turn can carry it, so the conversation opens one
    /// instead of ending — otherwise the dialogue is done.
    fn finish(&mut self) -> DialogueStep {
        if let Some(redirect) = self.pending_redirect.take() {
            if let Some(frames) = self.open_redirected_turn(&redirect) {
                return DialogueStep::Send(frames);
            }
        }
        self.phase = Phase::Done;
        DialogueStep::Terminal
    }

    /// The frames that open a fresh turn carrying `prompt` on the session this
    /// conversation already holds — the redirection's delivery.
    ///
    /// `None` when the server never named a session, which is the one case
    /// there is nothing to address a new turn to.
    fn open_redirected_turn(&mut self, prompt: &RedirectInput) -> Option<Vec<String>> {
        let session = self.session_id.clone()?;
        // The aborted turn's answer and the redirected one are different
        // answers; run together they read as one sentence that changes its mind.
        if !self.text.is_empty() {
            self.text.push('\n');
            self.text_item = None;
        }
        // The old turn is gone: codex mints a new id for the new one, and
        // interrupting against the stale id would address a turn that ended.
        self.turn_id = None;
        self.phase = Phase::InTurn;
        let id = self.take_id();
        self.turn_request_id = Some(id);
        Some(vec![request(
            id,
            self.turn_method(),
            self.turn_params(&session, prompt.as_str()),
        )])
    }

    fn turn_method(&self) -> &'static str {
        match self.shape {
            ControlShape::CodexAppServer => "turn/start",
            _ => "session/prompt",
        }
    }

    fn thread_start_params(&self) -> Value {
        let mut params = json!({
            "cwd": self.config.cwd.to_string(),
            "approvalPolicy": self.codex_approval_policy(),
        });
        if let Some(model) = &self.config.model {
            params["model"] = json!(model);
        }
        params
    }

    /// The params that open a turn on `session` carrying `prompt` — the run's
    /// own prompt first, and afterwards whatever a redirect delivered. One
    /// function for both, so a redirected turn is negotiated in exactly the
    /// posture and working directory the original was.
    fn turn_params(&self, session: &str, prompt: &str) -> Value {
        match self.shape {
            ControlShape::CodexAppServer => json!({
                "threadId": session,
                "cwd": self.config.cwd.to_string(),
                "input": [{"type": "text", "text": prompt}],
                "approvalPolicy": self.codex_approval_policy(),
                "sandboxPolicy": self.codex_sandbox_policy(),
            }),
            _ => json!({
                "sessionId": session,
                "prompt": [{"type": "text", "text": prompt}],
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
                "writableRoots": [self.config.cwd.to_string()],
                "networkAccess": false,
            })
        } else {
            json!({"type": "readOnly"})
        }
    }
}

/// What one ACP permission option does, read off its `kind`.
///
/// A closed set parsed at the boundary rather than substring checks on the
/// server's raw string: the four ACP kinds happen to be distinguishable by
/// substring, but an invented one need not be — `disallow_once` contains
/// "allow" and would be selected as a grant. An unknown kind is never
/// selected, because an option whose meaning cannot be read is not one to
/// grant.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(from = "String")]
enum OptionKind {
    AllowOnce,
    AllowAlways,
    RejectOnce,
    RejectAlways,
    #[default]
    Unknown,
}

impl From<String> for OptionKind {
    fn from(raw: String) -> Self {
        match raw.as_str() {
            "allow_once" => OptionKind::AllowOnce,
            "allow_always" => OptionKind::AllowAlways,
            "reject_once" => OptionKind::RejectOnce,
            "reject_always" => OptionKind::RejectAlways,
            _ => OptionKind::Unknown,
        }
    }
}

/// An ACP permission option the client may select, as the server offers it.
#[derive(Debug, Deserialize)]
struct PermissionOption {
    #[serde(rename = "optionId")]
    option_id: String,
    #[serde(default)]
    kind: OptionKind,
}

/// The option to select for a `session/request_permission` under a permissive
/// mode: the server's own **allow** option, preferring a one-shot grant over a
/// standing one.
///
/// `None` when no offered option says it allows anything — including when the
/// server offered options this build cannot read. Picking whatever came first
/// would be granting a permission nobody chose, which is worse than declining:
/// declining costs a turn, granting costs whatever the option was.
#[must_use]
pub fn acp_allow_option(options: &Value) -> Option<String> {
    let options: Vec<PermissionOption> = serde_json::from_value(options.clone()).ok()?;
    let of_kind = |wanted: OptionKind| options.iter().find(move |o| o.kind == wanted);
    of_kind(OptionKind::AllowOnce)
        .or_else(|| of_kind(OptionKind::AllowAlways))
        .map(|option| option.option_id.clone())
}

/// Whether `method` is one of Codex's approval requests — the only ones an
/// approval decision is a meaningful answer to. Sourced from the app-server's
/// own generated protocol schema, never guessed.
fn is_codex_approval(method: &str) -> bool {
    matches!(
        method,
        "applyPatchApproval"
            | "execCommandApproval"
            | "item/commandExecution/requestApproval"
            | "item/fileChange/requestApproval"
            | "item/permissions/requestApproval"
    )
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
    use crate::domain::control::{absolute_for_test, absolute_text_for_test};

    /// The one working directory every fixture turn runs in.
    const WORK: &str = "/work";

    fn config() -> DialogueConfig {
        DialogueConfig {
            prompt: "do the thing".to_string(),
            cwd: absolute_for_test(WORK),
            model: Some("gpt-5-codex".to_string()),
            mode: PermissionMode::Bypass,
        }
    }

    /// `text` as the validated redirection an interrupt would carry.
    fn redirection(text: &str) -> RedirectInput {
        RedirectInput::new(text).expect("fixture redirections are messages")
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
        assert!(d.interrupt(None).is_none());

        let step = d.on_line(r#"{"jsonrpc":"2.0","id":1,"result":{}}"#);
        assert!(!step.is_terminal());
        assert_eq!(step.frames().len(), 2);
        assert_eq!(parse(&step.frames()[0])["method"], "initialized");
        let start = parse(&step.frames()[1]);
        assert_eq!(start["method"], "thread/start");
        assert_eq!(start["params"]["cwd"], absolute_text_for_test(WORK));
        assert_eq!(start["params"]["approvalPolicy"], "never");
        assert_eq!(start["params"]["model"], "gpt-5-codex");

        let step = d.on_line(r#"{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"th-1"}}}"#);
        assert!(!step.is_terminal());
        assert_eq!(d.session_id(), Some("th-1"));
        let turn = parse(&step.frames()[0]);
        assert_eq!(turn["method"], "turn/start");
        assert_eq!(turn["params"]["threadId"], "th-1");
        assert_eq!(turn["params"]["input"][0]["text"], "do the thing");
        // A permissive mode confines writes to the working directory rather
        // than handing over full access.
        assert_eq!(turn["params"]["sandboxPolicy"]["type"], "workspaceWrite");
        assert_eq!(
            turn["params"]["sandboxPolicy"]["writableRoots"][0],
            absolute_text_for_test(WORK)
        );

        // The turn id arrives on the event stream and is what interrupt addresses.
        d.on_line(r#"{"jsonrpc":"2.0","method":"turn/started","params":{"threadId":"th-1","turn":{"id":"tu-9"}}}"#);
        let frames = d.interrupt(None).expect("a turn is in flight");
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
            !step.is_terminal(),
            "codex acknowledges turn/start immediately — ending here would end every run instantly"
        );
        assert!(d.interrupt(None).is_some(), "the turn is still in flight");

        // `turn/completed` is what ends it.
        let step = d.on_line(r#"{"jsonrpc":"2.0","method":"turn/completed","params":{}}"#);
        assert!(step.is_terminal());
        assert!(d.interrupt(None).is_none(), "no turn left to interrupt");
    }

    #[test]
    fn codex_takes_the_turn_id_from_the_turn_start_response_too() {
        // The id arrives on whichever comes first — the response or the
        // `turn/started` notification — and interrupt needs it either way.
        let mut d = Dialogue::new(ControlShape::CodexAppServer, config()).unwrap();
        d.open();
        d.on_line(r#"{"jsonrpc":"2.0","id":1,"result":{}}"#);
        d.on_line(r#"{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"th-1"}}}"#);
        assert!(d.interrupt(None).is_none(), "no turn id yet");
        d.on_line(r#"{"jsonrpc":"2.0","id":3,"result":{"turn":{"id":"tu-3"}}}"#);
        let abort = parse(&d.interrupt(None).expect("a turn is in flight")[0]);
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
        assert!(step.is_terminal());
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
        let new = parse(&step.frames()[0]);
        assert_eq!(new["method"], "session/new");
        assert_eq!(new["params"]["cwd"], absolute_text_for_test(WORK));

        let step = d.on_line(r#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"sess-7"}}"#);
        assert_eq!(d.session_id(), Some("sess-7"));
        let prompt = parse(&step.frames()[0]);
        assert_eq!(prompt["method"], "session/prompt");
        assert_eq!(prompt["params"]["sessionId"], "sess-7");
        assert_eq!(prompt["params"]["prompt"][0]["text"], "do the thing");

        // Cancel is a NOTIFICATION: with an `id`, goose answers -32601 and the
        // work carries on.
        let frames = d.interrupt(None).expect("a turn is in flight");
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
        assert!(step.is_terminal());
    }

    #[test]
    fn an_acp_permission_request_is_always_answered() {
        // Goose and Copilot block forever until the client replies, so a
        // request must never fall through unanswered.
        let mut d = Dialogue::new(ControlShape::AcpCancel, config()).unwrap();
        let step = d.on_line(
            r#"{"jsonrpc":"2.0","id":41,"method":"session/request_permission","params":{"options":[{"optionId":"a","kind":"allow_once"},{"optionId":"r","kind":"reject_once"}]}}"#,
        );
        assert_eq!(step.frames().len(), 1, "a request must be answered");
        let reply = parse(&step.frames()[0]);
        assert_eq!(reply["id"], 41);
        assert_eq!(reply["result"]["outcome"]["outcome"], "selected");
        assert_eq!(reply["result"]["outcome"]["optionId"], "a");
    }

    #[test]
    fn edit_mode_declines_rather_than_granting_shell_authority_it_denies() {
        // `edit` promises auto-approved edits with shell still gated, and an ACP
        // permission request carries no sourced way to tell those apart — so a
        // blanket grant here would hand the agent authority the mode refuses.
        // The command layer rejects `--mode edit` on a driven turn up front;
        // this pins the safe answer if one ever reaches the dialogue.
        let mut d = Dialogue::new(
            ControlShape::AcpCancel,
            DialogueConfig {
                mode: PermissionMode::Edit,
                ..config()
            },
        )
        .unwrap();
        let reply = parse(
            &d.on_line(
                r#"{"jsonrpc":"2.0","id":9,"method":"session/request_permission","params":{"options":[{"optionId":"a","kind":"allow_once"}]}}"#,
            )
            .frames()[0],
        );
        assert_eq!(
            reply["result"]["outcome"]["outcome"], "cancelled",
            "edit must not select an allow option it cannot narrow"
        );
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
            .frames()[0],
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
                .frames()[0],
        );
        assert_eq!(reply["result"]["decision"], "denied");
        assert_eq!(codex.codex_sandbox_policy()["type"], "readOnly");
    }

    #[test]
    fn codex_answers_an_unrecognized_request_without_granting_it() {
        // Answering is mandatory — an unanswered request stalls the turn — but a
        // permissive mode means "act without asking", not "approve any method
        // the server invents".
        let mut d = Dialogue::new(ControlShape::CodexAppServer, config()).unwrap();
        let reply = parse(
            &d.on_line(r#"{"jsonrpc":"2.0","id":8,"method":"attestation/generate","params":{}}"#)
                .frames()[0],
        );
        assert_eq!(reply["id"], 8, "the request is still answered");
        assert!(
            reply["result"].get("decision").is_none(),
            "an unrecognized method gets no approval: {reply}"
        );

        let approved = parse(
            &d.on_line(
                r#"{"jsonrpc":"2.0","id":9,"method":"item/commandExecution/requestApproval","params":{}}"#,
            )
            .frames()[0],
        );
        assert_eq!(approved["result"]["decision"], "approved");
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
        // No option says it allows anything, so nothing is granted — picking
        // the first would grant a permission nobody chose. `disallow_once` is
        // the trap a substring read falls into: it contains "allow".
        let none_allow = json!([
            {"optionId": "reject", "kind": "reject_once"},
            {"optionId": "mystery", "kind": "something_new"},
            {"optionId": "no", "kind": "disallow_once"},
            {"optionId": "unnamed"},
        ]);
        assert_eq!(acp_allow_option(&none_allow), None);
    }

    #[test]
    fn a_server_error_ends_the_turn_rather_than_hanging() {
        let mut d = Dialogue::new(ControlShape::AcpCancel, config()).unwrap();
        d.open();
        let step = d.on_line(
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32603,"message":"Internal error"}}"#,
        );
        assert!(step.is_terminal(), "there is nothing further to drive");
        assert_eq!(d.session_id(), None);
        assert_eq!(d.text(), None);
    }

    #[test]
    fn an_uncorrelated_server_error_does_not_end_the_active_dialogue() {
        let mut d = Dialogue::new(ControlShape::AcpCancel, config()).unwrap();
        d.open();
        let step = d.on_line(
            r#"{"jsonrpc":"2.0","id":99,"error":{"code":-32603,"message":"other request"}}"#,
        );
        assert_eq!(step, DialogueStep::Continue);

        let handshake = d.on_line(r#"{"jsonrpc":"2.0","id":1,"result":{}}"#);
        assert_eq!(handshake.frames().len(), 1, "the dialogue must remain live");
    }

    #[test]
    fn a_session_the_server_never_named_ends_the_turn() {
        // No identifier means no turn to address and no handle to resume, so
        // sending a prompt would create work nobody could interrupt.
        let mut d = Dialogue::new(ControlShape::AcpCancel, config()).unwrap();
        d.open();
        d.on_line(r#"{"jsonrpc":"2.0","id":1,"result":{}}"#);
        let step = d.on_line(r#"{"jsonrpc":"2.0","id":2,"result":{}}"#);
        assert!(step.is_terminal());
        assert!(step.frames().is_empty());
    }

    #[test]
    fn noise_and_partial_lines_are_ignored() {
        let mut d = Dialogue::new(ControlShape::CodexAppServer, config()).unwrap();
        d.open();
        for noise in ["", "   ", "not json", "{}", r#"{"jsonrpc":"2.0"}"#] {
            assert_eq!(d.on_line(noise), DialogueStep::default(), "{noise}");
        }
    }

    /// Drive a codex conversation to the point where a turn is in flight.
    fn codex_in_turn() -> Dialogue {
        let mut d = Dialogue::new(ControlShape::CodexAppServer, config()).unwrap();
        d.open();
        d.on_line(r#"{"jsonrpc":"2.0","id":1,"result":{}}"#);
        d.on_line(r#"{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"th-1"}}}"#);
        d.on_line(r#"{"jsonrpc":"2.0","method":"turn/started","params":{"threadId":"th-1","turn":{"id":"tu-9"}}}"#);
        d
    }

    #[test]
    fn a_codex_interrupt_carrying_a_redirection_opens_the_next_turn_when_this_one_ends() {
        // Codex refuses a second turn on a thread that already has one, so the
        // redirection is held from the moment the interrupt is served until
        // `turn/completed` — the first point a new turn can carry it. The
        // supervisor's message is the run's from the interrupt onwards, which is
        // what makes the two one operation.
        let mut d = codex_in_turn();
        d.on_line(
            r#"{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"itemId":"m1","delta":"wrong path"}}"#,
        );
        let abort = parse(
            &d.interrupt(Some(&redirection("do X instead")))
                .expect("a turn is in flight")[0],
        );
        assert_eq!(abort["method"], "turn/interrupt");
        assert!(d.has_pending_redirect());

        // The aborted turn ending is delivery, not the end of the conversation.
        let step = d.on_line(r#"{"jsonrpc":"2.0","method":"turn/completed","params":{}}"#);
        assert!(
            !step.is_terminal(),
            "the redirection still has to be carried"
        );
        assert!(!d.has_pending_redirect(), "it was handed over exactly once");
        let next = parse(&step.frames()[0]);
        assert_eq!(next["method"], "turn/start");
        assert_eq!(next["params"]["threadId"], "th-1", "same thread, new turn");
        assert_eq!(next["params"]["input"][0]["text"], "do X instead");
        // Negotiated in the run's own posture and directory, exactly as the
        // first turn was — a redirection is not a chance to widen either.
        assert_eq!(next["params"]["cwd"], absolute_text_for_test(WORK));
        assert_eq!(next["params"]["sandboxPolicy"]["type"], "workspaceWrite");
        assert_eq!(next["params"]["approvalPolicy"], "never");

        // The new turn is interruptible in its own right, once codex names it.
        assert!(
            d.interrupt(None).is_none(),
            "the old turn's id must not address the new one"
        );
        d.on_line(r#"{"jsonrpc":"2.0","method":"turn/started","params":{"turn":{"id":"tu-10"}}}"#);
        let again = parse(&d.interrupt(None).expect("the redirected turn is in flight")[0]);
        assert_eq!(again["params"]["turnId"], "tu-10");

        // And it ends the conversation, because nothing is queued behind it.
        let step = d.on_line(r#"{"jsonrpc":"2.0","method":"turn/completed","params":{}}"#);
        assert!(step.is_terminal());

        // Both answers are kept, separated rather than run together into one
        // sentence that changes its mind.
        d.on_line(
            r#"{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"itemId":"m2","delta":"ignored"}}"#,
        );
        assert_eq!(d.text().as_deref(), Some("wrong path"));
    }

    #[test]
    fn an_acp_interrupt_carrying_a_redirection_prompts_the_same_session_again() {
        let mut d = Dialogue::new(ControlShape::AcpCancel, config()).unwrap();
        d.open();
        d.on_line(r#"{"jsonrpc":"2.0","id":1,"result":{}}"#);
        d.on_line(r#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"sess-7"}}"#);
        let cancel = parse(
            &d.interrupt(Some(&redirection("do X instead")))
                .expect("in flight")[0],
        );
        assert_eq!(cancel["method"], "session/cancel");
        assert!(cancel.get("id").is_none(), "cancel is still a notification");

        // ACP ends the turn on the prompt response — reporting `end_turn` even
        // after a real cancellation — and that is where the redirection lands.
        let step = d.on_line(r#"{"jsonrpc":"2.0","id":3,"result":{"stopReason":"cancelled"}}"#);
        assert!(!step.is_terminal());
        let next = parse(&step.frames()[0]);
        assert_eq!(next["method"], "session/prompt");
        assert_eq!(
            next["params"]["sessionId"], "sess-7",
            "the session survived"
        );
        assert_eq!(next["params"]["prompt"][0]["text"], "do X instead");

        let step = d.on_line(r#"{"jsonrpc":"2.0","id":4,"result":{"stopReason":"end_turn"}}"#);
        assert!(step.is_terminal(), "nothing is queued behind the redirect");
    }

    #[test]
    fn a_redirection_is_only_taken_over_when_the_abort_it_rides_exists() {
        // The supervisor still owns its message unless the turn was really
        // stopped: taking it and then refusing the interrupt would be the exact
        // window this feature exists to close.
        let mut before_the_turn = Dialogue::new(ControlShape::CodexAppServer, config()).unwrap();
        before_the_turn.open();
        assert!(before_the_turn
            .interrupt(Some(&redirection("too early")))
            .is_none());
        assert!(!before_the_turn.has_pending_redirect());

        // In flight, but codex has not named the turn yet, so there is no id to
        // abort against.
        let mut unnamed = Dialogue::new(ControlShape::CodexAppServer, config()).unwrap();
        unnamed.open();
        unnamed.on_line(r#"{"jsonrpc":"2.0","id":1,"result":{}}"#);
        unnamed.on_line(r#"{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"th-1"}}}"#);
        assert!(unnamed
            .interrupt(Some(&redirection("no turn id yet")))
            .is_none());
        assert!(!unnamed.has_pending_redirect());

        // And one whose abort could not be written is given back, so the turn
        // that ends afterwards ends rather than carrying a message nobody
        // believes was delivered.
        let mut abandoned = codex_in_turn();
        assert!(abandoned.interrupt(Some(&redirection("dropped"))).is_some());
        abandoned.abandon_redirect();
        assert!(!abandoned.has_pending_redirect());
        let step = abandoned.on_line(r#"{"jsonrpc":"2.0","method":"turn/completed","params":{}}"#);
        assert!(step.is_terminal());
    }

    #[test]
    fn a_redirection_is_attempted_once_even_when_the_turn_ends_badly() {
        // A server error ends the turn too, and the message has been committed,
        // so it is still offered the one delivery it was promised — and then the
        // conversation ends rather than retrying forever.
        let mut d = codex_in_turn();
        d.interrupt(Some(&redirection("do X instead"))).unwrap();
        let step =
            d.on_line(r#"{"jsonrpc":"2.0","id":3,"error":{"code":-32603,"message":"boom"}}"#);
        assert!(!step.is_terminal());
        let next = parse(&step.frames()[0]);
        assert_eq!(next["method"], "turn/start");
        let step = d.on_line(&format!(
            r#"{{"jsonrpc":"2.0","id":{},"error":{{"code":-32603,"message":"boom"}}}}"#,
            next["id"]
        ));
        assert!(step.is_terminal(), "one attempt, not a loop");
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
