//! Out-of-band turn control: the I/O half.
//!
//! Two sides of one unix socket:
//!
//! * [`ControlListener`] — opened by `oneharness run --control` for the run's
//!   lifetime. It owns the socket file (0600, removed on drop, including on a
//!   panic) and serves one newline-terminated request per connection against a
//!   [`ControlHandle`], which knows how to reach the harness mechanism.
//! * [`send`] — used by the separate `oneharness interrupt` process. It is a
//!   different process from the run; that is the whole reason this is a socket
//!   and not a flag.
//!
//! Unix only. `std` has no unix-socket type on Windows, so the command layer
//! refuses `--control` there loudly rather than opening nothing and reporting
//! success.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::domain::control::{
    interrupt_frame, ControlEvent, ControlReason, ControlRequest, ControlResponse, ControlShape,
    ControlVerb,
};
use crate::domain::dialogue::Dialogue;
use crate::domain::http::HttpShape;
use crate::domain::usage::UtcInstant;
use crate::io::http_turn::HttpTurn;

/// How long a client waits for a run to answer before giving up. Generous
/// enough for a busy run, short enough that a wedged peer is not a hang.
///
/// Unix-gated with the socket code it bounds: on Windows there is no socket to
/// wait on, and [`imp::send`] refuses before it would ever have one.
#[cfg(unix)]
const CLIENT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// The most one frame may be — in either direction — before it is refused
/// unread.
///
/// A control frame is a single small JSON object, so this is orders of
/// magnitude of headroom. It exists because both readers are waiting for a
/// newline the *peer* controls: without a bound, a peer that writes without
/// ever terminating its line grows this process's buffer for as long as it
/// keeps writing. It binds the answer as well as the request — the client is
/// the supervisor's process, and whatever is listening on that socket path is
/// no more trusted by it than a caller is by the run.
///
/// Unix-gated with the two readers it bounds, for the same reason as
/// [`CLIENT_TIMEOUT`]: neither reader is compiled on a platform with no unix
/// socket to read from.
#[cfg(unix)]
const MAX_FRAME_BYTES: u64 = 64 * 1024;

/// The single clock read this module makes, kept here so
/// [`crate::domain::control`] stays pure. A pre-1970 clock falls back to the
/// epoch rather than panicking — control is a side channel and never takes a
/// run down.
fn now() -> UtcInstant {
    UtcInstant::from_epoch(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0),
    )
}

/// The live end of a stdin-borne control mechanism.
///
/// A control-enabled run parks the child's stdin here instead of dropping it
/// after the prompt write, so the listener thread can push an interrupt frame
/// into the *same* process the turn is running in. The state machine is what
/// makes `no_active_turn` honest: before the prompt is written and after the
/// turn's terminal document arrives, there is nothing to interrupt.
enum TurnState {
    /// No turn in flight (before the prompt, or after the turn ended).
    Idle,
    /// A turn is running; the child's stdin is held open for control frames.
    Active(std::process::ChildStdin),
}

/// What drives — and therefore what interrupts — this run's turn.
///
/// Exactly one applies, fixed from the [`ControlShape`] before the turn starts:
/// [`Dialogue::new`] answers `Some` only for the two protocol shapes, which are
/// disjoint from the ones [`HttpShape::of`] recognizes. Modelling them as one
/// value rather than a field each is what makes "a dialogue *and* an HTTP turn"
/// unspellable instead of merely unreachable.
enum Backend {
    /// Control rides the harness's ordinary run: nothing to negotiate, and an
    /// interrupt is a frame written to the child's own stdin.
    Stdin,
    /// A JSON-RPC conversation on the child's stdin drives the turn, and is the
    /// only thing that knows the ids an interrupt has to address.
    Dialogue(Dialogue),
    /// The turn was submitted to a control server, so an interrupt is one more
    /// request against the same session — there is no stdin in this path at
    /// all. `Some` only between the turn opening and ending, which is exactly
    /// the window in which there is something to abort.
    Http(Option<HttpTurn>),
}

