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

use oneharness_core::domain::config;
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
