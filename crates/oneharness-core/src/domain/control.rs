//! Out-of-band turn control: the pure half.
//!
//! A dispatched turn runs for many minutes, and until now the only lever a
//! supervisor had over one that went the wrong way was to kill the dispatch —
//! losing the whole turn and its session. `oneharness run --control` opens a
//! unix socket for the run's lifetime; a *separate* `oneharness interrupt
//! --session <NAME>` process resolves that socket and asks the run to abort the
//! current turn while keeping the session alive.
//!
//! This module holds everything with no I/O in it: the wire frames, the
//! per-harness capability shapes, the sidecar-server declaration and its pool
//! key, the harness-specific stdin frames, and the report block. The socket,
//! the process lifetimes, and the pool's disk state live in
//! [`crate::io::control`] and [`crate::io::server_pool`].

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::domain::history::sanitize_name;
use crate::domain::report::OutputFormat;

/// The control protocol version carried in every frame's `v` field.
///
/// It exists so a later verb can be added without breaking a supervisor pinned
/// to today's shape: `interrupt` is the only verb now, but some harnesses can
/// also *steer* a turn without ending it (codex `turn/steer`, opencode
/// `delivery:"steer"`), which is deliberately out of scope here.
pub const PROTOCOL_VERSION: u32 = 1;

/// The directory (under the session store) holding one socket per named run.
pub const CONTROL_DIR: &str = "control";

/// How a harness accepts an out-of-band interrupt for an in-flight turn.
///
/// Registry data on [`crate::domain::harness::HarnessSpec::control`], sourced
/// from a live interrupt against the real CLI — never guessed. `None` there
/// means `oneharness interrupt` is a loud usage error for the harness, never a
/// silent no-op: a supervisor that is told "ok" while the turn keeps running is
/// worse off than one told the lever does not exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ControlShape {
    /// Claude Code: the control frame rides the run process's **own stdin**
    /// (`-p --input-format stream-json`), so no sidecar server is needed. A
    /// `{"type":"control_request",…,"request":{"subtype":"interrupt"}}` line
    /// aborts the turn with a `result` document; the session survives and a
    /// later turn continues it with full context. Verified live: a plain user
    /// message written mid-turn is *silently dropped*, so the control frame is
    /// the only mechanism that works.
    ClaudeControlRequest,
    /// Codex: `turn/interrupt` over the `codex app-server` JSON-RPC stdio
    /// protocol, keyed on `CODEX_HOME`.
    CodexAppServer,
    /// OpenCode: `POST /api/session/{id}/interrupt` against `opencode serve`.
    OpencodeHttp,
    /// Goose and Copilot: the ACP `session/cancel` **notification** (no `id`)
    /// over their JSON-RPC stdio servers.
    AcpCancel,
    /// Crush: `POST /v1/workspaces/{id}/agent/sessions/{sid}/cancel` against
    /// `crush server`.
    CrushHttp,
}

impl ControlShape {
    /// The stable wire spelling reported as a served interrupt's `mechanism`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ControlShape::ClaudeControlRequest => "claude-control-request",
            ControlShape::CodexAppServer => "codex-app-server",
            ControlShape::OpencodeHttp => "opencode-http",
            ControlShape::AcpCancel => "acp-cancel",
            ControlShape::CrushHttp => "crush-http",
        }
    }

    /// Whether this mechanism needs a sidecar server process (everything except
    /// Claude Code, whose control rides the run's own stdin).
    #[must_use]
    pub fn needs_server(self) -> bool {
        !matches!(self, ControlShape::ClaudeControlRequest)
    }

    /// The output format a control-enabled run must use, when the mechanism
    /// pins one. Claude Code refuses the combination outright —
    /// "`--input-format=stream-json` requires output-format=stream-json" — so
    /// oneharness selects it, and turns a conflicting explicit `--output-format`
    /// into a usage error rather than a spawn failure the caller has to decode.
    /// `None` for a server-backed mechanism, whose turn does not go through the
    /// harness CLI's own output at all.
    #[must_use]
    pub fn required_format(self) -> Option<OutputFormat> {
        match self {
            ControlShape::ClaudeControlRequest => Some(OutputFormat::StreamJson),
            _ => None,
        }
    }
}

