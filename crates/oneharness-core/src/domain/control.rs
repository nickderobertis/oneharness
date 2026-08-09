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
use crate::domain::usage::UtcInstant;

/// The control protocol version carried in every frame's `v` field.
///
/// It exists so a later verb can be added without breaking a supervisor pinned
/// to today's shape: `interrupt` is the only verb now, but some harnesses can
/// also *steer* a turn without ending it (codex `turn/steer`, opencode
/// `delivery:"steer"`), which is deliberately out of scope here.
pub const PROTOCOL_VERSION: u32 = 1;

/// The directory (under the session store) holding one socket per named run.
pub const CONTROL_DIR: &str = "control";

/// A filesystem path that is known to be absolute.
///
/// Control addresses are handed to a *different process* — the socket path in
/// the report is how a supervisor finds the run — so a relative path is not a
/// smaller version of the right answer, it is a different file depending on who
/// reads it. The type is the difference between "we documented it as absolute"
/// and "it is".
// llmlint: ignore[contracts_have_one_source_or_a_drift_gate] The generated SDK
// contract is a plain string on purpose: this appears only in *output* (the
// report's `control.socket`), which oneharness produces and the SDKs validate
// leniently so a reader never rejects a report the CLI just emitted. There is
// also no portable JSON Schema for "absolute" — the rule differs between
// `/x` and `C:\x` — so a pattern would encode one platform's answer as the
// contract's.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct AbsolutePath(PathBuf);

impl AbsolutePath {
    /// Wrap `path`, or say why it cannot be an address.
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, String> {
        let path = path.into();
        if path.is_absolute() {
            Ok(AbsolutePath(path))
        } else {
            Err(format!("`{}` is not an absolute path", path.display()))
        }
    }

    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

impl std::fmt::Display for AbsolutePath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.display())
    }
}

