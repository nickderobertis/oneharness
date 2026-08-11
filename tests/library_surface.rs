//! The library face of every verb that is not `run`: what a Rust consumer gets
//! **without** spawning the `oneharness` binary and parsing its stdout.
//!
//! Each test drives one engine entry point in this process, the way a consumer
//! would — a real config file on disk, real PATH resolution, real probes — and
//! asserts the value it returns is the whole of what the CLI verb prints. That
//! is the parity claim: `oneagentgraph`'s `Command::new(oneharness_bin)` for
//! provider health has a typed replacement, and so does every other query verb.
//!
//! The capability manifest names these entry points, and
//! `scripts/check-capability-surface.sh` holds this file to it: a capability
//! whose `rust` path is not exercised by a `// capability: <method>` marker here
//! fails the gate, so the manifest cannot claim a Rust surface this file does
//! not actually call.
//!
//! Hermeticity is the same rule the rest of the suite follows: every request
//! sets `no_config` or names an explicit `config` file, so the developer's own
//! `oneharness.toml` and `ONEHARNESS_*` overrides can never reshape an
//! assertion.

use std::path::PathBuf;

use oneharness_core::domain::{config, report};
use oneharness_core::errors::OneharnessError;
use oneharness_core::io::detect::{self, DetectRequest};
use oneharness_core::io::init::{self, InitRequest};
use oneharness_core::io::registry::{self, ListRequest};
use oneharness_core::io::sync::{self, SyncRequest};
use oneharness_core::io::usage::{self, UsageRequest};

#[path = "support/library_fixture.rs"]
mod fixture;

/// A private directory for one test's files, removed and recreated so a rerun
/// starts clean.
fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "oneharness-library-surface-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// Make this test process hermetic for the calls that DO read configuration.
///
/// `no_config` is not available to them — the whole point is to exercise the
/// layering — so the two things that would otherwise leak in are neutralized
/// once, for the whole binary: the user layer is pinned to a planted empty file
/// (as `ONEHARNESS_CONFIG` does for the CLI suite), and every ambient
/// `ONEHARNESS_*` override is removed. A host that runs its agents through
/// oneharness carries several of those, and a leaked `ONEHARNESS_HARNESSES`
/// silently reselects the harnesses a sync assertion is about.
///
/// Idempotent and behind a `OnceLock`, so the parallel test threads that all
/// call it agree on one environment rather than racing to different ones.
fn hermetic_environment() {
    static ONCE: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    ONCE.get_or_init(|| {
        for (name, _) in std::env::vars() {
            if name.starts_with("ONEHARNESS_") {
                std::env::remove_var(name);
            }
        }
        let empty = scratch("user-layer").join("user.toml");
        std::fs::write(&empty, "").expect("an empty user layer");
        std::env::set_var("ONEHARNESS_CONFIG", empty);
    });
}

/// A scratch project carrying `contents` as its `oneharness.toml`, discovered
/// upward from the returned directory exactly as a run from there would.
fn project(tag: &str, contents: &str) -> PathBuf {
    hermetic_environment();
    let dir = scratch(tag);
    std::fs::write(dir.join("oneharness.toml"), contents).expect("project config");
    dir
}

/// A completed harness result — the terminal line a run appends to history when
/// it ends, which is what turns an in-flight run into a listable session.
fn finished_result() -> report::RunResult {
    report::RunResult {
        harness: "claude-code".to_string(),
        variant: None,
        harness_id: "claude-code".to_string(),
        bin: "claude".to_string(),
        available: true,
        status: report::Status::Ok,
        prompt: None,
        model: None,
        exit_code: Some(0),
        duration_ms: Some(10),
        telemetry: None,
        command: vec!["claude".to_string()],
        output_format: report::OutputFormat::Json,
        text: Some("done".to_string()),
        text_source: Some("raw".to_string()),
        usage: oneharness_core::domain::signals::Usage::default(),
        usage_source: None,
        session_id: None,
        events: None,
        events_source: None,
        structured: None,
        schema_valid: None,
        schema_attempts: None,
        schema_error: None,
        failure_kind: None,
        failure_kind_source: None,
        stdout: "done".to_string(),
        stderr: String::new(),
        error: None,
    }
}

