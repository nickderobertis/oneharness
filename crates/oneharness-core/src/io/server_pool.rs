//! The generic sidecar-server pool.
//!
//! Most controllable harnesses reach their turn only through a server process
//! (`codex app-server`, `opencode serve`, `crush server`, `goose acp`,
//! `copilot --acp`); Claude Code does not. That difference is declared per
//! harness in [`crate::domain::control::ServerSpec`] rather than special-cased,
//! so a server costing ~137MB is started once and shared by every dispatch with
//! the same key instead of once per turn.
//!
//! **Membership is a lease held by a live process, never a counter.** A counter
//! leaks a permanently-live server the first time a dispatch is `SIGKILL`ed
//! before it can decrement — and dispatch trees are killed routinely. Each
//! dispatch instead writes a lease file naming its own pid; reclamation asks
//! the OS whether that pid is still alive, so a lease can never outlive its
//! holder. Everything on disk is therefore self-healing: no shutdown hook is
//! load-bearing.
//!
//! Layout under `<state>/oneharness/servers/`:
//!
//! ```text
//! <pool-key>/server.json     the running server: pid, launch argv, address
//! <pool-key>/leases/<id>     one file per live dispatch, holding its pid
//! <pool-key>/.lock           whole-entry mutex around start/stop decisions
//! ```

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::domain::control::{is_pool_key, ServerAddress, ServerSpec};

/// How long an idle server (no live lease) is kept before reclamation, so a
/// burst of short dispatches does not thrash a heavyweight process up and down.
pub const DEFAULT_LINGER: Duration = Duration::from_secs(60);

const LEASE_DIR: &str = "leases";
const SERVER_FILE: &str = "server.json";
const LOCK_FILE: &str = ".lock";

/// The record describing the server currently backing a pool entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerRecord {
    /// The server process's pid.
    pub pid: u32,
    /// The full argv it was launched with (for diagnostics and drift).
    pub argv: Vec<String>,
    /// How it is reached — the transport and its coordinates together, so a
    /// reader can never see a port that belongs to a socket.
    pub address: ServerAddress,
    /// Epoch seconds the entry last had no live lease; `None` while leased.
    pub idle_since: Option<u64>,
}

/// A lease on a pooled server, released when dropped.
///
/// The drop is a courtesy, not the correctness mechanism: reclamation verifies
/// the holder pid is alive, so a lease whose holder was `SIGKILL`ed is
/// reclaimed by the next pool operation regardless.
#[derive(Debug)]
pub struct ServerLease {
    entry: PathBuf,
    lease_file: PathBuf,
    record: ServerRecord,
    linger: Duration,
}

impl ServerLease {
    /// The server this lease holds.
    #[must_use]
    pub fn record(&self) -> &ServerRecord {
        &self.record
    }

    /// The pool entry directory backing it.
    #[must_use]
    pub fn entry(&self) -> &Path {
        &self.entry
    }
}

impl Drop for ServerLease {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.lease_file);
        // Mark the entry idle so a later pool operation can time out the linger.
        if let Ok(_guard) = EntryLock::acquire(&self.entry) {
            let _ = reconcile(&self.entry, self.linger);
        }
    }
}

/// The pool root: `<state>/oneharness/servers`, or an explicit override.
#[must_use]
pub fn resolve_root(configured: Option<&str>) -> Option<PathBuf> {
    match configured {
        Some(p) if !p.is_empty() => Some(PathBuf::from(p)),
        _ => state_dir().map(|d| d.join("oneharness").join("servers")),
    }
}

/// The per-user state directory, resolved exactly like the history and session
/// stores: `%LOCALAPPDATA%` on Windows; `$XDG_STATE_HOME` (else
/// `~/.local/state`) elsewhere.
fn state_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        return std::env::var_os("LOCALAPPDATA")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
    }
    if let Some(xdg) = std::env::var_os("XDG_STATE_HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(xdg));
    }
    std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .map(|home| PathBuf::from(home).join(".local").join("state"))
}