/// How a sidecar server is reached once launched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ServerTransport {
    /// JSON-RPC over the server process's stdin/stdout (codex app-server, ACP).
    Stdio,
    /// HTTP over a unix domain socket the server binds (crush).
    UnixSocket,
    /// HTTP over a loopback TCP port the server binds (opencode).
    Tcp,
}

/// A harness's sidecar server: how to launch it, what makes two launches
/// interchangeable, and how it is reached.
///
/// Declared per harness rather than special-cased, because "needs a server" is
/// the common case and Claude Code (which does not) is the exception.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerSpec {
    /// The argv appended to the harness binary to start the server (e.g.
    /// `["serve"]`, `["app-server"]`, `["acp"]`). Never includes the binary.
    pub launch: &'static [&'static str],
    /// The environment variables whose *values* make two servers different
    /// state (codex's `CODEX_HOME`). Per-turn and per-thread settings — model,
    /// effort, cwd, sandbox/approval policy, instructions — are deliberately
    /// absent: they are negotiated per thread or per turn, so widening the key
    /// with them would start a fresh ~137MB process per dispatch.
    pub key_env: &'static [&'static str],
    pub transport: ServerTransport,
}

/// The identity two dispatches must share to reuse one sidecar server.
///
/// Pure string arithmetic over the harness id, the *resolved* values of the
/// spec's `key_env` names, and any caller launch overrides — so the same key is
/// computed identically by independent processes, which is what makes the
/// on-disk pool work at all.
#[must_use]
pub fn pool_key(
    harness_id: &str,
    key_env: &[(String, Option<String>)],
    launch_overrides: &[String],
) -> String {
    let mut material = String::from(harness_id);
    // Sort so two dispatches that resolved the same variables in a different
    // order still land on one server.
    let mut env: Vec<&(String, Option<String>)> = key_env.iter().collect();
    env.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, value) in env {
        material.push('\n');
        material.push_str(name);
        material.push('=');
        material.push_str(value.as_deref().unwrap_or(""));
    }
    for arg in launch_overrides {
        material.push('\n');
        material.push_str(arg);
    }
    format!("{harness_id}-{:016x}", fnv1a64(material.as_bytes()))
}

/// FNV-1a, 64-bit. A tiny, dependency-free, *stable* digest — `DefaultHasher`
/// is explicitly not guaranteed stable across Rust releases, and a pool key
/// that changes under a toolchain upgrade would orphan every live server.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

/// The control verb a supervisor sends. Only `interrupt` exists today; the
/// frame's `v` is what leaves room for `steer` later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ControlVerb {
    Interrupt,
}

impl ControlVerb {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ControlVerb::Interrupt => "interrupt",
        }
    }
}

/// One newline-terminated request frame: `{"v":1,"verb":"interrupt"}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ControlRequest {
    pub v: u32,
    pub verb: ControlVerb,
}

impl ControlRequest {
    #[must_use]
    pub fn interrupt() -> Self {
        ControlRequest {
            v: PROTOCOL_VERSION,
            verb: ControlVerb::Interrupt,
        }
    }
}

/// Why a control request could not be served. Distinct reasons because a
/// supervisor reacts differently to each: `unsupported` is permanent for the
/// harness, `not_running` means the dispatch is gone, `no_active_turn` means
/// the run is alive but between turns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ControlReason {
    /// The harness declares no [`ControlShape`] — the lever does not exist.
    Unsupported,
    /// The run is alive but no turn is in flight right now.
    NoActiveTurn,
    /// No run is listening on the socket (never started, already exited, or a
    /// stale socket file left by an abnormally-terminated dispatch).
    NotRunning,
}

impl ControlReason {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ControlReason::Unsupported => "unsupported",
            ControlReason::NoActiveTurn => "no_active_turn",
            ControlReason::NotRunning => "not_running",
        }
    }
}

