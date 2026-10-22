//! Buffered history callbacks through the two controlled execution models.
//! This binary has one test because HTTP server launches inherit process env;
//! its pool and mock settings must not affect another in-process library run.
#![cfg(unix)]

use std::time::Duration;

use oneharness_core::domain::fallback::RunMode;
use oneharness_core::domain::mode::PermissionMode;
use oneharness_core::domain::report::Status;
use oneharness_core::io::run::{run, EventSink, RunBegun, RunControls, RunRequest, SinkStep};
use oneharness_core::io::scratch::ScratchDir;
use oneharness_core::io::server_pool;
use serde_json::Value;

#[path = "common/library_fixture.rs"]
mod fixture;
use fixture::{request, ProfileScratch};

#[derive(Default)]
struct Locations {
    begun: Vec<RunBegun>,
    stop: bool,
}
impl EventSink for Locations {
    fn run_began(&mut self, begun: &RunBegun) -> SinkStep {
        let file = begun
            .history_dir
            .join(&begun.history_project)
            .join(format!("{}.jsonl", begun.history_session));
        let live = std::fs::read_to_string(file).unwrap();
        let lines: Vec<Value> = live
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let opening = lines
            .iter()
            .find(|line| {
                line["type"] == "run-started" && line["history_id"] == begun.history_id.to_string()
            })
            .unwrap();
        assert!(!lines.iter().any(
            |line| line["type"] == "run" && line["history_id"] == begun.history_id.to_string()
        ));
        assert_eq!(opening["history_id"], begun.history_id.to_string());
        assert_eq!(opening["harness_id"], begun.harness_id.as_str());
        self.begun.push(begun.clone());
        if self.stop {
            SinkStep::Stop
        } else {
            SinkStep::Continue
        }
    }
    fn event(&mut self, _: &str, _: &oneharness_core::domain::events::ActionEvent) -> SinkStep {
        panic!("buffered controlled runs publish no action events");
    }
}

struct PoolCleanup(std::path::PathBuf);
impl Drop for PoolCleanup {
    fn drop(&mut self) {
        server_pool::sweep(&self.0, Duration::ZERO).unwrap();
    }
}

#[test]
fn buffered_controlled_candidates_announce_once_including_a_retried_turn() {
    let profiles = ProfileScratch::new();
    // Both execution models bind control sockets under this store. macOS's
    // per-user temp directory is too long for its 104-byte socket addresses.
    let socket_root = std::fs::canonicalize("/tmp").unwrap();
    let store = ScratchDir::under(&socket_root, "bc").unwrap();
    let cwd = store.join("cwd");
    std::fs::create_dir(&cwd).unwrap();
    // Explicit config plus no ambient overrides keeps the per-dispatch Codex
    // app-server choice hermetic. There is no other test in this process.
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("ONEHARNESS_") {
            std::env::remove_var(key);
        }
    }
    let config = store.join("config.toml");
    std::fs::write(&config, "[harness.codex]\napp_server = 'per-dispatch'\n").unwrap();
    for (run_mode, stop) in [
        (RunMode::Parallel, false),
        (RunMode::Fallback, false),
        (RunMode::Parallel, true),
        (RunMode::Fallback, true),
    ] {
        let counter = store.join(format!("attempt-{run_mode:?}-{stop}"));
        let log = store.join(format!("codex-{run_mode:?}-{stop}.log"));
        let req = RunRequest {
            control: true,
            stream: Some(false),
            run_mode: Some(run_mode),
            session: Some(format!("retry-{run_mode:?}-{stop}")),
            session_dir: Some(store.to_path_buf()),
            cwd: Some(cwd.clone()),
            history: Some(true),
            history_dir: Some(store.join(format!("codex-history-{run_mode:?}-{stop}"))),
            server_overloaded_max_retries: Some(1),
            config: vec![config.clone()],
            no_config: false,
            ..request(
                &profiles,
                "codex",
                &[
                    ("MOCK_ATTEMPT_FILE", &counter.display().to_string()),
                    ("MOCK_CODEX_APP_SERVER_LOG", &log.display().to_string()),
                    ("MOCK_CODEX_OVERLOAD_ATTEMPTS", "1"),
                    ("MOCK_CODEX_COMPLETE_TURN", "1"),
                ],
            )
        };
        let mut sink = Locations {
            stop,
            ..Locations::default()
        };
        let outcome = run(
            &req,
            RunControls {
                events: Some(&mut sink),
                ..RunControls::default()
            },
        )
        .unwrap();
        assert!(!outcome.streamed);
        assert_eq!(
            outcome.report.results[0].status,
            if stop { Status::Cancelled } else { Status::Ok }
        );
        if stop {
            assert!(!counter.exists(), "Stop prevents any controlled attempt");
            assert!(!log.exists(), "Stop prevents the app-server launch");
        } else {
            assert_eq!(std::fs::read_to_string(counter).unwrap(), "2");
        }
        assert_eq!(
            sink.begun.len(),
            1,
            "a retry retains its candidate announcement"
        );
        let history = std::fs::read_to_string(outcome.report.history_file.unwrap()).unwrap();
        let openings = history
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .filter(|line| {
                line["type"] == "run-started"
                    && line["history_id"] == sink.begun[0].history_id.to_string()
            })
            .count();
        assert_eq!(openings, 1);
    }

    std::env::set_var("XDG_STATE_HOME", store.join("pool"));
    let (key, value) = profiles.as_str().split_once('=').unwrap();
    std::env::set_var(key, value);
    std::env::set_var("MOCK_HTTP_CONTROL_LOG", store.join("http.log"));
    let pool = server_pool::resolve_root(None).unwrap();
    let _cleanup = PoolCleanup(pool.clone());
    for (fault, expected, stop) in [
        ("slow-turn", Status::Ok, false),
        ("refuse-prompt", Status::Nonzero, false),
        ("stopped", Status::Cancelled, true),
    ] {
        std::env::set_var(
            "MOCK_HTTP_CONTROL_FAULT",
            if stop { "slow-turn" } else { fault },
        );
        let log = store.join(format!("http-{fault}.log"));
        std::env::set_var("MOCK_HTTP_CONTROL_LOG", &log);
        let req = RunRequest {
            control: true,
            stream: Some(false),
            run_mode: Some(RunMode::Parallel),
            mode: Some(PermissionMode::Bypass),
            session: Some(format!("http-{fault}")),
            session_dir: Some(store.to_path_buf()),
            cwd: Some(cwd.clone()),
            history: Some(true),
            history_dir: Some(store.join(fault)),
            ..request(&profiles, "opencode", &[])
        };
        let mut sink = Locations {
            stop,
            ..Locations::default()
        };
        let outcome = run(
            &req,
            RunControls {
                events: Some(&mut sink),
                ..RunControls::default()
            },
        )
        .unwrap();
        server_pool::sweep(&pool, Duration::ZERO).unwrap();
        assert!(!outcome.streamed);
        if stop {
            assert!(
                !log.exists(),
                "Stop must prevent the HTTP server launch and turn submission"
            );
        }
        assert_eq!(
            outcome.report.results[0].status, expected,
            "{:?}",
            outcome.report.results
        );
        assert_eq!(sink.begun.len(), 1);
        let lines: Vec<Value> = std::fs::read_to_string(outcome.report.history_file.unwrap())
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert!(lines.iter().any(|line| line["type"] == "run-started"
            && line["history_id"] == sink.begun[0].history_id.to_string()));
    }
}
