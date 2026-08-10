// llmlint: ignore-file[comments_earn_their_place] Contradictory rules leave no arrangement that passes both: run 20260730T015003Z-03f66 rejected usage prose duplicated from `docs/harness-usage.md`, and run 20260730T022546Z-ea300 rejected the requested deferral to that document.
//! The deterministic harness responder shared by the shipped CLI and test fixture.
//!
//! A fake harness binary the e2e tests drive via a `--bin` override, so the
//! spawn / capture / parallel / parse path is exercised hermetically and
//! cross-platform — no real CLI, no network. The responder ships as
//! `oneharness mock-harness`; the separate fixture binary remains feature-gated.
//!
//! Behavior is scripted entirely through environment variables:
//!   MOCK_STDOUT     bytes written to stdout (default: a JSON `result` doc)
//!   MOCK_STDERR     bytes written to stderr
//!   MOCK_EXIT       process exit code (default: 0)
//!   MOCK_SLEEP_MS   milliseconds to sleep before exiting (to force a timeout)
//!   MOCK_ARGV_FILE  if set, the received argv (one per line) is written here
//!   MOCK_ECHO_PWD   if set, write `PWD=<the inherited $PWD>` to stdout and exit
//!                   (used to assert that --cwd keeps $PWD consistent)
//!   MOCK_ECHO_ENV   if set to a variable NAME, write `NAME=<inherited value>`
//!                   to stdout and exit (used to assert per-harness env injection)
//!   MOCK_CAT_FILE   if set to a path, write that file's current contents to
//!                   stdout and exit — proving what a config file contained
//!                   WHILE the harness ran (the ephemeral mock-hook install,
//!                   restored afterwards, is only observable from inside).
//!   MOCK_CAT_ARG_AFTER  if set to a flag (e.g. `--settings`), find it in the
//!                   received argv and write the contents of the file named by
//!                   the NEXT argument to stdout, then exit — proving an
//!                   argv-delivered temp file existed and what it carried.
//!   MOCK_REPLY_AFTER_LINES  if set to N, read N newline-terminated request
//!                   lines from stdin, then answer with MOCK_STDOUT and exit.
//!   MOCK_REQUEST_FILE  with MOCK_REPLY_AFTER_LINES, the requests read from stdin
//!                   are written here, one per line, and reading continues for a
//!                   short grace past the Nth so a line the caller should NOT
//!                   have sent is recorded too. Answering after N lines cannot
//!                   prove WHICH lines arrived; the `usage` probes' zero-turn
//!                   property is a claim about exactly that, so a test has to
//!                   read the bodies back.
//!   MOCK_REPLY_DELAY_MS  with MOCK_REPLY_AFTER_LINES, act like a server whose
//!                   reply is asynchronous: answer this many ms after the last
//!                   request, and — because such a server shuts down on EOF with
//!                   that reply still in flight — exit WITHOUT answering if stdin
//!                   closes first. This is the codex `app-server` behavior, and
//!                   the only way a test can tell a caller that holds stdin open
//!                   for its answer from one that closes the pipe behind it.
//!                   A delay above `MAX_REPLY_DELAY_MS` is refused.
//!   MOCK_ECHO_STDIN if set, read ALL of stdin and write it verbatim to stdout,
//!                   then exit — proving a prompt delivered on the child's stdin
//!                   (the large-prompt escape hatch) actually arrived, and with
//!                   what content.
//!   MOCK_ATTEMPT_FILE  if set, a counter file: each invocation reads the prior
//!                   count, increments it, writes it back, and (when
//!                   MOCK_STDOUT_<n> is set for that 1-based attempt) emits that
//!                   instead of MOCK_STDOUT — used to script the structured-output
//!                   retry loop, where attempt 1 is invalid and a later one valid.
//!   MOCK_STREAM_DELAY_MS  if set, emit MOCK_STDOUT one line at a time, flushing
//!                   and sleeping this many ms between lines (so a streaming
//!                   consumer sees events arrive over time). On completion,
//!                   append `COMPLETE\n` to MOCK_LOG_FILE; on a failed write
//!                   (reader gone / killed mid-stream) exit WITHOUT it, so a test
//!                   can prove an early teardown by the sentinel's absence.
//!   MOCK_STREAM_CHUNK_BYTES  if set, emit MOCK_STDOUT in fixed-size byte chunks,
//!                   flushing each chunk and pausing by MOCK_STREAM_DELAY_MS
//!                   between chunks. This models providers that split one JSONL
//!                   record across multiple pipe reads.
//!   MOCK_PRESERVE_STDOUT  if `1`, bypass session-field normalization so a test
//!                   can preserve provider whitespace and framing exactly.
//!   MOCK_FAIL_IF_MODEL  if set to a model name, fail (exit 1) with a
//!                   `model not found` stderr when the received argv carries
//!                   `--model <that name>`; otherwise run normally. Lets a test
//!                   make one model in a fan-out unusable so a fallback run falls
//!                   through it to the next (the `model_not_found` classification).
//!                   MOCK_FAIL_STDERR overrides the emitted stderr (so the same
//!                   hook can simulate a rate limit instead of a missing model).
//!   MOCK_TURN_LOG   if set to a path, act like a *control-capable* harness whose
//!                   stdin is a message stream: emit MOCK_STDOUT immediately (the
//!                   in-turn transcript), then keep reading stdin, appending each
//!                   received line to the log verbatim. A line containing
//!                   `control_request` ends the turn — the log gains an
//!                   `INTERRUPTED` line and MOCK_TURN_RESULT is emitted, then a
//!                   `TURN_ENDED` line. The stream stays open afterwards, so a
//!                   message arriving next opens ANOTHER turn (which is how an
//!                   interrupt's redirection is delivered); EOF ends the process.
//!                   This is the hermetic stand-in for Claude Code's
//!                   `-p --input-format stream-json` control channel: the turn is
//!                   observably in flight (the prompt frame appears in the log),
//!                   so a test can drive a *separate* `oneharness interrupt`
//!                   process against the live run.
//!   MOCK_TURN_RESULT  with MOCK_TURN_LOG, the terminal document emitted when the
//!                   turn ends (default: a stream-json `result` document).
//!   MOCK_TURN_HOLD  with MOCK_TURN_LOG, keep the turn running after the prompt
//!                   instead of completing it, so a test can interrupt a turn
//!                   that is genuinely still in flight.
//!   MOCK_TURN_ONCE  with MOCK_TURN_LOG, exit as soon as the first turn ends
//!                   instead of reading on — the harness that is GONE by the
//!                   time a committed redirection would be written to it, so a
//!                   test can drive the run's recovery from that write failing.
//!   MOCK_ACP_LOG    if set to a path, act like an **ACP JSON-RPC server** on
//!                   stdio (the shape `copilot --acp` / `goose acp` speak):
//!                   answer `initialize` and `session/new`, hold `session/prompt`
//!                   open, ask one `session/request_permission`, and end the turn
//!                   only when a `session/cancel` NOTIFICATION arrives — reporting
//!                   `stopReason: "end_turn"`, exactly as the real harnesses do
//!                   after a genuine cancellation. Every received line is appended
//!                   to the log, so a test can assert the client answered the
//!                   permission request (without which a real turn never starts)
//!                   and that cancel carried no `id`.
//!   MOCK_CODEX_APP_SERVER_LOG  if set to a path, act like **`codex app-server`**
//!                   on stdio: answer `initialize` and `thread/start`, answer
//!                   `turn/start` with an in-progress turn (which is NOT the end
//!                   of the turn), and end it on `turn/completed` only once a
//!                   `turn/interrupt` naming the thread and turn arrives. Every
//!                   received line is appended to the log.
//!   MOCK_HTTP_CONTROL_LOG  if set to a path, act like an **opencode-shaped HTTP
//!                   control server** on the `--port` from its own argv: create a
//!                   session, block the turn on a permission request, and abort it
//!                   on the session's interrupt route. Every request line (and its
//!                   body) is appended to the log.
//!                   The `redirected-turn` fault instead models opencode's real
//!                   redirection shape, measured live: the prompt request is
//!                   HELD OPEN for the whole turn and answered with a refusal
//!                   when the interrupt aborts it, the aborted turn ends
//!                   SILENTLY (no text, no idle — the stream just stops), and
//!                   the session then takes a second prompt and runs it. A
//!                   driver that reads the aborted turn's refusal as the run's
//!                   outcome, or that waits for an end-of-turn event before
//!                   delivering the redirection, loses the message.
//!   MOCK_HTTP_CONTROL_FAULT  with MOCK_HTTP_CONTROL_LOG, break the server the way
//!                   a real one breaks, so the run's user-visible failure can be
//!                   asserted: `never-ready` exits before binding at all (nothing
//!                   ever answers), appending one `LAUNCHED never-ready` line per
//!                   launch so a relaunch is countable, `lose-port` dies on its
//!                   first launch the way a server that lost its reserved port
//!                   does and comes up healthy on the relaunch, `silent-server`
//!                   BINDS its port and stays alive but answers nothing (the
//!                   other half of "not ready": a process that is there and
//!                   quiet, which must be reported against the window and never
//!                   relaunched),
//!                   `refuse-session` answers the session-create
//!                   route `503`, and `no-session-id` answers it `200` with a body
//!                   naming no id, and `foreign-permission` asks permission for a
//!                   session this run does not own, and `close-stream` stops
//!                   the event stream mid-turn, `refuse-prompt` rejects prompt
//!                   submission, and `hang-prompt` never answers that request,
//!                   talking mid-turn with no end-of-turn event, and
//!                   `redirect-interrupt` answers the interrupt route `302`
//!                   without aborting anything. Each exits once
//!                   it has served
//!                   its fault, so nothing is left behind for the pool to reclaim.
//!                   An unrecognized value is refused rather than run as no fault.
//!   MOCK_LOG_FILE   if set, an append-only run log: each invocation appends `S\n`
//!                   when it starts (before MOCK_SLEEP_MS) and `E\n` when it ends
//!                   (after the sleep). With a sleep that exceeds spawn latency,
//!                   the interleaving reveals scheduling: concurrent calls all
//!                   write `S` before the first `E`; a barrier (one call, then the
//!                   rest) shows `S E` before any further `S`. Used to pin the
//!                   batch `speed` vs `min-tokens` wave ordering. Small single-byte
//!                   lines + O_APPEND keep cross-process writes from interleaving.
//!   MOCK_NATIVE_GRANDCHILD_MS  Unix/Windows: act like a launcher which starts a
//!                   long-lived native child. The child emits MOCK_STDOUT /
//!                   MOCK_STDERR, then ticks for this many milliseconds while
//!                   inheriting both output pipes; MOCK_TICK_FILE receives one
//!                   byte per tick when set. The launcher waits for it. On Unix
//!                   the child ignores TERM; on Windows it is a second copy of
//!                   this native binary, whose lifetime is independent of its
//!                   direct parent unless the Job Object contains it. This
//!                   reproduces the process-tree timeout boundary of npm shims.