/// Everything the pool needs to start a server for one key.
///
/// Built only through [`LaunchPlan::new`], so its argv always has a program in
/// it: a plan that cannot name what to spawn is not a smaller plan, it is not a
/// plan, and the pool would otherwise index into an empty vector.
#[derive(Debug)]
pub struct LaunchPlan {
    /// argv[0] is the harness binary; the rest is `ServerSpec::launch` plus any
    /// caller overrides and address flags. Never empty.
    argv: Vec<String>,
    /// Environment applied to the server process.
    env: Vec<(String, String)>,
    /// The address the server will be reachable at.
    address: ServerAddress,
}

impl LaunchPlan {
    /// The plan for `spec` against `bin`, with `overrides` appended after the
    /// declared launch args. Pure assembly; nothing is spawned.
    ///
    /// Refuses an address that does not speak the transport the spec declares —
    /// a TCP address for a stdio server names something no reader of that pool
    /// entry could dial, and the mismatch would only surface as a connection
    /// failure long after the server was started.
    pub fn new(
        bin: &str,
        spec: &ServerSpec,
        overrides: &[String],
        address: ServerAddress,
        env: Vec<(String, String)>,
    ) -> io::Result<Self> {
        if address.transport() != spec.transport {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "a `{:?}` address cannot back a server declared as `{:?}`",
                    address.transport(),
                    spec.transport
                ),
            ));
        }
        let mut argv = vec![bin.to_string()];
        argv.extend(spec.launch.iter().map(|a| (*a).to_string()));
        argv.extend(overrides.iter().cloned());
        Ok(LaunchPlan { argv, env, address })
    }

    /// The full argv the server is spawned with (argv[0] is the program).
    #[must_use]
    pub fn argv(&self) -> &[String] {
        &self.argv
    }

    /// The address the server will be reachable at.
    #[must_use]
    pub fn address(&self) -> &ServerAddress {
        &self.address
    }
}

/// Take a lease on the server for `key` under `root`, starting one if none is
/// live. Reuse is the normal path: a second dispatch with the same key gets the
/// same record without spawning anything.
pub fn acquire(
    root: &Path,
    key: &str,
    plan: &LaunchPlan,
    linger: Duration,
) -> io::Result<ServerLease> {
    // `key` reaches this published entry point from a caller, not necessarily
    // from `pool_key`; refuse anything that would place a lease tree somewhere
    // other than one directory under the root.
    if !is_pool_key(key) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("`{key}` is not a valid pool key (use `domain::control::pool_key`)"),
        ));
    }
    let entry = root.join(key);
    fs::create_dir_all(entry.join(LEASE_DIR))?;
    let _guard = EntryLock::acquire(&entry)?;

    let record = match reconcile(&entry, linger)? {
        Some(live) => live,
        None => start(&entry, plan)?,
    };

    let lease_file = entry.join(LEASE_DIR).join(lease_id());
    fs::write(&lease_file, std::process::id().to_string())?;
    // Now leased again: clear the idle stamp so the linger only ever measures
    // uninterrupted idleness.
    let cleared = ServerRecord {
        idle_since: None,
        ..record.clone()
    };
    write_record(&entry, &cleared)?;

    Ok(ServerLease {
        entry,
        lease_file,
        record: cleared,
        linger,
    })
}

/// Drop dead leases, then decide the entry's fate: keep a live server, or
/// reclaim one whose idle linger has expired. Returns the still-usable server,
/// if any. This is the single reclamation path — every pool operation runs it,
/// which is why a `SIGKILL`ed holder cannot leak a server.
fn reconcile(entry: &Path, linger: Duration) -> io::Result<Option<ServerRecord>> {
    reap_finished();
    let leases = entry.join(LEASE_DIR);
    let mut live_leases = 0usize;
    if let Ok(entries) = fs::read_dir(&leases) {
        for lease in entries.flatten() {
            let path = lease.path();
            let holder = fs::read_to_string(&path)
                .ok()
                .and_then(|text| text.trim().parse::<u32>().ok());
            match holder {
                Some(pid) if pid_alive(pid) => live_leases += 1,
                // No readable pid, or a pid the OS no longer knows: the holder
                // is gone, however it went.
                _ => {
                    let _ = fs::remove_file(&path);
                }
            }
        }
    }

    let Some(mut record) = read_record(entry) else {
        return Ok(None);
    };
    if !pid_alive(record.pid) {
        // The server died on its own; forget it so the next acquire restarts.
        let _ = fs::remove_file(entry.join(SERVER_FILE));
        return Ok(None);
    }
    if live_leases > 0 {
        record.idle_since = None;
        write_record(entry, &record)?;
        return Ok(Some(record));
    }

    // No live lease: the linger measures how long that has been true. An entry
    // seen idle for the first time is stamped now, so `now - since` is 0 and a
    // zero linger reclaims immediately while a real one keeps the process warm.
    let now = epoch_secs();
    let since = record.idle_since.unwrap_or(now);
    if now.saturating_sub(since) >= linger.as_secs() {
        kill(record.pid);
        let _ = fs::remove_file(entry.join(SERVER_FILE));
        return Ok(None);
    }
    if record.idle_since.is_none() {
        record.idle_since = Some(now);
        write_record(entry, &record)?;
    }
    Ok(Some(record))
}

