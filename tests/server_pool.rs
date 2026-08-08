//! The sidecar-server pool, driven through its **public library surface** the
//! way a consumer (this CLI, or a sibling tool depending on `oneharness-core`)
//! uses it — from outside the crate, against real processes.
//!
//! The in-crate unit tests cover the same reclamation rules; this exists because
//! the pool is a *published* API whose whole promise is cross-process: a lease is
//! held by a live pid, so the interesting failure — a dispatch that dies without
//! releasing — can only be exercised with real processes and the public
//! functions, not with in-module internals.

#![cfg(unix)]

use std::path::PathBuf;
use std::time::Duration;

use oneharness_core::domain::control::{ServerAddress, ServerSpec, ServerTransport};
use oneharness_core::io::server_pool::{self, LaunchPlan};

fn pool_root(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oh-pool-e2e-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A stand-in server: a real, long-lived process. The pool only ever asks the OS
/// about pids, so this exercises exactly the paths a `codex app-server` would.
fn plan() -> LaunchPlan {
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
    let holder_pid = holder.id();
    std::fs::write(
        root.join(&key).join("leases").join("stray.lease"),
        holder_pid.to_string(),
    )
    .unwrap();
    drop(first);
    drop(second);

    // The stray holder is alive, so this key's server is retained even past the
    // linger (the one entry the sweep reclaims is the other key's, whose leases
    // were all released).
    server_pool::sweep(&root, Duration::from_secs(0)).unwrap();
    assert!(
        root.join(&key).join("server.json").exists(),
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
        !root.join(&key).join("server.json").exists(),
        "a server no live process holds must be reclaimed"
    );
    assert!(!server_pool::pid_alive(other_pid));

    let _ = std::fs::remove_dir_all(&root);
}
