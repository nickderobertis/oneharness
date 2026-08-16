//! The library face of `run`: what an in-process caller gets that a subprocess
//! hop used to be needed for.
//!
//! These drive [`oneharness_core::io::run::run`] **in this process** — no
//! `oneharness` binary between the caller and the engine — through the same
//! `oneharness-mock-harness` fixture the CLI suite uses, so they stay hermetic
//! and deterministic. Four properties, one per journey: events arrive while the
//! run is still going, a sink that says stop cuts the turn off, cancelling
//! reaches the harness the run spawned rather than only the run itself, and a
//! caller can take the harness child into the process group / job object it
//! supervises (the grouping a subprocess hop used to provide) without losing the
//! teardown that hop also provided. The fifth — that the report comes back
//! without anything being printed to reach it — is `library_stdout.rs`, alone in
//! its own test binary.

use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::{mpsc, Mutex};
use std::time::{Duration, Instant};

use oneharness_core::domain::batch::BatchStrategy;
use oneharness_core::domain::events::ActionEvent;
use oneharness_core::domain::fallback::RunMode;
use oneharness_core::domain::report::Status;
use oneharness_core::io::cancel::CancelToken;
use oneharness_core::io::run::{
    run, run_supervised, EventSink, RunControls, RunOutcome, RunRequest, SinkStep,
};
use oneharness_core::io::runner::ProcessSupervisor;

#[path = "support/library_fixture.rs"]
mod fixture;
use fixture::{bin_override, request, tool_part_lines};

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

/// One hand-over recorded from the caller's side, so an assertion can be made
/// about it after the run has ended and the child is gone.
struct SeenChild {
    pid: u32,
    /// The process group the child is really in — the handle a watchdog polls
    /// and a kill reaps.
    #[cfg(unix)]
    group: libc::pid_t,
    /// The harness's own run log as it stood when the child was handed over.
    log_at_handover: String,
}

/// An **observing** supervisor: it records what each hook receives and leaves
/// the child in the tree oneharness owns. This is the shape a consumer uses to
/// hand a watchdog the pid (and, on Unix, the pgid it leads).
#[derive(Default)]
struct Recorder {
    /// An environment entry set on the spawning `Command` — the run's own answer
    /// then shows whether the hook held the `Command` that was really spawned.
    inject: Option<(String, String)>,
    /// The harness's run log, read at hand-over.
    log: Option<PathBuf>,
    commands: Mutex<Vec<(String, Vec<String>)>>,
    children: Mutex<Vec<SeenChild>>,
}

impl ProcessSupervisor for Recorder {
    fn spawning(&self, command: &mut Command) {
        if let Some((key, value)) = &self.inject {
            command.env(key, value);
        }
        let program = command.get_program().to_string_lossy().into_owned();
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        self.commands
            .lock()
            .expect("recorder mutex poisoned")
            .push((program, args));
    }

    fn spawned(&self, child: &Child) {
        let log_at_handover = self
            .log
            .as_ref()
            .map(|path| std::fs::read_to_string(path).unwrap_or_default())
            .unwrap_or_default();
        self.children
            .lock()
            .expect("recorder mutex poisoned")
            .push(SeenChild {
                pid: child.id(),
                #[cfg(unix)]
                group: child_group(child),
                log_at_handover,
            });
    }
}

/// The process group a spawned child belongs to, asked of the OS rather than
/// assumed — the whole question these hooks exist to let a caller answer.
#[cfg(unix)]
fn child_group(child: &Child) -> libc::pid_t {
    let pid = libc::pid_t::try_from(child.id()).expect("a child PID fits pid_t");
    // SAFETY: `getpgid` only reads the process table for `pid`, and the run
    // still holds this child, so the pid is live.
    unsafe { libc::getpgid(pid) }
}

/// A private path for one test's fixture file, cleared so a rerun starts fresh.
fn scratch(tag: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "oneharness-lib-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_file(&path);
    path
}