/// Shared, thread-safe access to a run's control mechanism plus the audit trail
/// of everything it served (which the report echoes).
///
/// Every lock here recovers from poisoning (`into_inner`) rather than
/// propagating a panic: control is a side channel, and a worker that panicked
/// mid-turn must not also take down the run whose results are the deliverable.
/// The guarded state is a stdin handle and an append-only log, neither of which
/// a panic can leave half-updated in a way a later write would misread.
pub struct ControlHandle {
    shape: ControlShape,
    state: Mutex<TurnState>,
    events: Mutex<Vec<ControlEvent>>,
    requests: Mutex<u64>,
    /// The mechanism driving the turn. Locked *before* `state` everywhere, so
    /// the two paths that touch both (serving an interrupt, advancing on a
    /// line) can never deadlock against each other.
    backend: Mutex<Backend>,
}

impl ControlHandle {
    #[must_use]
    pub fn new(shape: ControlShape) -> Self {
        Self::with_dialogue(shape, None)
    }

    /// A handle whose turn is driven by `dialogue` (a server-backed mechanism).
    #[must_use]
    pub fn with_dialogue(shape: ControlShape, dialogue: Option<Dialogue>) -> Self {
        let backend = match dialogue {
            Some(dialogue) => Backend::Dialogue(dialogue),
            None if HttpShape::of(shape).is_some() => Backend::Http(None),
            None => Backend::Stdin,
        };
        ControlHandle {
            shape,
            state: Mutex::new(TurnState::Idle),
            events: Mutex::new(Vec::new()),
            requests: Mutex::new(0),
            backend: Mutex::new(backend),
        }
    }

    /// Bind the live HTTP turn this handle interrupts. Called once the session
    /// exists on the control server — before that there is no turn to address,
    /// and `interrupt` correctly answers `no_active_turn`.
    pub fn begin_http_turn(&self, turn: HttpTurn) {
        *self.backend() = Backend::Http(Some(turn));
    }

    /// Release the HTTP turn: it is over, so a later interrupt is
    /// `no_active_turn` rather than a request against a finished session.
    pub fn end_http_turn(&self) {
        *self.backend() = Backend::Http(None);
    }

    /// The frames that open the turn: a dialogue's handshake, or the prompt
    /// frame for a mechanism that rides the harness's ordinary run.
    pub fn open_frames(&self, prompt: &str) -> Vec<String> {
        match &mut *self.backend() {
            Backend::Dialogue(dialogue) => dialogue.open(),
            Backend::Stdin | Backend::Http(_) => {
                crate::domain::control::prompt_frame(self.shape, prompt)
                    .into_iter()
                    .collect()
            }
        }
    }

    /// Feed one stdout line to the dialogue, writing whatever it answers with.
    /// Returns `true` when the turn is over.
    ///
    /// A write failure is not fatal here: the server has gone, which its own
    /// exit and output will report far more usefully than a panic would.
    pub fn advance(&self, line: &str) -> bool {
        let step = {
            match &mut *self.backend() {
                Backend::Dialogue(dialogue) => dialogue.on_line(line),
                // No conversation to advance: the harness's own end-of-turn
                // document is the only signal, and there is nothing to write
                // back.
                Backend::Stdin | Backend::Http(_) => {
                    return crate::domain::control::is_turn_terminal(self.shape, line);
                }
            }
        };
        if let crate::domain::dialogue::DialogueStep::Send(frames) = &step {
            for frame in frames {
                if let Err(err) = self.write_line(frame) {
                    eprintln!("oneharness: warning: could not write a control frame: {err}");
                    return true;
                }
            }
        }
        step == crate::domain::dialogue::DialogueStep::Terminal
    }

    /// The harness's native session id, when the dialogue captured one.
    #[must_use]
    pub fn session_id(&self) -> Option<String> {
        match &*self.backend() {
            Backend::Dialogue(dialogue) => dialogue.session_id().map(str::to_string),
            Backend::Stdin | Backend::Http(_) => None,
        }
    }

    /// The assistant text the dialogue accumulated, with how it was obtained.
    /// `None` when the turn produced none — never fabricated.
    #[must_use]
    pub fn text(&self) -> Option<(String, &'static str)> {
        match &*self.backend() {
            Backend::Dialogue(dialogue) => {
                dialogue.text().map(|text| (text, dialogue.text_source()))
            }
            Backend::Stdin | Backend::Http(_) => None,
        }
    }