impl<'de> Deserialize<'de> for AbsolutePath {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        AbsolutePath::new(PathBuf::from(String::deserialize(deserializer)?))
            .map_err(D::Error::custom)
    }
}

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
    /// Codex: `turn/interrupt {threadId,turnId}` over the `codex app-server`
    /// JSON-RPC stdio protocol, which oneharness spawns as the run's own child
    /// and drives the thread/turn lifecycle on — so the interrupt rides the same
    /// open stdin and nothing is shared or pooled.
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

    /// The shape a wire `mechanism` names, or `None` for one this build does not
    /// know — a supervisor reading an unrecognized mechanism gets a loud parse
    /// failure rather than a silently-degraded frame.
    #[must_use]
    pub fn from_wire(mechanism: &str) -> Option<ControlShape> {
        [
            ControlShape::ClaudeControlRequest,
            ControlShape::CodexAppServer,
            ControlShape::OpencodeHttp,
            ControlShape::AcpCancel,
            ControlShape::CrushHttp,
        ]
        .into_iter()
        .find(|shape| shape.as_str() == mechanism)
    }

    /// Whether oneharness *drives the turn itself* over this mechanism's own
    /// protocol, rather than riding the harness's ordinary headless run.
    ///
    /// True for everything except Claude Code, whose control frame rides the
    /// same `-p` run a plain dispatch uses. A driven turn negotiates its model,
    /// working directory and approvals on the wire, so none of them appear on
    /// the argv and the harness's stdout format has no bearing on its session id.
    #[must_use]
    pub fn drives_turn(self) -> bool {
        !matches!(self, ControlShape::ClaudeControlRequest)
    }

    /// Whether this mechanism needs a **shared, long-lived** server process,
    /// declared as a [`ServerSpec`] and managed by the pool.
    ///
    /// The stdio protocols do not: oneharness spawns the server as the run's own
    /// child, so the interrupt rides the same stdin the turn does and the
    /// process dies with the dispatch. Only the HTTP mechanisms need a server
    /// that outlives one turn and is worth sharing across dispatches.
    #[must_use]
    pub fn needs_pooled_server(self) -> bool {
        matches!(self, ControlShape::OpencodeHttp | ControlShape::CrushHttp)
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

/// How a sidecar server is reached once launched — the *declaration*, on
/// [`ServerSpec`]. The running server's actual address is [`ServerAddress`],
/// which carries the transport and its coordinates together so the two can
/// never disagree.
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

/// A TCP port something can actually be dialed on.
///
/// `0` is the kernel's "pick one for me" sentinel, never an address: a record
/// that lost its port and kept a `0` would send every later interrupt to
/// whatever happens to answer, which is the one outcome worse than sending none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct Port(u16);

impl Port {
    /// Accept `raw` as a dialable port, or say why it cannot be one.
    pub fn new(raw: u16) -> Result<Self, String> {
        if raw == 0 {
            Err("0 is not a dialable port".to_string())
        } else {
            Ok(Port(raw))
        }
    }

    #[must_use]
    pub fn get(self) -> u16 {
        self.0
    }
}

impl<'de> Deserialize<'de> for Port {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        Port::new(u16::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// Where a *running* sidecar server actually is.
///
/// One value rather than a transport tag beside a loose address string: a
/// stdio server has no address at all, and a TCP server's port is meaningless
/// to a unix-socket reader, so pairing them separately would make
/// "`Stdio` with a port" and "`Tcp` with a socket path" representable states a
/// reader would have to defend against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "kebab-case")]
pub enum ServerAddress {
    /// The transport is the process's own pipes; there is nothing to dial.
    Stdio,
    /// A unix domain socket the server bound.
    UnixSocket { path: AbsolutePath },
    /// A loopback TCP port the server bound. Loopback by construction: a
    /// control server is a private lever over a running agent, so an address
    /// reachable from anywhere else is not a variant worth being able to spell.
    Tcp { port: Port },
}

impl ServerAddress {
    /// The transport this address speaks.
    #[must_use]
    pub fn transport(&self) -> ServerTransport {
        match self {
            ServerAddress::Stdio => ServerTransport::Stdio,
            ServerAddress::UnixSocket { .. } => ServerTransport::UnixSocket,
            ServerAddress::Tcp { .. } => ServerTransport::Tcp,
        }
    }
}

/// A harness's sidecar server: how to launch it, what makes two launches
/// interchangeable, and how it is reached.
///
/// Declared per harness rather than special-cased. Only the two HTTP mechanisms
/// (opencode, crush) need one: their turn is submitted to a server that outlives
/// the dispatch, so it is pooled. The stdio protocols spawn their server as the
/// run's OWN child and Claude Code needs none at all, so both leave this `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerSpec {
    /// The argv appended to the harness binary to start the server (e.g.
    /// `["serve"]`, `["app-server"]`, `["acp"]`). Never includes the binary.
    pub launch: &'static [&'static str],
    /// How the chosen address reaches the server's argv, with `{address}`
    /// standing for the port or socket path (`["--port", "{address}"]`,
    /// `["-H", "unix://{address}"]`). Separate from `launch` because the
    /// address is picked per pool entry, not declared: two dispatches sharing a
    /// key must reach the SAME running server, so the address can never be part
    /// of what makes their keys differ.
    pub address_args: &'static [&'static str],
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
/// A validated value rather than a loose string: the pool joins it straight
/// into a filesystem path, so a key carrying a separator or `..` would place a
/// lease tree outside the pool root. Constructed by [`pool_key`], or parsed
/// from text with [`PoolKey::parse`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct PoolKey(String);

impl PoolKey {
    /// Accept `text` as a key, or say why it cannot be one.
    pub fn parse(text: &str) -> Result<Self, String> {
        if is_pool_key(text) {
            Ok(PoolKey(text.to_string()))
        } else {
            Err(format!(
                "`{text}` is not a valid pool key (it must be a single path segment of                  alphanumerics, `-`, `_`, or `.`, at most 128 characters)"
            ))
        }
    }

    /// The key's text, which is also its directory name under the pool root.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for PoolKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for PoolKey {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        PoolKey::parse(&String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// The key for `harness_id` under the given resolved `key_env` values and
/// launch overrides.
///
/// Pure string arithmetic — so the same key is computed identically by
/// independent processes, which is what makes the on-disk pool work at all.
/// Infallible: the digest covers every input including the id, so a caller
/// passing something unusable as a directory name gets a different key rather
/// than an error it has nothing useful to do with.
#[must_use]
pub fn pool_key(
    harness_id: &str,
    key_env: &[(String, Option<String>)],
    launch_overrides: &[String],
) -> PoolKey {
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
    // The prefix is only there to make a pool directory readable; the digest is
    // what makes the key unique, and it already covers the untouched id. So the
    // prefix is reduced to what a directory name can hold rather than refused.
    let prefix: String = harness_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(32)
        .collect();
    PoolKey(format!("{prefix}-{:016x}", fnv1a64(material.as_bytes())))
}

/// Whether `key` is shaped like a [`pool_key`] result — the single rule behind
/// [`PoolKey`].
fn is_pool_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 128
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        && !key.contains("..")
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
///
/// The version is not a field a caller sets: a `ControlRequest` that exists is
/// one this build speaks, so an unsupported `v` is rejected while parsing
/// rather than travelling as a value every reader must re-check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ControlRequest {
    v: u32,
    verb: ControlVerb,
}

impl ControlRequest {
    #[must_use]
    pub fn interrupt() -> Self {
        ControlRequest {
            v: PROTOCOL_VERSION,
            verb: ControlVerb::Interrupt,
        }
    }

    /// The verb requested.
    #[must_use]
    pub fn verb(self) -> ControlVerb {
        self.verb
    }
}

impl<'de> Deserialize<'de> for ControlRequest {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        // llmlint: ignore[invalid_states_unrepresentable] This is the literal
        // untrusted wire frame, whose arbitrary `v` is exactly what a peer can
        // send; it exists for one step so the version can be refused before a
        // `ControlRequest` — which cannot hold an unsupported version — is built.
        #[derive(Deserialize)]
        struct RequestWire {
            v: u32,
            verb: ControlVerb,
        }
        let wire = RequestWire::deserialize(deserializer)?;
        if wire.v != PROTOCOL_VERSION {
            return Err(D::Error::custom(format!(
                "unsupported control protocol version {} (this oneharness speaks v{PROTOCOL_VERSION})",
                wire.v
            )));
        }
        Ok(ControlRequest {
            v: wire.v,
            verb: wire.verb,
        })
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
///
/// A sum type, not an `ok` flag with optional companions: a frame is either a
/// success carrying the mechanism that served it, or a refusal carrying an
/// error *and* a reason. "Succeeded, and here is why it was refused" is a state
/// a supervisor would have to defend against, so it is not representable. The
/// serialized shape is the fixed wire contract either way —
/// `{"v":1,"ok":true,"mechanism":…}` / `{"v":1,"ok":false,"error":…,"reason":…}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlResponse {
    Served {
        mechanism: ControlShape,
    },
    Refused {
        error: String,
        reason: ControlReason,
    },
}

/// The serialized form of [`ControlResponse`]. Private: it exists only so serde
/// derives the exact wire frame, and its "any combination" shape is never
/// reachable from outside this module.
// llmlint: ignore[invalid_states_unrepresentable] This is the literal external
// wire frame, whose `ok` + optional-companion shape is fixed by the published
// protocol and is exactly what an untrusted peer can send. Its whole purpose is
// to hold that unvalidated form for one step so `Deserialize` can reject every
// contradictory combination; the public type it produces cannot represent one.
#[derive(Serialize, Deserialize)]
struct ResponseWire {
    v: u32,
    ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mechanism: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reason: Option<ControlReason>,
}

impl Serialize for ControlResponse {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let wire = match self {
            ControlResponse::Served { mechanism } => ResponseWire {
                v: PROTOCOL_VERSION,
                ok: true,
                mechanism: Some(mechanism.as_str().to_string()),
                error: None,
                reason: None,
            },
            ControlResponse::Refused { error, reason } => ResponseWire {
                v: PROTOCOL_VERSION,
                ok: false,
                mechanism: None,
                error: Some(error.clone()),
                reason: Some(*reason),
            },
        };
        wire.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ControlResponse {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let wire = ResponseWire::deserialize(deserializer)?;
        if wire.v != PROTOCOL_VERSION {
            return Err(D::Error::custom(format!(
                "unsupported control protocol version {} (this oneharness speaks v{PROTOCOL_VERSION})",
                wire.v
            )));
        }
        if wire.ok {
            if wire.error.is_some() || wire.reason.is_some() {
                return Err(D::Error::custom(
                    "a successful control frame must not carry an error or a refusal reason",
                ));
            }
            let mechanism = wire.mechanism.ok_or_else(|| {
                D::Error::custom("a successful control frame must carry a mechanism")
            })?;
            return ControlShape::from_wire(&mechanism)
                .map(|mechanism| ControlResponse::Served { mechanism })
                .ok_or_else(|| {
                    D::Error::custom(format!("unknown control mechanism `{mechanism}`"))
                });
        }
        if wire.mechanism.is_some() {
            return Err(D::Error::custom(
                "a refused control frame must not carry a mechanism",
            ));
        }
        Ok(ControlResponse::Refused {
            error: wire
                .error
                .ok_or_else(|| D::Error::custom("a refused control frame must carry an error"))?,
            reason: wire
                .reason
                .ok_or_else(|| D::Error::custom("a refused control frame must carry a reason"))?,
        })
    }
}

impl ControlResponse {
    /// The documented success frame, carrying the mechanism that served it.
    #[must_use]
    pub fn served(shape: ControlShape) -> Self {
        ControlResponse::Served { mechanism: shape }
    }

    /// The documented failure frame.
    #[must_use]
    pub fn refused(error: impl Into<String>, reason: ControlReason) -> Self {
        ControlResponse::Refused {
            error: error.into(),
            reason,
        }
    }

    /// Whether the request was served.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        matches!(self, ControlResponse::Served { .. })
    }

    /// The mechanism that served it, or `None` on a refusal.
    #[must_use]
    pub fn mechanism(&self) -> Option<ControlShape> {
        match self {
            ControlResponse::Served { mechanism } => Some(*mechanism),
            ControlResponse::Refused { .. } => None,
        }
    }

    /// Why it was refused, or `None` when it was served.
    #[must_use]
    pub fn reason(&self) -> Option<ControlReason> {
        match self {
            ControlResponse::Served { .. } => None,
            ControlResponse::Refused { reason, .. } => Some(*reason),
        }
    }

    /// The run-report record for having handled `verb` at `at` with this answer.
    #[must_use]
    pub fn record(&self, verb: ControlVerb, at: UtcInstant) -> ControlEvent {
        match self {
            ControlResponse::Served { .. } => ControlEvent::Served { verb, at },
            ControlResponse::Refused { reason, .. } => ControlEvent::Refused {
                verb,
                at,
                reason: *reason,
            },
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
    // Deserialization refuses an unsupported version, so a `ControlRequest`
    // that exists here is one this build speaks.
    serde_json::from_str(trimmed).map_err(|err| {
        let message = err.to_string();
        if message.contains("protocol version") {
            message
        } else {
            format!("malformed control request: {message}")
        }
    })
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

/// One control request the run handled, recorded in the report so a consumer
/// can tell an interrupted turn from one that simply ended.
///
/// A sum type discriminated by `outcome`, so a record can never say "served,
/// and here is the refusal reason": the reason exists exactly when there was a
/// refusal. Serialized flat — `{"outcome":"served","verb":…,"at":…}` /
/// `{"outcome":"refused","verb":…,"at":…,"reason":…}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ControlEvent {
    /// The mechanism accepted the request.
    Served {
        /// The verb requested.
        verb: ControlVerb,
        /// When the request was handled.
        at: UtcInstant,
    },
    /// The request was refused, with why.
    Refused {
        verb: ControlVerb,
        at: UtcInstant,
        reason: ControlReason,
    },
}

impl ControlEvent {
    /// The verb this record is about.
    #[must_use]
    pub fn verb(&self) -> ControlVerb {
        match self {
            ControlEvent::Served { verb, .. } | ControlEvent::Refused { verb, .. } => *verb,
        }
    }

    /// When it was handled.
    #[must_use]
    pub fn at(&self) -> &UtcInstant {
        match self {
            ControlEvent::Served { at, .. } | ControlEvent::Refused { at, .. } => at,
        }
    }

    /// Whether the mechanism accepted it.
    #[must_use]
    pub fn is_served(&self) -> bool {
        matches!(self, ControlEvent::Served { .. })
    }

    /// Why it was refused, or `None` when it was served.
    #[must_use]
    pub fn reason(&self) -> Option<ControlReason> {
        match self {
            ControlEvent::Served { .. } => None,
            ControlEvent::Refused { reason, .. } => Some(*reason),
        }
    }
}

/// The run report's `control` block: where the socket lived, which mechanism
/// backed it, and every request served over the run's lifetime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ControlReport {
    /// Absolute path of the socket this run listened on.
    pub socket: AbsolutePath,
    /// The harness mechanism backing it.
    pub mechanism: ControlShape,
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
    fn only_claude_control_rides_the_harnesss_ordinary_run() {
        assert!(!ControlShape::ClaudeControlRequest.drives_turn());
        for shape in [
            ControlShape::CodexAppServer,
            ControlShape::OpencodeHttp,
            ControlShape::AcpCancel,
            ControlShape::CrushHttp,
        ] {
            assert!(shape.drives_turn(), "{shape:?} should drive its own turn");
        }
    }

    #[test]
    fn only_the_http_mechanisms_need_a_pooled_server() {
        // The stdio protocols run as the dispatch's own child, so there is
        // nothing to share and nothing to leak; only a server that outlives one
        // turn is worth pooling.
        for shape in [
            ControlShape::ClaudeControlRequest,
            ControlShape::CodexAppServer,
            ControlShape::AcpCancel,
        ] {
            assert!(!shape.needs_pooled_server(), "{shape:?}");
        }
        assert!(ControlShape::OpencodeHttp.needs_pooled_server());
        assert!(ControlShape::CrushHttp.needs_pooled_server());
    }

    #[test]
    fn an_address_must_be_absolute_to_exist() {
        assert!(AbsolutePath::new("relative/x.sock").is_err());
        let ok = AbsolutePath::new("/tmp/x.sock").unwrap();
        assert_eq!(ok.to_string(), "/tmp/x.sock");
        assert_eq!(
            serde_json::to_value(&ok).unwrap(),
            serde_json::json!("/tmp/x.sock")
        );
        assert!(serde_json::from_value::<AbsolutePath>(serde_json::json!("rel")).is_err());
    }

    #[test]
    fn a_server_address_carries_its_own_transport() {
        assert_eq!(ServerAddress::Stdio.transport(), ServerTransport::Stdio);
        assert_eq!(
            ServerAddress::UnixSocket {
                path: AbsolutePath::new("/tmp/x.sock").unwrap()
            }
            .transport(),
            ServerTransport::UnixSocket
        );
        let tcp = ServerAddress::Tcp {
            port: Port::new(7777).unwrap(),
        };
        assert!(Port::new(0).is_err(), "0 is the kernel's sentinel");
        assert_eq!(tcp.transport(), ServerTransport::Tcp);
        // Round-trips with the transport as its discriminator, so a reader can
        // never see coordinates that belong to a different transport.
        let value = serde_json::to_value(&tcp).unwrap();
        assert_eq!(value["transport"], "tcp");
        assert_eq!(value["port"], 7777);
        assert_eq!(serde_json::from_value::<ServerAddress>(value).unwrap(), tcp);
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
        // The version cannot be carried past parsing: there is no way to build
        // a request that claims one this build does not speak.
        assert_eq!(ControlRequest::interrupt().verb(), ControlVerb::Interrupt);
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
    fn a_frame_that_contradicts_itself_is_refused_at_parse_time() {
        // The type cannot represent "ok, and here is the refusal reason", so a
        // frame claiming success without a mechanism (or a refusal without one)
        // fails loudly instead of decoding into a half-state.
        for bad in [
            r#"{"v":1,"ok":true}"#,
            r#"{"v":1,"ok":true,"mechanism":"made-up"}"#,
            r#"{"v":1,"ok":true,"mechanism":"acp-cancel","error":"nope"}"#,
            r#"{"v":1,"ok":true,"mechanism":"acp-cancel","reason":"not_running"}"#,
            r#"{"v":1,"ok":false,"error":"nope","reason":"not_running","mechanism":"acp-cancel"}"#,
            r#"{"v":1,"ok":false,"reason":"not_running"}"#,
            r#"{"v":1,"ok":false,"error":"nope"}"#,
            r#"{"v":2,"ok":true,"mechanism":"claude-control-request"}"#,
        ] {
            assert!(
                serde_json::from_str::<ControlResponse>(bad).is_err(),
                "should have refused {bad}"
            );
        }
        let served: ControlResponse =
            serde_json::from_str(r#"{"v":1,"ok":true,"mechanism":"acp-cancel"}"#).unwrap();
        assert_eq!(served.mechanism(), Some(ControlShape::AcpCancel));
        assert!(served.is_ok());
    }

    #[test]
    fn a_recorded_event_carries_its_reason_only_when_refused() {
        let at = UtcInstant::from_epoch(1_786_190_000);
        let served = ControlResponse::served(ControlShape::ClaudeControlRequest)
            .record(ControlVerb::Interrupt, at.clone());
        assert!(served.is_served());
        assert_eq!(served.verb(), ControlVerb::Interrupt);
        assert_eq!(served.at(), &at);
        assert_eq!(served.reason(), None);
        let value = serde_json::to_value(&served).unwrap();
        assert_eq!(value["outcome"], "served");
        assert!(value.get("reason").is_none());

        let refused = ControlResponse::refused("nope", ControlReason::NoActiveTurn)
            .record(ControlVerb::Interrupt, at);
        assert_eq!(refused.reason(), Some(ControlReason::NoActiveTurn));
        let value = serde_json::to_value(&refused).unwrap();
        assert_eq!(value["outcome"], "refused");
        assert_eq!(value["reason"], "no_active_turn");
    }

    #[test]
    fn every_shape_round_trips_through_its_wire_name() {
        for shape in [
            ControlShape::ClaudeControlRequest,
            ControlShape::CodexAppServer,
            ControlShape::OpencodeHttp,
            ControlShape::AcpCancel,
            ControlShape::CrushHttp,
        ] {
            assert_eq!(ControlShape::from_wire(shape.as_str()), Some(shape));
        }
        assert_eq!(ControlShape::from_wire("nope"), None);
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
        assert!(a.as_str().starts_with("codex-"), "{a}");
    }

    #[test]
    fn a_pool_key_is_always_a_safe_segment_whatever_the_caller_passed() {
        // The digest already covers the id, so an id that cannot be a directory
        // name yields a different key rather than a refusal (or a panic) the
        // caller could do nothing with.
        for id in ["codex", "../escape", "a/b", "", "wild name!"] {
            let key = pool_key(id, &[], &[]);
            assert!(
                PoolKey::parse(key.as_str()).is_ok(),
                "`{id}` produced an unusable key `{key}`"
            );
        }
        assert_ne!(pool_key("a/b", &[], &[]), pool_key("ab", &[], &[]));
    }

    #[test]
    fn a_pool_key_that_could_escape_the_pool_root_is_refused() {
        assert!(PoolKey::parse(pool_key("codex", &[], &[]).as_str()).is_ok());
        for bad in [
            "",
            "../escape",
            "a/b",
            "a\\b",
            "..",
            "x..y",
            &"a".repeat(129),
        ] {
            assert!(
                PoolKey::parse(bad).is_err(),
                "`{bad}` should not be a pool key"
            );
        }
        assert!(serde_json::from_value::<PoolKey>(serde_json::json!("../escape")).is_err());
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