/// Reclaim every pool entry whose server has no live lease and whose linger has
/// expired. Lazily driven (any pool operation, or an explicit sweep) rather
/// than by a daemon: there is no oneharness process alive to do it otherwise.
pub fn sweep(root: &Path, linger: Duration) -> io::Result<usize> {
    let mut reclaimed = 0;
    let Ok(entries) = fs::read_dir(root) else {
        return Ok(0);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Ok(_guard) = EntryLock::acquire(&path) else {
            continue;
        };
        let had = read_record(&path).is_some();
        if reconcile(&path, linger)?.is_none() && had {
            reclaimed += 1;
        }
    }
    Ok(reclaimed)
}

/// Spawn the server and record it. The child is detached from this dispatch's
/// process tree on purpose: it must outlive the run that started it, which is
/// the entire point of pooling.
fn start(entry: &Path, plan: &LaunchPlan) -> io::Result<ServerRecord> {
    let (program, args) = plan
        .argv
        .split_first()
        .expect("LaunchPlan::new always puts the program in argv[0]");
    let mut command = std::process::Command::new(program);
    command
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    for (key, value) in &plan.env {
        command.env(key, value);
    }
    detach(&mut command);
    let child = command.spawn()?;
    let pid = child.id();
    remember_spawned(child);
    let record = ServerRecord {
        pid,
        argv: plan.argv.clone(),
        address: plan.address.clone(),
        idle_since: None,
    };
    write_record(entry, &record)?;
    Ok(record)
}

#[cfg(unix)]
fn detach(command: &mut std::process::Command) {
    use std::os::unix::process::CommandExt;
    // Its own session/process group, so terminating the dispatch's tree does
    // not take the shared server down with it.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(not(unix))]
fn detach(_command: &mut std::process::Command) {}

/// Whether the OS still knows `pid`. This is the pool's whole correctness
/// argument, so it asks the kernel rather than trusting any bookkeeping.
#[must_use]
pub fn pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // Signal 0 performs the existence/permission check without delivering.
        // A zombie still answers, but a zombie server is reaped by its parent's
        // exit, so treating it as alive only defers reclamation by one sweep.
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return false;
            }
            CloseHandle(handle);
            true
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        false
    }
}

/// Servers this process itself spawned, retained only so a still-running
/// dispatch can *reap* one it later reclaims. Without this the terminated
/// server lingers as a zombie for the life of the parent — harmless in a short
/// CLI run, but it would make "is the pid gone?" answer wrong for a long-lived
/// supervisor. A server outliving its starter is reparented and reaped by init.
fn spawned() -> &'static std::sync::Mutex<Vec<std::process::Child>> {
    static SPAWNED: std::sync::OnceLock<std::sync::Mutex<Vec<std::process::Child>>> =
        std::sync::OnceLock::new();
    SPAWNED.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

fn remember_spawned(child: std::process::Child) {
    if let Ok(mut children) = spawned().lock() {
        children.push(child);
    }
}

/// Reap a server this process started, if it is one of ours.
fn reap_spawned(pid: u32) {
    let Ok(mut children) = spawned().lock() else {
        return;
    };
    if let Some(index) = children.iter().position(|child| child.id() == pid) {
        let mut child = children.remove(index);
        let _ = child.wait();
    }
}

