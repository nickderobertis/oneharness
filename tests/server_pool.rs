//! The sidecar-server pool, driven through its **public library surface** the
//! way a consumer (this CLI, or a sibling tool depending on `oneharness-core`)
//! uses it — from outside the crate, against real processes.
//!
//! The in-crate unit tests cover the same reclamation rules; this exists because
//! the pool is a *published* API whose whole promise is cross-process: a lease is
//! held by a live process, so the interesting failures — a dispatch that dies
//! without releasing, and the pid it left behind being handed to a stranger —
//! can only be exercised with real processes and the public functions, not with
//! in-module internals.

#![cfg(unix)]

use std::path::PathBuf;
use std::time::Duration;

use oneharness_core::domain::control::{ServerAddress, ServerSpec, ServerTransport};
use oneharness_core::io::scratch::ScratchDir;
use oneharness_core::io::server_pool::{
    self, LaunchArgv, LaunchPlan, LeaseHolder, Pid, ProcessIdentity, ServerRecord,
};

fn pool_root(tag: &str) -> ScratchDir {
    ScratchDir::new(&format!("oh-pool-e2e-{tag}-{}", std::process::id()))
}

/// A stand-in server: a real, long-lived process. The pool only ever asks the OS
/// about pids, so this exercises exactly the paths a `codex app-server` would.
fn plan() -> LaunchPlan {
    LaunchPlan::new(
        "sleep",
        &ServerSpec {
            address_args: &[],
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
fn the_pool_root_defaults_under_the_platform_state_dir_and_honors_an_override() {
    // The default is what a consumer gets when it configures nothing, so it is
    // part of the published surface — an override that silently fell back would
    // put a sibling tool's servers in someone else's pool.
    assert_eq!(
        server_pool::resolve_root(Some("/custom/servers")),
        Some(PathBuf::from("/custom/servers"))
    );
    let default = server_pool::resolve_root(None).expect("a state dir on this platform");
    assert!(default.is_absolute(), "{default:?}");
    assert!(default.ends_with("oneharness/servers"), "{default:?}");
    // An empty override is not an override.
    assert_eq!(server_pool::resolve_root(Some("")), Some(default));
}

#[test]
fn a_record_with_a_recycled_live_pid_is_not_reused_even_when_argv_matches() {
    // A pool entry says a pid is this key's server. It is the only thing that
    // says so, and it outlives every dispatch — so it can name a pid the OS has
    // since recycled onto an unrelated process, or a launch from before the
    // argv changed. Reusing it on liveness alone submits the turn to whatever
    // answers there and later signals that pid.
    let root = pool_root("foreign");
    let key = oneharness_core::domain::control::pool_key("opencode", &[], &[]);
    let entry = root.join(key.as_str());
    std::fs::create_dir_all(entry.join("leases")).unwrap();

    // A real, live process that is not this plan's server, recorded through the
    // very types the pool persists — so the entry is one it would itself write.
    let mut stranger = std::process::Command::new("sleep")
        .arg("120")
        .spawn()
        .unwrap();
    let stranger_pid = Pid::new(stranger.id()).unwrap();
    std::fs::write(
        entry.join("server.json"),
        serde_json::to_string(&ServerRecord {
            pid: stranger_pid,
            // A recycled pid can belong to the exact same executable and argv;
            // only its kernel birth identity distinguishes the incarnation.
            identity: ProcessIdentity::new("previous-incarnation".to_string()).unwrap(),
            argv: LaunchArgv::new(vec!["sleep".to_string(), "120".to_string()]).unwrap(),
            address: ServerAddress::Stdio,
            idle_since: None,
        })
        .unwrap(),
    )
    .unwrap();

    let lease = server_pool::acquire(&root, &key, &plan(), server_pool::DEFAULT_LINGER).unwrap();
    assert_ne!(
        lease.record().pid,
        stranger_pid,
        "a live pid this plan did not launch was adopted as its server"
    );
    assert_eq!(
        lease.record().argv.as_slice(),
        ["sleep", "120"],
        "the entry must now describe the server this dispatch started"
    );
    assert!(
        server_pool::pid_alive(stranger_pid),
        "a process the pool could not vouch for must not be signalled"
    );

    let started = lease.record().pid;
    drop(lease);
    server_pool::sweep(&root, Duration::from_secs(0)).unwrap();
    assert!(!server_pool::pid_alive(started));
    stranger.kill().unwrap();
    stranger.wait().unwrap();
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_lease_held_by_a_recycled_pid_does_not_retain_the_server() {
    // A lease file outlives the dispatch that wrote it whenever that dispatch
    // was `SIGKILL`ed, and the OS reuses pids. Judged on liveness alone, the
    // lease is then held by whatever process inherited the number — for as long
    // as that stranger lives, on an entry every sweep agrees is still held. So
    // the leak a lease exists to prevent comes back, and unbounded.
    //
    // The stand-in is exactly that stranger: a live pid whose incarnation is
    // not the one that took the lease.
    let root = pool_root("recycled");
    let key = oneharness_core::domain::control::pool_key("crush", &[], &[]);
    let lease = server_pool::acquire(&root, &key, &plan(), server_pool::DEFAULT_LINGER).unwrap();
    let server = lease.record().pid;
    drop(lease);

    let mut stranger = std::process::Command::new("sleep")
        .arg("120")
        .spawn()
        .unwrap();
    let stranger_pid = Pid::new(stranger.id()).unwrap();
    let lease_file = root
        .join(key.as_str())
        .join("leases")
        .join("recycled.lease");
    std::fs::write(
        &lease_file,
        serde_json::to_string(&LeaseHolder {
            pid: stranger_pid,
            identity: ProcessIdentity::new("previous-incarnation".to_string()).unwrap(),
        })
        .unwrap(),
    )
    .unwrap();
    assert!(
        server_pool::pid_alive(stranger_pid),
        "the stand-in must be alive, or this proves nothing about liveness"
    );

    assert_eq!(
        server_pool::sweep(&root, Duration::from_secs(0)).unwrap(),
        1,
        "a lease no living holder took must not retain the server"
    );
    assert!(!root.join(key.as_str()).join("server.json").exists());
    assert!(
        !lease_file.exists(),
        "the stale lease must be dropped, not merely disbelieved once"
    );
    assert!(!server_pool::pid_alive(server));
    assert!(
        server_pool::pid_alive(stranger_pid),
        "a process the pool could not vouch for must not be signalled"
    );

    stranger.kill().unwrap();
    stranger.wait().unwrap();
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn two_dispatches_sharing_a_key_share_one_server_and_a_dead_holder_cannot_leak_it() {
    let root = pool_root("shared");
    let key = oneharness_core::domain::control::pool_key(
        "codex",
        &[("CODEX_HOME".to_string(), Some("/tmp/home".to_string()))],
        &[],
    );

    // Two leases on one key start one server, not two.
    let first = server_pool::acquire(&root, &key, &plan(), server_pool::DEFAULT_LINGER).unwrap();
    let second = server_pool::acquire(&root, &key, &plan(), server_pool::DEFAULT_LINGER).unwrap();
    let server = first.record().pid;
    assert_eq!(second.record().pid, server);
    assert_eq!(first.record().address, ServerAddress::Stdio);
    assert!(server_pool::pid_alive(server));

    // A per-thread setting must not fork the pool: only the declared key
    // material does. A different `CODEX_HOME` is a different server.
    let other_key = oneharness_core::domain::control::pool_key(
        "codex",
        &[("CODEX_HOME".to_string(), Some("/tmp/other".to_string()))],
        &[],
    );
    let other =
        server_pool::acquire(&root, &other_key, &plan(), server_pool::DEFAULT_LINGER).unwrap();
    assert_ne!(other.record().pid, server);
    let other_pid = other.record().pid;
    drop(other);

    // A THIRD holder that dies without releasing — the leak a reference counter
    // cannot survive, and the reason membership is a lease naming a live pid.
    let holder = std::process::Command::new("sleep")
        .arg("120")
        .spawn()
        .unwrap();
    let holder_pid = Pid::new(holder.id()).unwrap();
    let stray = LeaseHolder {
        pid: holder_pid,
        identity: server_pool::process_identity(holder_pid)
            .expect("a live process has an identity"),
    };
    std::fs::write(
        root.join(key.as_str()).join("leases").join("stray.lease"),
        serde_json::to_string(&stray).unwrap(),
    )
    .unwrap();
    drop(first);
    drop(second);

    // The stray holder is alive, so this key's server is retained even past the
    // linger (the one entry the sweep reclaims is the other key's, whose leases
    // were all released).
    server_pool::sweep(&root, Duration::from_secs(0)).unwrap();
    assert!(
        root.join(key.as_str()).join("server.json").exists(),
        "a live lease must retain the server whatever the linger says"
    );
    assert!(
        server_pool::pid_alive(server),
        "a live lease must retain it"
    );

    let mut holder = holder;
    holder.kill().unwrap();
    holder.wait().unwrap();

    // Now nothing live holds it, and the next sweep reclaims it.
    assert_eq!(
        server_pool::sweep(&root, Duration::from_secs(0)).unwrap(),
        1
    );
    assert!(
        !root.join(key.as_str()).join("server.json").exists(),
        "a server no live process holds must be reclaimed"
    );
    assert!(!server_pool::pid_alive(other_pid));

    let _ = std::fs::remove_dir_all(&root);
}