    /// Whether this handle drives its turn over a JSON-RPC dialogue on the
    /// child's stdin, in which case the runner tears the server down once the
    /// turn ends instead of waiting for it to exit on its own.
    ///
    /// Narrower than [`ControlShape::drives_turn`] on purpose: the HTTP
    /// mechanisms drive their turns too, but not through any child of this
    /// process — nothing about a spawned run applies to them.
    #[must_use]
    pub fn drives_turn_over_stdin(&self) -> bool {
        matches!(&*self.backend(), Backend::Dialogue(_))
    }

    fn backend(&self) -> std::sync::MutexGuard<'_, Backend> {
        self.backend
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[must_use]
    pub fn shape(&self) -> ControlShape {
        self.shape
    }

    /// Park the child's stdin and mark a turn in flight.
    pub fn begin_turn(&self, stdin: std::process::ChildStdin) {
        *self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = TurnState::Active(stdin);
    }

    /// End the turn and close the child's stdin (dropping it signals EOF, which
    /// is how a control-enabled child — whose stdin was deliberately held open
    /// — learns it may exit).
    pub fn end_turn(&self) {
        *self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = TurnState::Idle;
    }

    /// Write one raw line into the live turn's stdin, e.g. the prompt frame.
    /// Errors when no turn is active or the child closed its end.
    pub fn write_line(&self, line: &str) -> io::Result<()> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match &mut *state {
            TurnState::Active(stdin) => {
                use std::io::Write;
                stdin.write_all(line.as_bytes())?;
                stdin.flush()
            }
            TurnState::Idle => Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "no active turn to write to",
            )),
        }
    }

    /// Serve one request, recording it for the report either way.
    pub fn serve(&self, request: ControlRequest) -> ControlResponse {
        let response = match request.verb() {
            ControlVerb::Interrupt => self.interrupt(),
        };
        self.events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(response.record(request.verb(), now()));
        response
    }

    fn interrupt(&self) -> ControlResponse {
        /// How the live backend delivers an abort, decided under the backend
        /// lock and carried out after it is released: an HTTP abort is a
        /// network request and a frame is a write to the child, and neither
        /// should be held against the thread advancing the turn.
        enum Delivery {
            /// One more request against the same session — there is no stdin in
            /// this path at all; the server is not even this process's child.
            Request(HttpTurn),
            /// Frames onto the live turn's stdin.
            Frames(Vec<String>),
        }
        let no_active_turn = || {
            ControlResponse::refused(
                "the run is alive but no turn is in flight",
                ControlReason::NoActiveTurn,
            )
        };
        let delivery = match &mut *self.backend() {
            Backend::Http(turn) => match turn.clone() {
                Some(turn) => Delivery::Request(turn),
                None => return no_active_turn(),
            },
            // A server-backed mechanism addresses the live thread/session by
            // the ids the dialogue captured, so it alone knows whether there is
            // a turn to abort at all.
            Backend::Dialogue(dialogue) => match dialogue.interrupt() {
                Some(frames) => Delivery::Frames(frames),
                None => return no_active_turn(),
            },
            Backend::Stdin => match interrupt_frame(self.shape, &self.next_request_id()) {
                Some(frame) => Delivery::Frames(vec![frame]),
                None => {
                    return ControlResponse::refused(
                        format!(
                            "the `{}` control mechanism is not wired for stdin-borne interrupts",
                            self.shape.as_str()
                        ),
                        ControlReason::Unsupported,
                    )
                }
            },
        };
        let frames = match delivery {
            Delivery::Request(turn) => {
                return match turn.interrupt() {
                    Ok(()) => ControlResponse::served(self.shape),
                    Err(err) => ControlResponse::refused(
                        format!("could not deliver the interrupt to the control server: {err}"),
                        ControlReason::NotRunning,
                    ),
                }
            }
            Delivery::Frames(frames) => frames,
        };
        for frame in &frames {
            match self.write_line(frame) {
                Ok(()) => {}
                Err(err) if err.kind() == io::ErrorKind::NotConnected => return no_active_turn(),
                Err(err) => {
                    return ControlResponse::refused(
                        format!("could not deliver the interrupt to the harness: {err}"),
                        ControlReason::NotRunning,
                    )
                }
            }
        }
        ControlResponse::served(self.shape)
    }

    fn next_request_id(&self) -> String {
        let mut counter = self
            .requests
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *counter += 1;
        format!("oh-control-{counter}")
    }

    /// Every request served so far, for the run report's `control` block.
    #[must_use]
    pub fn events(&self) -> Vec<ControlEvent> {
        self.events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

/// A bound control socket, serving requests on a background thread until it is
/// dropped.
///
/// Dropping it removes the socket file — the run's contract that the address
/// disappears when the run does. A dispatch killed with `SIGKILL` cannot run
/// `Drop`, so a stale socket can survive; a client then gets `ECONNREFUSED` and
/// reports `not_running`, which is the same answer as a missing file.
pub struct ControlListener {
    path: PathBuf,
    handle: Arc<ControlHandle>,
    #[cfg(unix)]
    shutdown: Option<std::sync::mpsc::Sender<()>>,
    #[cfg(unix)]
    worker: Option<std::thread::JoinHandle<()>>,
}

impl ControlListener {
    /// The socket this run is listening on.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The mechanism handle the run writes prompts and turn boundaries through.
    #[must_use]
    pub fn handle(&self) -> Arc<ControlHandle> {
        Arc::clone(&self.handle)
    }

    /// The same handle borrowed, for a caller that outlives the listener.
    #[must_use]
    pub fn handle_ref(&self) -> &ControlHandle {
        &self.handle
    }
}

#[cfg(unix)]
mod imp {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::{UnixListener, UnixStream};

    /// Bind the control socket at `path` (creating its directory) and start
    /// serving. A pre-existing file is removed first only when nothing is
    /// listening on it, so two concurrent runs of the same session name cannot
    /// silently steal each other's address.
    pub fn bind(
        path: &Path,
        shape: ControlShape,
        dialogue: Option<Dialogue>,
    ) -> io::Result<ControlListener> {
        // Canonicalize the directory before binding: the socket path is an
        // address handed to a *different* process, and a relative one resolves
        // differently depending on where that process runs.
        let path = &match path.parent() {
            Some(parent) => {
                std::fs::create_dir_all(parent)?;
                std::fs::canonicalize(parent)?.join(path.file_name().ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "the control socket path names no file",
                    )
                })?)
            }
            None => path.to_path_buf(),
        };
        if let Some(parent) = path.parent() {
            // The directory holds addressable levers over running agents, so
            // owner-only is a requirement, not a preference: failing to narrow it
            // must abort the run rather than leave a wider default in place.
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
        }
        if path.exists() {
            if UnixStream::connect(path).is_ok() {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "another run is already listening on this session's control socket",
                ));
            }
            std::fs::remove_file(path)?;
        }
        let listener = UnixListener::bind(path)?;
        // 0600: the socket is a lever over a running agent, so only its owner
        // may address it.
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;

        let handle = Arc::new(ControlHandle::with_dialogue(shape, dialogue));
        let worker_handle = Arc::clone(&handle);
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let worker = std::thread::spawn(move || serve_loop(listener, worker_handle, &rx));

        Ok(ControlListener {
            path: path.to_path_buf(),
            handle,
            shutdown: Some(tx),
            worker: Some(worker),
        })
    }

    /// Accept connections until asked to stop. Non-blocking accept plus a short
    /// sleep keeps the thread responsive to shutdown without a second socket.
    fn serve_loop(
        listener: UnixListener,
        handle: Arc<ControlHandle>,
        shutdown: &std::sync::mpsc::Receiver<()>,
    ) {
        loop {
            match listener.accept() {
                Ok((stream, _)) => serve_connection(stream, &handle),
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                    match shutdown.recv_timeout(std::time::Duration::from_millis(25)) {
                        Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    }
                }
                // A failed accept is one client's problem, never the run's:
                // control is a side channel and must not take results down.
                Err(_) => return,
            }
        }
    }

    /// One request, one response, connection closed — the whole protocol.
    ///
    /// `pub(super)` so the tests can drive it over a connection they put in the
    /// state a BSD accept leaves one in (see the blocking reset below), which is
    /// otherwise unreachable from Linux.
    pub(super) fn serve_connection(stream: UnixStream, handle: &ControlHandle) {
        use std::io::{BufRead, BufReader, Read, Write};
        // Explicitly blocking, because the accept that produced this stream is
        // not portable about it: the listener is non-blocking (so the accept
        // loop can notice a shutdown), and a BSD accept — macOS's — hands back a
        // socket that INHERITED that flag, while Linux's does not. Left
        // inherited, a read that has to wait for the rest of a frame answers
        // `WouldBlock` instead of waiting, which the reader below reports as a
        // run that could not be read from — an interrupt silently lost on one
        // platform. The timeouts are what bound the wait; this is what makes
        // there be one.
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(CLIENT_TIMEOUT));
        let _ = stream.set_write_timeout(Some(CLIENT_TIMEOUT));
        // Bounded before it is accumulated: the newline that ends a request is
        // the peer's to send, so an unbounded read is the peer deciding how
        // much of the run's memory this connection costs. One byte past the
        // bound is enough to know the frame is not one this version parses.
        let mut reader = BufReader::new(match stream.try_clone() {
            Ok(clone) => clone,
            Err(_) => return,
        })
        .take(MAX_FRAME_BYTES + 1);
        let mut line = String::new();
        let response = match reader.read_line(&mut line) {
            Ok(read) if read as u64 > MAX_FRAME_BYTES => ControlResponse::refused(
                format!("the control request is longer than {MAX_FRAME_BYTES} bytes"),
                ControlReason::Unsupported,
            ),
            Ok(_) => match crate::domain::control::parse_request(&line) {
                Ok(request) => handle.serve(request),
                // A frame this version cannot parse is refused loudly rather
                // than being mistaken for an interrupt.
                Err(err) => ControlResponse::refused(err, ControlReason::Unsupported),
            },
            Err(err) => ControlResponse::refused(
                format!("could not read the control request: {err}"),
                ControlReason::NotRunning,
            ),
        };
        let mut stream = stream;
        let _ = stream.write_all(response.to_line().as_bytes());
        let _ = stream.flush();
    }

    /// Send one request to a run listening at `path` and read its answer.
    /// A missing socket, or one nothing is listening on, is `not_running` —
    /// exactly what a supervisor needs to distinguish from a refusal.
    pub fn send(path: &Path, request: ControlRequest) -> ControlResponse {
        use std::io::{BufRead, BufReader, Read, Write};
        let mut stream = match UnixStream::connect(path) {
            Ok(stream) => stream,
            Err(err) => {
                return ControlResponse::refused(
                    format!(
                        "no run is listening on `{}` ({err}). Suggestion: start the run with `--control --session <NAME>`",
                        path.display()
                    ),
                    ControlReason::NotRunning,
                )
            }
        };
        let _ = stream.set_read_timeout(Some(CLIENT_TIMEOUT));
        let _ = stream.set_write_timeout(Some(CLIENT_TIMEOUT));
        let mut line = match serde_json::to_string(&request) {
            Ok(line) => line,
            Err(err) => {
                return ControlResponse::refused(
                    format!("could not encode the control request: {err}"),
                    ControlReason::NotRunning,
                )
            }
        };
        line.push('\n');
        if let Err(err) = stream
            .write_all(line.as_bytes())
            .and_then(|()| stream.flush())
        {
            return ControlResponse::refused(
                format!("could not send the control request: {err}"),
                ControlReason::NotRunning,
            );
        }
        let mut reply = String::new();
        // Bounded exactly like the request the run reads: the answer arrives
        // from whatever is listening on that path, and the newline ending it is
        // that peer's to withhold.
        match BufReader::new(&stream)
            .take(MAX_FRAME_BYTES + 1)
            .read_line(&mut reply)
        {
            Ok(0) => ControlResponse::refused(
                "the run closed the control connection without answering",
                ControlReason::NotRunning,
            ),
            Ok(read) if read as u64 > MAX_FRAME_BYTES => ControlResponse::refused(
                format!("the control answer is longer than {MAX_FRAME_BYTES} bytes"),
                ControlReason::NotRunning,
            ),
            Ok(_) => serde_json::from_str(reply.trim()).unwrap_or_else(|err| {
                ControlResponse::refused(
                    format!("the run sent an unreadable control response: {err}"),
                    ControlReason::NotRunning,
                )
            }),
            Err(err) => ControlResponse::refused(
                format!("could not read the control response: {err}"),
                ControlReason::NotRunning,
            ),
        }
    }

    pub fn shutdown(listener: &mut ControlListener) {
        if let Some(tx) = listener.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(worker) = listener.worker.take() {
            let _ = worker.join();
        }
        let _ = std::fs::remove_file(&listener.path);
    }
}

