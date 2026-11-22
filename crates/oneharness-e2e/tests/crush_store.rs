//! End-to-end journeys for a `crush run` reply its stdout cut short: the run's
//! `text` is read back from crush's own session store when stdout carried only
//! its start, and `text_source` says where the text came from.
//!
//! Each drives the real `oneharness` binary with the mock harness standing in
//! for `crush` (`MOCK_CRUSH_STORE`): its `run` records the whole reply in a
//! store while printing only `MOCK_STDOUT`, as crush v0.102.0 does when its
//! agent finishes before the queued deltas are printed, and its `session list
//! --json` / `session show <id> --json` print that store in crush's shape.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use oneharness_core::io::scratch::ScratchDir;
use serde_json::Value;

#[path = "../../../tests/profile-scratch/profile_scratch.rs"]
mod profile_scratch;
use profile_scratch::ProfileScratch;

const REPLY: &str = "ONEHARNESS-LIVE-3-alpha-bravo-charlie";

fn workspace_bin(name: &str) -> PathBuf {
    let test_exe = std::env::current_exe().expect("test executable path");
    let profile_dir = test_exe
        .parent()
        .and_then(Path::parent)
        .expect("test executable under <target>/<profile>/deps");
    let path = profile_dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    assert!(
        path.is_file(),
        "{} is not built; run this suite through its Nx target (`just e2e`)",
        path.display()
    );
    path
}

// llmlint: ignore-block[e2e_not_mocked] These journeys must drive the real built `oneharness` against the repository's mock harness standing in for crush's process: the deterministic gate cannot run the paid, authenticated CLI, and crush's printing race cannot be provoked on demand. Only the harness subprocess is replaced, at the existing `--bin` seam; it prints the `session show --json` shape crush v0.87.0 and v0.102.0 print, and `just live-crush` holds the same path to the real harness.
fn crush_bin() -> String {
    format!(
        "crush={}",
        workspace_bin("oneharness-mock-harness").display()
    )
}
// llmlint: ignore-end[e2e_not_mocked]