use std::io::Write;

/// How long the request reader keeps listening for a line the caller should not
/// have sent, when a request log is being written and no reply delay sets its
/// own window. A caller writes its whole exchange up front, so an extra line is
/// already in the pipe; this only has to outlast the scheduling of one read.
const EXTRA_REQUEST_GRACE: std::time::Duration = std::time::Duration::from_millis(250);

/// The largest reply delay `MOCK_REPLY_DELAY_MS` accepts. A scripted delay only
/// has to outlast a probe's own timeout, so this cap sits far past any real use,
/// while any larger value is a typo that would otherwise overflow the `Instant`
/// deadline the delay becomes.
const MAX_REPLY_DELAY_MS: u64 = 600_000;

/// Read `wanted` newline-terminated request lines from stdin, then keep
/// listening for a window, and report whether stdin reached EOF.
///
/// Reading on a thread is what makes both of those observable at once: a caller
/// may hold stdin open for as long as it waits for an answer, so a blocking read
/// past the last request would never return, and yet the *absence* of an extra
/// line and the arrival of EOF are each something a test has to be able to see.
/// The window is `delay` when a reply delay was asked for (the caller's EOF has
/// to be noticed inside it) and otherwise a short grace when the requests are
/// being logged.
fn read_requests(wanted: usize, delay: u64, recording: bool) -> (Vec<String>, bool) {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        loop {
            let mut line = String::new();
            match std::io::BufRead::read_line(&mut stdin.lock(), &mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) => {
                    if sender
                        .send(line.trim_end_matches(['\r', '\n']).to_string())
                        .is_err()
                    {
                        return;
                    }
                }
            }
        }
    });

    let mut requests = Vec::new();
    let mut saw_eof = false;
    while requests.len() < wanted && !saw_eof {
        match receiver.recv() {
            Ok(line) => requests.push(line),
            Err(_) => saw_eof = true,
        }
    }

    let window = if delay > 0 {
        std::time::Duration::from_millis(delay)
    } else if recording {
        EXTRA_REQUEST_GRACE
    } else {
        std::time::Duration::ZERO
    };
    let deadline = std::time::Instant::now() + window;
    while !saw_eof {
        let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) else {
            break;
        };
        match receiver.recv_timeout(remaining) {
            Ok(line) => requests.push(line),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => saw_eof = true,
        }
    }
    (requests, saw_eof)
}

/// Return the value immediately following `flag`, matching the simple
/// flag/value shapes emitted by every adapter that selects an output format.
fn arg_value_after<'a>(argv: &'a [String], flag: &str) -> Option<&'a str> {
    argv.iter()
        .position(|arg| arg == flag)
        .and_then(|index| argv.get(index + 1))
        .map(String::as_str)
}

