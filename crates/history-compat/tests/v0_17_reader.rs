//! History this build writes stays readable by the released reader of
//! oneharness v0.17.0 (`oneharness-core` 0.19.0, a pinned dev-dependency — the
//! published crate, not this tree; see this crate's `Cargo.toml` for why it
//! lives here rather than in the binary crate). A session now carries `message` and
//! `reasoning` events and names itself on every event line; an older reader
//! must still list the session and hand back its events.

use std::path::{Path, PathBuf};
use std::process::Command;

use oneharness_core::io::scratch::ScratchDir;
use oneharness_core_v0_19 as released;

/// A binary the root crate builds into the target directory this test runs
/// from (`<target>/<profile>/deps/<this test>`): another package's
/// `CARGO_BIN_EXE_*` is not visible here, and a workspace-wide build (`just
/// test`, `just coverage`) has built both before any test runs.
fn workspace_bin(name: &str) -> PathBuf {
    let test_exe = std::env::current_exe().expect("test executable path");
    let profile_dir = test_exe
        .parent()
        .and_then(Path::parent)
        .expect("test executable under <target>/<profile>/deps");
    let path = profile_dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    assert!(
        path.is_file(),
        "{} is not built; run the workspace suite with `just test`",
        path.display()
    );
    path
}

fn oneharness_bin() -> PathBuf {
    workspace_bin("oneharness")
}

/// The mock harness built beside the binary under the `mock-harness` feature.
fn mock_bin() -> PathBuf {
    workspace_bin("oneharness-mock-harness")
}

#[test]
fn oneharness_v0_17_0_reads_the_history_this_build_writes() {
    let scratch = ScratchDir::new("history-compat").unwrap();
    let store = scratch.join("store");
    let output = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        .env(
            "MOCK_STDOUT",
            include_str!("../../../tests/fixtures/codex-exec-turn.jsonl"),
        )
        .args([
            "run",
            "--harness",
            "codex",
            "--prompt",
            "compat",
            "--bin",
            &format!("codex={}", mock_bin().display()),
            "--stream",
            "--format",
            "json",
            "--history",
            "--history-dir",
            &store.display().to_string(),
            "--history-name",
            "compat",
            "--cwd",
            &scratch.display().to_string(),
        ])
        .output()
        .expect("run oneharness");
    assert!(output.status.success(), "{output:?}");

    // What this build wrote carries the new content the old reader meets.
    let sessions = released::io::history::list_sessions(&store, None).unwrap();
    assert_eq!(sessions.len(), 1, "{sessions:?}");
    let written = std::fs::read_to_string(&sessions[0].path).unwrap();
    for new_content in [
        r#""session_name":"compat""#,
        r#""kind":"message""#,
        r#""kind":"reasoning""#,
    ] {
        assert!(written.contains(new_content), "{new_content}: {written}");
    }

    // v0.17.0's reader lists the session and returns every event.
    assert_eq!(sessions[0].name, "compat");
    assert_eq!(sessions[0].record_count, 1);
    let records = released::io::history::read_session(Path::new(&sessions[0].path)).unwrap();
    assert_eq!(records.len(), 1);
    let kinds: Vec<&str> = records[0]
        .events
        .iter()
        .flatten()
        .map(|event| event.kind.as_str())
        .collect();
    assert_eq!(
        kinds,
        [
            "message",
            "reasoning",
            "tool_call",
            "tool_call",
            "tool_call",
            "message"
        ]
    );
    let shown = released::io::history::read_session_display(Path::new(&sessions[0].path)).unwrap();
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0]["events"].as_array().unwrap().len(), kinds.len());

    // Its watcher learns of the run through its own reconcile, which walks the
    // tree and appends the record to its legacy `.index.jsonl`. This build
    // writes live events only to the dated segments, so that watcher sees none
    // of them — the documented cost of sharing a store with an older core
    // (docs/history-index.md).
    let mut watcher =
        released::io::history::HistoryWatcher::open(&store, None, Default::default(), None, true)
            .unwrap();
    assert_eq!(watcher.drain_events().len(), 0);
    assert_eq!(watcher.drain_available().len(), 1);
}

/// Every segment file under the store's index directory, with its bytes.
fn segments(store: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut found: Vec<(PathBuf, Vec<u8>)> = std::fs::read_dir(store.join(".index.d"))
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            let bytes = std::fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect();
    found.sort();
    found
}

#[test]
fn oneharness_v0_17_0_never_reads_rewrites_or_deletes_a_segment() {
    // The released pre-change core itself — `oneharness-core` 0.19.0 from
    // crates.io, the engine of oneharness v0.17.0 — sharing a store this build
    // indexed: its reconcile on open, its listing, its watcher, its migrate and
    // its `clear --all-projects --yes` (`remove_sessions(dir, None)`) all walk
    // the store's subdirectories. They see `.index.d/` as a project holding no
    // `*.jsonl`, so nothing of it is read, rewritten or deleted.
    let scratch = ScratchDir::new("history-compat-segments").unwrap();
    let store = scratch.join("store");
    let project = scratch.join("project");
    std::fs::create_dir_all(&project).unwrap();
    for name in ["first", "second"] {
        let output = Command::new(oneharness_bin())
            .env("ONEHARNESS_NO_CONFIG", "1")
            .env(
                "MOCK_STDOUT",
                include_str!("../../../tests/fixtures/codex-exec-turn.jsonl"),
            )
            .args(["run", "--harness", "codex", "--prompt", name])
            .args(["--bin", &format!("codex={}", mock_bin().display())])
            .args(["--history", "--history-dir", &store.display().to_string()])
            .args([
                "--history-name",
                name,
                "--cwd",
                &project.display().to_string(),
            ])
            .args(["--bypass", "--format", "json"])
            .output()
            .expect("run oneharness");
        assert!(output.status.success(), "{output:?}");
    }
    let before = segments(&store);
    assert!(before.len() >= 2, "runs and events segments: {before:?}");
    // Make one segment unreadable too: a released core that opened it would
    // fail rather than pass quietly.
    #[cfg(unix)]
    let unreadable = {
        use std::os::unix::fs::PermissionsExt;
        let path = before[0].0.clone();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        path
    };

    let released_labels = released::domain::history::HistoryLabels::default();
    released::io::history::HistoryWriter::open(&store, &project, "released", released_labels)
        .expect("the released core opens the shared store");
    let listed = released::io::history::list_sessions(&store, None).unwrap();
    assert_eq!(listed.len(), 2, "its tree walk sees this build's runs");
    released::io::history::HistoryWatcher::open(&store, None, Default::default(), None, true)
        .expect("the released watcher opens");
    released::io::history::migrate(&store).expect("the released migrate runs");
    let removed = released::io::history::remove_sessions(&store, None).unwrap();
    assert_eq!(
        removed.len(),
        2,
        "its clear removes the session files: {removed:?}"
    );
    assert!(
        removed.iter().all(|path| path.ends_with(".jsonl")),
        "{removed:?}"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
    assert!(
        store.join(".index.d").is_dir(),
        "the index directory survives its clear"
    );
    assert_eq!(segments(&store), before, "every segment byte-identical");
}
