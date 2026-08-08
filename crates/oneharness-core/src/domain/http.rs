//! The pure half of the HTTP-driven turn: response framing and each
//! mechanism's protocol.
//!
//! Two control mechanisms reach their turn only over HTTP against a sidecar
//! server (`opencode serve`, `crush server`), and interrupting a CLI-driven run
//! was live-REFUTED for both — so oneharness submits the turn to that server
//! itself. Everything decidable without a socket lives here: the response
//! framing a reader must get right, the exact route and body of each request,
//! and what one line of an event stream means. The socket work is
//! [`crate::io::http`].
//!
//! Three facts here cost real time to find and are what the unit tests pin:
//!
//! * Both servers answer `Transfer-Encoding: chunked`, so a reader that hands
//!   the raw framing to a JSON parser reports a decode error where the harness
//!   in fact answered correctly.
//! * Both block on a permission decision the client must answer — crush emits a
//!   `permission_request` event and waits, opencode a `permission.*` event —
//!   exactly like ACP's `session/request_permission`. Without an answer the
//!   turn never does any work and every downstream assertion is vacuous.
//! * Crush's `client_id` is a self-assigned UUID that travels in the **body**
//!   when creating a workspace but as a **query parameter** on every other
//!   route; a mismatch answers a bare `{"message":"invalid client_id"}`.

use serde_json::{json, Value};

use crate::domain::control::ControlShape;
use crate::domain::mode::PermissionMode;

/// One HTTP request to a control server: everything but the socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: &'static str,
    pub path: String,
    /// The JSON body, already serialized. `None` sends no body at all, which is
    /// not the same as `{}` for a route that validates its shape.
    pub body: Option<String>,
}

impl HttpRequest {
    fn new(method: &'static str, path: String, body: Option<Value>) -> Self {
        HttpRequest {
            method,
            path,
            body: body.map(|value| value.to_string()),
        }
    }
}

/// What one line of a server's event stream tells the driver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnEvent {
    /// The server is blocked on a permission decision and will not proceed
    /// until this exact request is answered.
    PermissionRequest(PermissionAsk),
    /// Assistant text to accumulate.
    Text(String),
    /// The turn ended, however it ended (completed, failed, or cancelled).
    Finished,
    /// A line that means nothing to the driver.
    Ignored,
}

/// A permission decision the server is waiting on, addressed the way that
/// server addresses it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionAsk {
    /// The request's own id, as it appears in the answer's route or body.
    pub id: String,
    /// The session the request belongs to, when the event names one — opencode
    /// routes the answer through it, so a request for another session must not
    /// be answered on this one's path.
    pub session: Option<String>,
    /// The whole request payload, for a server (crush) that wants it echoed
    /// back rather than referenced by id.
    pub payload: Value,
}

/// The subset of [`ControlShape`] whose turn is submitted over HTTP.
///
/// A separate type rather than a `ControlShape` plus a runtime check: every
/// function here needs a route table, and the shapes that have none (the stdio
/// protocols, Claude's own stdin) would otherwise have to be handled — and
/// mishandled — at each one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpShape {
    /// `opencode serve` on loopback TCP.
    Opencode,
    /// `crush server` on a unix socket.
    Crush,
}

impl HttpShape {
    /// The HTTP shape `shape` names, or `None` when its turn is not submitted
    /// over HTTP.
    #[must_use]
    pub fn of(shape: ControlShape) -> Option<Self> {
        match shape {
            ControlShape::OpencodeHttp => Some(HttpShape::Opencode),
            ControlShape::CrushHttp => Some(HttpShape::Crush),
            _ => None,
        }
    }

    /// The mechanism this shape is declared as, for the report's `mechanism`.
    #[must_use]
    pub fn shape(self) -> ControlShape {
        match self {
            HttpShape::Opencode => ControlShape::OpencodeHttp,
            HttpShape::Crush => ControlShape::CrushHttp,
        }
    }
}

