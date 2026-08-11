//! The library face of `run`: what an in-process caller gets that a subprocess
//! hop used to be needed for.
//!
//! These drive [`oneharness_core::io::run::run`] **in this process** — no
//! `oneharness` binary between the caller and the engine — through the same
//! `oneharness-mock-harness` fixture the CLI suite uses, so they stay hermetic
//! and deterministic. Three properties, one per journey: the report comes back
//! (and nothing is printed to reach it), events arrive while the run is still
//! going, and cancelling reaches the harness the run spawned rather than only
//! the run itself.

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use oneharness_core::domain::events::ActionEvent;
use oneharness_core::domain::report::Status;
use oneharness_core::io::cancel::CancelToken;
use oneharness_core::io::run::{run, EventSink, RunControls, RunRequest, SinkStep};

/// The mock harness is built beside the main binary when the `mock-harness`
/// feature is enabled (which `just test` / `just check` and CI do).
fn mock_bin() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_BIN_EXE_oneharness"));
    path.set_file_name(format!(
        "oneharness-mock-harness{}",
        std::env::consts::EXE_SUFFIX
    ));
    path
}

fn bin_override(id: &str) -> String {
    format!("{id}={}", mock_bin().display())
}

/// A harness child this test will have killed must not leave a truncated
/// coverage profile in the target directory, where `just coverage` would collect
/// it and fail the whole merge. Same redirect the CLI suite applies.
fn profile_redirect() -> String {
    format!(
        "LLVM_PROFILE_FILE={}",
        std::env::temp_dir()
            .join("oneharness-killed-mock-%p.profraw")
            .display()
    )
}

/// A hermetic single-harness request against the mock fixture. `no_config` keeps
/// the developer's real config files and `ONEHARNESS_*` overrides out of the
/// assertion, exactly as `ONEHARNESS_NO_CONFIG=1` does for the CLI suite.
fn request(id: &str, env: &[(&str, &str)]) -> RunRequest {
    let mut child_env: Vec<String> = env
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect();
    child_env.push(profile_redirect());
    RunRequest {
        harness: vec![id.to_string()],
        prompt: vec!["hi".to_string()],
        bin: vec![bin_override(id)],
        env: child_env,
        no_config: true,
        timeout: Some(60),
        ..RunRequest::default()
    }
}

