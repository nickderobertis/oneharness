//! End-to-end journeys for a `crush run` reply its stdout cut short: the run's
//! `text` is read back from crush's own session store when stdout carried only
//! its start, and `text_source` says where the text came from.
//!
//! Each drives the real `oneharness` binary with the mock harness standing in
//! for `crush` (`MOCK_CRUSH_STORE`): its `run` records the whole reply in a
//! store while printing only `MOCK_STDOUT`, as crush v0.102.0 does when its
//! agent finishes before the queued deltas are printed, and its `session list
//! --json` / `session show <id> --json` print that store in the shape captured
//! from real crush in `tests/fixtures/crush-0.87.0-session-*.json`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use oneharness_core::io::scratch::ScratchDir;
use serde_json::Value;

#[path = "../../../tests/profile-scratch/profile_scratch.rs"]
mod profile_scratch;
use profile_scratch::ProfileScratch;

const REPLY: &str = "ONEHARNESS-LIVE-3-alpha-bravo-charlie";
const CUT: &str = "ONEHARNESS-LIVE-3\n";

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

// llmlint: ignore-block[e2e_not_mocked] These journeys must drive the real built `oneharness` against the repository's mock harness standing in for crush's process: the deterministic gate cannot run the paid, authenticated CLI, and crush's printing race cannot be provoked on demand. Only the harness subprocess is replaced, at the existing `--bin` seam; it prints the `session show --json` shape captured from crush v0.87.0, and `just live-crush` holds the same path to the real harness.
fn crush_bin() -> String {
    format!(
        "crush={}",
        workspace_bin("oneharness-mock-harness").display()
    )
}
// llmlint: ignore-end[e2e_not_mocked]

/// One hermetic `oneharness run` on crush in `dir`, with the mock's store
/// under `dir/store`. `args` go before the run's own flags, so a test can add
/// prompts, `--resume` or `--schema`, and `-- <crush args>` after them.
fn oh(dir: &ScratchDir, args: &[&str], envs: &[(&str, &str)]) -> (Value, Output) {
    let profiles = ProfileScratch::new();
    let mut cmd = Command::new(workspace_bin("oneharness"));
    for (var, _) in std::env::vars().filter(|(var, _)| var.starts_with("ONEHARNESS_")) {
        cmd.env_remove(var);
    }
    let cwd = dir.display().to_string();
    let bin = crush_bin();
    let (own, passthrough) = match args.iter().position(|a| *a == "--") {
        Some(at) => args.split_at(at),
        None => (args, &[][..]),
    };
    cmd.args(own)
        .args([
            "--harness",
            "crush",
            "--cwd",
            &cwd,
            "--bin",
            &bin,
            "--env",
            profiles.as_str(),
            "--format",
            "json",
        ])
        .args(passthrough)
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
    (report, output)
}

/// One crush run of `prompt`; its one result.
fn run_crush(dir: &ScratchDir, extra: &[&str], envs: &[(&str, &str)]) -> (Value, Output) {
    let mut args = vec!["run", "--prompt", "say the marker"];
    args.extend_from_slice(extra);
    let (report, output) = oh(dir, &args, envs);
    (report["results"][0].clone(), output)
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn scratch(tag: &str) -> ScratchDir {
    ScratchDir::new(&format!("crush-store-{tag}")).expect("scratch dir")
}

fn listed_sessions(store: &Path) -> Vec<Value> {
    serde_json::from_slice::<Value>(
        &std::fs::read(store.join("sessions.json")).expect("the mock stored a session"),
    )
    .unwrap()
    .as_array()
    .cloned()
    .unwrap()
}

#[test]
fn a_reply_crush_printed_only_the_start_of_is_read_back_from_its_session_store() {
    let dir = scratch("cut");
    let (result, output) = run_crush(
        &dir,
        &[],
        &[("MOCK_STDOUT", CUT), ("MOCK_CRUSH_REPLY", REPLY)],
    );
    assert_eq!(result["status"], "ok", "{output:?}");
    assert_eq!(result["text"], REPLY);
    assert_eq!(result["text_source"], "crush-session");
    // The envelope keeps what crush actually printed.
    assert_eq!(result["stdout"], CUT);
}

#[test]
fn a_turn_of_several_assistant_messages_is_read_back_as_crush_prints_it() {
    let dir = scratch("messages");
    // Stored with the leading whitespace crush drops when it prints a message.
    let reply = serde_json::json!(["\n  Checking the directory.", " ONEHARNESS-LIVE-3-alpha"]);
    let (result, _) = run_crush(
        &dir,
        &[],
        &[
            ("MOCK_STDOUT", "Checking the directory.ONEHARNESS-LIVE\n"),
            ("MOCK_CRUSH_REPLY", &reply.to_string()),
        ],
    );
    assert_eq!(
        result["text"],
        "Checking the directory.ONEHARNESS-LIVE-3-alpha"
    );
    assert_eq!(result["text_source"], "crush-session");
}

#[test]
fn a_run_that_printed_nothing_returns_the_stored_reply() {
    let dir = scratch("empty");
    let (result, _) = run_crush(
        &dir,
        &[],
        &[("MOCK_STDOUT", ""), ("MOCK_CRUSH_REPLY", REPLY)],
    );
    assert_eq!(result["text"], REPLY);
    assert_eq!(result["text_source"], "crush-session");
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
            ("MOCK_STDOUT", CUT),
            ("MOCK_CRUSH_REPLY", "a different reply altogether"),
        ],
    );
    assert_eq!(result["text"], "ONEHARNESS-LIVE-3");
    assert_eq!(result["text_source"], "raw");
}

