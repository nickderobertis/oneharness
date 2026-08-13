//! The library face of `run`: what an in-process caller gets that a subprocess
//! hop used to be needed for.
//!
//! These drive [`oneharness_core::io::run::run`] **in this process** — no
//! `oneharness` binary between the caller and the engine — through the same
//! `oneharness-mock-harness` fixture the CLI suite uses, so they stay hermetic
//! and deterministic. Three properties, one per journey: events arrive while the
//! run is still going, a sink that says stop cuts the turn off, and cancelling
//! reaches the harness the run spawned rather than only the run itself. The
//! fourth — that the report comes back without anything being printed to reach
//! it — is `library_stdout.rs`, alone in its own test binary.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use oneharness_core::domain::events::ActionEvent;
use oneharness_core::domain::report::Status;
use oneharness_core::io::cancel::CancelToken;
use oneharness_core::io::run::{run, EventSink, RunControls, RunRequest, SinkStep};

#[path = "support/library_fixture.rs"]
mod fixture;
use fixture::{request, tool_part_lines};

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

#[test]
fn an_omitted_library_timeout_outlives_the_former_120_second_default() {
    // This is intentionally wall-clock evidence at the public library boundary:
    // resolving `None` is not enough to prove a hidden runner default cannot
    // still kill the child. The mock crosses the old deadline, then exits cleanly.
    let request = RunRequest {
        timeout: None,
        ..request(
            "claude-code",
            &[
                ("MOCK_SLEEP_MS", "121000"),
                (
                    "MOCK_STDOUT",
                    r#"{"result":"finished after the old deadline"}"#,
                ),
            ],
        )
    };
    let started = Instant::now();
    let outcome = run(&request, RunControls::default()).expect("the mock run is valid");
    assert!(started.elapsed() >= Duration::from_secs(120));
    assert_eq!(outcome.report.results[0].status, Status::Ok);
    assert_eq!(
        outcome.report.results[0].text.as_deref(),
        Some("finished after the old deadline")
    );
}

// capability: run
// capability: runStream
#[test]
fn a_streaming_caller_sees_an_event_before_the_run_finishes() {
    // "As they occur" is the property, so the test has to observe an event while
    // the run is demonstrably still in flight — not merely receive five of them
    // by the end, which a buffered implementation would also satisfy. The mock
    // spaces five events 300 ms apart, so the first arrives with well over a
    // second of run still to go.
    let request = RunRequest {
        stream: Some(true),
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

#[test]
fn a_sink_that_stops_tears_the_harness_down_mid_turn() {
    // The short-circuit the sink exists for: a consumer that has seen enough
    // says so, and the turn is cut off rather than paid for in full. The CLI
    // reaches this through a broken stdout pipe; a library caller reaches it by
    // answering `Stop`. Proven by the fixture's own sentinel — it appends
    // COMPLETE only if it streamed every line, so its absence is the teardown.
    let sentinel = std::env::temp_dir().join(format!(
        "oneharness-lib-stop-{}-{:?}.log",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_file(&sentinel);
    let request = RunRequest {
        stream: Some(true),
        ..request(
            "opencode",
            &[
                ("MOCK_STDOUT", &tool_part_lines(5)),
                ("MOCK_STREAM_DELAY_MS", "300"),
                ("MOCK_LOG_FILE", &sentinel.display().to_string()),
            ],
        )
    };

    let mut sink = StopAfterFirst(0);
    let outcome = run(
        &request,
        RunControls {
            events: Some(&mut sink),
            ..RunControls::default()
        },
    )
    .expect("a stopped stream still reports");

    assert_eq!(
        sink.0, 1,
        "the run kept publishing after the sink said stop"
    );
    let log = std::fs::read_to_string(&sentinel).unwrap_or_default();
    assert!(
        !log.contains("COMPLETE"),
        "the harness was not torn down on the sink's stop (it ran to completion): {log:?}"
    );
    // A consumer-driven stop is not a failure, and the report is still returned
    // with whatever the harness had produced by then.
    assert_eq!(outcome.exit_code, 0);
    assert_eq!(outcome.report.results[0].status, Status::Ok);
    let _ = std::fs::remove_file(&sentinel);
}

/// Answers [`SinkStep::Stop`] to the very first event, counting how many it was
/// offered — so a run that ignored the stop is visible as a count above one.
struct StopAfterFirst(usize);

impl EventSink for StopAfterFirst {
    fn event(&mut self, _harness_id: &str, _event: &ActionEvent) -> SinkStep {
        self.0 += 1;
        SinkStep::Stop
    }
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
