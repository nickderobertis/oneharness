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
/// How a `beat` meets a stop request: `plain`, `ignore-stop`, `fork-on-stop`;
/// or `protected`, hiding its environment from other processes (Linux).
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
        "ignore-stop" => ignore_stop_requests(),
        "fork-on-stop" => fork_on_stop::install(),
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
        if behaviour == "fork-on-stop" && fork_on_stop::requested() {
            fork_on_stop::start_child_and_exit(&ticks);
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

#[cfg(windows)]
fn ignore_stop_requests() {
    // Windows' nearest thing to SIGTERM is a console control event.
    // SAFETY: a null handler with TRUE makes the process ignore Ctrl+C.
    unsafe { windows_sys::Win32::System::Console::SetConsoleCtrlHandler(None, 1) };
}

/// On `SIGTERM`, start a further marked `beat` (it inherits the marker) and
/// exit — a process that forks while being stopped.
#[cfg(unix)]
mod fork_on_stop {
    use std::path::Path;
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicBool, Ordering};

    static REQUESTED: AtomicBool = AtomicBool::new(false);

    extern "C" fn on_term(_: libc::c_int) {
        REQUESTED.store(true, Ordering::SeqCst);
    }

    pub(super) fn install() {
        let handler: extern "C" fn(libc::c_int) = on_term;
        // SAFETY: the handler only stores to an atomic, which is
        // async-signal-safe.
        unsafe { libc::signal(libc::SIGTERM, handler as libc::sighandler_t) };
    }

    pub(super) fn requested() -> bool {
        REQUESTED.load(Ordering::SeqCst)
    }

    /// Start the child and exit, leaving it to be reparented and reaped.
    pub(super) fn start_child_and_exit(ticks: &Path) -> ! {
        let child_ticks = super::child_ticks(ticks);
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["role", "--exact", "--nocapture", "--test-threads=1"])
            .env(super::ROLE_VAR, "beat")
            .env(super::TICKS_VAR, &child_ticks)
            .env(super::BEHAVIOUR_VAR, "plain")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        std::fs::write(child_ticks.with_extension("pid"), child.id().to_string()).unwrap();
        std::process::exit(0);
    }
}

#[cfg(windows)]
mod fork_on_stop {
    pub(super) fn install() {}
    pub(super) fn requested() -> bool {
        false
    }
    pub(super) fn start_child_and_exit(_ticks: &std::path::Path) -> ! {
        unreachable!("a stop request on Windows is termination, never observed")
    }
}

#[cfg(unix)]
fn child_ticks(ticks: &Path) -> PathBuf {
    ticks.with_extension("child.ticks")
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
    let guard = Instant::now() + PATIENCE;
    while running(pid) {
        assert!(Instant::now() < guard, "pid {pid} still exists");
        std::thread::sleep(Duration::from_millis(20));
    }
    let before = count_beats(ticks);
    std::thread::sleep(Duration::from_millis(1200));
    assert_eq!(
        count_beats(ticks),
        before,
        "{} kept heartbeating after the stop",
        ticks.display()
    );
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
    if cfg!(unix) {
        // SIGTERM is ignored, so only the deadline's SIGKILL ends it.
        assert_eq!(report.killed, [stubborn.pid], "{report:?}");
        assert!(
            elapsed >= deadline && elapsed < deadline + Duration::from_secs(3),
            "stopped after {elapsed:?}, configured deadline {deadline:?}"
        );
    } else {
        // Windows has no stop request to ignore: it is terminated at once.
        assert!(report.killed.is_empty(), "{report:?}");
        assert!(elapsed < deadline, "stopped after {elapsed:?}");
    }
    assert_stopped(stubborn.pid, &stubborn.ticks);
}

/// Unix only, by construction: on Windows the stop request *is* termination,
/// which a process cannot answer by starting anything.
#[cfg(unix)]
#[test]
fn a_marked_child_started_while_stopping_is_stopped_too() {
    let dir = ScratchDir::new("marked-forking").unwrap();
    let value = unique_value("forking");
    let parent = Beater::start(
        dir.path(),
        "parent",
        Some((MARKER_NAME, &value)),
        "fork-on-stop",
    );

    let report = stop_marked(&marker(&value), Duration::from_secs(5)).unwrap();
    assert!(report.survivors.is_empty(), "{report:?}");

    let child_ticks = child_ticks(&parent.ticks);
    let child_pid: u32 = std::fs::read_to_string(child_ticks.with_extension("pid"))
        .expect("the parent never started its child")
        .trim()
        .parse()
        .unwrap();
    let _child = Beater {
        pid: child_pid,
        ticks: child_ticks.clone(),
    };
    assert!(
        report.signalled.contains(&parent.pid) && report.signalled.contains(&child_pid),
        "both the parent and the child it started must have been stopped: {report:?}"
    );
    assert_stopped(parent.pid, &parent.ticks);
    // The child can be stopped before its first heartbeat; its pid file is
    // what proves it was started.
    assert_gone(child_pid, &child_ticks);
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