#[test]
fn a_failed_run_is_not_read_back() {
    let dir = scratch("failed");
    let (result, _) = run_crush(
        &dir,
        &[],
        &[
            ("MOCK_STDOUT", CUT),
            ("MOCK_CRUSH_REPLY", REPLY),
            ("MOCK_EXIT", "1"),
        ],
    );
    assert_eq!(result["status"], "nonzero");
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
            ("MOCK_STDOUT", CUT),
            ("MOCK_CRUSH_REPLY", REPLY),
            ("MOCK_CRUSH_STORE_READ", "fail"),
        ],
    );
    assert_eq!(result["status"], "ok", "{output:?}");
    assert_eq!(result["text"], "ONEHARNESS-LIVE-3");
    assert_eq!(result["text_source"], "raw:crush-session-unavailable");
    let stderr = stderr(&output);
    assert!(
        stderr.contains("crush session store unavailable")
            && stderr.contains("failed to connect to database"),
        "{stderr}"
    );
}

#[test]
fn an_unreadable_store_after_an_empty_reply_leaves_text_and_its_source_null() {
    let dir = scratch("unreadable-empty");
    let (result, _) = run_crush(
        &dir,
        &[],
        &[
            ("MOCK_STDOUT", ""),
            ("MOCK_CRUSH_REPLY", REPLY),
            ("MOCK_CRUSH_STORE_READ", "fail"),
        ],
    );
    assert_eq!(result["text"], Value::Null);
    assert_eq!(result["text_source"], Value::Null);
}