/// One hermetic `oneharness run` on crush in `dir`, with the mock's store
/// under `dir/store`; returns the run's one result.
fn run_crush(dir: &ScratchDir, extra_args: &[&str], envs: &[(&str, &str)]) -> (Value, Output) {
    let profiles = ProfileScratch::new();
    let mut cmd = Command::new(workspace_bin("oneharness"));
    for (var, _) in std::env::vars().filter(|(var, _)| var.starts_with("ONEHARNESS_")) {
        cmd.env_remove(var);
    }
    let cwd = dir.display().to_string();
    let bin = crush_bin();
    let mut args = vec![
        "run",
        "--harness",
        "crush",
        "--prompt",
        "say the marker",
        "--cwd",
        &cwd,
        "--bin",
        &bin,
        "--env",
        profiles.as_str(),
        "--format",
        "json",
    ];
    args.extend_from_slice(extra_args);
    cmd.args(&args)
        .env("ONEHARNESS_NO_CONFIG", "1")
        .env("MOCK_CRUSH_STORE", dir.join("store"))
        .env("MOCK_PRESERVE_STDOUT", "1");
    for (key, value) in envs {
        cmd.env(key, value);
    }
    let output = cmd.output().expect("run oneharness");
    let report: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|err| {
        panic!(
            "stdout was not JSON ({err}):\n{}\n--- stderr ---\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (report["results"][0].clone(), output)
}

fn scratch(tag: &str) -> ScratchDir {
    ScratchDir::new(&format!("crush-store-{tag}")).expect("scratch dir")
}

#[test]
fn a_reply_crush_printed_only_the_start_of_is_read_back_from_its_session_store() {
    let dir = scratch("cut");
    let (result, output) = run_crush(
        &dir,
        &[],
        &[
            ("MOCK_STDOUT", "ONEHARNESS-LIVE-3\n"),
            ("MOCK_CRUSH_REPLY", REPLY),
        ],
    );
    assert_eq!(result["status"], "ok", "{output:?}");
    assert_eq!(result["text"], REPLY);
    assert_eq!(result["text_source"], "crush-session");
    // The envelope keeps what crush actually printed.
    assert_eq!(result["stdout"], "ONEHARNESS-LIVE-3\n");
}

#[test]
fn a_reply_crush_printed_whole_is_stdouts() {
    let dir = scratch("whole");
    let printed = format!("{REPLY}\n");
    let (result, _) = run_crush(
        &dir,
        &[],
        &[
            ("MOCK_STDOUT", printed.as_str()),
            ("MOCK_CRUSH_REPLY", REPLY),
        ],
    );
    assert_eq!(result["text"], REPLY);
    assert_eq!(result["text_source"], "raw");
}

#[test]
fn a_stored_reply_stdout_does_not_begin_leaves_stdouts_text_unchanged() {
    let dir = scratch("differs");
    let (result, _) = run_crush(
        &dir,
        &[],
        &[
            ("MOCK_STDOUT", "ONEHARNESS-LIVE-3\n"),
            ("MOCK_CRUSH_REPLY", "a different reply altogether"),
        ],
    );
    assert_eq!(result["text"], "ONEHARNESS-LIVE-3");
    assert_eq!(result["text_source"], "raw");
}

#[test]
fn an_unreadable_store_leaves_stdouts_text_and_says_the_store_was_unavailable() {
    let dir = scratch("unreadable");
    let (result, output) = run_crush(
        &dir,
        &[],
        &[
            ("MOCK_STDOUT", "ONEHARNESS-LIVE-3\n"),
            ("MOCK_CRUSH_REPLY", REPLY),
            ("MOCK_CRUSH_STORE_READ", "fail"),
        ],
    );
    assert_eq!(result["status"], "ok", "{output:?}");
    assert_eq!(result["text"], "ONEHARNESS-LIVE-3");
    assert_eq!(result["text_source"], "raw:crush-session-unavailable");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("crush session store unavailable"),
        "{stderr}"
    );
}

#[test]
fn a_resumed_run_reads_its_own_session_back_by_id() {
    let dir = scratch("resumed");
    // The first turn creates the session the second continues.
    let (first, _) = run_crush(
        &dir,
        &[],
        &[("MOCK_STDOUT", "first\n"), ("MOCK_CRUSH_REPLY", "first")],
    );
    let listed: Value = serde_json::from_slice(
        &std::fs::read(dir.join("store").join("sessions.json")).expect("the mock stored a session"),
    )
    .unwrap();
    let sid = listed[0]["uuid"]
        .as_str()
        .expect("a session id")
        .to_string();
    assert_eq!(first["text_source"], "raw");
    // A session another run creates meanwhile cannot be mistaken for this one:
    // the resumed run is read by its id, not by what is newest.
    let mut crowded = listed.as_array().unwrap().clone();
    crowded.push(
        serde_json::json!({"id": "other", "uuid": "other", "title": "t",
        "created": "2999-01-01T00:00:00Z", "modified": "2999-01-01T00:00:00Z"}),
    );
    std::fs::write(
        dir.join("store").join("sessions.json"),
        Value::from(crowded).to_string(),
    )
    .unwrap();
    let (second, _) = run_crush(
        &dir,
        &["--resume", &sid],
        &[
            ("MOCK_STDOUT", "ONEHARNESS-LIVE-3\n"),
            ("MOCK_CRUSH_REPLY", REPLY),
        ],
    );
    assert_eq!(second["text"], REPLY);
    assert_eq!(second["text_source"], "crush-session");
}

#[test]
fn a_fresh_run_whose_session_cannot_be_told_apart_keeps_stdout_and_says_so() {
    let dir = scratch("unidentified");
    std::fs::create_dir_all(dir.join("store")).unwrap();
    // Another session in the same store, created after this run began.
    std::fs::write(
        dir.join("store").join("sessions.json"),
        serde_json::json!([{"id": "other", "uuid": "other", "title": "t",
            "created": "2999-01-01T00:00:00Z", "modified": "2999-01-01T00:00:00Z"}])
        .to_string(),
    )
    .unwrap();
    let (result, _) = run_crush(
        &dir,
        &[],
        &[
            ("MOCK_STDOUT", "ONEHARNESS-LIVE-3\n"),
            ("MOCK_CRUSH_REPLY", REPLY),
        ],
    );
    assert_eq!(result["text"], "ONEHARNESS-LIVE-3");
    assert_eq!(result["text_source"], "raw:crush-session-unidentified");
}
