//! The fixture executable answers the way the suites that spawn it rely on:
//! it writes `MOCK_STDOUT`/`MOCK_STDERR` verbatim and exits `MOCK_EXIT`.
//!
//! Being an integration test is also what makes `cargo test`/`nextest` build
//! this crate's binary when a consumer's test target selects the package
//! beside its own (`-p oneharness-mock-harness`).

use std::process::Command;

#[test]
fn the_fixture_writes_its_scripted_streams_and_exit_code() {
    let output = Command::new(env!("CARGO_BIN_EXE_oneharness-mock-harness"))
        .env("MOCK_STDOUT", "scripted stdout")
        .env("MOCK_STDERR", "scripted stderr")
        .env("MOCK_EXIT", "3")
        .output()
        .expect("spawn the mock harness");
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "scripted stdout");
    assert_eq!(String::from_utf8_lossy(&output.stderr), "scripted stderr");
}

#[test]
fn the_fixture_echoes_the_environment_variable_it_is_asked_about() {
    let output = Command::new(env!("CARGO_BIN_EXE_oneharness-mock-harness"))
        .env("MOCK_ECHO_ENV", "FIXTURE_PROBE")
        .env("FIXTURE_PROBE", "inherited value")
        .output()
        .expect("spawn the mock harness");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim_end(),
        "FIXTURE_PROBE=inherited value"
    );
}

/// A scratch directory under the host temp dir, removed when the test ends,
/// passing or failing.
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("a clock after the epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "oneharness-mock-fixture-{tag}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir(&path).expect("create the scratch directory");
        Scratch(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The crush batch journey (`crates/oneharness-e2e/tests/crush_store.rs`)
/// holds only if the stand-in store keeps every session that concurrent runs
/// create, as crush's database does. Many `crush run`s started at once into
/// one store must each be listed and shown: a store that drops one leaves a
/// run seeing a single new session and reading another run's reply as its own.
#[test]
fn a_crush_store_shared_by_concurrent_runs_lists_every_session_they_create() {
    const RUNS: usize = 16;
    const REPLY: &str = "ONEHARNESS-LIVE-3-alpha-bravo-charlie";
    let scratch = Scratch::new("crush-concurrent");
    let store = scratch.0.join("store");
    let mock = env!("CARGO_BIN_EXE_oneharness-mock-harness");
    let children: Vec<_> = (0..RUNS)
        .map(|n| {
            Command::new(mock)
                .args(["run", &format!("say {n}")])
                .env("MOCK_CRUSH_STORE", &store)
                .env("MOCK_CRUSH_REPLY", REPLY)
                .env("MOCK_STDOUT", "ONEHARNESS-LIVE-3\n")
                .stdout(std::process::Stdio::null())
                .spawn()
                .expect("spawn a mock crush run")
        })
        .collect();
    for mut child in children {
        assert!(child.wait().expect("a mock crush run ends").success());
    }
    let listed = Command::new(mock)
        .args(["session", "list", "--json"])
        .env("MOCK_CRUSH_STORE", &store)
        .output()
        .expect("run the mock's `session list`");
    assert!(listed.status.success(), "{listed:?}");
    let listed: Vec<serde_json::Value> =
        serde_json::from_slice(&listed.stdout).expect("`session list --json` prints a JSON array");
    assert_eq!(listed.len(), RUNS, "{listed:?}");
    for session in &listed {
        let shown = Command::new(mock)
            .args([
                "session",
                "show",
                session["uuid"].as_str().expect("a session id"),
                "--json",
            ])
            .env("MOCK_CRUSH_STORE", &store)
            .output()
            .expect("run the mock's `session show`");
        assert!(shown.status.success(), "{shown:?}");
        assert!(
            String::from_utf8_lossy(&shown.stdout).contains(REPLY),
            "{shown:?}"
        );
    }
}