/// Remove top-level session handles from a JSON document or JSONL transcript.
/// The fake harness is shared by every adapter, so its scripted stdout must not
/// expose an id in an argv mode where the real harness would omit it.
fn without_session_fields(stdout: &str, fields: &[&str]) -> String {
    fn remove(value: &mut serde_json::Value, fields: &[&str]) {
        match value {
            serde_json::Value::Object(object) => {
                for field in fields {
                    object.remove(*field);
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    remove(value, fields);
                }
            }
            _ => {}
        }
    }

    if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(stdout.trim()) {
        remove(&mut value, fields);
        return serde_json::to_string(&value).unwrap_or_else(|_| stdout.to_string());
    }

    stdout
        .lines()
        .map(|line| {
            let Ok(mut value) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
                return line.to_string();
            };
            remove(&mut value, fields);
            serde_json::to_string(&value).unwrap_or_else(|_| line.to_string())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Make scripted session output obey the real harness's format contract. In
/// particular, Codex emits `thread_id` only under `exec --json`, and Qwen emits
/// `session_id` only in its JSON output modes. The remaining adapters are kept
/// honest here too so future session tests cannot accidentally prove a text-only
/// invocation captured an id that the real CLI would never print.
fn session_faithful_stdout(argv: &[String], stdout: String) -> String {
    let is_codex = argv.first().is_some_and(|arg| arg == "exec");
    if is_codex {
        return if argv.iter().any(|arg| arg == "--json") {
            without_session_fields(&stdout, &["session_id", "sessionID"])
        } else {
            without_session_fields(&stdout, &["session_id", "sessionID", "thread_id"])
        };
    }

    let format = arg_value_after(argv, "--output-format");
    let is_qwen = argv
        .iter()
        .any(|arg| arg == "--approval-mode" || arg == "--yolo");
    if is_qwen {
        return if matches!(format, Some("json" | "stream-json")) {
            without_session_fields(&stdout, &["sessionID", "thread_id"])
        } else {
            without_session_fields(&stdout, &["session_id", "sessionID", "thread_id"])
        };
    }

    let is_claude = argv.iter().any(|arg| arg == "--permission-mode");
    if is_claude {
        return if matches!(format, Some("json" | "stream-json")) {
            without_session_fields(&stdout, &["sessionID", "thread_id"])
        } else {
            without_session_fields(&stdout, &["session_id", "sessionID", "thread_id"])
        };
    }

    let is_cursor = argv.first().is_some_and(|arg| arg == "-p")
        && argv.iter().any(|arg| arg == "--force" || arg == "--trust")
        && argv.iter().any(|arg| arg == "--output-format");
    if is_cursor {
        return if format == Some("stream-json") {
            without_session_fields(&stdout, &["sessionID", "thread_id"])
        } else {
            without_session_fields(&stdout, &["session_id", "sessionID", "thread_id"])
        };
    }

    let opencode_format = arg_value_after(argv, "--format");
    let is_opencode = argv.first().is_some_and(|arg| arg == "run") && opencode_format.is_some();
    if is_opencode {
        return if opencode_format == Some("json") {
            without_session_fields(&stdout, &["session_id", "thread_id"])
        } else {
            without_session_fields(&stdout, &["session_id", "sessionID", "thread_id"])
        };
    }

    let cannot_emit_session = (argv.first().is_some_and(|arg| arg == "run")
        && argv
            .iter()
            .any(|arg| arg == "--with-builtin" || arg == "-q"))
        || argv.iter().any(|arg| arg == "--no-ask-user");
    if cannot_emit_session {
        return without_session_fields(&stdout, &["session_id", "sessionID", "thread_id"]);
    }

    stdout
}

#[cfg(windows)]
fn run_native_descendant() -> ! {
    let run_for = std::env::var("MOCK_NATIVE_GRANDCHILD_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(1);
    let mut tick_file = std::env::var("MOCK_TICK_FILE").ok().and_then(|path| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()
    });
    let mut tick = || {
        if let Some(file) = tick_file.as_mut() {
            let _ = file.write_all(b"x");
            let _ = file.flush();
        }
    };

    // Write the durable witness before the first stream line. A streaming
    // consumer can stop on that line and still prove this descendant existed.
    tick();
    if let Ok(text) = std::env::var("MOCK_STDOUT") {
        let _ = std::io::stdout().write_all(text.as_bytes());
        let _ = std::io::stdout().flush();
    }
    if let Ok(text) = std::env::var("MOCK_STDERR") {
        let _ = std::io::stderr().write_all(text.as_bytes());
        let _ = std::io::stderr().flush();
    }

    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(run_for);
    while std::time::Instant::now() < deadline {
        tick();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    std::process::exit(0);
}

/// How the mock control server is asked to break, parsed at the boundary.
///
/// An enum rather than the raw string: an unrecognized value read as "no fault"
/// would turn a misspelled knob into a passing happy-path run, which is the one
/// outcome a fault test must never quietly produce.
#[derive(Clone, Copy, PartialEq, Eq)]
enum HttpControlFault {
    None,
    NeverReady,
    LosePort,
    SilentServer,
    RefuseSession,
    NoSessionId,
    ForeignPermission,
    CloseStream,
    RefusePrompt,
    HangPrompt,
    RefuseEvents,
    UnreadableEvents,
    RedirectInterrupt,
    RedirectedTurn,
    RefuseRedirect,
}

impl HttpControlFault {
    fn from_env() -> Self {
        match std::env::var("MOCK_HTTP_CONTROL_FAULT")
            .unwrap_or_default()
            .as_str()
        {
            "" => HttpControlFault::None,
            "never-ready" => HttpControlFault::NeverReady,
            "lose-port" => HttpControlFault::LosePort,
            "silent-server" => HttpControlFault::SilentServer,
            "refuse-session" => HttpControlFault::RefuseSession,
            "no-session-id" => HttpControlFault::NoSessionId,
            "foreign-permission" => HttpControlFault::ForeignPermission,
            "close-stream" => HttpControlFault::CloseStream,
            "refuse-prompt" => HttpControlFault::RefusePrompt,
            "hang-prompt" => HttpControlFault::HangPrompt,
            "refuse-events" => HttpControlFault::RefuseEvents,
            "unreadable-events" => HttpControlFault::UnreadableEvents,
            "redirect-interrupt" => HttpControlFault::RedirectInterrupt,
            "redirected-turn" => HttpControlFault::RedirectedTurn,
            "refuse-redirect" => HttpControlFault::RefuseRedirect,
            other => panic!("mock harness: MOCK_HTTP_CONTROL_FAULT names no fault: `{other}`"),
        }
    }
}

/// The largest request body the mock control server will reserve room for. A
/// control request is a small JSON object; the bound exists because the number
/// that sizes the allocation arrives on the socket.
const MAX_REQUEST_BODY: usize = 1 << 20;

/// The largest request line or header line it will hold. Both end at a newline
/// the *peer* sends, so without a bound the peer chooses how much this fixture
/// accumulates before it has validated anything at all.
const MAX_HEAD_LINE: usize = 16 * 1024;

/// Read one head line into `line`, or `None` when it outgrew [`MAX_HEAD_LINE`]
/// before ending. `Some(0)` is the end of the stream.
fn read_bounded_line<R: std::io::BufRead>(reader: &mut R, line: &mut String) -> Option<usize> {
    use std::io::Read;
    match std::io::BufRead::read_line(&mut reader.take(MAX_HEAD_LINE as u64), line) {
        Ok(read) if read == MAX_HEAD_LINE && !line.ends_with('\n') => None,
        Ok(read) => Some(read),
        Err(_) => Some(0),
    }
}

/// Record that a control server launch happened and which fault it was serving,
/// so a test can count the launches a dispatch made. The line is all a fault
/// that exits before binding ever gets to say.
fn note_launch(log_path: &str, fault: &str) {
    if let Ok(mut log) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
    {
        let _ = writeln!(log, "LAUNCHED {fault}");
    }
}

/// Act like an opencode-shaped HTTP control server: the third execution model,
/// where the turn is submitted to a server rather than to a CLI.
///
/// Reproduces the two behaviors that decide whether the model works at all: the
/// server blocks on a permission request until the client answers it, and it
/// announces `session.idle` BEFORE the prompt is admitted as well as after the
/// turn ends — so a driver that reads the first one ends the run having done
/// nothing. The port comes off the argv the pool launched it with, exactly as
/// `opencode serve --port N` takes it.
fn run_http_control_server(log_path: &str) -> ! {
    use std::io::Read;
    let args: Vec<String> = std::env::args().collect();
    let fault = HttpControlFault::from_env();
    // A server the pool started and that then never listens — the shape a real
    // one takes when it dies on startup. Exiting before binding is what makes
    // the bring-up, not a route, the thing under test. The launch is logged
    // first, and it is the only thing this fault ever writes: a dispatch that
    // relaunches a server which died leaves one line per attempt, so the log
    // counts the attempts.
    if fault == HttpControlFault::NeverReady {
        note_launch(log_path, "never-ready");
        std::process::exit(0);
    }
    // A server that lost its reserved port to whoever asked the kernel next.
    // The port is reserved by binding and letting go, so the loser dies on
    // `EADDRINUSE` through no fault of its own — and only the FIRST launch
    // does: the relaunch it is owed comes up healthy, which is the whole
    // behavior under test. The log is what tells the two launches apart, since
    // nothing else survives the exit.
    let fault = if fault == HttpControlFault::LosePort {
        if std::fs::read_to_string(log_path)
            .unwrap_or_default()
            .contains("LAUNCHED lose-port")
        {
            HttpControlFault::None
        } else {
            note_launch(log_path, "lose-port");
            std::process::exit(1);
        }
    } else {
        fault
    };
    // The pool dials the port it put on the argv, so an absent or unreadable
    // one is refused rather than defaulted: binding `0` would listen on some
    // port nobody is going to connect to, which reads as a server that never
    // came up rather than as a launch that was wrong.
    // `0` is refused with the rest: it parses as a `u16` but names no address —
    // the kernel picks one, and the dispatch goes on dialing the port that is
    // written down. Same rule the real [`crate::domain::control::Port`] holds.
    let port: u16 = match args.windows(2).find(|w| w[0] == "--port") {
        Some(pair) => pair[1]
            .parse::<u16>()
            .ok()
            .filter(|port| *port != 0)
            .unwrap_or_else(|| panic!("mock harness: `--port {}` is not a dialable port", pair[1])),
        None => panic!("mock harness: the control server was launched with no --port"),
    };
    let listener = std::net::TcpListener::bind(("127.0.0.1", port))
        .expect("the mock control server could not bind its port");
    // A server that is there and says nothing. It holds the port for the whole
    // readiness window — so the address is demonstrably ITS own, not a
    // stranger's — accepts each dial and closes it without answering, then
    // exits once the caller has stopped dialing so nothing is left for the pool
    // to reclaim. Both waits are bounded; neither polls open-endedly.
    if fault == HttpControlFault::SilentServer {
        note_launch(log_path, "silent-server");
        listener
            .set_nonblocking(true)
            .expect("the mock control server could not poll its listener");
        let started = std::time::Instant::now();
        let mut last_seen = std::time::Instant::now();
        while started.elapsed() < std::time::Duration::from_secs(30)
            && last_seen.elapsed() < std::time::Duration::from_millis(1500)
        {
            match listener.accept() {
                // Closed without a byte of answer: a definite, immediate error
                // for the dialer every time, so the window is the only clock.
                Ok((socket, _)) => {
                    last_seen = std::time::Instant::now();
                    drop(socket);
                }
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(25)),
            }
        }
        std::process::exit(0);
    }
    // The approval policy this server was LAUNCHED with. opencode carries a
    // mode like `edit` in its own config environment rather than on a route, so
    // a controlled run delivers it to the server process — and the only way to
    // prove that from outside is to have the server say what it received.
    if let Ok(config) = std::env::var("OPENCODE_CONFIG_CONTENT") {
        note_launch(log_path, &format!("config {config}"));
    }
    let log = std::sync::Arc::new(std::sync::Mutex::new(
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
            .expect("the mock control server could not open its log"),
    ));
    // Shared turn state: whether the prompt was admitted and whether the turn
    // has been aborted. The event stream reads both.
    let admitted = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let aborted = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    // How many prompts this session has taken. A redirection makes a SECOND
    // one, which is the whole thing the redirect fault exercises.
    let prompts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

    for connection in listener.incoming() {
        let Ok(mut socket) = connection else { continue };
        let log = std::sync::Arc::clone(&log);
        let admitted = std::sync::Arc::clone(&admitted);
        let aborted = std::sync::Arc::clone(&aborted);
        let prompts = std::sync::Arc::clone(&prompts);
        std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(socket.try_clone().expect("clone"));
            let mut request_line = String::new();
            match read_bounded_line(&mut reader, &mut request_line) {
                Some(0) | None => {
                    // Nothing, or more of a request line than this fixture will
                    // hold before it has even seen the end of it.
                    if request_line.len() >= MAX_HEAD_LINE {
                        reply_bad_request(&mut socket);
                    }
                    return;
                }
                Some(_) => {}
            }
            let mut length = 0usize;
            loop {
                let mut header = String::new();
                match read_bounded_line(&mut reader, &mut header) {
                    Some(0) => break,
                    // A header line with no ending in it is the same hazard as
                    // an endless request line, and refused the same way.
                    None => {
                        reply_bad_request(&mut socket);
                        return;
                    }
                    Some(_) => {}
                }
                if let Some(value) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                    // A length that cannot be read is not a zero-length body:
                    // reading none of a body that is there leaves the next
                    // request's bytes in the stream.
                    let Ok(declared) = value.trim().parse::<usize>() else {
                        reply_bad_request(&mut socket);
                        return;
                    };
                    // The declaration is a number off a socket, and it is about
                    // to size an allocation. A control request is a small JSON
                    // object, so anything past the bound is refused rather than
                    // reserved — otherwise the peer, not this fixture, decides
                    // how much memory it commits and how long it then blocks
                    // waiting for a body that is never coming.
                    if declared > MAX_REQUEST_BODY {
                        reply_bad_request(&mut socket);
                        return;
                    }
                    length = declared;
                }
                if header == "\r\n" {
                    break;
                }
            }
            let mut body = vec![0u8; length];
            let _ = reader.read_exact(&mut body);
            let line = request_line.trim().to_string();
            {
                let mut file = log.lock().unwrap_or_else(|e| e.into_inner());
                let _ = writeln!(file, "{line} {}", String::from_utf8_lossy(&body));
                let _ = file.flush();
            }
            let reply = |socket: &mut std::net::TcpStream, status: &str, body: &str| {
                let _ = write!(
                    socket,
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.flush();
            };
            if line.starts_with("GET /api/event") {
                if fault == HttpControlFault::RefuseEvents {
                    reply(
                        &mut socket,
                        "503 Service Unavailable",
                        "{\"error\":\"events unavailable\"}",
                    );
                    exit_shortly();
                    return;
                }
                if fault == HttpControlFault::UnreadableEvents {
                    let _ = write!(
                        socket,
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 7\r\nContent-Length: 9\r\n\r\ndata: {{}}\n"
                    );
                    let _ = socket.flush();
                    exit_shortly();
                    return;
                }
                // The stream a driver follows. The idle BEFORE admission is the
                // trap: acting on it ends the run before any work happens.
                let send = |socket: &mut std::net::TcpStream, payload: &str| {
                    let _ = write!(socket, "data: {payload}\n\n");
                    let _ = socket.flush();
                };
                let _ = write!(
                    socket,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: keep-alive\r\n\r\n"
                );
                send(&mut socket, "{\"type\":\"session.idle\",\"data\":{}}");
                let mut asked = false;
                loop {
                    if admitted.load(std::sync::atomic::Ordering::SeqCst) && !asked {
                        asked = true;
                        send(
                            &mut socket,
                            "{\"type\":\"session.next.prompt.admitted\",\"data\":{}}",
                        );
                        // A server that stops talking mid-turn: the stream ends
                        // with no end-of-turn event on it at all.
                        if fault == HttpControlFault::CloseStream {
                            exit_shortly();
                            return;
                        }
                        // The shared stream carries every session's asks, so a
                        // faulted server asks about one this run does not own.
                        // Nothing may answer it — and the turn then ends on its
                        // own, rather than leaving the run to its timeout.
                        if fault == HttpControlFault::ForeignPermission {
                            send(
                                &mut socket,
                                "{\"type\":\"permission.requested\",\"data\":{\"id\":\"per_1\",\"sessionID\":\"ses_intruder\"}}",
                            );
                            std::thread::sleep(std::time::Duration::from_millis(400));
                            send(&mut socket, "{\"type\":\"session.idle\",\"data\":{}}");
                            exit_shortly();
                            return;
                        }
                        send(
                            &mut socket,
                            "{\"type\":\"permission.requested\",\"data\":{\"id\":\"per_1\",\"sessionID\":\"ses_mock\"}}",
                        );
                    }
                    if aborted.load(std::sync::atomic::Ordering::SeqCst) {
                        // A refused redirection ends the run through the
                        // submission, not the stream: the stream stays silent
                        // exactly as opencode's does after an abort.
                        if fault == HttpControlFault::RefuseRedirect {
                            std::thread::sleep(std::time::Duration::from_secs(30));
                            return;
                        }
                        if fault != HttpControlFault::RedirectedTurn {
                            send(
                                &mut socket,
                                "{\"type\":\"session.next.text.ended\",\"data\":{\"text\":\"stopped\"}}",
                            );
                            send(&mut socket, "{\"type\":\"session.idle\",\"data\":{}}");
                            return;
                        }
                        // Opencode's real shape, measured live: an aborted turn
                        // ends SILENTLY. No text, no idle — the stream just
                        // stops until something else happens on the session. A
                        // driver holding a redirection for an end-of-turn event
                        // would wait here until its timeout, so the served
                        // interrupt has to be what releases it.
                        while prompts.load(std::sync::atomic::Ordering::SeqCst) < 2 {
                            std::thread::sleep(std::time::Duration::from_millis(25));
                        }
                        send(
                            &mut socket,
                            "{\"type\":\"session.next.prompt.admitted\",\"data\":{}}",
                        );
                        send(
                            &mut socket,
                            "{\"type\":\"session.next.text.ended\",\"data\":{\"text\":\"redirected\"}}",
                        );
                        send(&mut socket, "{\"type\":\"session.idle\",\"data\":{}}");
                        exit_shortly();
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
            if line.starts_with("POST /api/session/") && line.contains("/interrupt") {
                // A route that answers "ask elsewhere" has not aborted
                // anything: the turn is deliberately left running, so a client
                // that read a redirect as acceptance would report a stop that
                // never happened.
                if fault == HttpControlFault::RedirectInterrupt {
                    let _ = write!(
                        socket,
                        "HTTP/1.1 302 Found\r\nLocation: /v2/session/ses_mock/interrupt\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    );
                    let _ = socket.flush();
                    exit_shortly();
                    return;
                }
                aborted.store(true, std::sync::atomic::Ordering::SeqCst);
                reply(&mut socket, "204 No Content", "");
                // The redirect fault has a second turn to serve, so the server
                // stays up until the event stream has carried it.
                if !matches!(
                    fault,
                    HttpControlFault::RedirectedTurn | HttpControlFault::RefuseRedirect
                ) {
                    exit_shortly();
                }
            } else if line.starts_with("POST /api/session/") && line.contains("/prompt") {
                match fault {
                    HttpControlFault::RefusePrompt => {
                        reply(
                            &mut socket,
                            "503 Service Unavailable",
                            "{\"error\":\"model unavailable\"}",
                        );
                        exit_shortly();
                    }
                    HttpControlFault::HangPrompt => {
                        // Keep the request blocked past the run's one-second
                        // budget, then let the fixture process exit cleanly so
                        // its coverage profile is complete.
                        std::thread::spawn(|| {
                            std::thread::sleep(std::time::Duration::from_secs(3));
                            std::process::exit(0);
                        });
                        std::thread::sleep(std::time::Duration::from_secs(120));
                    }
                    // The redirected prompt is the one refused: the first
                    // turn runs normally so there is something to interrupt,
                    // and the SECOND submission — the redirection — is what the
                    // server will not take. A run that swallowed that would
                    // report success having done none of the redirected work.
                    HttpControlFault::RefuseRedirect => {
                        let seq = prompts.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                        if seq == 1 {
                            admitted.store(true, std::sync::atomic::Ordering::SeqCst);
                            while !aborted.load(std::sync::atomic::Ordering::SeqCst) {
                                std::thread::sleep(std::time::Duration::from_millis(25));
                            }
                            reply(
                                &mut socket,
                                "409 Conflict",
                                "{\"error\":\"the turn was aborted\"}",
                            );
                        } else {
                            reply(
                                &mut socket,
                                "503 Service Unavailable",
                                "{\"error\":\"model unavailable\"}",
                            );
                            exit_shortly();
                        }
                    }
                    HttpControlFault::RedirectedTurn => {
                        let seq = prompts.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                        admitted.store(true, std::sync::atomic::Ordering::SeqCst);
                        if seq == 1 {
                            // Opencode holds the prompt request open for the
                            // whole turn and answers it with a REFUSAL when the
                            // turn is aborted. A driver that reads that refusal
                            // as the run's outcome stops before the redirection
                            // it accepted can become the next turn — the message
                            // lost by another route.
                            while !aborted.load(std::sync::atomic::Ordering::SeqCst) {
                                std::thread::sleep(std::time::Duration::from_millis(25));
                            }
                            reply(
                                &mut socket,
                                "409 Conflict",
                                "{\"error\":\"the turn was aborted\"}",
                            );
                        } else {
                            reply(&mut socket, "200 OK", "{\"data\":{\"admittedSeq\":2}}");
                        }
                    }
                    _ => {
                        prompts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        admitted.store(true, std::sync::atomic::Ordering::SeqCst);
                        reply(&mut socket, "200 OK", "{\"data\":{\"admittedSeq\":1}}");
                    }
                }
            } else if line.starts_with("POST /api/session/") && line.contains("/permission/") {
                let mut file = log.lock().unwrap_or_else(|e| e.into_inner());
                let _ = writeln!(file, "PERMISSION_ANSWERED");
                let _ = file.flush();
                reply(&mut socket, "200 OK", "{}");
            } else if line.starts_with("POST /api/session") {
                // The two ways session creation fails against a server that IS
                // up: it refuses, or it answers something with no id in it.
                // Either way the run has no turn to drive, and the server then
                // exits rather than lingering as a pooled orphan — a process
                // reclaimed with SIGTERM leaves a truncated coverage profile.
                match fault {
                    HttpControlFault::RefuseSession => {
                        reply(
                            &mut socket,
                            "503 Service Unavailable",
                            "{\"error\":\"no provider configured\"}",
                        );
                        exit_shortly();
                    }
                    HttpControlFault::NoSessionId => {
                        reply(&mut socket, "200 OK", "{\"data\":{\"kind\":\"session\"}}");
                        exit_shortly();
                    }
                    _ => reply(&mut socket, "200 OK", "{\"data\":{\"id\":\"ses_mock\"}}"),
                }
            } else {
                reply(&mut socket, "200 OK", "{}");
            }
        });
    }
    std::process::exit(0);
}

/// Refuse a request whose framing this fixture could not read. Answering keeps
/// the client from waiting out a timeout on a connection nobody will write to.
fn reply_bad_request(socket: &mut std::net::TcpStream) {
    let _ = write!(
        socket,
        "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    let _ = socket.flush();
}

/// Shut the mock control server down once it has served what it was standing up
/// for, rather than waiting to be reclaimed: the pool stops a server with
/// SIGTERM, whose default disposition skips the at-exit handlers — including the
/// one that writes this binary's coverage profile, leaving a truncated file that
/// fails the whole coverage merge. The delay lets the answer just written (and,
/// for an aborted turn, its final events) reach the client first.
fn exit_shortly() {
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(1500));
        std::process::exit(0);
    });
}

/// Act like `codex app-server`: the JSON-RPC protocol a codex turn is driven
/// over, including the fact that decides whether a controlled run works at all.
///
/// `turn/start` answers IMMEDIATELY with the new turn's id and
/// `status:"inProgress"` — a client that reads that response as the end of the
/// turn finishes in under half a second having done nothing. The turn ends only
/// on the `turn/completed` notification, which this emits once the interrupt
/// naming both the thread and the turn arrives.
fn run_codex_app_server(log_path: &str) -> ! {
    use serde_json::{json, Value};
    // Opened once, up front: a log path that cannot be written is a fixture
    // that answers correctly while recording nothing, which reads as a client
    // that never sent the frames the test is looking for.
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .expect("the mock app-server could not open its log");
    let mut append = |line: &str| {
        let _ = writeln!(log, "{line}");
        let _ = log.flush();
    };
    let mut out = std::io::stdout();
    let mut send = |value: &Value| {
        let _ = writeln!(out, "{value}");
        let _ = out.flush();
    };

    for line in std::io::BufRead::lines(std::io::stdin().lock()) {
        let Ok(line) = line else { break };
        append(&line);
        // A real frame or nothing: the fixture answers what it can parse rather
        // than scanning the text for fields that may not be there.
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let id = message.get("id").cloned().unwrap_or(Value::Null);
        match message.get("method").and_then(Value::as_str) {
            Some("initialize") => send(&json!({"jsonrpc": "2.0", "id": id, "result": {}})),
            Some("thread/start") => send(&json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {"thread": {"id": "mock-codex-thread"}},
            })),
            Some("turn/start") => {
                send(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {"turn": {"id": "mock-codex-turn", "status": "inProgress"}},
                }));
                send(&json!({
                    "jsonrpc": "2.0",
                    "method": "item/agentMessage/delta",
                    "params": {"itemId": "item_1", "delta": "still working"},
                }));
            }
            // Only an interrupt that names BOTH coordinates stops the turn:
            // the real app-server takes `{threadId, turnId}`, and a fixture
            // that ends on the method alone would pass a client that addressed
            // no particular turn.
            Some("turn/interrupt") => {
                let params = message.get("params").unwrap_or(&Value::Null);
                let names = |key: &str, expected: &str| {
                    params.get(key).and_then(Value::as_str) == Some(expected)
                };
                if !names("threadId", "mock-codex-thread") || !names("turnId", "mock-codex-turn") {
                    append("INTERRUPT_MISADDRESSED");
                    send(&json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {"code": -32602, "message": "invalid interrupt coordinates"},
                    }));
                    continue;
                }
                send(&json!({"jsonrpc": "2.0", "id": id, "result": {}}));
                send(&json!({
                    "jsonrpc": "2.0",
                    "method": "turn/completed",
                    "params": {"turnId": "mock-codex-turn"},
                }));
            }
            _ => {}
        }
    }
    std::process::exit(0);
}