/// Reap any server of ours that has already exited, so a dead pid reads as dead
/// rather than as a zombie that still answers signal 0.
fn reap_finished() {
    let Ok(mut children) = spawned().lock() else {
        return;
    };
    children.retain_mut(|child| !matches!(child.try_wait(), Ok(Some(_))));
}

fn kill(pid: u32) {
    #[cfg(unix)]
    unsafe {
        // The server was started in its own session; signal the whole group so
        // a launcher shim's native child goes with it.
        libc::kill(-(pid as libc::pid_t), libc::SIGTERM);
        libc::kill(pid as libc::pid_t, libc::SIGTERM);
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, TerminateProcess, PROCESS_TERMINATE,
        };
        unsafe {
            let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if !handle.is_null() {
                TerminateProcess(handle, 1);
                CloseHandle(handle);
            }
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
    }
    reap_spawned(pid);
}

fn read_record(entry: &Path) -> Option<ServerRecord> {
    let text = fs::read_to_string(entry.join(SERVER_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_record(entry: &Path, record: &ServerRecord) -> io::Result<()> {
    let mut text = serde_json::to_string_pretty(record)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    text.push('\n');
    let tmp = entry.join("server.json.tmp");
    fs::write(&tmp, text)?;
    fs::rename(tmp, entry.join(SERVER_FILE))
}

fn lease_id() -> String {
    format!(
        "{}-{}.lease",
        std::process::id(),
        uuid::Uuid::now_v7().simple()
    )
}

fn epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A whole-entry mutex around start/stop decisions, so two dispatches racing to
/// first-acquire the same key cannot both spawn a server. Uses the same
/// cross-platform advisory locking as the history index.
struct EntryLock {
    _file: fs::File,
}

impl EntryLock {
    fn acquire(entry: &Path) -> io::Result<Self> {
        use fs2::FileExt;
        fs::create_dir_all(entry)?;
        let file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(entry.join(LOCK_FILE))?;
        file.lock_exclusive()?;
        Ok(EntryLock { _file: file })
    }
}

impl Drop for EntryLock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self._file);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::control::ServerTransport;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oh-pool-{}-{}-{}",
            tag,
            std::process::id(),
            epoch_secs()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A stand-in server: a real, long-lived child process. The pool only ever
    /// asks the OS about pids, so a `sleep` exercises exactly the same paths a
    /// `codex app-server` would.
    fn sleeper_plan() -> LaunchPlan {
        LaunchPlan::new(
            "sleep",
            &ServerSpec {
                launch: &["120"],
                key_env: &[],
                transport: ServerTransport::Stdio,
            },
            &[],
            ServerAddress::Stdio,
            Vec::new(),
        )
        .expect("a stdio address backs a stdio spec")
    }

    #[test]
    fn acquire_refuses_a_key_that_is_not_a_pool_key() {
        let root = temp_root("badkey");
        let err = acquire(&root, "../escape", &sleeper_plan(), DEFAULT_LINGER).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(!root.parent().unwrap().join("escape").exists());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn resolve_root_prefers_the_configured_path() {
        assert_eq!(
            resolve_root(Some("/custom/servers")),
            Some(PathBuf::from("/custom/servers"))
        );
        assert_eq!(resolve_root(Some("")), resolve_root(None));
    }

    #[test]
    fn launch_plan_appends_declared_args_then_overrides() {
        let spec = ServerSpec {
            launch: &["serve"],
            key_env: &[],
            transport: ServerTransport::Tcp,
        };
        let address = ServerAddress::Tcp {
            host: "127.0.0.1".to_string(),
            port: 7777,
        };
        let plan = LaunchPlan::new(
            "opencode",
            &spec,
            &["--port".to_string(), "7777".to_string()],
            address.clone(),
            Vec::new(),
        )
        .unwrap();
        assert_eq!(plan.argv(), ["opencode", "serve", "--port", "7777"]);
        assert_eq!(plan.address(), &address);

        // An address that speaks a different transport is refused, not asserted
        // away in debug builds only.
        let mismatched = LaunchPlan::new("opencode", &spec, &[], ServerAddress::Stdio, Vec::new());
        assert_eq!(mismatched.unwrap_err().kind(), io::ErrorKind::InvalidInput);
    }

    #[cfg(unix)]
    #[test]
    fn one_server_is_reused_across_dispatches_sharing_a_key() {
        let root = temp_root("reuse");
        let first = acquire(&root, "k", &sleeper_plan(), DEFAULT_LINGER).unwrap();
        let second = acquire(&root, "k", &sleeper_plan(), DEFAULT_LINGER).unwrap();
        assert_eq!(first.record().pid, second.record().pid);
        assert!(pid_alive(first.record().pid));

        // Two live leases on the one entry.
        let leases = fs::read_dir(root.join("k").join(LEASE_DIR))
            .unwrap()
            .count();
        assert_eq!(leases, 2);

        let pid = first.record().pid;
        drop(first);
        drop(second);
        // Still alive: the linger has not expired, which is what keeps bursty
        // rounds from thrashing the process.
        assert!(pid_alive(pid));
        assert_eq!(sweep(&root, DEFAULT_LINGER).unwrap(), 0);
        assert!(pid_alive(pid));

        // Once idle past the linger it is reclaimed.
        assert_eq!(sweep(&root, Duration::from_secs(0)).unwrap(), 1);
        wait_until_dead(pid);
        fs::remove_dir_all(&root).ok();
    }

    #[cfg(unix)]
    #[test]
    fn a_sigkilled_lease_holder_does_not_leak_the_server() {
        let root = temp_root("sigkill");
        // A real second process takes the lease and is killed without releasing
        // it — the exact leak a reference counter cannot survive.
        let holder = std::process::Command::new("sleep")
            .arg("120")
            .spawn()
            .unwrap();
        let holder_pid = holder.id();
        let entry = root.join("k");
        fs::create_dir_all(entry.join(LEASE_DIR)).unwrap();
        let ours = acquire(&root, "k", &sleeper_plan(), DEFAULT_LINGER).unwrap();
        let server_pid = ours.record().pid;
        fs::write(
            entry.join(LEASE_DIR).join("stray.lease"),
            holder_pid.to_string(),
        )
        .unwrap();
        drop(ours);

        // While the stray holder lives, the server is retained even past the
        // linger: reclamation is by lease liveness, not by elapsed time.
        assert_eq!(sweep(&root, Duration::from_secs(0)).unwrap(), 0);
        assert!(pid_alive(server_pid));

        let mut holder = holder;
        unsafe {
            libc::kill(holder_pid as libc::pid_t, libc::SIGKILL);
        }
        holder.wait().ok();

        // The holder never released anything; the pool notices it is gone.
        assert_eq!(sweep(&root, Duration::from_secs(0)).unwrap(), 1);
        wait_until_dead(server_pid);
        assert!(!entry.join(LEASE_DIR).join("stray.lease").exists());
        fs::remove_dir_all(&root).ok();
    }

    #[cfg(unix)]
    #[test]
    fn a_server_that_died_is_replaced_on_the_next_acquire() {
        let root = temp_root("dead");
        let first = acquire(&root, "k", &sleeper_plan(), DEFAULT_LINGER).unwrap();
        let dead_pid = first.record().pid;
        unsafe {
            libc::kill(dead_pid as libc::pid_t, libc::SIGKILL);
        }
        wait_until_dead(dead_pid);

        let second = acquire(&root, "k", &sleeper_plan(), DEFAULT_LINGER).unwrap();
        assert_ne!(second.record().pid, dead_pid);
        assert!(pid_alive(second.record().pid));
        let pid = second.record().pid;
        drop(second);
        sweep(&root, Duration::from_secs(0)).unwrap();
        wait_until_dead(pid);
        fs::remove_dir_all(&root).ok();
    }

    #[cfg(unix)]
    fn wait_until_dead(pid: u32) {
        // A terminated child may briefly remain a zombie; give the reap a moment.
        for _ in 0..100 {
            if !pid_alive(pid) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        // Zombies answer signal 0; the process is no longer running either way,
        // which is what the reclamation contract is about.
    }
}
