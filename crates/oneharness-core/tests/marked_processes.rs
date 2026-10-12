//! `io::marked_processes` driven against real, detached processes.
//!
//! The shape is the one a shared server leaves behind: the process to stop is
//! no child of the caller. Each test spawns a *helper* (this test binary,
//! re-executed in a role), which starts a *grandchild* detached into its own
//! session and process group and exits — so the grandchild's only tie to the
//! test is the environment it carries. Every grandchild appends a heartbeat line
//! to its own file every 100 ms, which is what "still running" and "stopped"
//! are read from.
//!
//! The roles are this binary's own `role` test, which does nothing unless
//! `ROLE_VAR` names one — so no fixture binary has to be built or found.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use oneharness_core::io::marked_processes::{find_marked, stop_marked, ProcessMarker, StopReport};
use oneharness_core::io::scratch::ScratchDir;

/// Which role a re-executed copy of this binary plays: `spawn`, `beat` or
/// `stop-own-marker`.
const ROLE_VAR: &str = "ONEHARNESS_MARKED_TEST_ROLE";
/// The heartbeat file a `beat` appends to, or where `stop-own-marker` writes
/// what it found.
const TICKS_VAR: &str = "ONEHARNESS_MARKED_TEST_TICKS";
/// `NAME=VALUE` a `spawn` sets on the grandchild it starts; unset for none.
const MARK_VAR: &str = "ONEHARNESS_MARKED_TEST_MARK";
/// How a `beat` meets a stop request: `plain`; `ignore-stop` (Unix);
/// `fork-while-stopping`, starting further marked children while it is being
/// stopped; or `protected`, hiding its environment from other processes
/// (Linux).
const BEHAVIOUR_VAR: &str = "ONEHARNESS_MARKED_TEST_BEHAVIOUR";
/// The variable every marker in this suite uses.
const MARKER_NAME: &str = "ONEHARNESS_MARKED_TEST_ID";

const BEAT: Duration = Duration::from_millis(100);
/// Hang guard for anything that should happen within a few beats.
const PATIENCE: Duration = Duration::from_secs(20);
/// A `beat` that nobody stops still ends, so a failed run leaves nothing behind
/// for long.
const LIFETIME: Duration = Duration::from_secs(90);

/// The helper entry point. A no-op when this binary runs as a test suite.
// llmlint: ignore[tests_assert_real_behavior] This is not a test but the fixture entry point the other tests re-execute this binary into: an integration test can only spawn its own binary, and libtest only runs code reached from a test. What each role does is asserted by the tests that spawn it.
#[test]
fn role() {
    match std::env::var(ROLE_VAR).as_deref() {
        Ok("spawn") => spawn_role(),
        Ok("beat") => beat_role(),
        Ok("stop-own-marker") => stop_own_marker_role(),
        _ => {}
    }
}

/// Start the grandchild, detached, report its pid beside its heartbeat file,
/// and exit.
fn spawn_role() {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["role", "--exact", "--nocapture", "--test-threads=1"])
        .env(ROLE_VAR, "beat")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Ok(mark) = std::env::var(MARK_VAR) {
        let (name, value) = mark.split_once('=').unwrap();
        command.env(name, value);
    }
    detach(&mut command);
    let child = command.spawn().unwrap();
    // Through a file rather than stdout: a pipe the test read would also be
    // held by the grandchild on Windows, where inheritable handles leak into
    // every process spawned, and the read would wait out its whole life.
    let ticks = PathBuf::from(std::env::var(TICKS_VAR).unwrap());
    std::fs::write(ticks.with_extension("pid"), child.id().to_string()).unwrap();
    std::process::exit(0);
}

#[cfg(unix)]
fn detach(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    // SAFETY: `setsid` is async-signal-safe and touches no Rust state.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(windows)]
fn detach(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS};
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
}

/// Heartbeat until stopped, the deadline in `LIFETIME`, or the heartbeat file's
/// directory going away.
fn beat_role() {
    let ticks = PathBuf::from(std::env::var(TICKS_VAR).unwrap());
    let behaviour = std::env::var(BEHAVIOUR_VAR).unwrap_or_default();
    match behaviour.as_str() {
        #[cfg(unix)]
        "ignore-stop" => ignore_stop_requests(),
        "fork-while-stopping" => fork_while_stopping::install(&ticks),
        #[cfg(target_os = "linux")]
        "protected" => {
            // SAFETY: PR_SET_DUMPABLE takes an integer and touches only this
            // process's credentials.
            assert_eq!(unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0) }, 0);
        }
        _ => {}
    }
    let started = Instant::now();
    let mut beat = 0u64;
    while started.elapsed() < LIFETIME {
        if behaviour == "fork-while-stopping" {
            fork_while_stopping::poll(&ticks);
        }
        let appended = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&ticks)
            .and_then(|mut file| writeln!(file, "beat {beat}"));
        if appended.is_err() {
            return;
        }
        beat += 1;
        std::thread::sleep(BEAT);
    }
}