/// One newline-terminated response frame, the whole answer to one connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ControlResponse {
    pub v: u32,
    pub ok: bool,
    /// The harness mechanism that served the interrupt; present iff `ok`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mechanism: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<ControlReason>,
}

impl ControlResponse {
    /// The documented success frame, carrying the mechanism that served it.
    #[must_use]
    pub fn served(shape: ControlShape) -> Self {
        ControlResponse {
            v: PROTOCOL_VERSION,
            ok: true,
            mechanism: Some(shape.as_str().to_string()),
            error: None,
            reason: None,
        }
    }

    /// The documented failure frame.
    #[must_use]
    pub fn refused(error: impl Into<String>, reason: ControlReason) -> Self {
        ControlResponse {
            v: PROTOCOL_VERSION,
            ok: false,
            mechanism: None,
            error: Some(error.into()),
            reason: Some(reason),
        }
    }

    /// The frame as one wire line (newline-terminated), never failing: the
    /// shape is closed and always serializable.
    #[must_use]
    pub fn to_line(&self) -> String {
        let mut line = serde_json::to_string(self).unwrap_or_else(|_| {
            format!(
                "{{\"v\":{PROTOCOL_VERSION},\"ok\":false,\"error\":\"response could not be encoded\",\"reason\":\"not_running\"}}"
            )
        });
        line.push('\n');
        line
    }
}

/// Parse one request line, or say exactly why it is not a request this version
/// serves. Every rejection is loud — an unparseable frame is never treated as
/// an interrupt, and an interrupt is never treated as noise.
pub fn parse_request(line: &str) -> Result<ControlRequest, String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Err("empty control request".to_string());
    }
    let request: ControlRequest =
        serde_json::from_str(trimmed).map_err(|err| format!("malformed control request: {err}"))?;
    if request.v != PROTOCOL_VERSION {
        return Err(format!(
            "unsupported control protocol version {} (this oneharness speaks v{PROTOCOL_VERSION})",
            request.v
        ));
    }
    Ok(request)
}

/// The socket file name backing session `name` (`<sanitized name>.sock`).
#[must_use]
pub fn socket_file_name(name: &str) -> String {
    format!("{}.sock", sanitize_name(name))
}

/// The socket backing session `name` under the session store `dir`:
/// `<dir>/control/<name>.sock`. Pure path arithmetic; touches no disk.
#[must_use]
pub fn socket_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(CONTROL_DIR).join(socket_file_name(name))
}

/// The stdin line that delivers the user prompt for a control-enabled run, for
/// a harness whose control rides its own stdin. `None` for a server-backed
/// mechanism, whose prompt goes over the server protocol instead.
///
/// Claude Code's `-p --input-format stream-json` reads one JSON message per
/// line; the prompt is a `user` message rather than an argv positional, which
/// is what lets the stdin handle stay open for the control frame afterwards.
#[must_use]
pub fn prompt_frame(shape: ControlShape, prompt: &str) -> Option<String> {
    match shape {
        ControlShape::ClaudeControlRequest => {
            let value = serde_json::json!({
                "type": "user",
                "message": {
                    "role": "user",
                    "content": [{"type": "text", "text": prompt}],
                },
            });
            Some(format!("{value}\n"))
        }
        _ => None,
    }
}

/// The stdin line that aborts the in-flight turn, for a stdin-borne mechanism.
/// `None` for a server-backed mechanism.
#[must_use]
pub fn interrupt_frame(shape: ControlShape, request_id: &str) -> Option<String> {
    match shape {
        ControlShape::ClaudeControlRequest => {
            let value = serde_json::json!({
                "type": "control_request",
                "request_id": request_id,
                "request": {"subtype": "interrupt"},
            });
            Some(format!("{value}\n"))
        }
        _ => None,
    }
}