#[test]
fn a_session_list_cut_off_mid_json_is_an_unavailable_store() {
    let dir = scratch("malformed-list");
    let (result, output) = run_crush(
        &dir,
        &[],
        &[
            ("MOCK_STDOUT", CUT),
            ("MOCK_CRUSH_REPLY", REPLY),
            ("MOCK_CRUSH_STORE_READ", "malformed"),
        ],
    );
    assert_eq!(result["text"], "ONEHARNESS-LIVE-3");
    assert_eq!(result["text_source"], "raw:crush-session-unavailable");
    assert!(
        stderr(&output).contains("`crush session list --json`"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_resumed_run_reads_its_own_session_back_by_id() {
    let dir = scratch("resumed");
    let store = dir.join("store");
    // The first turn creates the session the third continues.
    run_crush(
        &dir,
        &[],
        &[("MOCK_STDOUT", "first\n"), ("MOCK_CRUSH_REPLY", "first")],
    );
    let sid = listed_sessions(&store)[0]["uuid"]
        .as_str()
        .expect("a session id")
        .to_string();
    // Another run creates a newer session in the same store: the resumed run
    // is read by its id, not by what is newest.
    run_crush(
        &dir,
        &[],
        &[("MOCK_STDOUT", "other\n"), ("MOCK_CRUSH_REPLY", "other")],
    );
    assert_eq!(listed_sessions(&store).len(), 2);
    let (resumed, _) = run_crush(
        &dir,
        &["--resume", &sid],
        &[("MOCK_STDOUT", CUT), ("MOCK_CRUSH_REPLY", REPLY)],
    );
    assert_eq!(resumed["text"], REPLY);
    assert_eq!(resumed["text_source"], "crush-session");
}

#[test]
fn a_resumed_run_whose_session_is_shown_cut_off_keeps_stdout() {
    let dir = scratch("malformed-show");
    run_crush(
        &dir,
        &[],
        &[("MOCK_STDOUT", "first\n"), ("MOCK_CRUSH_REPLY", "first")],
    );
    let sid = listed_sessions(&dir.join("store"))[0]["uuid"]
        .as_str()
        .unwrap()
        .to_string();
    let (resumed, output) = run_crush(
        &dir,
        &["--resume", &sid],
        &[
            ("MOCK_STDOUT", CUT),
            ("MOCK_CRUSH_REPLY", REPLY),
            ("MOCK_CRUSH_STORE_READ", "malformed"),
        ],
    );
    assert_eq!(resumed["text"], "ONEHARNESS-LIVE-3");
    assert_eq!(resumed["text_source"], "raw:crush-session-unavailable");
    assert!(
        stderr(&output).contains("`crush session show --json`"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_fresh_run_reads_its_own_session_not_an_older_one() {
    let dir = scratch("older");
    let store = dir.join("store");
    run_crush(
        &dir,
        &[],
        &[
            ("MOCK_STDOUT", "older\n"),
            ("MOCK_CRUSH_REPLY", "ONEHARNESS-LIVE-3-older"),
        ],
    );
    // Crush records creation to the second, so the next run begins in a later
    // second than the older session's, as any real crush turn does.
    std::thread::sleep(std::time::Duration::from_millis(1_100));
    let (result, _) = run_crush(
        &dir,
        &[],
        &[("MOCK_STDOUT", CUT), ("MOCK_CRUSH_REPLY", REPLY)],
    );
    assert_eq!(listed_sessions(&store).len(), 2);
    assert_eq!(result["text"], REPLY);
    assert_eq!(result["text_source"], "crush-session");
}

#[test]
fn a_run_that_created_no_session_keeps_stdout_and_says_its_session_is_unidentified() {
    let dir = scratch("none");
    // Without MOCK_CRUSH_REPLY the mock's run stores nothing.
    let (result, output) = run_crush(&dir, &[], &[("MOCK_STDOUT", CUT)]);
    assert_eq!(result["text"], "ONEHARNESS-LIVE-3");
    assert_eq!(result["text_source"], "raw:crush-session-unidentified");
    assert!(
        stderr(&output).contains("no session created since the run began"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn runs_sharing_a_store_at_once_cannot_be_told_apart_and_keep_stdout() {
    let dir = scratch("batch");
    // A batch's two turns each create a session in the one store while both
    // runs are in flight, so neither can say which is its own.
    let (report, output) = oh(
        &dir,
        &["run", "--prompt", "say one", "--prompt", "say two"],
        &[("MOCK_STDOUT", CUT), ("MOCK_CRUSH_REPLY", REPLY)],
    );
    let results = report["results"].as_array().expect("a batch of results");
    assert_eq!(results.len(), 2, "{output:?}");
    for result in results {
        assert_eq!(result["text"], "ONEHARNESS-LIVE-3");
        assert_eq!(result["text_source"], "raw:crush-session-unidentified");
    }
    assert!(
        stderr(&output).contains("more than one session created since the run began"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_continue_run_names_no_session_and_keeps_stdout() {
    let dir = scratch("continue");
    let (result, output) = run_crush(
        &dir,
        &["--", "--continue"],
        &[("MOCK_STDOUT", CUT), ("MOCK_CRUSH_REPLY", REPLY)],
    );
    assert_eq!(result["text"], "ONEHARNESS-LIVE-3");
    assert_eq!(result["text_source"], "raw:crush-session-unidentified");
    assert!(
        stderr(&output).contains("`--continue`"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_run_with_its_own_data_dir_is_read_back_from_that_store() {
    let dir = scratch("data-dir");
    let elsewhere = dir.join("elsewhere");
    let elsewhere_arg = elsewhere.display().to_string();
    let (result, _) = run_crush(
        &dir,
        &["--", "--data-dir", &elsewhere_arg],
        &[("MOCK_STDOUT", CUT), ("MOCK_CRUSH_REPLY", REPLY)],
    );
    // The turn went to the run's own store, and was read back from there.
    assert_eq!(listed_sessions(&elsewhere).len(), 1);
    assert!(!dir.join("store").exists());
    assert_eq!(result["text"], REPLY);
    assert_eq!(result["text_source"], "crush-session");
}

#[test]
fn a_schema_answer_read_back_from_the_store_is_validated_again() {
    let dir = scratch("schema");
    let schema = dir.join("schema.json");
    std::fs::write(
        &schema,
        r#"{"type":"object","required":["ok"],"properties":{"ok":{"type":"boolean"}}}"#,
    )
    .unwrap();
    let schema_arg = schema.display().to_string();
    let args = ["--schema", schema_arg.as_str(), "--schema-max-retries", "0"];
    let (valid, _) = run_crush(
        &dir,
        &args,
        &[
            ("MOCK_STDOUT", "{\"ok\":\n"),
            ("MOCK_CRUSH_REPLY", r#"{"ok":true}"#),
        ],
    );
    assert_eq!(valid["text"], r#"{"ok":true}"#);
    assert_eq!(valid["text_source"], "crush-session");
    assert_eq!(valid["schema_valid"], true);
    assert_eq!(valid["structured"], serde_json::json!({"ok": true}));
    assert_eq!(valid["schema_error"], Value::Null);

    let dir = scratch("schema-invalid");
    let (invalid, _) = run_crush(
        &dir,
        &args,
        &[
            ("MOCK_STDOUT", "{\"ok\":\n"),
            ("MOCK_CRUSH_REPLY", r#"{"ok":"yes"}"#),
        ],
    );
    assert_eq!(invalid["text_source"], "crush-session");
    assert_eq!(invalid["schema_valid"], false);
    assert!(invalid["schema_error"]
        .as_str()
        .is_some_and(|e| !e.is_empty()));
}

#[test]
fn a_run_given_crushs_cwd_is_read_back_from_that_directorys_store() {
    let dir = scratch("cwd");
    let project = dir.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let project_arg = project.display().to_string();
    let (result, _) = run_crush(
        &dir,
        &["--", "--cwd", &project_arg],
        &[("MOCK_STDOUT", CUT), ("MOCK_CRUSH_REPLY", REPLY)],
    );
    // `--cwd` moves crush's default store to `<cwd>/.crush`; the read followed it.
    assert_eq!(listed_sessions(&project.join(".crush")).len(), 1);
    assert_eq!(result["text"], REPLY);
    assert_eq!(result["text_source"], "crush-session");
}

#[test]
fn a_listed_session_id_crush_never_mints_is_an_unavailable_store() {
    let dir = scratch("bad-id");
    let (result, output) = run_crush(
        &dir,
        &[],
        &[
            ("MOCK_STDOUT", CUT),
            ("MOCK_CRUSH_REPLY", REPLY),
            ("MOCK_CRUSH_STORE_READ", "bad-id"),
        ],
    );
    assert_eq!(result["text"], "ONEHARNESS-LIVE-3");
    assert_eq!(result["text_source"], "raw:crush-session-unavailable");
    assert!(
        stderr(&output).contains("is not a crush session id"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_listed_creation_time_that_is_no_instant_is_an_unavailable_store() {
    let dir = scratch("bad-time");
    let (result, output) = run_crush(
        &dir,
        &[],
        &[
            ("MOCK_STDOUT", CUT),
            ("MOCK_CRUSH_REPLY", REPLY),
            ("MOCK_CRUSH_STORE_READ", "bad-time"),
        ],
    );
    assert_eq!(result["text"], "ONEHARNESS-LIVE-3");
    assert_eq!(result["text_source"], "raw:crush-session-unavailable");
    assert!(
        stderr(&output).contains("`yesterday` is not an RFC 3339 instant"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_re_asked_schema_run_holds_a_session_per_attempt_and_keeps_its_last_stdout() {
    let dir = scratch("schema-reask");
    let schema = dir.join("schema.json");
    std::fs::write(
        &schema,
        r#"{"type":"object","required":["ok"],"properties":{"ok":{"type":"boolean"}}}"#,
    )
    .unwrap();
    let schema_arg = schema.display().to_string();
    // Each re-ask is a fresh `crush run`, so the run's attempts leave one
    // session each and none of them is the run's alone.
    let (result, output) = run_crush(
        &dir,
        &["--schema", &schema_arg, "--schema-max-retries", "1"],
        &[
            ("MOCK_STDOUT", "{\"ok\":\n"),
            ("MOCK_CRUSH_REPLY", r#"{"ok":true}"#),
        ],
    );
    assert_eq!(listed_sessions(&dir.join("store")).len(), 2);
    assert_eq!(result["schema_attempts"], 2, "{output:?}");
    assert_eq!(result["text"], "{\"ok\":");
    assert_eq!(result["text_source"], "raw:crush-session-unidentified");
    assert_eq!(result["schema_valid"], false);
}

#[test]
fn every_spelling_of_a_store_flag_reads_the_store_the_run_wrote() {
    // (crush args after the run's own, where the turn is stored) — a flag past
    // crush's own `--` is prompt text to crush, so the default store holds it.
    let cases: [(&[&str], &str); 5] = [
        (&["-D", "{elsewhere}"], "elsewhere"),
        (&["--data-dir={elsewhere}"], "elsewhere"),
        (&["-c", "{project}"], "project/.crush"),
        (&["--cwd={project}"], "project/.crush"),
        (&["--", "--data-dir", "{elsewhere}"], "store"),
    ];
    for (crush_args, stored_under) in cases {
        let dir = scratch("spelling");
        std::fs::create_dir_all(dir.join("project")).unwrap();
        let fill = |arg: &str| {
            arg.replace("{elsewhere}", &dir.join("elsewhere").display().to_string())
                .replace("{project}", &dir.join("project").display().to_string())
        };
        let crush_args: Vec<String> = crush_args.iter().map(|a| fill(a)).collect();
        let mut args = vec!["--"];
        args.extend(crush_args.iter().map(String::as_str));
        let (result, output) = run_crush(
            &dir,
            &args,
            &[("MOCK_STDOUT", CUT), ("MOCK_CRUSH_REPLY", REPLY)],
        );
        assert_eq!(
            listed_sessions(&dir.join(stored_under)).len(),
            1,
            "{crush_args:?}"
        );
        assert_eq!(result["text"], REPLY, "{crush_args:?}: {output:?}");
        assert_eq!(result["text_source"], "crush-session", "{crush_args:?}");
    }
}

#[test]
fn every_spelling_of_a_named_session_reads_that_session_back() {
    for spelling in ["-s", "--session="] {
        let dir = scratch("session-spelling");
        let store = dir.join("store");
        run_crush(
            &dir,
            &[],
            &[("MOCK_STDOUT", "first\n"), ("MOCK_CRUSH_REPLY", "first")],
        );
        let sid = listed_sessions(&store)[0]["uuid"]
            .as_str()
            .unwrap()
            .to_string();
        run_crush(
            &dir,
            &[],
            &[("MOCK_STDOUT", "other\n"), ("MOCK_CRUSH_REPLY", "other")],
        );
        let named: Vec<String> = if spelling == "-s" {
            vec!["-s".into(), sid.clone()]
        } else {
            vec![format!("--session={sid}")]
        };
        let mut args = vec!["--"];
        args.extend(named.iter().map(String::as_str));
        let (result, _) = run_crush(
            &dir,
            &args,
            &[("MOCK_STDOUT", CUT), ("MOCK_CRUSH_REPLY", REPLY)],
        );
        assert_eq!(result["text"], REPLY, "{spelling}");
        assert_eq!(result["text_source"], "crush-session", "{spelling}");
    }
}

#[test]
fn a_store_read_that_fails_without_a_word_is_named_by_its_exit_code() {
    let dir = scratch("silent-fail");
    let (result, output) = run_crush(
        &dir,
        &[],
        &[
            ("MOCK_STDOUT", CUT),
            ("MOCK_CRUSH_REPLY", REPLY),
            ("MOCK_CRUSH_STORE_READ", "silent-fail"),
        ],
    );
    assert_eq!(result["text"], "ONEHARNESS-LIVE-3");
    assert_eq!(result["text_source"], "raw:crush-session-unavailable");
    assert!(
        stderr(&output).contains("exited 1 without a word"),
        "{}",
        stderr(&output)
    );
}