/// A harness-supplied id, checked before it becomes part of a request path.
///
/// The ids come from another program's JSON, so they are external input: one
/// carrying `/`, `?` or `..` would silently retarget a later request at a route
/// oneharness never meant to call — including the interrupt, whose whole job is
/// to reach one specific turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceId(String);

impl ResourceId {
    /// Accept `raw` as an id usable in a path, or say why it cannot be one.
    pub fn new(raw: &str) -> Result<Self, String> {
        if raw.is_empty() || raw.len() > 128 {
            return Err(format!(
                "`{raw}` is not a usable id (empty or over 128 characters)"
            ));
        }
        if !raw
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        {
            return Err(format!(
                "`{raw}` carries characters no id may put in a path"
            ));
        }
        // All dots (`.`, `..`) clears the character check yet still resolves as
        // a traversal — the retargeting this type exists to refuse.
        if raw.chars().all(|c| c == '.') {
            return Err(format!("`{raw}` traverses rather than names a resource"));
        }
        Ok(ResourceId(raw.to_string()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The ids one HTTP turn is addressed by, as that server addresses it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnAddress {
    /// Crush addresses everything through a workspace; opencode has none.
    pub workspace: Option<ResourceId>,
    pub session: ResourceId,
    /// The client identity crush requires on every route (see the module note).
    pub client: Option<String>,
}

impl TurnAddress {
    fn query(&self) -> String {
        match &self.client {
            Some(client) => format!("?client_id={client}"),
            None => String::new(),
        }
    }

    fn workspace_path(&self) -> String {
        match &self.workspace {
            Some(workspace) => format!("/v1/workspaces/{}", workspace.as_str()),
            None => String::new(),
        }
    }
}

/// Whether this run's posture means "act without asking" — the same rule the
/// stdio dialogue applies, so an HTTP turn and a protocol turn answer a
/// permission request the same way.
#[must_use]
pub fn permits_action(mode: PermissionMode) -> bool {
    matches!(
        mode,
        PermissionMode::Bypass | PermissionMode::Auto | PermissionMode::Edit
    )
}

/// The request that creates the turn's own session (opencode), or the workspace
/// everything else hangs off (crush).
#[must_use]
pub fn open_request(shape: HttpShape, cwd: &str, client: Option<&str>) -> HttpRequest {
    match shape {
        HttpShape::Opencode => {
            HttpRequest::new("POST", "/api/session".to_string(), Some(json!({})))
        }
        // The workspace is where crush's `client_id` travels in the BODY.
        HttpShape::Crush => HttpRequest::new(
            "POST",
            "/v1/workspaces".to_string(),
            Some(json!({"client_id": client.unwrap_or_default(), "path": cwd})),
        ),
    }
}

/// Crush's session-create request, which hangs off the workspace the open
/// request produced. Opencode's open request already made its session.
#[must_use]
pub fn session_request(
    shape: HttpShape,
    workspace: &ResourceId,
    client: &str,
) -> Option<HttpRequest> {
    match shape {
        HttpShape::Opencode => None,
        // Sessions are created on the WORKSPACE; `/agent/sessions` is where an
        // EXISTING one is addressed, and creating there answers `404 page not
        // found`.
        HttpShape::Crush => Some(HttpRequest::new(
            "POST",
            format!(
                "/v1/workspaces/{}/sessions?client_id={client}",
                workspace.as_str()
            ),
            Some(json!({"title": "oneharness"})),
        )),
    }
}

/// The id in a create response, wherever that server puts it. Never guessed: an
/// unrecognized shape yields `None` so the run fails loudly rather than
/// addressing an id nobody returned.
#[must_use]
pub fn parse_id(body: &str) -> Option<ResourceId> {
    let value: Value = serde_json::from_str(body).ok()?;
    // Opencode wraps every answer in a `{"data": …}` envelope; crush does not.
    let object = value.get("data").unwrap_or(&value);
    let raw = object.get("id").and_then(Value::as_str)?;
    ResourceId::new(raw).ok()
}

/// The request that submits the prompt and starts the turn.
#[must_use]
pub fn prompt_request(shape: HttpShape, address: &TurnAddress, prompt: &str) -> HttpRequest {
    match shape {
        HttpShape::Opencode => HttpRequest::new(
            "POST",
            format!("/api/session/{}/prompt", address.session.as_str()),
            Some(json!({"prompt": {"text": prompt}})),
        ),
        // The prompt goes to the workspace's agent with the session in the
        // BODY: `/agent/sessions/{sid}` is a GET-only resource and answers
        // `405 Method Not Allowed`. It returns 202 with the turn running in the
        // background, so completion is read off the event stream.
        HttpShape::Crush => HttpRequest::new(
            "POST",
            format!("{}/agent{}", address.workspace_path(), address.query()),
            Some(json!({"prompt": prompt, "session_id": address.session.as_str()})),
        ),
    }
}

/// The request that aborts the in-flight turn — the whole point of the
/// mechanism.
#[must_use]
pub fn interrupt_request(shape: HttpShape, address: &TurnAddress) -> HttpRequest {
    match shape {
        HttpShape::Opencode => HttpRequest::new(
            "POST",
            format!("/api/session/{}/interrupt", address.session.as_str()),
            Some(json!({})),
        ),
        HttpShape::Crush => HttpRequest::new(
            "POST",
            format!(
                "{}/agent/sessions/{}/cancel{}",
                address.workspace_path(),
                address.session.as_str(),
                address.query()
            ),
            Some(json!({})),
        ),
    }
}

/// The blanket "stop asking" request a permissive run makes once, where the
/// server has one. Crush's `permissions/skip` is its `--yolo`; opencode has no
/// equivalent route, so it answers each request as it arrives.
#[must_use]
pub fn skip_permissions_request(
    shape: HttpShape,
    address: &TurnAddress,
    allow: bool,
) -> Option<HttpRequest> {
    match shape {
        HttpShape::Opencode => None,
        HttpShape::Crush => allow.then(|| {
            HttpRequest::new(
                "POST",
                format!(
                    "{}/permissions/skip{}",
                    address.workspace_path(),
                    address.query()
                ),
                Some(json!({"skip": true})),
            )
        }),
    }
}

/// The request that answers one permission ask, or `None` when this server has
/// no per-request answer route.
#[must_use]
pub fn permission_reply_request(
    shape: HttpShape,
    address: &TurnAddress,
    ask: &PermissionAsk,
    allow: bool,
) -> Option<HttpRequest> {
    match shape {
        HttpShape::Opencode => {
            let session = ask.session.as_deref().unwrap_or(address.session.as_str());
            let session = ResourceId::new(session).ok()?;
            let request = ResourceId::new(&ask.id).ok()?;
            Some(HttpRequest::new(
                "POST",
                format!(
                    "/api/session/{}/permission/{}/reply",
                    session.as_str(),
                    request.as_str()
                ),
                // `once` rather than `always`: a grant this run makes must not
                // outlive it into the next one.
                Some(json!({"reply": if allow { "once" } else { "reject" }})),
            ))
        }
        HttpShape::Crush => Some(HttpRequest::new(
            "POST",
            format!(
                "{}/permissions/grant{}",
                address.workspace_path(),
                address.query()
            ),
            Some(json!({
                "action": if allow { "allow" } else { "deny" },
                "permission": ask.payload,
            })),
        )),
    }
}

/// The event stream to follow for this turn.
#[must_use]
pub fn event_stream_request(shape: HttpShape, address: &TurnAddress) -> HttpRequest {
    match shape {
        HttpShape::Opencode => HttpRequest::new("GET", "/api/event".to_string(), None),
        HttpShape::Crush => HttpRequest::new(
            "GET",
            format!("{}/events{}", address.workspace_path(), address.query()),
            None,
        ),
    }
}

/// What one `data:` payload from the event stream means.
#[must_use]
pub fn classify_event(shape: HttpShape, payload: &str) -> TurnEvent {
    let Ok(value) = serde_json::from_str::<Value>(payload) else {
        return TurnEvent::Ignored;
    };
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match shape {
        HttpShape::Opencode => classify_opencode(kind, &value),
        HttpShape::Crush => classify_crush(kind, &value),
    }
}

fn classify_opencode(kind: &str, value: &Value) -> TurnEvent {
    let data = value.get("data").unwrap_or(&Value::Null).clone();
    match kind {
        // A permission ask carries its own id and the session to answer on.
        k if k.starts_with("permission.") || k == "permission" => {
            let id = data
                .get("id")
                .or_else(|| data.get("requestID"))
                .and_then(Value::as_str);
            match id {
                Some(id) => TurnEvent::PermissionRequest(PermissionAsk {
                    id: id.to_string(),
                    session: data
                        .get("sessionID")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    payload: data,
                }),
                None => TurnEvent::Ignored,
            }
        }
        "session.idle" => TurnEvent::Finished,
        "session.next.text.ended" => data
            .get("text")
            .and_then(Value::as_str)
            .map_or(TurnEvent::Ignored, |text| TurnEvent::Text(text.to_string())),
        _ => TurnEvent::Ignored,
    }
}

fn classify_crush(kind: &str, value: &Value) -> TurnEvent {
    // Crush nests the interesting object two payloads deep:
    // `{"type":K,"payload":{"type":"created","payload":{…}}}`.
    let inner = value
        .get("payload")
        .and_then(|p| p.get("payload"))
        .cloned()
        .unwrap_or(Value::Null);
    match kind {
        "permission_request" => match inner.get("id").and_then(Value::as_str) {
            Some(id) => TurnEvent::PermissionRequest(PermissionAsk {
                id: id.to_string(),
                session: inner
                    .get("session_id")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                payload: inner,
            }),
            None => TurnEvent::Ignored,
        },
        // Emitted however the run ended — completed, errored, or cancelled.
        "run_complete" => TurnEvent::Finished,
        _ => TurnEvent::Ignored,
    }
}

/// A response's head, as far as a reader must understand it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResponseHead {
    pub status: u16,
    /// Where the body starts in the buffer the head was parsed from.
    pub body_at: usize,
    /// Whether the body arrives in `Transfer-Encoding: chunked` framing. Both
    /// servers use it, and a reader that skips de-chunking reports a JSON
    /// decode error where the harness in fact answered correctly.
    pub chunked: bool,
}

/// Parse the head out of `raw`, or `None` while it is still incomplete.
#[must_use]
pub fn parse_head(raw: &[u8]) -> Option<ResponseHead> {
    let end = raw.windows(4).position(|w| w == b"\r\n\r\n")? + 4;
    let head = String::from_utf8_lossy(&raw[..end]);
    let status = head
        .lines()
        .next()?
        .split_whitespace()
        .nth(1)?
        .parse::<u16>()
        .ok()?;
    let chunked = head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked");
    Some(ResponseHead {
        status,
        body_at: end,
        chunked,
    })
}

/// Reassembles a `Transfer-Encoding: chunked` body as its bytes arrive.
#[derive(Debug, Default)]
pub struct ChunkedDecoder {
    pending: Vec<u8>,
    /// Whether any chunk has been decoded yet. Once one has, an unreadable
    /// size header is an incomplete arrival to wait on — never a body to hand
    /// back raw, which would splice framing bytes into the caller's data.
    decoded_any: bool,
}

impl ChunkedDecoder {
    /// Feed `bytes`, returning whatever is now completely decoded. Incomplete
    /// framing is retained for the next call rather than guessed at.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<u8> {
        self.pending.extend_from_slice(bytes);
        let mut out = Vec::new();
        loop {
            // A chunk's terminating CRLF can arrive in a later read than its
            // body, so it is skipped here rather than after the body: leaving
            // it in front of the next size header parses as an empty size and
            // ends the stream early.
            while self
                .pending
                .first()
                .is_some_and(|b| matches!(b, b'\r' | b'\n'))
            {
                self.pending.remove(0);
            }
            let Some(line_end) = self
                .pending
                .windows(2)
                .position(|w| w == b"\r\n")
                .map(|at| at + 2)
            else {
                return out;
            };
            let header = String::from_utf8_lossy(&self.pending[..line_end - 2]).to_string();
            let size =
                usize::from_str_radix(header.split(';').next().unwrap_or_default().trim(), 16);
            let Ok(size) = size else {
                if self.decoded_any {
                    return out;
                }
                // Not chunk framing after all: hand back what is there rather
                // than dropping a body a server sent unframed.
                out.extend_from_slice(&self.pending);
                self.pending.clear();
                return out;
            };
            if size == 0 {
                self.pending.clear();
                return out;
            }
            if self.pending.len() < line_end + size {
                return out;
            }
            out.extend_from_slice(&self.pending[line_end..line_end + size]);
            self.pending.drain(..line_end + size);
            self.decoded_any = true;
        }
    }
}

/// Splits a server-sent-events body into whole `data:` payloads as they arrive.
#[derive(Debug, Default)]
pub struct SseAccumulator {
    pending: String,
}

impl SseAccumulator {
    /// Feed decoded body bytes, returning every complete `data:` payload in
    /// them. A partial line is held back, never emitted half-parsed.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<String> {
        self.pending.push_str(&String::from_utf8_lossy(bytes));
        let mut out = Vec::new();
        while let Some(at) = self.pending.find('\n') {
            let line: String = self.pending.drain(..=at).collect();
            if let Some(data) = line.trim().strip_prefix("data:") {
                let data = data.trim();
                if !data.is_empty() {
                    out.push(data.to_string());
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crush_address() -> TurnAddress {
        TurnAddress {
            workspace: Some(ResourceId::new("ws-1").unwrap()),
            session: ResourceId::new("ses-1").unwrap(),
            client: Some("client-9".to_string()),
        }
    }

    fn opencode_address() -> TurnAddress {
        TurnAddress {
            workspace: None,
            session: ResourceId::new("ses_abc").unwrap(),
            client: None,
        }
    }

    #[test]
    fn an_id_that_could_retarget_a_request_is_refused() {
        // Every id here comes out of another program's JSON and goes straight
        // into a path — including the interrupt's.
        for bad in ["", "..", ".", "a/b", "a?b", "a b", &"x".repeat(129)] {
            assert!(ResourceId::new(bad).is_err(), "accepted `{bad}`");
        }
        assert_eq!(
            ResourceId::new("ses_01d2.a-b").unwrap().as_str(),
            "ses_01d2.a-b"
        );
    }

    #[test]
    fn crush_carries_its_client_id_in_the_body_then_the_query() {
        // The one detail that answers a bare `{"message":"invalid client_id"}`
        // when it is got wrong.
        let open = open_request(HttpShape::Crush, "/work", Some("client-9"));
        assert_eq!(open.path, "/v1/workspaces");
        let body: Value = serde_json::from_str(open.body.as_ref().unwrap()).unwrap();
        assert_eq!(body["client_id"], "client-9");
        assert_eq!(body["path"], "/work");

        let address = crush_address();
        for request in [
            prompt_request(HttpShape::Crush, &address, "hi"),
            interrupt_request(HttpShape::Crush, &address),
            event_stream_request(HttpShape::Crush, &address),
            skip_permissions_request(HttpShape::Crush, &address, true).unwrap(),
        ] {
            assert!(
                request.path.contains("client_id=client-9"),
                "{} lost the client id",
                request.path
            );
        }
    }

    #[test]
    fn each_shape_addresses_its_own_routes() {
        let crush = crush_address();
        assert_eq!(
            prompt_request(HttpShape::Crush, &crush, "hi").path,
            "/v1/workspaces/ws-1/agent?client_id=client-9"
        );
        assert_eq!(
            interrupt_request(HttpShape::Crush, &crush).path,
            "/v1/workspaces/ws-1/agent/sessions/ses-1/cancel?client_id=client-9"
        );
        // The session is created on the workspace, not under `/agent`.
        let session = session_request(
            HttpShape::Crush,
            crush.workspace.as_ref().unwrap(),
            "client-9",
        );
        assert_eq!(
            session.unwrap().path,
            "/v1/workspaces/ws-1/sessions?client_id=client-9"
        );

        let opencode = opencode_address();
        assert_eq!(
            prompt_request(HttpShape::Opencode, &opencode, "hi").path,
            "/api/session/ses_abc/prompt"
        );
        assert_eq!(
            interrupt_request(HttpShape::Opencode, &opencode).path,
            "/api/session/ses_abc/interrupt"
        );
        // Opencode's open request already creates the session.
        assert!(session_request(
            HttpShape::Opencode,
            &ResourceId::new("unused").unwrap(),
            "c"
        )
        .is_none());
    }

    #[test]
    fn the_prompt_rides_each_servers_own_field() {
        let opencode: Value = serde_json::from_str(
            prompt_request(HttpShape::Opencode, &opencode_address(), "do it")
                .body
                .as_ref()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(opencode["prompt"]["text"], "do it");

        let crush: Value = serde_json::from_str(
            prompt_request(HttpShape::Crush, &crush_address(), "do it")
                .body
                .as_ref()
                .unwrap(),
        )
        .unwrap();
        // A missing `prompt` or `session_id` is a 500 naming the field.
        assert_eq!(crush["prompt"], "do it");
        assert_eq!(crush["session_id"], "ses-1");
    }

    #[test]
    fn a_create_response_yields_its_id_through_either_envelope() {
        assert_eq!(
            parse_id(r#"{"data":{"id":"ses_01"}}"#).unwrap().as_str(),
            "ses_01"
        );
        assert_eq!(parse_id(r#"{"id":"ws-2"}"#).unwrap().as_str(), "ws-2");
        // Never fabricated, and never an id that could retarget a path.
        assert!(parse_id(r#"{"data":{}}"#).is_none());
        assert!(parse_id("not json").is_none());
        assert!(parse_id(r#"{"id":"../other"}"#).is_none());
    }

    #[test]
    fn a_permission_ask_is_recognized_and_answered_on_its_own_session() {
        // Without this the turn never starts work at all, so every downstream
        // assertion would pass vacuously.
        let event = classify_event(
            HttpShape::Opencode,
            r#"{"type":"permission.requested","data":{"id":"per_1","sessionID":"ses_other"}}"#,
        );
        let TurnEvent::PermissionRequest(ask) = event else {
            panic!("opencode permission ask not recognized");
        };
        let reply =
            permission_reply_request(HttpShape::Opencode, &opencode_address(), &ask, true).unwrap();
        assert_eq!(reply.path, "/api/session/ses_other/permission/per_1/reply");
        let body: Value = serde_json::from_str(reply.body.as_ref().unwrap()).unwrap();
        assert_eq!(body["reply"], "once");
        let denied =
            permission_reply_request(HttpShape::Opencode, &opencode_address(), &ask, false)
                .unwrap();
        let body: Value = serde_json::from_str(denied.body.as_ref().unwrap()).unwrap();
        assert_eq!(body["reply"], "reject");

        let event = classify_event(
            HttpShape::Crush,
            r#"{"type":"permission_request","payload":{"type":"created","payload":{"id":"p9","session_id":"s1","tool_name":"bash"}}}"#,
        );
        let TurnEvent::PermissionRequest(ask) = event else {
            panic!("crush permission ask not recognized");
        };
        let reply =
            permission_reply_request(HttpShape::Crush, &crush_address(), &ask, true).unwrap();
        assert_eq!(
            reply.path,
            "/v1/workspaces/ws-1/permissions/grant?client_id=client-9"
        );
        let body: Value = serde_json::from_str(reply.body.as_ref().unwrap()).unwrap();
        assert_eq!(body["action"], "allow");
        // Crush wants the request echoed back, not referenced by id.
        assert_eq!(body["permission"]["tool_name"], "bash");
    }

    #[test]
    fn the_end_of_a_turn_is_read_from_each_streams_own_document() {
        assert_eq!(
            classify_event(HttpShape::Opencode, r#"{"type":"session.idle","data":{}}"#),
            TurnEvent::Finished
        );
        assert_eq!(
            classify_event(
                HttpShape::Crush,
                r#"{"type":"run_complete","payload":{"type":"updated","payload":{"cancelled":true}}}"#
            ),
            TurnEvent::Finished
        );
        // Everything else is noise, including a line that is not JSON at all.
        for noise in [
            r#"{"type":"plugin.added","data":{}}"#,
            r#"{"type":"session","payload":{}}"#,
            "not json",
        ] {
            assert!(matches!(
                classify_event(HttpShape::Opencode, noise),
                TurnEvent::Ignored | TurnEvent::Text(_)
            ));
        }
    }

    #[test]
    fn only_a_permissive_run_skips_the_asking() {
        assert!(permits_action(PermissionMode::Bypass));
        assert!(!permits_action(PermissionMode::Default));
        // Crush can be told once; opencode has no such route and answers each.
        let address = crush_address();
        assert!(skip_permissions_request(HttpShape::Crush, &address, true).is_some());
        assert!(skip_permissions_request(HttpShape::Crush, &address, false).is_none());
        assert!(skip_permissions_request(HttpShape::Opencode, &address, true).is_none());
    }

    #[test]
    fn a_chunked_body_is_reassembled_across_arbitrary_arrival_boundaries() {
        // The framing both servers answer with. A reader that skipped this
        // reported a JSON decode error where the harness answered correctly.
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n";
        let head = parse_head(raw).unwrap();
        assert_eq!(head.status, 200);
        assert!(head.chunked);

        let wire = b"5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
        let mut whole = ChunkedDecoder::default();
        assert_eq!(whole.feed(wire), b"hello world".to_vec());

        // Split at every offset: the decoder must never emit a partial chunk.
        for split in 1..wire.len() {
            let mut decoder = ChunkedDecoder::default();
            let mut out = decoder.feed(&wire[..split]);
            out.extend(decoder.feed(&wire[split..]));
            assert_eq!(out, b"hello world".to_vec(), "split at {split}");
        }
    }

    #[test]
    fn an_unframed_body_survives_a_decoder_that_expected_chunks() {
        // A server that answered without the framing must not lose its body.
        let mut decoder = ChunkedDecoder::default();
        assert_eq!(
            decoder.feed(b"{\"id\":\"x\"}\r\n"),
            b"{\"id\":\"x\"}\r\n".to_vec()
        );
    }

    #[test]
    fn a_head_is_only_parsed_once_it_is_whole() {
        assert!(parse_head(b"HTTP/1.1 200 OK\r\n").is_none());
        let head = parse_head(b"HTTP/1.1 204 No Content\r\n\r\n").unwrap();
        assert_eq!(head.status, 204);
        assert!(!head.chunked);
        assert_eq!(head.body_at, 27);
        assert!(parse_head(b"garbage\r\n\r\n").is_none());
    }

    #[test]
    fn sse_payloads_are_emitted_only_once_their_line_is_complete() {
        let mut sse = SseAccumulator::default();
        assert!(sse.feed(b"data: {\"type\":\"a\"}").is_empty());
        assert_eq!(sse.feed(b"}\n"), vec!["{\"type\":\"a\"}}".to_string()]);
        assert_eq!(
            sse.feed(b": comment\n\ndata: one\ndata: two\n"),
            vec!["one".to_string(), "two".to_string()]
        );
    }

    #[test]
    fn only_the_http_mechanisms_have_an_http_shape() {
        assert_eq!(
            HttpShape::of(ControlShape::OpencodeHttp),
            Some(HttpShape::Opencode)
        );
        assert_eq!(
            HttpShape::of(ControlShape::CrushHttp),
            Some(HttpShape::Crush)
        );
        assert_eq!(HttpShape::of(ControlShape::AcpCancel), None);
        assert_eq!(HttpShape::of(ControlShape::ClaudeControlRequest), None);
        assert_eq!(HttpShape::Crush.shape(), ControlShape::CrushHttp);
        assert_eq!(HttpShape::Opencode.shape(), ControlShape::OpencodeHttp);
    }
}