/// Act like an ACP server: the protocol `copilot --acp` and `goose acp` speak,
/// including the two behaviors that decide whether a turn works at all or
/// silently never starts — a mandatory `session/request_permission`, and a
/// cancel that is a notification rather than a request.
fn run_acp_server(log_path: &str) -> ! {
    let append = |line: &str| {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
        {
            let _ = writeln!(file, "{line}");
            let _ = file.flush();
        }
    };
    let mut out = std::io::stdout();
    let mut send = |value: &str| {
        let _ = writeln!(out, "{value}");
        let _ = out.flush();
    };

    let mut prompt_id: Option<String> = None;
    let mut asked_permission = false;
    for line in std::io::BufRead::lines(std::io::stdin().lock()) {
        let Ok(line) = line else { break };
        append(&line);
        // A real frame or nothing: a fixture that scanned the text for its
        // fields would answer something that was never a JSON-RPC message.
        let Ok(message) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let id = message
            .get("id")
            .filter(|id| id.is_number())
            .map(ToString::to_string);
        let method = message
            .get("method")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        match method.as_deref() {
            Some("initialize") => send(&format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":{},\"result\":{{\"protocolVersion\":1}}}}",
                id.unwrap_or_else(|| "1".to_string())
            )),
            Some("session/new") => send(&format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":{},\"result\":{{\"sessionId\":\"mock-acp-session\"}}}}",
                id.unwrap_or_else(|| "2".to_string())
            )),
            Some("session/prompt") => {
                prompt_id = id.clone();
                // The turn does not begin until the client answers this.
                asked_permission = true;
                send(
                    "{\"jsonrpc\":\"2.0\",\"id\":9001,\"method\":\"session/request_permission\",\"params\":{\"sessionId\":\"mock-acp-session\",\"options\":[{\"optionId\":\"allow\",\"kind\":\"allow_once\"},{\"optionId\":\"deny\",\"kind\":\"reject_once\"}]}}",
                );
            }
            // A cancel carries no `id`. The turn then ends reporting a NORMAL
            // stop reason — the lie oneharness must not read as the truth.
            Some("session/cancel") => {
                send("{\"jsonrpc\":\"2.0\",\"method\":\"session/update\",\"params\":{\"sessionId\":\"mock-acp-session\",\"update\":{\"sessionUpdate\":\"agent_message_chunk\",\"content\":{\"type\":\"text\",\"text\":\"Info: Operation cancelled by user\"}}}}");
                if let Some(prompt_id) = prompt_id.take() {
                    send(&format!(
                        "{{\"jsonrpc\":\"2.0\",\"id\":{prompt_id},\"result\":{{\"stopReason\":\"end_turn\"}}}}"
                    ));
                }
                break;
            }
            _ => {
                // A permission answer arrives as a plain response; record it so
                // a test can assert the client actually sent one.
                if method.is_none() && id.is_some() && asked_permission {
                    append("PERMISSION_ANSWERED");
                }
            }
        }
    }
    std::process::exit(0);
}