#[cfg(not(unix))]
mod imp {
    use super::*;

    pub fn bind(
        _path: &Path,
        _shape: ControlShape,
        _dialogue: Option<Dialogue>,
    ) -> io::Result<ControlListener> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "unix domain sockets are unavailable on this platform",
        ))
    }

    pub fn send(path: &Path, _request: ControlRequest) -> ControlResponse {
        ControlResponse::refused(
            format!(
                "control sockets are unavailable on this platform (`{}`)",
                path.display()
            ),
            ControlReason::NotRunning,
        )
    }

    pub fn shutdown(_listener: &mut ControlListener) {}
}

/// Bind the run's control socket at `path`, serving `shape`'s mechanism.
///
/// `dialogue` is the protocol conversation that drives the turn for a
/// server-backed mechanism; `None` for one that rides the harness's ordinary
/// run (Claude Code).
pub fn bind(
    path: &Path,
    shape: ControlShape,
    dialogue: Option<Dialogue>,
) -> io::Result<ControlListener> {
    imp::bind(path, shape, dialogue)
}

/// Send one control request to the run listening at `path`.
#[must_use]
pub fn send(path: &Path, request: ControlRequest) -> ControlResponse {
    imp::send(path, request)
}

/// Whether this platform can open a control socket at all.
#[must_use]
pub fn supported() -> bool {
    cfg!(unix)
}