#[test]
fn the_pre_spawn_hook_holds_the_command_the_run_actually_spawns() {
    // The promise is that the `Command` handed to `spawning` is the one spawned,
    // not a copy taken before the run finished assembling it — otherwise a
    // `pre_exec`/`creation_flags` a caller registers for grouping never reaches
    // the harness. Proven from both ends: the environment the hook sets is what
    // the harness answers with (overriding the request's own value, since the
    // hook is the last writer), and the program and argv it saw are the ones the
    // report says were run.
    let recorder = Recorder {
        inject: Some((
            "MOCK_STDOUT".to_string(),
            r#"{"result":"the hook's command ran"}"#.to_string(),
        )),
        ..Recorder::default()
    };
    let request = request(
        "claude-code",
        &[("MOCK_STDOUT", r#"{"result":"the request's command ran"}"#)],
    );

    let outcome = run_supervised(&request, RunControls::default(), Some(&recorder))
        .expect("the mock run is valid");

    assert_eq!(outcome.report.results[0].status, Status::Ok);
    assert_eq!(
        outcome.report.results[0].text.as_deref(),
        Some("the hook's command ran"),
        "the hook mutated a Command the run then did not spawn"
    );
    let commands = recorder.commands.lock().expect("recorder mutex poisoned");
    assert_eq!(commands.len(), 1, "one harness, one spawn: {commands:?}");
    let (program, args) = &commands[0];
    assert!(
        program.contains("oneharness-mock-harness"),
        "the hook was handed some other process's command: {program}"
    );
    assert_eq!(
        args.as_slice(),
        &outcome.report.results[0].command[1..],
        "the argv the hook saw is not the argv the report says was run"
    );
}

#[test]
fn the_post_spawn_hook_reaches_the_child_before_its_exit_is_observable() {
    // Handing the child over only once the run had observed it exit would give a
    // caller a pid it cannot adopt — and one the OS may already have reused. The
    // fixture writes `S` when it starts and `E` when it ends, two seconds apart,
    // so an `E`-free log at hand-over places the hook inside the child's own
    // lifetime. (On Windows it is stricter still: the child is handed over
    // suspended, before its first instruction.)
    let log = scratch("handover.log");
    let recorder = Recorder {
        log: Some(log.clone()),
        ..Recorder::default()
    };
    let request = request(
        "claude-code",
        &[
            ("MOCK_SLEEP_MS", "2000"),
            ("MOCK_LOG_FILE", &log.display().to_string()),
            ("MOCK_STDOUT", r#"{"result":"handed over while alive"}"#),
        ],
    );

    let started = Instant::now();
    let outcome = run_supervised(&request, RunControls::default(), Some(&recorder))
        .expect("the mock run is valid");

    assert_eq!(outcome.report.results[0].status, Status::Ok);
    assert!(started.elapsed() >= Duration::from_secs(2));
    let children = recorder.children.lock().expect("recorder mutex poisoned");
    assert_eq!(children.len(), 1, "one harness, one hand-over");
    assert!(children[0].pid > 0);
    assert!(
        !children[0].log_at_handover.contains('E'),
        "the child was handed over after it had already ended: {:?}",
        children[0].log_at_handover
    );
    // Without this the assertion above would pass against a fixture that never
    // wrote an end marker at all, proving nothing.
    assert!(
        std::fs::read_to_string(&log)
            .unwrap_or_default()
            .contains('E'),
        "the fixture never recorded its own end, so its absence at hand-over is not evidence"
    );
    let _ = std::fs::remove_file(&log);
}

#[cfg(any(unix, windows))]
#[test]
fn an_observing_supervisor_leaves_the_descendant_teardown_to_oneharness() {
    // Setting a hook must not quietly move teardown to the caller. A supervisor
    // that only observes re-parents nothing, so the harness still leads the tree
    // oneharness created and a cancellation still ends the whole of it — the
    // same proof as the no-supervisor test, run with a supervisor set. On Unix
    // the recorded group says so directly: the child leads its own.
    let ticks = scratch("observed-cancel.ticks");
    let recorder = Recorder::default();
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

    let outcome = run_supervised(
        &request,
        RunControls {
            cancel: token,
            ..RunControls::default()
        },
        Some(&recorder),
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
    assert_eq!(outcome.report.results[0].status, Status::Cancelled);
    let children = recorder.children.lock().expect("recorder mutex poisoned");
    assert_eq!(children.len(), 1);
    #[cfg(unix)]
    assert_eq!(
        children[0].group,
        libc::pid_t::try_from(children[0].pid).expect("a child PID fits pid_t"),
        "an observing supervisor changed the group oneharness put the child in"
    );
    assert_descendant_stopped(&ticks);
    let _ = std::fs::remove_file(&ticks);
}

/// A supervisor that does what the two downstream consumers asked for: it moves
/// each harness child into a process group the **caller** owns, so one watchdog
/// sees the whole subtree and one `kill(-group)` reaps it.
///
/// The move lands even though oneharness has already made the child a group
/// leader — POSIX refuses `setpgid` for a **session** leader, not a group one —
/// and `a_caller_can_take_the_harness_child_into_its_own_process_group` proves
/// it the only way that claim can be proven: by asking the OS which group the
/// child ended up in.
#[cfg(unix)]
struct AdoptIntoCallerGroup {
    group: libc::pid_t,
    /// `(pid, the group the child ended up in)`, read at hand-over.
    seen: Mutex<Vec<(u32, libc::pid_t)>>,
}

#[cfg(unix)]
impl ProcessSupervisor for AdoptIntoCallerGroup {
    fn spawning(&self, command: &mut Command) {
        use std::os::unix::process::CommandExt;
        let group = self.group;
        // Registered after oneharness's own `setpgid(0, 0)`, so this runs second
        // and is the assignment the child keeps: an error here would fail the
        // spawn, and the group the OS reports afterwards is the caller's.
        //
        // SAFETY: `pre_exec` runs after fork in the child, before exec. The
        // closure calls only async-signal-safe `setpgid`, captures one integer,
        // and reports the OS error directly.
        unsafe {
            command.pre_exec(move || {
                if libc::setpgid(0, group) == -1 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
    }

    fn spawned(&self, child: &Child) {
        self.seen
            .lock()
            .expect("supervisor mutex poisoned")
            .push((child.id(), child_group(child)));
    }
}

#[cfg(unix)]
#[test]
fn a_caller_can_take_the_harness_child_into_its_own_process_group() {
    // The acceptance criterion, from the caller's side: with only the public
    // API, the harness child lands in the process group this test process is in
    // — which is what makes the subtree one unit to a watchdog and one target to
    // a kill.
    // SAFETY: `getpgrp` reads this process's own group and cannot fail.
    let caller_group = unsafe { libc::getpgrp() };
    let supervisor = AdoptIntoCallerGroup {
        group: caller_group,
        seen: Mutex::new(Vec::new()),
    };
    let request = request("claude-code", &[("MOCK_STDOUT", r#"{"result":"adopted"}"#)]);

    let outcome = run_supervised(&request, RunControls::default(), Some(&supervisor))
        .expect("the mock run is valid");

    assert_eq!(outcome.report.results[0].status, Status::Ok);
    assert_eq!(outcome.report.results[0].text.as_deref(), Some("adopted"));
    let seen = supervisor.seen.lock().expect("supervisor mutex poisoned");
    assert_eq!(seen.len(), 1, "one harness, one hand-over");
    let (pid, group) = seen[0];
    assert_eq!(
        group, caller_group,
        "the harness child did not join the caller's process group"
    );
    assert_ne!(
        group,
        libc::pid_t::try_from(pid).expect("a child PID fits pid_t"),
        "the child still leads a group of its own — the caller's setpgid did not win"
    );
}

/// A process group the **caller** owns outright, led by a process of its own —
/// not the test runner's group, which a test could never kill to prove
/// ownership. `sleep` is the leader; the harness children join it.
#[cfg(unix)]
fn caller_owned_group() -> (Child, libc::pid_t) {
    use std::os::unix::process::CommandExt;
    let mut command = Command::new("sleep");
    command
        .arg("30")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // SAFETY: `pre_exec` runs after fork in the child, before exec; `setpgid` is
    // async-signal-safe, captures nothing, and reports its OS error directly.
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == -1 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    let leader = command.spawn().expect("a group leader of the caller's own");
    let group = libc::pid_t::try_from(leader.id()).expect("a PID fits pid_t");
    (leader, group)
}

/// Prove from outside the harness tree that the fixture's native descendant is
/// still running: it appends a byte per tick, so growth over a window is the
/// only evidence a caller who cannot signal that process could have.
#[cfg(unix)]
fn assert_descendant_ticking(path: &std::path::Path, why: &str) {
    let length = || std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    let before = length();
    let deadline = Instant::now() + Duration::from_secs(3);
    while length() <= before {
        assert!(Instant::now() < deadline, "{why}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(unix)]
#[test]
fn an_adopted_tree_is_the_callers_to_reap_and_oneharness_leaves_it_alone() {
    // The other side of the bargain, and the reason teardown asks the OS which
    // group the child is really in. Once a `spawning` hook has moved the harness
    // into a group of the caller's, that group is the caller's to signal —
    // oneharness may end the process it spawned and nothing else, because the
    // group can hold the caller's own processes. So all three halves are checked
    // here: the run's own child is gone, the group's other members are not, the
    // adopted descendant is still running, and one `kill(-group)` — the whole
    // point of adopting — reaps it.
    let (mut leader, group) = caller_owned_group();
    let ticks = scratch("adopted-cancel.ticks");
    let supervisor = AdoptIntoCallerGroup {
        group,
        seen: Mutex::new(Vec::new()),
    };
    let request = request(
        "claude-code",
        &[
            ("MOCK_NATIVE_GRANDCHILD_MS", "20000"),
            ("MOCK_TICK_FILE", &ticks.display().to_string()),
        ],
    );

    let token = CancelToken::new();
    let (outcome, cancelled_at) = std::thread::scope(|scope| {
        let canceller = {
            let token = token.clone();
            let ticks = ticks.clone();
            scope.spawn(move || {
                // Cancel only once the descendant has proven it is really
                // running, so the assertions are about a live tree rather than a
                // spawn race.
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
        let outcome = run_supervised(
            &request,
            RunControls {
                cancel: token.clone(),
                ..RunControls::default()
            },
            Some(&supervisor),
        )
        .expect("a cancelled run still reports");
        (
            outcome,
            canceller
                .join()
                .expect("the cancelling thread did not panic"),
        )
    });

    // oneharness still ends the process it spawned: it is the *group* it may not
    // signal, not the child.
    assert_eq!(outcome.report.results[0].status, Status::Cancelled);
    assert!(
        cancelled_at.elapsed() < Duration::from_secs(15),
        "the cancelled run did not tear the adopted child down promptly: {:?}",
        cancelled_at.elapsed()
    );
    assert!(
        leader.try_wait().expect("the leader is waitable").is_none(),
        "oneharness signalled a process group it did not create"
    );
    assert_descendant_ticking(
        &ticks,
        "the adopted descendant stopped: oneharness reached into a tree it had handed to the caller",
    );

    // And the caller can do what adopting the group was for.
    // SAFETY: a negative PID addresses the group created above, whose only
    // members are this test's leader and the harness tree it adopted.
    unsafe { libc::kill(-group, libc::SIGKILL) };
    assert_descendant_stopped(&ticks);
    let _ = leader.wait();
    let _ = std::fs::remove_file(&ticks);
}

/// The Windows half of the same claim: the caller's own Job Object, which the
/// child is assigned to while it is still suspended.
///
/// A raw job handle is a kernel handle, valid from any thread, which is what
/// makes sharing it across a parallel run's spawns sound.
#[cfg(windows)]
struct CallerJob(windows_sys::Win32::Foundation::HANDLE);

// SAFETY: a Job Object handle is process-wide and thread-agnostic; this test
// owns it for the whole run and closes it exactly once, in `Drop`.
#[cfg(windows)]
unsafe impl Send for CallerJob {}
#[cfg(windows)]
unsafe impl Sync for CallerJob {}

#[cfg(windows)]
impl CallerJob {
    fn create() -> Self {
        // SAFETY: null security/name pointers request an unnamed job object with
        // default security; the returned handle is owned by `CallerJob`.
        let job = unsafe {
            windows_sys::Win32::System::JobObjects::CreateJobObjectW(
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        assert!(
            !job.is_null(),
            "could not create the caller's job object: {}",
            std::io::Error::last_os_error()
        );
        Self(job)
    }
}

#[cfg(windows)]
impl Drop for CallerJob {
    fn drop(&mut self) {
        // SAFETY: `self.0` is owned here and closed exactly once. The job
        // carries no kill-on-close limit, so this ends nothing.
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
    }
}

/// Assigns each harness child to a job object the **caller** owns, so one
/// `TerminateJobObject` reaps the whole subtree — the Windows shape of what the
/// two downstream consumers asked for.
#[cfg(windows)]
struct AdoptIntoCallerJob {
    job: CallerJob,
    /// `(the assignment succeeded, the child is a member)`, read at hand-over.
    seen: Mutex<Vec<(bool, bool)>>,
}

#[cfg(windows)]
impl ProcessSupervisor for AdoptIntoCallerJob {
    fn spawned(&self, child: &Child) {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::HANDLE;
        use windows_sys::Win32::System::JobObjects::{AssignProcessToJobObject, IsProcessInJob};

        let process = child.as_raw_handle() as HANDLE;
        // SAFETY: both handles are live, and the child is still suspended — the
        // window this hook is called in exists precisely so a descendant cannot
        // be created outside the caller's job.
        let assigned = unsafe { AssignProcessToJobObject(self.job.0, process) } != 0;
        let mut member: i32 = 0;
        // SAFETY: `member` is a live i32 the call writes a BOOL into.
        let queried = unsafe { IsProcessInJob(process, self.job.0, &mut member) } != 0;
        self.seen
            .lock()
            .expect("supervisor mutex poisoned")
            .push((assigned, queried && member != 0));
    }
}

#[cfg(windows)]
#[test]
fn a_caller_can_take_the_harness_child_into_its_own_job_object() {
    // The acceptance criterion on Windows, asked of the OS from the caller's
    // side: `IsProcessInJob` says the harness child is in the job this test
    // created, so terminating that job reaps the subtree. Job objects nest, so
    // the child is in oneharness's job as well and either side's teardown ends
    // it — which is why nothing changes hands here.
    let supervisor = AdoptIntoCallerJob {
        job: CallerJob::create(),
        seen: Mutex::new(Vec::new()),
    };
    let request = request("claude-code", &[("MOCK_STDOUT", r#"{"result":"adopted"}"#)]);

    let outcome = run_supervised(&request, RunControls::default(), Some(&supervisor))
        .expect("the mock run is valid");

    assert_eq!(outcome.report.results[0].status, Status::Ok);
    assert_eq!(outcome.report.results[0].text.as_deref(), Some("adopted"));
    let seen = supervisor.seen.lock().expect("supervisor mutex poisoned");
    assert_eq!(seen.len(), 1, "one harness, one hand-over");
    let (assigned, member) = seen[0];
    assert!(
        assigned,
        "the harness child could not be assigned to the caller's job object: {}",
        std::io::Error::last_os_error()
    );
    assert!(
        member,
        "the harness child is not a member of the caller's job object"
    );
}

/// Drive `request` with an observing supervisor and hand back both the outcome
/// and every child the run offered it.
fn supervised_spawns(request: &RunRequest) -> (RunOutcome, Vec<SeenChild>) {
    let recorder = Recorder::default();
    let outcome = run_supervised(request, RunControls::default(), Some(&recorder))
        .expect("the mock run is valid");
    let children = recorder
        .children
        .into_inner()
        .expect("recorder mutex poisoned");
    (outcome, children)
}

/// Every child in `seen` is a distinct process that led the process group
/// oneharness put it in — so a count is evidence about real spawns rather than
/// about how often a hook happened to fire.
fn assert_distinct_live_children(seen: &[SeenChild], what: &str) {
    for (i, child) in seen.iter().enumerate() {
        assert!(child.pid > 0, "{what}: hand-over {i} carried no process");
        #[cfg(unix)]
        assert_eq!(
            child.group,
            libc::pid_t::try_from(child.pid).expect("a child PID fits pid_t"),
            "{what}: hand-over {i} was not the leader of the group oneharness created"
        );
        assert!(
            seen[..i].iter().all(|earlier| earlier.pid != child.pid),
            "{what}: the same process was handed over twice"
        );
    }
}

#[test]
fn every_execution_model_hands_its_harness_children_to_the_supervisor() {
    // A run reaches the runner through several entry points — buffered waves,
    // the streaming driver, a fork batch's warm-up and fan-out — and each is a
    // separate place the supervisor has to be carried to. A hook dropped at one
    // of them is exactly how a consumer loses the grouping for the run that
    // needed it, and nothing else in the report would say so.
    let (outcome, seen) = supervised_spawns(&RunRequest {
        stream: Some(true),
        ..request("opencode", &[("MOCK_STDOUT", &tool_part_lines(2))])
    });
    assert!(outcome.streamed);
    assert_eq!(seen.len(), 1, "a streaming run spawns one harness");
    assert_distinct_live_children(&seen, "streaming");

    // A parallel wave spawns from several worker threads at once — the reason
    // the supervisor is shared as `&dyn ProcessSupervisor + Sync`.
    let mut parallel = request("claude-code", &[("MOCK_STDOUT", r#"{"result":"ok"}"#)]);
    parallel.harness.push("opencode".to_string());
    parallel.bin.push(bin_override("opencode"));
    let (outcome, seen) = supervised_spawns(&parallel);
    assert_eq!(outcome.report.results.len(), 2);
    assert_eq!(seen.len(), 2, "two harnesses, two hand-overs");
    assert_distinct_live_children(&seen, "parallel");

    // A `min-tokens` fork batch is two spawn sites in one run: the warm-up that
    // establishes the session, then the fan-out that forks it.
    let (outcome, seen) = supervised_spawns(&RunRequest {
        prompt: vec!["warm".to_string(), "q1".to_string(), "q2".to_string()],
        batch_strategy: Some(BatchStrategy::MinTokens),
        ..request(
            "claude-code",
            &[(
                "MOCK_STDOUT",
                r#"{"result":"ok","session_id":"SID-SUPERVISED"}"#,
            )],
        )
    });
    assert!(
        outcome.report.batch.as_ref().is_some_and(|b| b.forked),
        "the batch did not fork, so its fan-out is not the spawn site under test"
    );
    assert_eq!(seen.len(), 3, "one warm-up plus two forked fan-out calls");
    assert_distinct_live_children(&seen, "fork batch");

    // A fallback chain spawns candidate by candidate through its own driver;
    // the first here cannot run at all, so the one that does is the second.
    let mut fallback = request("claude-code", &[("MOCK_STDOUT", r#"{"result":"ok"}"#)]);
    fallback.harness.insert(0, "codex".to_string());
    fallback
        .bin
        .push("codex=/nonexistent/oneharness-absent-harness".to_string());
    fallback.run_mode = Some(RunMode::Fallback);
    let (outcome, seen) = supervised_spawns(&fallback);
    assert_eq!(outcome.report.results[0].status, Status::Skipped);
    assert_eq!(outcome.report.results[1].status, Status::Ok);
    assert_eq!(seen.len(), 1, "only the candidate that ran was spawned");
    assert_distinct_live_children(&seen, "fallback");

    // A structured-output retry re-spawns the same job, and a caller that
    // supervises the first attempt must be handed the second one too.
    let schema = scratch("supervised-schema.json");
    std::fs::write(
        &schema,
        r#"{"type":"object","properties":{"name":{"type":"string"},"age":{"type":"integer"}},"required":["name","age"]}"#,
    )
    .expect("a schema file");
    let counter = scratch("supervised-attempts");
    let (outcome, seen) = supervised_spawns(&RunRequest {
        schema: Some(schema.clone()),
        schema_max_retries: Some(3),
        ..request(
            "crush",
            &[
                ("MOCK_ATTEMPT_FILE", &counter.display().to_string()),
                ("MOCK_STDOUT_1", r#"{"name":"Ada"}"#),
                ("MOCK_STDOUT_2", r#"{"name":"Ada","age":36}"#),
            ],
        )
    });
    assert_eq!(outcome.report.results[0].schema_attempts, Some(2));
    assert_eq!(seen.len(), 2, "one hand-over per attempt");
    assert_distinct_live_children(&seen, "structured-output retry");
    let _ = std::fs::remove_file(&schema);
    let _ = std::fs::remove_file(&counter);
}

/// A short `/tmp`-rooted directory, because a controlled run's socket address
/// lives under it and `sockaddr_un.sun_path` is 104 bytes on macOS (which
/// canonicalizes `/tmp` to `/private/tmp` before binding).
#[cfg(unix)]
fn control_store(tag: &str) -> PathBuf {
    let dir = PathBuf::from("/tmp").join(format!("oh-lib-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a control store directory");
    dir
}

#[cfg(unix)]
#[test]
fn a_controlled_turn_hands_its_harness_child_to_the_supervisor() {
    // The execution model where the grouping matters most: a controlled run is
    // one live, billing turn a supervisor exists to be able to reap. It spawns
    // through its own entry point, so it needs its own proof.
    let store = control_store("ctl");
    let cwd = control_store("ctlcwd");
    let turn_log = store.join("turn.log");
    let request = RunRequest {
        control: true,
        session: Some("sup".to_string()),
        session_dir: Some(store.clone()),
        cwd: Some(cwd.clone()),
        ..request(
            "claude-code",
            &[("MOCK_TURN_LOG", &turn_log.display().to_string())],
        )
    };

    let (outcome, seen) = supervised_spawns(&request);

    assert_eq!(outcome.report.results[0].status, Status::Ok);
    assert_eq!(outcome.report.results[0].text.as_deref(), Some("mock turn"));
    assert_eq!(seen.len(), 1, "one controlled turn, one hand-over");
    assert_distinct_live_children(&seen, "controlled turn");
    let _ = std::fs::remove_dir_all(&store);
    let _ = std::fs::remove_dir_all(&cwd);
}