/// Act like a harness whose turn is driven over an open stdin message stream and
/// can be aborted out of band. Emits the in-turn transcript, then blocks reading
/// stdin — so the turn is genuinely still running while a separate process sends
/// the interrupt — and ends on either a control frame or EOF.
fn run_controlled_turn(log_path: &str) -> ! {
    let append = |line: &str| {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
        {
            let _ = writeln!(file, "{line}");
            let _ = file.flush();
        }
    };
    let mut out = std::io::stdout();
    if let Ok(stdout) = std::env::var("MOCK_STDOUT") {
        if !stdout.is_empty() {
            let _ = writeln!(out, "{}", stdout.trim_end_matches('\n'));
            let _ = out.flush();
        }
    }
    append("TURN_STARTED");

    // A turn that completes on its own ends as soon as it has the prompt (the
    // ordinary case); MOCK_TURN_HOLD keeps it running so a test can interrupt a
    // genuinely in-flight turn.
    let hold = std::env::var_os("MOCK_TURN_HOLD").is_some();
    let mut lines = std::io::BufRead::lines(std::io::stdin().lock());
    // Turns, not one turn: the stream stays open after a turn ends, so a
    // message arriving afterwards opens the next one. That is what an interrupt
    // carrying a redirection depends on, and modelling it here is what lets a
    // hermetic test see the redirected turn actually run.
    let mut index = 0usize;
    loop {
        let mut interrupted = false;
        let mut ended = false;
        for line in lines.by_ref() {
            let Ok(line) = line else { break };
            append(&line);
            // A control frame, not a line that merely mentions one: a prompt
            // whose text says `control_request` would otherwise end the turn
            // nobody asked to stop.
            let control = serde_json::from_str::<serde_json::Value>(&line)
                .ok()
                .is_some_and(|frame| {
                    frame.get("type").and_then(serde_json::Value::as_str) == Some("control_request")
                });
            if control {
                interrupted = true;
                ended = true;
                append("INTERRUPTED");
                break;
            }
            // Only the run's own turn is held open; a redirected one ends as
            // soon as it has its message, so a test does not have to interrupt
            // twice to watch a run finish.
            if !hold || index > 0 {
                ended = true;
                break;
            }
        }
        // The stream closed rather than a turn ending: there is nothing further
        // to answer, and emitting another document would invent a turn.
        if !ended {
            break;
        }
        let once = std::env::var_os("MOCK_TURN_ONCE").is_some();
        let result = std::env::var("MOCK_TURN_RESULT").unwrap_or_else(|_| {
            format!(
                r#"{{"type":"result","subtype":"{}","session_id":"mock-session","result":"mock turn"}}"#,
                if interrupted {
                    "error_during_execution"
                } else {
                    "success"
                }
            )
        });
        let _ = writeln!(out, "{result}");
        let _ = out.flush();
        append("TURN_ENDED");
        if once {
            break;
        }
        index += 1;
    }
    std::process::exit(0);
}