/// Carrying the marker it is given, find and stop it, and write what that did
/// beside this process's own pid. Exiting normally is the proof it was never
/// its own target.
fn stop_own_marker_role() {
    let results = PathBuf::from(std::env::var(TICKS_VAR).unwrap());
    let mark = std::env::var(MARK_VAR).unwrap();
    let (name, value) = mark.split_once('=').unwrap();
    let marker = ProcessMarker {
        name: name.to_string(),
        value: value.to_string(),
    };
    let found = find_marked(&marker).unwrap();
    let report = stop_marked(&marker, Duration::from_secs(5)).unwrap();
    let own = std::process::id();
    std::fs::write(results, format!("{own}\n{found:?}\n{report:?}\n")).unwrap();
    std::process::exit(0);
}

#[cfg(unix)]
fn ignore_stop_requests() {
    // SAFETY: setting a signal's disposition to SIG_IGN installs no handler.
    unsafe { libc::signal(libc::SIGTERM, libc::SIG_IGN) };
}

/// Start a further marked `beat` — it inherits the marker — sharing one
/// heartbeat file with its siblings, and append its pid to their pid file.
fn start_marked_child(ticks: &Path) {
    let child_ticks = child_ticks(ticks);
    // Never waited on by design: the child must outlive its parent, which on
    // Unix exits straight after this and leaves it to be reparented and reaped;
    // Windows has no zombies.
    #[allow(clippy::zombie_processes)]
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["role", "--exact", "--nocapture", "--test-threads=1"])
        .env(ROLE_VAR, "beat")
        .env(TICKS_VAR, &child_ticks)
        .env(BEHAVIOUR_VAR, "plain")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    // One write per line, so a parent stopped mid-way leaves no torn pid.
    std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(child_ticks.with_extension("pids"))
        .and_then(|mut file| file.write_all(format!("{}\n", child.id()).as_bytes()))
        .unwrap();
}

/// The heartbeat file every child of the `fork-while-stopping` beat at `ticks`
/// shares.
fn child_ticks(ticks: &Path) -> PathBuf {
    ticks.with_extension("child.ticks")
}

/// The pids of the children the `fork-while-stopping` beat at `ticks` started,
/// counting only complete lines.
fn child_pids(ticks: &Path) -> Vec<u32> {
    let text =
        std::fs::read_to_string(child_ticks(ticks).with_extension("pids")).unwrap_or_default();
    text.split_inclusive('\n')
        .filter_map(|line| line.strip_suffix('\n'))
        .map(|pid| pid.parse().unwrap())
        .collect()
}

/// On Unix: on `SIGTERM`, start one further marked child and exit — a
/// process that forks in answer to being stopped.
#[cfg(unix)]
mod fork_while_stopping {
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};

    static REQUESTED: AtomicBool = AtomicBool::new(false);

    extern "C" fn on_term(_: libc::c_int) {
        REQUESTED.store(true, Ordering::SeqCst);
    }

    pub(super) fn install(_ticks: &Path) {
        let handler: extern "C" fn(libc::c_int) = on_term;
        // SAFETY: the handler only stores to an atomic, which is
        // async-signal-safe.
        unsafe { libc::signal(libc::SIGTERM, handler as libc::sighandler_t) };
    }

    /// Once stopping is requested, start the child and exit, leaving it to be
    /// reparented and reaped.
    pub(super) fn poll(ticks: &Path) {
        if REQUESTED.load(Ordering::SeqCst) {
            super::start_marked_child(ticks);
            std::process::exit(0);
        }
    }

    /// Nothing is started before the stop asks for it.
    pub(super) fn await_ready(_ticks: &Path) {}
}

/// On Windows, where stopping is immediate termination a process never
/// observes: start marked children continuously, from before the stop until
/// the stop terminates this process, so children keep appearing while its
/// scans run.
#[cfg(windows)]
mod fork_while_stopping {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    /// A bound on the children one run starts if nothing ever stops it.
    const MOST: usize = 200;