// capability: list
#[test]
fn a_consumer_reads_the_whole_registry_without_spawning_the_cli() {
    // What `oneharness list` prints, as a value: every harness, each with the
    // argv oneharness would build for it. A consumer choosing a harness reads
    // this instead of shelling out and parsing.
    let report = registry::list(&ListRequest {
        no_config: true,
        ..ListRequest::default()
    })
    .expect("the registry describes itself without configuration");

    assert!(
        report.harnesses.len() >= 8,
        "expected the whole fleet, got {}",
        report.harnesses.len()
    );
    let claude = report
        .harnesses
        .iter()
        .find(|h| h.id == "claude-code")
        .expect("claude-code is in the registry");
    assert_eq!(claude.default_bin, "claude");
    assert!(
        claude.example_command.iter().any(|arg| arg == "<PROMPT>"),
        "the example argv should show where the prompt goes: {:?}",
        claude.example_command
    );
    assert!(
        claude.modes.iter().any(|m| m.mode.as_str() == "bypass"),
        "every harness declares bypass"
    );
}

// capability: list
#[test]
fn a_configured_variant_reaches_the_consumer_as_a_described_identity() {
    // The one part of the description that reads configuration. A consumer
    // asking "which identities are configured here?" gets them resolved, not a
    // raw config file to interpret.
    let dir = project(
        "list-variant",
        "[harness.claude-code.variant.work]\nmodel = \"claude-opus-4-8\"\n",
    );
    let report = registry::list(&ListRequest {
        cwd: Some(dir.clone()),
        ..ListRequest::default()
    })
    .expect("a project config with a variant describes cleanly");

    let claude = report
        .harnesses
        .iter()
        .find(|h| h.id == "claude-code")
        .expect("claude-code is in the registry");
    let variant = claude
        .variants
        .iter()
        .find(|v| v.name == "work")
        .expect("the declared variant is described");
    assert_eq!(variant.harness_id, "claude-code:work");
    assert_eq!(variant.model.as_deref(), Some("claude-opus-4-8"));
    let _ = std::fs::remove_dir_all(&dir);
}

// capability: detect
#[test]
fn a_consumer_probes_one_harness_binary_and_reads_its_version() {
    // Pointed at the mock fixture, which is a real executable on disk, so this
    // exercises the same resolve-and-probe path a real harness takes.
    let report = detect::detect(&DetectRequest {
        harness: vec!["opencode".to_string()],
        bin: vec![fixture::bin_override("opencode")],
        no_config: true,
        ..DetectRequest::default()
    })
    .expect("probing a named harness needs no configuration");

    assert_eq!(report.detected.len(), 1);
    let probed = &report.detected[0];
    assert_eq!(probed.id, "opencode");
    assert!(probed.available, "the mock fixture is on disk: {probed:?}");
    assert!(!report.any_missing());
}

// capability: detect
#[test]
fn a_harness_that_is_not_installed_is_data_rather_than_an_error() {
    // The recovery path: a consumer must be able to ask about a harness that is
    // absent and get an answer, not an exception.
    let report = detect::detect(&DetectRequest {
        harness: vec!["goose".to_string()],
        bin: vec!["goose=/nonexistent/oneharness-not-a-real-binary".to_string()],
        no_config: true,
        ..DetectRequest::default()
    })
    .expect("an absent harness is reported, never raised");

    assert!(!report.detected[0].available);
    assert_eq!(report.detected[0].version, None);
    assert!(
        report.any_missing(),
        "the flag the CLI turns into --require-available's exit code"
    );
}

// capability: detect
#[test]
fn an_unknown_harness_id_is_a_loud_error_for_a_library_caller_too() {
    let error = detect::detect(&DetectRequest {
        harness: vec!["not-a-harness".to_string()],
        no_config: true,
        ..DetectRequest::default()
    })
    .expect_err("an unknown id is a usage error, not an empty report");
    assert!(matches!(error, OneharnessError::UnknownHarness { .. }));
}