impl Drop for ControlListener {
    fn drop(&mut self) {
        imp::shutdown(self);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::domain::control::socket_path;

    /// A private directory for one test's socket.
    ///
    /// Rooted at `/tmp` rather than `std::env::temp_dir()` because this path
    /// becomes a socket *address*, not just a place to put files:
    /// `sockaddr_un.sun_path` holds 104 bytes on macOS against Linux's 108, and
    /// macOS's per-user `$TMPDIR` is a `/var/folders/…` path that [`bind`]
    /// canonicalizes to `/private/var/folders/…` before binding — spending over
    /// half the budget on the root alone, so the address overruns `sun_path`
    /// there while fitting comfortably on Linux.
    /// Resolved, because a bound socket path IS resolved: [`bind`] canonicalizes
    /// the directory before binding (the address is handed to another process),
    /// so on macOS — where `/tmp` is a symlink to `/private/tmp` — a listener
    /// asked for `/tmp/…` reports `/private/tmp/…`. Rooting the tests at the
    /// unresolved path still binds and still interrupts, but no longer matches
    /// the address the listener names.
    fn temp_dir(tag: &str) -> PathBuf {
        let root = std::fs::canonicalize("/tmp").expect("/tmp must exist to root a control socket");
        let dir = root.join(format!(
            "oh-control-{}-{}-{:?}",
            tag,
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn socket_is_created_owner_only_and_removed_on_drop() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("lifecycle");
        let path = socket_path(&dir, "flow");
        {
            let listener = bind(&path, ControlShape::ClaudeControlRequest, None).unwrap();
            assert!(path.exists(), "socket should exist while the run is alive");
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "socket must be owner-only");
            assert_eq!(listener.path(), path);
        }
        assert!(!path.exists(), "socket must be removed when the run exits");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn interrupt_without_an_active_turn_is_refused_with_no_active_turn() {
        let dir = temp_dir("noturn");
        let path = socket_path(&dir, "idle");
        let _listener = bind(&path, ControlShape::ClaudeControlRequest, None).unwrap();
        let response = send(&path, ControlRequest::interrupt());
        assert!(!response.is_ok());
        assert_eq!(response.reason(), Some(ControlReason::NoActiveTurn));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn interrupt_against_a_missing_socket_is_not_running() {
        let dir = temp_dir("missing");
        let path = socket_path(&dir, "gone");
        let response = send(&path, ControlRequest::interrupt());
        assert!(!response.is_ok());
        assert_eq!(response.reason(), Some(ControlReason::NotRunning));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_stale_socket_file_is_reclaimed_by_the_next_run() {
        let dir = temp_dir("stale");
        let path = socket_path(&dir, "stale");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"").unwrap();
        let listener = bind(&path, ControlShape::ClaudeControlRequest, None).unwrap();
        assert_eq!(listener.path(), path);
        // And a live listener refuses to be displaced.
        let conflict = bind(&path, ControlShape::ClaudeControlRequest, None);
        assert!(conflict.is_err(), "a live socket must not be stolen");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_served_interrupt_is_recorded_and_reaches_the_child_stdin() {
        use std::io::Read;
        let dir = temp_dir("served");
        let path = socket_path(&dir, "live");
        let listener = bind(&path, ControlShape::ClaudeControlRequest, None).unwrap();
        let handle = listener.handle();

        // A real child process standing in for the harness: it echoes whatever
        // reaches its stdin, so the assertion is on delivered bytes, not on an
        // internal flag.
        let mut child = std::process::Command::new("cat")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        handle.begin_turn(child.stdin.take().unwrap());

        let response = send(&path, ControlRequest::interrupt());
        assert!(response.is_ok(), "{response:?}");
        assert_eq!(
            response.mechanism(),
            Some(ControlShape::ClaudeControlRequest)
        );

        handle.end_turn();
        let mut delivered = String::new();
        child
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut delivered)
            .unwrap();
        child.wait().ok();
        let frame: serde_json::Value = serde_json::from_str(delivered.trim()).unwrap();
        assert_eq!(frame["type"], "control_request");
        assert_eq!(frame["request"]["subtype"], "interrupt");

        let events = handle.events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].verb(), ControlVerb::Interrupt);
        assert!(events[0].is_served());
        assert!(!events[0].at().as_str().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_frame_that_never_ends_is_refused_at_its_bound_rather_than_accumulated() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixStream;
        // A request is one small JSON object. A peer that writes without ever
        // sending the newline is not sending one, so the reader stops at the
        // bound instead of growing a buffer for as long as the peer keeps
        // typing — and answers, rather than holding the connection until its
        // read timeout expires.
        let dir = temp_dir("unbounded");
        let path = socket_path(&dir, "flood");
        let listener = bind(&path, ControlShape::ClaudeControlRequest, None).unwrap();
        let mut stream = UnixStream::connect(&path).unwrap();
        stream
            .set_write_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let started = std::time::Instant::now();
        // Past the bound, with no newline anywhere in it. The write may be cut
        // short by the refusal that closes the connection, which is the point.
        let _ = stream.write_all(&vec![b'x'; 4 * MAX_FRAME_BYTES as usize]);
        let mut reply = String::new();
        BufReader::new(&stream).read_line(&mut reply).unwrap();
        let response: ControlResponse = serde_json::from_str(reply.trim()).unwrap();
        assert!(!response.is_ok(), "{response:?}");
        assert_eq!(response.reason(), Some(ControlReason::Unsupported));
        assert!(
            started.elapsed() < CLIENT_TIMEOUT,
            "the reader waited out its timeout instead of stopping at the bound"
        );
        assert!(listener.handle().events().is_empty());

        // And the listener is still serving: one flooding client must not cost
        // the run its control lever.
        drop(stream);
        let response = send(&path, ControlRequest::interrupt());
        assert_eq!(response.reason(), Some(ControlReason::NoActiveTurn));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_answer_that_never_ends_is_refused_at_its_bound_too() {
        use std::io::Write;
        use std::os::unix::net::UnixListener;
        // The other direction: `oneharness interrupt` is a separate process
        // reading whatever is listening on that path. It waits for a newline
        // that peer controls, so the answer is bounded exactly like the request
        // — otherwise a wedged or hostile listener decides how much memory the
        // supervisor's process spends before anything is parsed.
        let dir = temp_dir("flood-reply");
        let path = socket_path(&dir, "loud");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let listener = UnixListener::bind(&path).unwrap();
        let written = Arc::new(Mutex::new(0usize));
        let counted = Arc::clone(&written);
        let flooder = std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            // Never a newline, and it keeps going until the reader hangs up:
            // the client has to stop on its own terms. Capped so a regression
            // is a failed assertion rather than a test that runs out of memory.
            let blob = vec![b'y'; 8 * 1024];
            for _ in 0..4096 {
                if stream.write_all(&blob).is_err() {
                    return;
                }
                *counted.lock().unwrap_or_else(|e| e.into_inner()) += blob.len();
            }
        });

        let started = std::time::Instant::now();
        let response = send(&path, ControlRequest::interrupt());
        assert!(!response.is_ok(), "{response:?}");
        assert_eq!(response.reason(), Some(ControlReason::NotRunning));
        // Refused *as oversized* rather than parsed and found unreadable: the
        // second answer is what a reader that swallowed the whole flood gives.
        assert!(
            response.to_line().contains("longer than"),
            "the answer was accumulated instead of bounded: {}",
            response.to_line()
        );
        assert!(
            started.elapsed() < CLIENT_TIMEOUT,
            "the client waited out its timeout instead of stopping at the bound"
        );
        let _ = flooder.join();
        // The peer was cut off near the bound rather than drained to its cap.
        let written = *written.lock().unwrap_or_else(|e| e.into_inner());
        assert!(
            (written as u64) < 8 * MAX_FRAME_BYTES,
            "the client read {written} bytes of an unterminated answer"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_malformed_frame_is_refused_without_interrupting() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixStream;
        let dir = temp_dir("malformed");
        let path = socket_path(&dir, "bad");
        let listener = bind(&path, ControlShape::ClaudeControlRequest, None).unwrap();
        let mut stream = UnixStream::connect(&path).unwrap();
        stream
            .write_all(b"{\"v\":9,\"verb\":\"interrupt\"}\n")
            .unwrap();
        let mut reply = String::new();
        BufReader::new(&stream).read_line(&mut reply).unwrap();
        let response: ControlResponse = serde_json::from_str(reply.trim()).unwrap();
        assert!(!response.is_ok());
        assert_eq!(response.reason(), Some(ControlReason::Unsupported));
        assert!(listener.handle().events().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_request_that_arrives_in_pieces_is_waited_for_on_a_non_blocking_connection() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::{UnixListener, UnixStream};
        // A supervisor's frame is one write, but a socket does not promise it
        // arrives as one, so the run has to wait for the rest of a request it
        // has only part of. The accept it comes from does not promise a
        // connection that CAN wait: the listener is non-blocking, and a BSD
        // accept — macOS's — returns a socket that inherited that flag while
        // Linux's returns one that did not. So the state is set here
        // explicitly; it is the only way this platform can drive the shape
        // macOS is always in, and without it the run answers "could not read"
        // and the interrupt is lost.
        let dir = temp_dir("pieces");
        let path = socket_path(&dir, "split");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let listener = UnixListener::bind(&path).unwrap();

        let client_path = path.clone();
        // `None` for an exchange the run broke off — the supervisor's side of a
        // reader that gave up is a hang-up, not an answer, so it is reported as
        // that rather than as a panicked thread.
        let client = std::thread::spawn(move || -> Option<String> {
            let mut stream = UnixStream::connect(&client_path).ok()?;
            let mut frame = serde_json::to_string(&ControlRequest::interrupt()).ok()?;
            frame.push('\n');
            let (head, tail) = frame.split_at(frame.len() / 2);
            stream.write_all(head.as_bytes()).ok()?;
            stream.flush().ok()?;
            // Long enough that the reader cannot have the rest yet, so a
            // connection that will not wait has to answer without it.
            std::thread::sleep(std::time::Duration::from_millis(200));
            stream.write_all(tail.as_bytes()).ok()?;
            stream.flush().ok()?;
            let mut reply = String::new();
            BufReader::new(&stream).read_line(&mut reply).ok()?;
            Some(reply)
        });

        let (accepted, _) = listener.accept().unwrap();
        accepted.set_nonblocking(true).unwrap();
        let handle = ControlHandle::new(ControlShape::ClaudeControlRequest);
        imp::serve_connection(accepted, &handle);

        let reply = client.join().unwrap().expect(
            "the run broke off the exchange instead of waiting for the rest of the request",
        );
        let response: ControlResponse = serde_json::from_str(reply.trim()).unwrap();
        // The run's own verdict on a whole request — there is no turn in flight
        // — rather than the refusal a reader that gave up mid-frame reports.
        assert_eq!(
            response.reason(),
            Some(ControlReason::NoActiveTurn),
            "{}",
            response.to_line()
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