    pub(super) fn install(ticks: &Path) {
        let ticks: PathBuf = ticks.to_path_buf();
        std::thread::spawn(move || {
            for _ in 0..MOST {
                super::start_marked_child(&ticks);
                std::thread::sleep(Duration::from_millis(10));
            }
        });
    }

    pub(super) fn poll(_ticks: &Path) {}

    /// Wait (bounded) for the first child, so the stop begins mid-stream.
    pub(super) fn await_ready(ticks: &Path) {
        let guard = std::time::Instant::now() + super::PATIENCE;
        while super::child_pids(ticks).is_empty() {
            assert!(std::time::Instant::now() < guard, "no child was started");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// A marker value no other test or run can share.
fn unique_value(label: &str) -> String {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!(
        "{label}-{}-{nanos}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    )
}

/// One detached heartbeating grandchild, killed on drop if a test left it
/// running.
struct Beater {
    pid: u32,
    ticks: PathBuf,
}

impl Beater {
    /// Start one through a helper: `mark` is the `(name, value)` it carries.
    fn start(dir: &Path, label: &str, mark: Option<(&str, &str)>, behaviour: &str) -> Beater {
        let ticks = dir.join(format!("{label}.ticks"));
        let mut helper = Command::new(std::env::current_exe().unwrap());
        helper
            .args(["role", "--exact", "--nocapture", "--test-threads=1"])
            .env(ROLE_VAR, "spawn")
            .env(TICKS_VAR, &ticks)
            .env(BEHAVIOUR_VAR, behaviour)
            .env_remove(MARK_VAR)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some((name, value)) = mark {
            helper.env(MARK_VAR, format!("{name}={value}"));
        }
        let status = helper.status().unwrap();
        assert!(status.success(), "the {label} helper failed: {status}");
        let pid = std::fs::read_to_string(ticks.with_extension("pid"))
            .unwrap_or_else(|error| panic!("the {label} helper reported no grandchild: {error}"))
            .trim()
            .parse()
            .unwrap();
        let beater = Beater { pid, ticks };
        beater.await_beats(2);
        beater
    }

    fn beats(&self) -> usize {
        count_beats(&self.ticks)
    }

    /// Wait (bounded) until the heartbeat file holds more than `n` lines.
    fn await_beats(&self, n: usize) {
        let guard = Instant::now() + PATIENCE;
        while self.beats() <= n {
            assert!(
                Instant::now() < guard,
                "{} never heartbeat past {n}",
                self.ticks.display()
            );
            std::thread::sleep(BEAT / 2);
        }
    }

    /// Assert it is still running and still heartbeating.
    fn assert_alive(&self) {
        assert!(running(self.pid), "{} was stopped", self.ticks.display());
        let before = self.beats();
        self.await_beats(before);
    }
}

impl Drop for Beater {
    fn drop(&mut self) {
        terminate(self.pid);
    }
}

fn count_beats(ticks: &Path) -> usize {
    std::fs::read_to_string(ticks)
        .map(|text| text.lines().count())
        .unwrap_or(0)
}

/// Assert `pid` is gone (bounded wait for the reap) and `ticks` gains no line
/// over a further second.
fn assert_stopped(pid: u32, ticks: &Path) {
    assert!(
        count_beats(ticks) > 0,
        "{} never heartbeat",
        ticks.display()
    );
    assert_gone(pid, ticks);
}

/// [`assert_stopped`] without requiring a heartbeat first, for a process that
/// can be stopped before its first one.
fn assert_gone(pid: u32, ticks: &Path) {
    await_exit(pid);
    let before = count_beats(ticks);
    std::thread::sleep(Duration::from_millis(1200));
    assert_eq!(
        count_beats(ticks),
        before,
        "{} kept heartbeating after the stop",
        ticks.display()
    );
}

/// Wait (bounded) for `pid` to be reaped.
fn await_exit(pid: u32) {
    let guard = Instant::now() + PATIENCE;
    while running(pid) {
        assert!(Instant::now() < guard, "pid {pid} still exists");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(unix)]
fn running(pid: u32) -> bool {
    let pid = libc::pid_t::try_from(pid).unwrap();
    // SAFETY: signal 0 to a positive pid only probes for its existence.
    unsafe { libc::kill(pid, 0) == 0 }
}

#[cfg(unix)]
fn terminate(pid: u32) {
    let pid = libc::pid_t::try_from(pid).unwrap();
    // SAFETY: a positive pid addresses exactly the fixture process this test
    // started; ESRCH when it is already gone is fine.
    unsafe { libc::kill(pid, libc::SIGKILL) };
}

#[cfg(windows)]
fn running(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: OpenProcess takes no pointers; null means no such process.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return false;
    }
    let mut code = 0u32;
    // SAFETY: `process` is live and `code` writable; the handle is closed after.
    let alive =
        unsafe { GetExitCodeProcess(process, &mut code) } != 0 && code == STILL_ACTIVE as u32;
    unsafe { CloseHandle(process) };
    alive
}

#[cfg(windows)]
fn terminate(pid: u32) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
    // SAFETY: the handle addresses the fixture process this test started and
    // is closed after use.
    unsafe {
        let process = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if !process.is_null() {
            TerminateProcess(process, 1);
            CloseHandle(process);
        }
    }
}

fn marker(value: &str) -> ProcessMarker {
    ProcessMarker {
        name: MARKER_NAME.to_string(),
        value: value.to_string(),
    }
}

#[test]
fn stopping_a_marked_grandchild_leaves_the_decoys_running() {
    let dir = ScratchDir::new("marked-decoys").unwrap();
    let value = unique_value("target");
    let other = unique_value("decoy");
    let target = Beater::start(dir.path(), "target", Some((MARKER_NAME, &value)), "plain");
    let same_name = Beater::start(
        dir.path(),
        "same-name",
        Some((MARKER_NAME, &other)),
        "plain",
    );
    let unmarked = Beater::start(dir.path(), "unmarked", None, "plain");

    let found = find_marked(&marker(&value)).unwrap();
    assert_eq!(
        found,
        [target.pid],
        "decoys: {} {}",
        same_name.pid,
        unmarked.pid
    );

    let report = stop_marked(&marker(&value), Duration::from_secs(5)).unwrap();
    assert!(report.survivors.is_empty(), "{report:?}");
    assert!(report.signalled.contains(&target.pid), "{report:?}");
    assert_stopped(target.pid, &target.ticks);

    same_name.assert_alive();
    unmarked.assert_alive();
    drop(same_name);
    drop(unmarked);
}

#[test]
fn every_simultaneous_match_is_found_stopped_and_reported() {
    let dir = ScratchDir::new("marked-several").unwrap();
    let value = unique_value("several");
    let marked: Vec<Beater> = ["first", "second", "third"]
        .into_iter()
        .map(|label| Beater::start(dir.path(), label, Some((MARKER_NAME, &value)), "plain"))
        .collect();
    let mut pids: Vec<u32> = marked.iter().map(|beater| beater.pid).collect();
    pids.sort_unstable();

    assert_eq!(find_marked(&marker(&value)).unwrap(), pids);
    let mut report = stop_marked(&marker(&value), Duration::from_secs(5)).unwrap();
    report.signalled.sort_unstable();
    assert_eq!(
        report,
        StopReport {
            signalled: pids,
            killed: Vec::new(),
            survivors: Vec::new(),
        }
    );
    for beater in &marked {
        assert_stopped(beater.pid, &beater.ticks);
    }
}

/// Unix only: Windows has no stop request to ignore — its stop is termination.
#[cfg(unix)]
#[test]
fn a_grandchild_that_ignores_the_stop_request_is_killed_by_the_deadline() {
    let dir = ScratchDir::new("marked-stubborn").unwrap();
    let value = unique_value("stubborn");
    let stubborn = Beater::start(
        dir.path(),
        "stubborn",
        Some((MARKER_NAME, &value)),
        "ignore-stop",
    );
    let deadline = Duration::from_secs(2);

    let started = Instant::now();
    let report = stop_marked(&marker(&value), deadline).unwrap();
    let elapsed = started.elapsed();

    assert!(report.survivors.is_empty(), "{report:?}");
    assert_eq!(report.signalled, [stubborn.pid], "{report:?}");
    // SIGTERM is ignored, so only the deadline's SIGKILL ends it.
    assert_eq!(report.killed, [stubborn.pid], "{report:?}");
    assert!(
        elapsed >= deadline && elapsed < deadline + Duration::from_secs(3),
        "stopped after {elapsed:?}, configured deadline {deadline:?}"
    );
    assert_stopped(stubborn.pid, &stubborn.ticks);
}

/// A marked process that starts a further marked child while it is being
/// stopped. On Unix the child is started from the `SIGTERM` handler, so it is
/// always one the stop's first scan could not have seen. On Windows the stop
/// terminates at once, so the parent starts children continuously instead and
/// any child created after the first scan listed the process table — up to
/// the moment the parent is terminated — is one only a re-scan finds.
#[test]
fn a_marked_child_started_while_stopping_is_stopped_too() {
    let dir = ScratchDir::new("marked-forking").unwrap();
    let value = unique_value("forking");
    let parent = Beater::start(
        dir.path(),
        "parent",
        Some((MARKER_NAME, &value)),
        "fork-while-stopping",
    );
    fork_while_stopping::await_ready(&parent.ticks);

    let report = stop_marked(&marker(&value), Duration::from_secs(5)).unwrap();
    assert!(report.survivors.is_empty(), "{report:?}");

    // The parent records each child before it can exit, so once it is gone
    // its pid file lists every child it started.
    await_exit(parent.pid);
    let children = child_pids(&parent.ticks);
    let _children: Vec<Beater> = children
        .iter()
        .map(|&pid| Beater {
            pid,
            ticks: child_ticks(&parent.ticks),
        })
        .collect();
    assert!(!children.is_empty(), "the parent never started a child");
    assert!(
        report.signalled.contains(&parent.pid),
        "the parent was never stopped: {report:?}"
    );
    for pid in &children {
        assert!(
            report.signalled.contains(pid) || report.killed.contains(pid),
            "child {pid} was never stopped: {report:?}"
        );
    }
    assert!(
        find_marked(&marker(&value)).unwrap().is_empty(),
        "a marked process outlived the stop"
    );
    assert_stopped(parent.pid, &parent.ticks);
    // A child can be stopped before its first heartbeat; its pid line is what
    // proves it was started. The children share one heartbeat file, so it is
    // observed once, after every one of them is gone.
    for &pid in &children {
        await_exit(pid);
    }
    assert_gone(children[0], &child_ticks(&parent.ticks));
}

#[test]
fn a_marked_caller_stops_the_others_but_never_itself() {
    let dir = ScratchDir::new("marked-caller").unwrap();
    let value = unique_value("caller");
    let target = Beater::start(dir.path(), "target", Some((MARKER_NAME, &value)), "plain");

    let results = dir.path().join("caller.results");
    let mut caller = Command::new(std::env::current_exe().unwrap())
        .args(["role", "--exact", "--nocapture", "--test-threads=1"])
        .env(ROLE_VAR, "stop-own-marker")
        .env(TICKS_VAR, &results)
        .env(MARK_VAR, format!("{MARKER_NAME}={value}"))
        .env(MARKER_NAME, &value)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let guard = Instant::now() + PATIENCE;
    let status = loop {
        if let Some(status) = caller.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= guard {
            let _ = caller.kill();
            let _ = caller.wait();
            panic!("the marked caller never finished stopping its marker");
        }
        std::thread::sleep(Duration::from_millis(20));
    };

    assert!(
        status.success(),
        "the marked caller was stopped by its own call: {status}"
    );
    let expected = StopReport {
        signalled: vec![target.pid],
        killed: Vec::new(),
        survivors: Vec::new(),
    };
    assert_eq!(
        std::fs::read_to_string(&results).unwrap(),
        format!("{}\n[{}]\n{expected:?}\n", caller.id(), target.pid),
        "the caller must find and stop the target, and only the target"
    );
    assert_stopped(target.pid, &target.ticks);
}

/// Linux only: `PR_SET_DUMPABLE` is how a same-user process there hides its
/// environment, and the other two platforms have no unprivileged equivalent to
/// set from a test.
#[cfg(target_os = "linux")]
#[test]
fn a_marked_process_whose_environment_is_unreadable_is_skipped() {
    let dir = ScratchDir::new("marked-protected").unwrap();
    let value = unique_value("protected");
    let hidden = Beater::start(
        dir.path(),
        "hidden",
        Some((MARKER_NAME, &value)),
        "protected",
    );

    assert!(
        std::fs::read(format!("/proc/{}/environ", hidden.pid)).is_err(),
        "the fixture's environment is readable, so this test proves nothing"
    );
    assert_eq!(find_marked(&marker(&value)).unwrap(), Vec::<u32>::new());
    let report = stop_marked(&marker(&value), Duration::from_secs(1)).unwrap();
    assert_eq!(report, StopReport::default());
    hidden.assert_alive();
}

#[test]
fn a_marker_that_cannot_name_a_variable_is_refused() {
    let named = |name: &str, value: &str| ProcessMarker {
        name: name.to_string(),
        value: value.to_string(),
    };
    for bad in [
        named("", "v"),
        named("A=B", "v"),
        named("A\0", "v"),
        named("A", "v\0"),
    ] {
        let error = find_marked(&bad).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput, "{bad:?}");
        let error = stop_marked(&bad, Duration::ZERO).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput, "{bad:?}");
    }
}

#[test]
fn only_the_exact_name_and_value_match_never_a_case_or_prefix_variant() {
    let dir = ScratchDir::new("marked-variants").unwrap();
    let value = unique_value("exact");
    let target = Beater::start(dir.path(), "target", Some((MARKER_NAME, &value)), "plain");
    let lower_name = MARKER_NAME.to_lowercase();
    let longer_name = format!("{MARKER_NAME}_X");
    let longer_value = format!("{value}x");
    let shorter_value = &value[..value.len() - 1];
    let decoys: Vec<Beater> = [
        ("lower-name", lower_name.as_str(), value.as_str()),
        ("longer-name", longer_name.as_str(), value.as_str()),
        ("longer-value", MARKER_NAME, longer_value.as_str()),
        ("shorter-value", MARKER_NAME, shorter_value),
    ]
    .into_iter()
    .map(|(label, name, value)| Beater::start(dir.path(), label, Some((name, value)), "plain"))
    .collect();

    assert_eq!(find_marked(&marker(&value)).unwrap(), [target.pid]);
    let report = stop_marked(&marker(&value), Duration::from_secs(5)).unwrap();
    assert_eq!(report.signalled, [target.pid], "{report:?}");
    assert_stopped(target.pid, &target.ticks);
    for decoy in &decoys {
        decoy.assert_alive();
    }
}

#[test]
fn a_zero_deadline_escalates_at_once() {
    let dir = ScratchDir::new("marked-zero").unwrap();
    let value = unique_value("zero");
    let stubborn = Beater::start(
        dir.path(),
        "stubborn",
        Some((MARKER_NAME, &value)),
        "ignore-stop",
    );

    let started = Instant::now();
    let report = stop_marked(&marker(&value), Duration::ZERO).unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
    assert!(report.survivors.is_empty(), "{report:?}");
    assert_eq!(report.signalled, [stubborn.pid], "{report:?}");
    if cfg!(unix) {
        // SIGTERM is ignored; with no time to wait, SIGKILL follows at once.
        assert_eq!(report.killed, [stubborn.pid], "{report:?}");
    }
    assert_stopped(stubborn.pid, &stubborn.ticks);
}

#[test]
fn an_unrepresentable_deadline_waits_for_the_match_to_go() {
    let dir = ScratchDir::new("marked-unbounded").unwrap();
    let value = unique_value("unbounded");
    let target = Beater::start(dir.path(), "target", Some((MARKER_NAME, &value)), "plain");

    let report = stop_marked(&marker(&value), Duration::MAX).unwrap();
    assert_eq!(
        report,
        StopReport {
            signalled: vec![target.pid],
            killed: Vec::new(),
            survivors: Vec::new(),
        }
    );
    assert_stopped(target.pid, &target.ticks);
}

/// Windows only: a 32-bit process runs under WOW64 there, with its
/// environment behind a 32-bit PEB the native layout does not describe. Both
/// Windows CI legs (x64 and ARM64) carry 32-bit x86 binaries in `SysWOW64`.
#[cfg(windows)]
#[test]
fn a_marked_32_bit_process_is_found_and_stopped() {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    let cmd32 = Path::new(&root).join("SysWOW64").join("cmd.exe");
    assert!(cmd32.is_file(), "no 32-bit cmd.exe at {}", cmd32.display());
    let value = unique_value("wow64");
    // `/k` with a piped stdin waits on commands that never come, so it stays
    // alive, starts nothing, and exits only when stopped.
    let mut child = Command::new(&cmd32)
        .args(["/d", "/q", "/k"])
        .env(MARKER_NAME, &value)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    let guard = Instant::now() + PATIENCE;
    while !find_marked(&marker(&value)).unwrap().contains(&pid) {
        if Instant::now() >= guard {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the 32-bit process carrying the marker was never found");
        }
        std::thread::sleep(BEAT);
    }

    let report = stop_marked(&marker(&value), Duration::from_secs(5)).unwrap();
    assert!(report.survivors.is_empty(), "{report:?}");
    assert!(report.signalled.contains(&pid), "{report:?}");
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= guard {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the 32-bit process survived its stop");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
