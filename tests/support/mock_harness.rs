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
        let Ok(delay) = std::env::var("MOCK_REPLY_DELAY_MS")
            .unwrap_or_else(|_| "0".to_string())
            .parse::<u64>()
        else {
            let _ = write!(
                std::io::stderr(),
                "mock harness: MOCK_REPLY_DELAY_MS must be a whole number of milliseconds"
            );
            let _ = std::io::stderr().flush();
            std::process::exit(2);
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