/// Five opencode tool parts — five `tool_call` events when streamed one line at
/// a time.
fn tool_part_lines(count: usize) -> String {
    (0..count)
        .map(|i| {
            format!(
                r#"{{"type":"tool_use","part":{{"type":"tool","tool":"bash","state":{{"input":{{"command":"step {i}"}}}}}}}}"#
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Collects each event the run publishes, forwarding it to the test thread the
/// moment it arrives.
struct ChannelSink(mpsc::Sender<ActionEvent>);

impl EventSink for ChannelSink {
    fn event(&mut self, _harness_id: &str, event: &ActionEvent) -> SinkStep {
        // A receiver that has gone away is the consumer short-circuiting, which
        // is the same stop signal a broken stdout pipe is for the CLI.
        match self.0.send(event.clone()) {
            Ok(()) => SinkStep::Continue,
            Err(_) => SinkStep::Stop,
        }
    }
}

#[cfg(unix)]
#[test]
fn a_library_caller_gets_the_report_back_without_the_engine_printing_it() {
    // The whole point of the library entry point: the report is a *value*, and
    // getting it costs the caller nothing on its own stdout — which a consumer
    // like onejudge cannot lend out, because its stdout is its own contract.
    // Proven by redirecting this process's fd 1 to a file across the call and
    // asserting the file stayed empty while the report came back in full.
    let stdout = concat!(
        r#"{"type":"text","part":{"type":"text","text":"working"}}"#,
        "\n",
        r#"{"type":"tool_use","part":{"type":"tool","tool":"bash","state":{"status":"completed","input":{"command":"echo hi"},"output":"hi"}}}"#,
        "\n",
    );
    let request = request("opencode", &[("MOCK_STDOUT", stdout)]);

    let captured = std::env::temp_dir().join(format!("oh-lib-stdout-{}.txt", std::process::id()));
    let _ = std::fs::remove_file(&captured);
    let redirect = StdoutRedirect::to(&captured);
    let outcome = run(&request, RunControls::default());
    drop(redirect);

    let outcome = outcome.expect("a hermetic mock run is not a usage error");
    assert_eq!(outcome.exit_code, 0, "{:?}", outcome.report.results[0]);
    assert!(!outcome.streamed, "a buffered run publishes nothing");
    assert!(outcome.failure_summary.is_none());
    let result = &outcome.report.results[0];
    assert_eq!(result.status, Status::Ok);
    assert_eq!(result.text.as_deref(), Some("working"));
    assert_eq!(result.events.as_ref().map(Vec::len), Some(1));
    // The report names the engine's own version when the caller supplies none.
    assert!(!outcome.report.oneharness_version.is_empty());

    let printed = std::fs::read_to_string(&captured).expect("the redirect file exists");
    assert!(
        printed.is_empty(),
        "the engine wrote to the caller's stdout: {printed:?}"
    );
    let _ = std::fs::remove_file(&captured);
}

#[test]
fn a_streaming_caller_sees_an_event_before_the_run_finishes() {
    // "As they occur" is the property, so the test has to observe an event while
    // the run is demonstrably still in flight — not merely receive five of them
    // by the end, which a buffered implementation would also satisfy. The mock
    // spaces five events 300 ms apart, so the first arrives with well over a
    // second of run still to go.
    let request = RunRequest {
        stream: true,
        ..request(
            "opencode",
            &[
                ("MOCK_STDOUT", &tool_part_lines(5)),
                ("MOCK_STREAM_DELAY_MS", "300"),
            ],
        )
    };

    let (tx, rx) = mpsc::channel();
    let running = std::thread::spawn(move || {
        let mut sink = ChannelSink(tx);
        run(
            &request,
            RunControls {
                events: Some(&mut sink),
                ..RunControls::default()
            },
        )
    });

    let first = rx
        .recv_timeout(Duration::from_secs(30))
        .expect("the first event reaches the sink");
    let first_at = Instant::now();
    assert_eq!(first.name.as_deref(), Some("bash"));
    assert_eq!(first.index, 0);
    assert!(
        !running.is_finished(),
        "the event only arrived once the run was over — that is not streaming"
    );

    let outcome = running
        .join()
        .expect("the run thread did not panic")
        .expect("a hermetic mock stream is not a usage error");
    // The load-bearing assertion, and the one an implementation that collected
    // the events and handed them over at the end would fail: four more 300 ms
    // lines were still to come when the first event landed, so the run had well
    // over a second left to live.
    assert!(
        first_at.elapsed() >= Duration::from_millis(600),
        "the first event arrived only as the run ended ({:?} before it finished) — \
         that is a buffered handover, not a live stream",
        first_at.elapsed()
    );
    assert!(outcome.streamed);
    assert_eq!(outcome.exit_code, 0);
    // Every event the sink saw is also on the returned report, so a caller that
    // ignored the incremental stream loses nothing.
    let delivered: Vec<ActionEvent> = std::iter::once(first).chain(rx.try_iter()).collect();
    assert_eq!(delivered.len(), 5, "{delivered:?}");
    assert_eq!(
        outcome.report.results[0].events.as_ref().map(Vec::len),
        Some(5)
    );
}

#[cfg(any(unix, windows))]
#[test]
fn cancelling_a_run_terminates_the_harness_the_run_spawned() {
    // The supervision a subprocess hop used to provide. The harness leads its own
    // process group (Unix) / job object (Windows), so nothing the caller signals
    // its own group reaches it — the token is the only handle that does. The
    // fixture is a launcher whose native descendant ignores TERM and ticks a file
    // while alive, so the proof comes from *outside* the tree: it stopped
    // answering. The harness is otherwise silent, so no output line could have
    // ended the run instead, and its timeout is far beyond the teardown measured
    // here.
    let ticks = std::env::temp_dir().join(format!(
        "oneharness-lib-cancel-{}-{:?}.ticks",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_file(&ticks);
    let request = request(
        "claude-code",
        &[
            ("MOCK_NATIVE_GRANDCHILD_MS", "20000"),
            ("MOCK_TICK_FILE", &ticks.display().to_string()),
        ],
    );

    let token = CancelToken::new();
    let canceller = {
        let token = token.clone();
        let ticks = ticks.clone();
        std::thread::spawn(move || {
            // Cancel only once the descendant has proven it is really running, so
            // the teardown assertion is about a live tree rather than a spawn race.
            let deadline = Instant::now() + Duration::from_secs(20);
            while std::fs::metadata(&ticks).map(|m| m.len()).unwrap_or(0) == 0 {
                assert!(
                    Instant::now() < deadline,
                    "the silent harness never started its descendant"
                );
                std::thread::sleep(Duration::from_millis(20));
            }
            let cancelled_at = Instant::now();
            token.cancel();
            cancelled_at
        })
    };

    let outcome = run(
        &request,
        RunControls {
            cancel: token,
            ..RunControls::default()
        },
    )
    .expect("a cancelled run still reports");
    let cancelled_at = canceller
        .join()
        .expect("the cancelling thread did not panic");

    assert!(
        cancelled_at.elapsed() < Duration::from_secs(15),
        "the cancelled run did not tear down promptly: {:?}",
        cancelled_at.elapsed()
    );
    // A cancelled run is a value a consumer reads, not a process that vanished.
    let result = &outcome.report.results[0];
    assert_eq!(result.status, Status::Cancelled);
    assert_eq!(result.exit_code, None);
    assert!(
        result
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("cancel"),
        "{:?}",
        result.error
    );
    assert_eq!(outcome.exit_code, 1);
    assert!(outcome.failure_summary.is_some());

    assert_descendant_stopped(&ticks);
    let _ = std::fs::remove_file(&ticks);
}

/// Prove from outside the harness tree that the fixture's native descendant
/// stopped: it appends a byte every tick while alive, so a sustained quiet
/// interval is the only evidence needed — and the only evidence a caller who
/// cannot signal that process could ever have.
#[cfg(any(unix, windows))]
fn assert_descendant_stopped(path: &std::path::Path) {
    let witness_deadline = Instant::now() + Duration::from_secs(2);
    let witnessed = loop {
        let length = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
        if length > 0 {
            break length;
        }
        assert!(
            Instant::now() < witness_deadline,
            "native descendant never wrote its durable tick witness"
        );
        std::thread::sleep(Duration::from_millis(20));
    };

    // A live fixture ticks at least every 50 ms. Poll for a sustained quiet
    // interval instead of assuming one fixed sleep is enough on a busy runner.
    let stop_deadline = Instant::now() + Duration::from_secs(3);
    let mut last_length = witnessed;
    let mut quiet_since = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(20));
        let length = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
        if length != last_length {
            last_length = length;
            quiet_since = Instant::now();
        }
        if quiet_since.elapsed() >= Duration::from_millis(750) {
            return;
        }
        assert!(
            Instant::now() < stop_deadline,
            "native descendant kept ticking after the cancelled run's teardown"
        );
    }
}

/// This process's stdout, pointed at a file for as long as the guard lives.
///
/// The only way to ask "did that call print anything?" of code running in your
/// own process. Restored on drop, so a panicking assertion still leaves the test
/// harness able to report.
#[cfg(unix)]
struct StdoutRedirect(std::os::fd::OwnedFd);

#[cfg(unix)]
impl StdoutRedirect {
    fn to(path: &std::path::Path) -> Self {
        use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
        let file = std::fs::File::create(path).expect("create the redirect file");
        // SAFETY: fd 1 is open for the lifetime of the process, and the
        // descriptor `dup` returns is owned by the guard until it restores it.
        let saved = unsafe { libc::dup(libc::STDOUT_FILENO) };
        assert!(saved >= 0, "could not duplicate stdout");
        // SAFETY: `saved` is a fresh, valid, exclusively-owned descriptor.
        let saved = unsafe { OwnedFd::from_raw_fd(saved) };
        // SAFETY: both arguments are valid open descriptors.
        assert!(
            unsafe { libc::dup2(file.as_raw_fd(), libc::STDOUT_FILENO) } >= 0,
            "could not redirect stdout"
        );
        Self(saved)
    }
}

#[cfg(unix)]
impl Drop for StdoutRedirect {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        // SAFETY: the saved descriptor is still owned and open here, and fd 1 is
        // a valid target.
        unsafe { libc::dup2(self.0.as_raw_fd(), libc::STDOUT_FILENO) };
    }
}