/// Whether `line` is the harness's end-of-turn document, for a stdin-borne
/// mechanism whose process stays alive waiting for more input.
///
/// This is load-bearing: with stdin held open (the very thing that makes
/// control possible) Claude Code does **not** exit when the turn ends — it
/// waits for the next message. Recognizing the terminal document is how the
/// run knows to close stdin and let the process finish, so a control-enabled
/// run still terminates on its own like an ordinary one.
#[must_use]
pub fn is_turn_terminal(shape: ControlShape, line: &str) -> bool {
    match shape {
        ControlShape::ClaudeControlRequest => {
            let trimmed = line.trim();
            if !trimmed.starts_with('{') {
                return false;
            }
            serde_json::from_str::<serde_json::Value>(trimmed)
                .ok()
                .and_then(|value| {
                    value
                        .get("type")
                        .and_then(|t| t.as_str())
                        .map(|t| t == "result")
                })
                .unwrap_or(false)
        }
        _ => false,
    }
}

/// One interrupt the run served, recorded in the report so a consumer can tell
/// an interrupted turn from one that simply ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ControlEvent {
    /// The verb requested (`interrupt`).
    pub verb: String,
    /// RFC 3339 timestamp the request was served.
    pub at: String,
    /// Whether the mechanism accepted it.
    pub ok: bool,
    /// Why it was refused; `null` when `ok`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<ControlReason>,
}