pub fn run() -> ! {
    #[cfg(windows)]
    if std::env::var_os("ONEHARNESS_MOCK_NATIVE_DESCENDANT").is_some() {
        run_native_descendant();
    }

    let argv: Vec<String> = std::env::args().skip(1).collect();

    if let Ok(path) = std::env::var("MOCK_ARGV_FILE") {
        let _ = std::fs::write(path, argv.join("\n"));
    }

    // Append one line to the run log (when configured), opening fresh each time so
    // concurrent invocations share the file via the OS's append semantics.
    let log_line = |line: &str| {
        if let Ok(path) = std::env::var("MOCK_LOG_FILE") {
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = f.write_all(line.as_bytes());
                let _ = f.flush();
            }
        }
    };

    #[cfg(unix)]
    if let Ok(ms) = std::env::var("MOCK_NATIVE_GRANDCHILD_MS") {
        let count = ms
            .parse::<u64>()
            .ok()
            .map(|value| (value / 50).max(1))
            .unwrap_or(1);
        // The shell receives transcript bytes through inherited environment, not
        // interpolation, so arbitrary JSON remains data. Ignoring TERM forces
        // oneharness to exercise the group-wide KILL fallback after its grace.
        let script = r#"
            trap '' TERM
            printf '%s' "${MOCK_STDOUT:-}"
            printf '%s' "${MOCK_STDERR:-}" >&2
            i=0
            while [ "$i" -lt "$1" ]; do
                if [ -n "${MOCK_TICK_FILE:-}" ]; then
                    printf x >> "$MOCK_TICK_FILE"
                fi
                i=$((i + 1))
                sleep 0.05
            done
        "#;
        let status = std::process::Command::new("sh")
            .args(["-c", script, "oneharness-native-child", &count.to_string()])
            .status();
        std::process::exit(status.ok().and_then(|s| s.code()).unwrap_or(1));
    }

    #[cfg(windows)]
    if std::env::var_os("MOCK_NATIVE_GRANDCHILD_MS").is_some() {
        // Windows does not tie a process's lifetime to its parent. This native
        // child would therefore survive a direct launcher kill and retain the
        // inherited stdout/stderr handles; the runner's Job Object must own it.
        let status = std::env::current_exe()
            .and_then(|executable| {
                std::process::Command::new(executable)
                    .env("ONEHARNESS_MOCK_NATIVE_DESCENDANT", "1")
                    .status()
            })
            .ok();
        std::process::exit(status.and_then(|value| value.code()).unwrap_or(1));
    }

    if std::env::var_os("MOCK_ECHO_PWD").is_some() {
        let pwd = std::env::var("PWD").unwrap_or_default();
        let _ = write!(std::io::stdout(), "PWD={pwd}");
        let _ = std::io::stdout().flush();
        std::process::exit(0);
    }

    if let Ok(name) = std::env::var("MOCK_ECHO_ENV") {
        let value = std::env::var(&name).unwrap_or_default();
        let _ = write!(std::io::stdout(), "{name}={value}");
        let _ = std::io::stdout().flush();
        std::process::exit(0);
    }

    if let Ok(path) = std::env::var("MOCK_CAT_FILE") {
        let contents = std::fs::read_to_string(&path).unwrap_or_default();
        let _ = write!(std::io::stdout(), "{contents}");
        let _ = std::io::stdout().flush();
        std::process::exit(0);
    }

    // An early stdin EOF still answers, so a probe that writes fewer lines than
    // expected sees a real response rather than a hang.
    if let Ok(path) = std::env::var("MOCK_TURN_LOG") {
        run_controlled_turn(&path);
    }

    if let Ok(path) = std::env::var("MOCK_CODEX_APP_SERVER_LOG") {
        run_codex_app_server(&path);
    }
    if let Ok(path) = std::env::var("MOCK_ACP_LOG") {
        run_acp_server(&path);
    }

    if let Ok(path) = std::env::var("MOCK_HTTP_CONTROL_LOG") {
        run_http_control_server(&path);
    }

    if let Ok(count) = std::env::var("MOCK_REPLY_AFTER_LINES") {
        // Loud on a malformed value: silently reading it as 1 would let a
        // typo'd test drive the wrong exchange and still report success.
        let Ok(wanted) = count.parse::<usize>() else {
            let _ = write!(
                std::io::stderr(),
                "mock harness: MOCK_REPLY_AFTER_LINES must be a whole number of stdin lines, got `{count}`"
            );
            let _ = std::io::stderr().flush();
            std::process::exit(2);
        };
        // Bounded, not merely numeric: the delay becomes an `Instant` deadline
        // further down, and `Instant + Duration` panics rather than saturates
        // once the sum leaves the platform clock's range. Refusing the value
        // here makes a mistyped knob one legible message on every platform,
        // instead of a crash on some and a wait no run outlasts on the rest.
        let raw_delay = std::env::var("MOCK_REPLY_DELAY_MS").unwrap_or_else(|_| "0".to_string());
        let delay = match raw_delay.parse::<u64>() {
            Ok(delay) if delay <= MAX_REPLY_DELAY_MS => delay,
            _ => {
                let _ = write!(
                    std::io::stderr(),
                    "mock harness: MOCK_REPLY_DELAY_MS must be a whole number of milliseconds \
                     from 0 to {MAX_REPLY_DELAY_MS} ({} minutes), got `{raw_delay}`",
                    MAX_REPLY_DELAY_MS / 60_000
                );
                let _ = std::io::stderr().flush();
                std::process::exit(2);
            }
        };
        let record = std::env::var("MOCK_REQUEST_FILE").ok();
        let (requests, saw_eof) = read_requests(wanted, delay, record.is_some());
        if let Some(path) = record {
            let _ = std::fs::write(path, requests.join("\n"));
        }
        // A delayed answer models a server whose reply is asynchronous, and such
        // a server shuts down on EOF with that reply still in flight. Exiting
        // unanswered is the whole point of the mode: a caller that closed stdin
        // behind its request gets silence, exactly as the real one gives it.
        if delay > 0 && saw_eof {
            std::process::exit(0);
        }
        if let Ok(text) = std::env::var("MOCK_STDERR") {
            let _ = write!(std::io::stderr(), "{text}");
            let _ = std::io::stderr().flush();
        }
        let _ = writeln!(
            std::io::stdout(),
            "{}",
            std::env::var("MOCK_STDOUT").unwrap_or_default()
        );
        let _ = std::io::stdout().flush();
        std::process::exit(
            std::env::var("MOCK_EXIT")
                .ok()
                .and_then(|code| code.parse().ok())
                .unwrap_or(0),
        );
    }

    if std::env::var_os("MOCK_ECHO_STDIN").is_some() {
        let mut buf = Vec::new();
        let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut buf);
        let _ = std::io::stdout().write_all(&buf);
        let _ = std::io::stdout().flush();
        std::process::exit(0);
    }

    if let Ok(flag) = std::env::var("MOCK_CAT_ARG_AFTER") {
        let path = argv
            .iter()
            .position(|a| *a == flag)
            .and_then(|i| argv.get(i + 1));
        let contents = path
            .map(|p| std::fs::read_to_string(p).unwrap_or_default())
            .unwrap_or_default();
        let _ = write!(std::io::stdout(), "{contents}");
        let _ = std::io::stdout().flush();
        std::process::exit(0);
    }

    // Per-model failure: if the received argv selects the doomed model, fail with
    // a classifiable `model not found` stderr so a fallback fan-out falls through
    // it to the next model. Any other model runs normally.
    if let Ok(bad) = std::env::var("MOCK_FAIL_IF_MODEL") {
        let requested = argv
            .iter()
            .position(|a| a == "--model")
            .and_then(|i| argv.get(i + 1));
        if requested.map(String::as_str) == Some(bad.as_str()) {
            let msg = std::env::var("MOCK_FAIL_STDERR")
                .unwrap_or_else(|_| format!("error: model not found: {bad}"));
            let _ = write!(std::io::stderr(), "{msg}");
            let _ = std::io::stderr().flush();
            std::process::exit(1);
        }
    }

    log_line("S\n");
    if let Ok(ms) = std::env::var("MOCK_SLEEP_MS") {
        if let Ok(ms) = ms.parse::<u64>() {
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
    }
    log_line("E\n");

    if let Ok(text) = std::env::var("MOCK_STDERR") {
        let _ = write!(std::io::stderr(), "{text}");
    }

    // Per-attempt scripting: with MOCK_ATTEMPT_FILE set, increment a counter and
    // prefer MOCK_STDOUT_<attempt> when present, so a test can make the first
    // response invalid and a later one valid to exercise the retry loop.
    let attempt = std::env::var("MOCK_ATTEMPT_FILE").ok().map(|path| {
        let prior = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(0);
        let n = prior + 1;
        let _ = std::fs::write(&path, n.to_string());
        n
    });
    let attempt_stdout = attempt.and_then(|n| std::env::var(format!("MOCK_STDOUT_{n}")).ok());
    let stdout = attempt_stdout
        .or_else(|| std::env::var("MOCK_STDOUT").ok())
        .unwrap_or_else(|| "{\"result\":\"mock ok\"}".to_string());
    let stdout = if std::env::var("MOCK_PRESERVE_STDOUT").as_deref() == Ok("1") {
        stdout
    } else {
        session_faithful_stdout(&argv, stdout)
    };

    if let Ok(chunk_bytes) = std::env::var("MOCK_STREAM_CHUNK_BYTES") {
        let chunk_bytes = chunk_bytes.parse::<usize>().unwrap_or(0);
        assert!(chunk_bytes > 0, "MOCK_STREAM_CHUNK_BYTES must be positive");
        let delay = std::env::var("MOCK_STREAM_DELAY_MS")
            .ok()
            .map(|value| {
                value
                    .parse::<u64>()
                    .expect("MOCK_STREAM_DELAY_MS must be an unsigned integer")
            })
            .unwrap_or(0);
        let mut out = std::io::stdout();
        for (index, chunk) in stdout.as_bytes().chunks(chunk_bytes).enumerate() {
            if index > 0 {
                std::thread::sleep(std::time::Duration::from_millis(delay));
            }
            if out.write_all(chunk).is_err() || out.flush().is_err() {
                std::process::exit(0);
            }
        }
        log_line("COMPLETE\n");
        std::process::exit(0);
    }

    // Incremental streaming mode: with MOCK_STREAM_DELAY_MS set, emit MOCK_STDOUT
    // one line at a time, flushing and sleeping between lines — so a streaming
    // consumer sees events arrive over time and can short-circuit mid-stream. On
    // the FIRST write that fails (the reader — oneharness — was killed or stopped
    // reading, or oneharness forwarded an event to a consumer that closed the
    // pipe and then killed this child), we exit *without* writing the completion
    // sentinel, which is how a test proves the child was torn down early. When the
    // stream runs to completion, "COMPLETE\n" is appended to MOCK_LOG_FILE.
    if let Ok(delay) = std::env::var("MOCK_STREAM_DELAY_MS") {
        let delay = delay.parse::<u64>().unwrap_or(0);
        let mut out = std::io::stdout();
        for (i, line) in stdout.lines().enumerate() {
            if i > 0 {
                std::thread::sleep(std::time::Duration::from_millis(delay));
            }
            if writeln!(out, "{line}").is_err() || out.flush().is_err() {
                std::process::exit(0); // reader gone — do not signal completion
            }
        }
        log_line("COMPLETE\n");
        std::process::exit(0);
    }

    let _ = write!(std::io::stdout(), "{stdout}");
    let _ = std::io::stdout().flush();

    let code = std::env::var("MOCK_EXIT")
        .ok()
        .and_then(|c| c.parse::<i32>().ok())
        .unwrap_or(0);
    std::process::exit(code);
}