// capability: config
#[test]
fn a_consumer_reads_the_layered_configuration_with_each_value_attributed() {
    // `oneharness config`'s whole output: the effective value plus the file it
    // came from. Loading the layers and explaining them are both library calls.
    let dir = project("config-explain", "model = \"from-project\"\n");
    let layers = oneharness_core::io::config::load_layers(None, false, &dir)
        .expect("the planted layers load");
    let report = config::explain(&layers);

    assert_eq!(
        report.model.value.as_deref(),
        Some("from-project"),
        "the project value is the effective one"
    );
    let source = report
        .model
        .source
        .as_deref()
        .expect("an effective value is attributed");
    assert!(
        source.contains("oneharness.toml"),
        "the project file should be named as the source, got {source}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// capability: sync
#[test]
fn a_consumer_syncs_a_policy_into_a_harness_config_and_re_syncs_idempotently() {
    // The happy path and the property that makes `sync` safe to run repeatedly:
    // the second call reports `unchanged` and rewrites nothing.
    let dir = project("sync-apply", "allowed_tools = [\"Bash(echo *)\"]\n");
    let request = SyncRequest {
        cwd: Some(dir.clone()),
        harness: vec!["claude-code".to_string()],
        ..SyncRequest::default()
    };

    let first = sync::sync(&request).expect("the policy merges into the harness config");
    let result = &first.results[0];
    assert_eq!(result.harness, "claude-code");
    assert_eq!(result.status, "created");
    let written = std::fs::read_to_string(dir.join(".claude").join("settings.json"))
        .expect("sync wrote the harness's own config file");
    assert!(written.contains("Bash(echo *)"), "got {written}");

    let second = sync::sync(&request).expect("re-syncing is safe");
    assert_eq!(second.results[0].status, "unchanged");
    assert!(!second.pending_changes());
    let _ = std::fs::remove_dir_all(&dir);
}

// capability: sync
#[test]
fn a_check_only_sync_writes_nothing_and_says_a_change_is_pending() {
    // The CI mode a consumer wires into its own gate: it must be able to learn
    // "out of sync" without the call having fixed it.
    let dir = project("sync-check", "allowed_tools = [\"Bash(echo *)\"]\n");
    let report = sync::sync(&SyncRequest {
        cwd: Some(dir.clone()),
        harness: vec!["claude-code".to_string()],
        check: true,
        ..SyncRequest::default()
    })
    .expect("a check-only sync reports rather than writes");

    assert!(report.check);
    assert!(report.pending_changes(), "the pending change is reported");
    assert!(
        !dir.join(".claude").join("settings.json").exists(),
        "a check must not write the file it describes"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// capability: usage
#[test]
fn a_consumer_reads_provider_headroom_for_the_whole_fleet_in_process() {
    // This is the call `oneagentgraph`'s health check spawns the CLI for. It
    // reports every harness — those that cannot say how much headroom they have
    // report *which kind of cannot*, never a fabricated zero.
    let report = usage::report(&UsageRequest {
        all: true,
        // Every probe that spawns a harness resolves to a path that is not
        // there, so nothing here reaches a provider: the assertion is about the
        // report's shape and honesty, not about anyone's real quota.
        bin: oneharness_core::domain::harness::all()
            .iter()
            .map(|spec| format!("{}=/nonexistent/oneharness-not-a-real-binary", spec.id))
            .collect(),
        no_config: true,
        timeout: Some(std::time::Duration::from_secs(5)),
        ..UsageRequest::default()
    })
    .expect("a fleet sweep needs no configuration");

    assert_eq!(
        report.identities.len(),
        oneharness_core::domain::harness::all().len(),
        "the sweep covers the fleet"
    );
    for identity in &report.identities {
        let rendered = serde_json::to_string(identity).expect("an identity serializes");
        assert!(
            !rendered.contains("\"used_percent\":0.0"),
            "an absent figure must never be reported as 0%: {rendered}"
        );
    }
    // And the report round-trips through the public wire contract a consumer
    // would otherwise have parsed off stdout.
    let json = serde_json::to_value(&report).expect("the report serializes");
    assert!(json.get("observed_at").is_some());
    assert!(json.get("schema_version").is_some());
}

// capability: usage
#[test]
fn an_undeclared_variant_is_refused_before_any_probe_runs() {
    // The failure path: a selector naming an identity the config never declared
    // must be a loud error, not a probe silently attributed to the base
    // harness's credentials.
    let error = usage::report(&UsageRequest {
        harness: vec!["claude-code:nope".to_string()],
        no_config: true,
        ..UsageRequest::default()
    })
    .expect_err("an undeclared variant is a usage error");
    assert!(matches!(
        error,
        OneharnessError::UnknownHarnessVariant { .. }
    ));
}

// capability: init
#[test]
fn a_consumer_scaffolds_a_config_that_the_config_parser_accepts() {
    let dir = scratch("init-write");
    let path = dir.join("oneharness.toml");
    let report = init::init(&InitRequest {
        path: Some(path.clone()),
        force: false,
    })
    .expect("a fresh path is written");

    assert_eq!(report.path, path.display().to_string());
    let written = std::fs::read_to_string(&path).expect("the scaffold is on disk");
    // The whole point of the scaffold: what it writes is a config oneharness
    // can read back.
    config::parse(&written).expect("the starter config parses");
    let _ = std::fs::remove_dir_all(&dir);
}

// capability: init
#[test]
fn a_scaffold_never_clobbers_an_existing_config_without_being_told_to() {
    let dir = scratch("init-existing");
    let path = dir.join("oneharness.toml");
    std::fs::write(&path, "harnesses = [\"codex\"]\n").expect("plant a real config");

    let error = init::init(&InitRequest {
        path: Some(path.clone()),
        force: false,
    })
    .expect_err("an existing file is refused");
    assert!(matches!(error, OneharnessError::InitFileExists { .. }));
    assert_eq!(
        std::fs::read_to_string(&path).expect("still there"),
        "harnesses = [\"codex\"]\n",
        "the refusal must leave the original untouched"
    );

    init::init(&InitRequest {
        path: Some(path.clone()),
        force: true,
    })
    .expect("--force overwrites deliberately");
    assert_eq!(
        std::fs::read_to_string(&path).expect("overwritten"),
        init::starter_config()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// capability: gate
#[test]
fn a_consumer_hosting_its_own_hook_runner_renders_a_harness_native_deny() {
    // `oneharness gate` is a stdin/stdout shell over this decision, so a
    // consumer running its own hook host answers the harness directly. The
    // verdict shape is per harness and must stay each one's own.
    use oneharness_core::domain::gate;

    let spec = oneharness_core::domain::harness::by_id("claude-code")
        .expect("claude-code is in the registry");
    let shape = spec
        .gate_deny
        .expect("claude-code expresses a pre-tool deny");
    let verdict: serde_json::Value = serde_json::from_str(&gate::render_deny(shape, "no network"))
        .expect("the verdict is the harness's own JSON");
    assert_eq!(
        verdict["hookSpecificOutput"]["permissionDecision"], "deny",
        "got {verdict}"
    );
    assert_eq!(
        verdict["hookSpecificOutput"]["permissionDecisionReason"],
        "no network"
    );

    // And the decision itself: a call that does not match is allowed through,
    // which is the fall-through the gate emits nothing for.
    assert!(gate::should_deny(
        r#"{"command":"curl example.com"}"#,
        "curl"
    ));
    assert!(!gate::should_deny(r#"{"command":"ls"}"#, "curl"));
}

// capability: mock
#[test]
fn a_consumer_decides_a_tool_call_against_a_ruleset_without_spawning_the_cli() {
    // The read-write sibling of `gate`: the same hook loop, driven by a
    // ruleset. First matching rule wins, and a call nothing matches proceeds.
    use oneharness_core::domain::mock;

    let rules = mock::parse_rules(
        r#"{"rules":[{"match":{"tool":"Bash"},"action":{"deny":{"message":"no shell"}}}]}"#,
    )
    .expect("a well-formed ruleset parses");

    let matched = mock::decide(
        r#"{"tool_name":"Bash","tool_input":{"command":"ls"}}"#,
        &rules,
    );
    assert!(matched.is_some(), "the Bash rule should match");

    assert!(
        mock::decide(r#"{"tool_name":"Read","tool_input":{}}"#, &rules).is_none(),
        "an unmatched call proceeds — the responder emits nothing"
    );

    // The failure path: a ruleset that is not one is refused loudly at parse
    // time rather than silently allowing everything through.
    assert!(mock::parse_rules("{\"rules\":[{\"match\":{}}]}").is_err());
}

// capability: interrupt
#[test]
fn interrupting_a_session_nobody_is_running_is_answered_rather_than_hung() {
    // The recovery path a supervisor hits most: it asks a run to stop and the
    // run is already gone. That must come back as a refusal frame it can read,
    // not a hang or a panic — and `is_ok()` is what the CLI turns into its
    // exit code.
    use oneharness_core::domain::control::ControlRequest;
    use oneharness_core::io::control;

    let dir = scratch("interrupt-absent");
    let socket = dir.join("control").join("nobody.sock");
    let response = control::send(&socket, &ControlRequest::interrupt());

    assert!(
        !response.is_ok(),
        "there is no run behind that socket: {response:?}"
    );
    let rendered = serde_json::to_value(&response).expect("the frame serializes");
    assert!(
        rendered.get("reason").is_some(),
        "a refusal names which refusal it is: {rendered}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// capability: historyList
// capability: history
// capability: historyClear
#[test]
fn a_consumer_lists_reads_and_clears_the_run_history_it_recorded() {
    // One journey across three entry points, because they share a store: record
    // a run, list it back, read its records, then delete it. A consumer building
    // its own history view does exactly this.
    use oneharness_core::domain::history::HistoryLabels;
    use oneharness_core::io::history::{self, HistoryWriter};

    hermetic_environment();
    let dir = scratch("history-journey");
    let project = scratch("history-journey-project");
    let writer = HistoryWriter::open(&dir, &project, "a recorded run", HistoryLabels::default())
        .expect("the store opens");
    let run = writer.begin_run();
    writer
        .append_event(
            run,
            "claude-code",
            oneharness_core::domain::events::ActionEvent {
                kind: "tool_call".to_string(),
                name: Some("Bash".to_string()),
                input: Some(serde_json::json!({"command": "true"})),
                output: None,
                index: 0,
                tool_call_id: Some("tool-1".to_string()),
                started_at: None,
                finished_at: None,
                duration_ms: None,
                status: None,
                timing_source: None,
            },
        )
        .expect("the event is durable");

    // An in-flight run is not a listed session yet: `list_sessions` counts
    // completed run records, and only events have landed so far.
    assert!(
        history::list_sessions(&dir, None)
            .expect("the store lists")
            .is_empty(),
        "a run with no terminal record is not a session to list"
    );

    writer
        .append_streamed(
            run,
            oneharness_core::domain::mode::PermissionMode::Default,
            None,
            "do the thing",
            &finished_result(),
            &std::collections::BTreeSet::from([0]),
        )
        .expect("the terminal record is durable");

    let sessions = history::list_sessions(&dir, None).expect("the store lists");
    assert_eq!(sessions.len(), 1, "the completed session is listed");
    // The name is oneharness-derived and slugged, never the harness's — headless
    // harnesses expose only an opaque session id, so there is nothing to take.
    assert_eq!(sessions[0].name, "a-recorded-run");

    let records = history::read_session(std::path::Path::new(&sessions[0].path))
        .expect("the session materializes");
    assert_eq!(records.len(), 1, "one completed run");

    let removed = history::remove_sessions(&dir, None).expect("the store clears");
    assert_eq!(removed.len(), 1, "clearing removed the one session");
    assert!(
        history::list_sessions(&dir, None)
            .expect("the emptied store still lists")
            .is_empty(),
        "nothing is left after a clear"
    );
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&project);
}

// capability: historyMigrate
#[test]
fn a_legacy_history_store_migrates_in_process() {
    // The 0.x whole-record stores predate the event-sourced line format, so a
    // consumer inheriting one has to rewrite it before reading. An empty store
    // is the boundary case: nothing to do, and no error.
    use oneharness_core::io::history;

    let dir = scratch("history-migrate");
    let summaries = history::migrate(&dir).expect("an empty store migrates cleanly");
    assert!(summaries.is_empty(), "nothing to rewrite: {summaries:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

// capability: historyWatch
#[test]
fn a_consumer_follows_the_history_store_as_records_land() {
    // The streaming view: a watcher opened on a store yields what is already
    // there, so a consumer resuming from a cursor never has to re-scan the tree.
    use oneharness_core::domain::history::HistoryLabels;
    use oneharness_core::io::history::{self, HistoryWriter};

    hermetic_environment();
    let dir = scratch("history-watch");
    let project = scratch("history-watch-project");
    let writer = HistoryWriter::open(&dir, &project, "watched", HistoryLabels::default())
        .expect("the store opens");
    let run = writer.begin_run();
    writer
        .append_event(
            run,
            "opencode",
            oneharness_core::domain::events::ActionEvent {
                kind: "tool_call".to_string(),
                name: Some("Bash".to_string()),
                input: None,
                output: None,
                index: 0,
                tool_call_id: None,
                started_at: None,
                finished_at: None,
                duration_ms: None,
                status: None,
                timing_source: None,
            },
        )
        .expect("the event is durable");

    let mut watcher =
        history::HistoryWatcher::open(&dir, None, HistoryLabels::default(), None, true)
            .expect("the watcher opens on the store");
    // Opening reconciles the index, so the event this run already appended is
    // pending before anything new lands — which is what makes a resumed watch
    // complete rather than only forward-looking.
    watcher.poll().expect("the watcher reads the store");
    let events = watcher.drain_events();
    assert!(
        events.iter().any(|line| line.harness == "opencode"),
        "the already-recorded event should be waiting for a fresh watcher: {events:?}"
    );

    // The failure path: resuming from a cursor the store has never seen is a
    // loud error, not a silent replay from the beginning.
    let unknown = "01900000-0000-7000-8000-000000000000"
        .parse()
        .expect("a well-formed UUIDv7");
    assert!(
        history::HistoryWatcher::open(&dir, Some(unknown), HistoryLabels::default(), None, true)
            .is_err(),
        "an unknown cursor must be refused"
    );
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&project);
}