/// The run report's `control` block: where the socket lived, which mechanism
/// backed it, and every request served over the run's lifetime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ControlReport {
    /// Absolute path of the socket this run listened on.
    pub socket: String,
    /// The harness mechanism backing it.
    pub mechanism: String,
    /// Every control request served, in order.
    pub interrupts: Vec<ControlEvent>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_wire_names_are_stable() {
        assert_eq!(
            ControlShape::ClaudeControlRequest.as_str(),
            "claude-control-request"
        );
        assert_eq!(ControlShape::CodexAppServer.as_str(), "codex-app-server");
        assert_eq!(ControlShape::OpencodeHttp.as_str(), "opencode-http");
        assert_eq!(ControlShape::AcpCancel.as_str(), "acp-cancel");
        assert_eq!(ControlShape::CrushHttp.as_str(), "crush-http");
    }

    #[test]
    fn only_claude_control_rides_the_run_process() {
        assert!(!ControlShape::ClaudeControlRequest.needs_server());
        for shape in [
            ControlShape::CodexAppServer,
            ControlShape::OpencodeHttp,
            ControlShape::AcpCancel,
            ControlShape::CrushHttp,
        ] {
            assert!(shape.needs_server(), "{shape:?} should need a server");
        }
    }

    #[test]
    fn only_the_stdin_borne_mechanism_pins_an_output_format() {
        assert_eq!(
            ControlShape::ClaudeControlRequest.required_format(),
            Some(OutputFormat::StreamJson)
        );
        assert_eq!(ControlShape::CodexAppServer.required_format(), None);
    }

    #[test]
    fn request_round_trips_on_the_wire() {
        let line = serde_json::to_string(&ControlRequest::interrupt()).unwrap();
        assert_eq!(line, r#"{"v":1,"verb":"interrupt"}"#);
        assert_eq!(parse_request(&line).unwrap(), ControlRequest::interrupt());
    }

    #[test]
    fn parse_rejects_empty_malformed_and_future_versions() {
        assert!(parse_request("   ").unwrap_err().contains("empty"));
        assert!(parse_request("{ nope").unwrap_err().contains("malformed"));
        assert!(parse_request(r#"{"v":1,"verb":"steer"}"#)
            .unwrap_err()
            .contains("malformed"));
        let future = parse_request(r#"{"v":2,"verb":"interrupt"}"#).unwrap_err();
        assert!(future.contains("version 2"), "{future}");
    }

    #[test]
    fn success_frame_carries_the_mechanism_and_omits_error_fields() {
        let line = ControlResponse::served(ControlShape::ClaudeControlRequest).to_line();
        assert_eq!(
            line,
            "{\"v\":1,\"ok\":true,\"mechanism\":\"claude-control-request\"}\n"
        );
    }

    #[test]
    fn failure_frame_carries_the_reason_and_omits_the_mechanism() {
        let line = ControlResponse::refused("no run is listening", ControlReason::NotRunning);
        assert_eq!(
            line.to_line(),
            "{\"v\":1,\"ok\":false,\"error\":\"no run is listening\",\"reason\":\"not_running\"}\n"
        );
        assert_eq!(ControlReason::NoActiveTurn.as_str(), "no_active_turn");
        assert_eq!(ControlReason::Unsupported.as_str(), "unsupported");
    }

    #[test]
    fn socket_path_sanitizes_the_name_under_the_control_dir() {
        assert_eq!(
            socket_path(Path::new("/store"), "Greet Flow"),
            PathBuf::from("/store/control/greet-flow.sock")
        );
    }

    #[test]
    fn pool_key_ignores_env_order_but_not_values() {
        let a = pool_key(
            "codex",
            &[
                ("CODEX_HOME".into(), Some("/a".into())),
                ("OTHER".into(), None),
            ],
            &[],
        );
        let reordered = pool_key(
            "codex",
            &[
                ("OTHER".into(), None),
                ("CODEX_HOME".into(), Some("/a".into())),
            ],
            &[],
        );
        assert_eq!(a, reordered);

        let different = pool_key("codex", &[("CODEX_HOME".into(), Some("/b".into()))], &[]);
        assert_ne!(a, different);
        assert!(a.starts_with("codex-"), "{a}");
    }

    #[test]
    fn pool_key_separates_launch_overrides() {
        let plain = pool_key("opencode", &[], &[]);
        let overridden = pool_key("opencode", &[], &["--hostname".into(), "::1".into()]);
        assert_ne!(plain, overridden);
    }

    #[test]
    fn claude_prompt_and_interrupt_frames_match_the_live_protocol() {
        let prompt = prompt_frame(ControlShape::ClaudeControlRequest, "hi").unwrap();
        assert!(prompt.ends_with('\n'), "frames are newline-terminated");
        let prompt: serde_json::Value = serde_json::from_str(&prompt).unwrap();
        assert_eq!(prompt["type"], "user");
        assert_eq!(prompt["message"]["role"], "user");
        assert_eq!(prompt["message"]["content"][0]["type"], "text");
        assert_eq!(prompt["message"]["content"][0]["text"], "hi");

        let interrupt = interrupt_frame(ControlShape::ClaudeControlRequest, "req-1").unwrap();
        assert!(interrupt.ends_with('\n'));
        let interrupt: serde_json::Value = serde_json::from_str(&interrupt).unwrap();
        assert_eq!(interrupt["type"], "control_request");
        assert_eq!(interrupt["request_id"], "req-1");
        assert_eq!(interrupt["request"]["subtype"], "interrupt");
    }

    #[test]
    fn server_backed_shapes_have_no_stdin_frames() {
        for shape in [
            ControlShape::CodexAppServer,
            ControlShape::OpencodeHttp,
            ControlShape::AcpCancel,
            ControlShape::CrushHttp,
        ] {
            assert!(prompt_frame(shape, "hi").is_none());
            assert!(interrupt_frame(shape, "r").is_none());
            assert!(!is_turn_terminal(shape, r#"{"type":"result"}"#));
        }
    }

    #[test]
    fn turn_terminal_recognizes_only_the_result_document() {
        let shape = ControlShape::ClaudeControlRequest;
        assert!(is_turn_terminal(
            shape,
            r#"{"type":"result","subtype":"error_during_execution"}"#
        ));
        assert!(!is_turn_terminal(shape, r#"{"type":"assistant"}"#));
        assert!(!is_turn_terminal(shape, "not json"));
        assert!(!is_turn_terminal(shape, ""));
        // A `result` string appearing inside another document is not terminal.
        assert!(!is_turn_terminal(
            shape,
            r#"{"type":"user","message":{"content":"result"}}"#
        ));
    }
}
