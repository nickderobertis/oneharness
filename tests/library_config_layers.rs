//! Repeatable config files through the library: every request struct that
//! reads configuration takes `config: Vec<PathBuf>`, layered in order with each
//! later file overriding the earlier ones — the same result the CLI's repeated
//! `--config` gives (`crates/oneharness-e2e/tests/cli.rs` drives that half through the binary).
//!
//! These calls read the environment layer, so this is its own test binary: it
//! strips every ambient `ONEHARNESS_*` override once, and the one test that
//! sets an override holds [`ENV_LOCK`] as every test here does, so no other
//! test's load can observe it.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use oneharness_core::domain::mode::PermissionMode;
use oneharness_core::io::config;
use oneharness_core::io::detect::{self, DetectRequest};
use oneharness_core::io::registry::{self, ListRequest};
use oneharness_core::io::run::{run, RunControls, RunRequest};
use oneharness_core::io::scratch::ScratchDir;
use oneharness_core::io::sync::{self, SyncRequest};
use oneharness_core::io::usage::{self, UsageRequest};

#[path = "support/library_fixture.rs"]
mod fixture;

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Serialize this binary's tests over the process environment, stripping every
/// ambient `ONEHARNESS_*` override (a host running its agents through
/// oneharness carries several) before the first one runs.
fn hermetic() -> MutexGuard<'static, ()> {
    let guard = ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
    for (name, _) in std::env::vars() {
        if name.starts_with("ONEHARNESS_") {
            std::env::remove_var(name);
        }
    }
    guard
}

/// A scratch directory holding `d.toml` (a wrapper's defaults) and `j.toml`
/// (its user's config), plus a project `oneharness.toml` that explicit files
/// must never discover.
fn two_configs(tag: &str, d: &str, j: &str) -> (ScratchDir, PathBuf, PathBuf) {
    let dir = ScratchDir::new(&format!("library-layers-{tag}")).unwrap();
    std::fs::write(dir.join("oneharness.toml"), "model = \"project\"\n").unwrap();
    let (d_path, j_path) = (dir.join("d.toml"), dir.join("j.toml"));
    std::fs::write(&d_path, d).unwrap();
    std::fs::write(&j_path, j).unwrap();
    (dir, d_path, j_path)
}

fn toml_path(path: &Path) -> String {
    format!("'{}'", path.display())
}

/// Top-level `settings`, then a selection of the mock claude-code.
fn mock_claude_defaults(settings: &str) -> String {
    format!(
        "{settings}harnesses = [\"claude-code\"]\n[harness.claude-code]\nbin = {}\n",
        toml_path(&fixture::mock_bin())
    )
}

/// A layered run of the mock, recording the argv it received.
fn layered_run(dir: &Path, config: Vec<PathBuf>, argv_file: &Path) -> RunRequest {
    RunRequest {
        prompt: vec!["hi".to_string()],
        cwd: Some(dir.to_path_buf()),
        config,
        env: vec![
            r#"MOCK_STDOUT={"result":"layered"}"#.to_string(),
            format!("MOCK_ARGV_FILE={}", argv_file.display()),
            fixture::profile_redirect(),
        ],
        timeout: Some(60),
        ..RunRequest::default()
    }
}

#[test]
fn a_run_request_layers_its_config_files_beneath_env_and_request_fields() {
    let _env = hermetic();
    let defaults = mock_claude_defaults("mode = \"read-only\"\n");
    let cases = [
        (
            "the later file's mode wins",
            "mode = \"auto\"\n",
            None,
            None,
            "auto",
            ["--permission-mode", "auto"],
            false,
        ),
        (
            "the earlier file's mode holds where the later file sets none",
            "timeout = 60\n",
            None,
            None,
            "read-only",
            ["--permission-mode", "bypassPermissions"],
            true,
        ),
        (
            "ONEHARNESS_MODE beats both files",
            "mode = \"auto\"\n",
            Some("plan"),
            None,
            "plan",
            ["--permission-mode", "plan"],
            false,
        ),
        (
            "the request's mode beats the environment and both files",
            "mode = \"auto\"\n",
            Some("plan"),
            Some(PermissionMode::Bypass),
            "bypass",
            ["--permission-mode", "bypassPermissions"],
            false,
        ),
    ];
    for (i, (label, later, env_mode, mode, expected, pair, read_only_tools)) in
        cases.into_iter().enumerate()
    {
        let (dir, d, j) = two_configs(&format!("mode-{i}"), &defaults, later);
        let argv_file = dir.join("argv.txt");
        match env_mode {
            Some(value) => std::env::set_var("ONEHARNESS_MODE", value),
            None => std::env::remove_var("ONEHARNESS_MODE"),
        }
        let outcome = run(
            &RunRequest {
                mode,
                ..layered_run(&dir, vec![d, j], &argv_file)
            },
            RunControls::default(),
        );
        std::env::remove_var("ONEHARNESS_MODE");
        let outcome = outcome.unwrap_or_else(|err| panic!("{label}: {err}"));
        assert_eq!(outcome.report.permission_mode.as_str(), expected, "{label}");
        let received: Vec<String> = std::fs::read_to_string(&argv_file)
            .unwrap_or_else(|err| panic!("{label}: the harness never ran: {err}"))
            .lines()
            .map(str::to_string)
            .collect();
        assert!(
            received.windows(2).any(|w| w == pair),
            "{label}: the harness received {received:?}"
        );
        assert_eq!(
            received.iter().any(|arg| arg == "--tools"),
            read_only_tools,
            "{label}: only read-only narrows the tool set: {received:?}"
        );
    }
}

#[test]
fn a_run_request_reports_every_layer_in_the_order_config_explains_them() {
    let _env = hermetic();
    let (dir, d, j) = two_configs(
        "provenance",
        &mock_claude_defaults("model = \"d-model\"\n"),
        "extends = \"shared/j-base.toml\"\nmode = \"auto\"\n",
    );
    std::fs::create_dir_all(dir.join("shared")).unwrap();
    std::fs::write(
        dir.join("shared").join("j-base.toml"),
        "model = \"j-base-model\"\n",
    )
    .unwrap();
    let parent = dir.join("shared/j-base.toml");
    let expected = [
        d.display().to_string(),
        parent.display().to_string(),
        j.display().to_string(),
        "environment".to_string(),
    ];

    std::env::set_var("ONEHARNESS_MAX_PARALLEL", "3");
    let layers = config::load_layers(&[d.clone(), j.clone()], false, &dir);
    let outcome = run(
        &layered_run(&dir, vec![d.clone(), j.clone()], &dir.join("argv.txt")),
        RunControls::default(),
    );
    std::env::remove_var("ONEHARNESS_MAX_PARALLEL");

    let layers: Vec<String> = layers.unwrap().into_iter().map(|(path, _)| path).collect();
    assert_eq!(layers, expected);
    let report = outcome.expect("the layered run is valid").report;
    assert_eq!(report.config_files, expected);
    // The parent sits above `d.toml`, so its model is the one the run used.
    assert_eq!(report.results[0].model.as_deref(), Some("j-base-model"));
    assert_eq!(report.permission_mode, PermissionMode::Auto);
}

#[test]
fn a_list_request_layers_its_config_files() {
    let _env = hermetic();
    // `bin` is set only in the earlier file; `model` in both.
    let (dir, d, j) = two_configs(
        "list",
        "[harness.codex.variant.work]\nmodel = \"d-model\"\nbin = \"/d/codex\"\n",
        "[harness.codex.variant.work]\nmodel = \"j-model\"\n",
    );
    let report = registry::list(&ListRequest {
        config: vec![d, j],
        cwd: Some(dir.to_path_buf()),
        ..ListRequest::default()
    })
    .expect("the layered files load");
    let codex = report
        .harnesses
        .iter()
        .find(|h| h.id == "codex")
        .expect("codex is described");
    let work = codex
        .variants
        .iter()
        .find(|v| v.name == "work")
        .expect("the variant both files declare");
    assert_eq!(work.bin.as_deref(), Some("/d/codex"));
    assert_eq!(work.model.as_deref(), Some("j-model"));
}

/// codex's `bin` is set only in the earlier file; claude-code's in both, where
/// the later file's wins. Every path is absent, so nothing real runs.
fn detect_and_usage_configs(tag: &str) -> (ScratchDir, Vec<PathBuf>) {
    let (dir, d, j) = two_configs(
        tag,
        "[harness.codex]\nbin = '/nonexistent/d-codex'\n\
         [harness.claude-code]\nbin = '/nonexistent/d-claude'\n",
        "[harness.claude-code]\nbin = '/nonexistent/j-claude'\n",
    );
    (dir, vec![d, j])
}

const SELECTION: [&str; 2] = ["claude-code", "codex"];
const EXPECTED_BINS: [(&str, &str); 2] = [
    ("claude-code", "/nonexistent/j-claude"),
    ("codex", "/nonexistent/d-codex"),
];

#[test]
fn a_detect_request_layers_its_config_files() {
    let _env = hermetic();
    let (dir, config) = detect_and_usage_configs("detect");
    let report = detect::detect(&DetectRequest {
        config,
        cwd: Some(dir.to_path_buf()),
        harness: SELECTION.map(str::to_string).to_vec(),
        ..DetectRequest::default()
    })
    .expect("the layered files name what to probe");
    let bins: Vec<(&str, &str)> = report
        .detected
        .iter()
        .map(|h| (h.id.as_str(), h.bin.as_str()))
        .collect();
    assert_eq!(bins, EXPECTED_BINS);
}

#[test]
fn a_usage_request_layers_its_config_files() {
    let _env = hermetic();
    let (dir, config) = detect_and_usage_configs("usage");
    let report = usage::report(&UsageRequest {
        config,
        cwd: Some(dir.to_path_buf()),
        harness: SELECTION.map(str::to_string).to_vec(),
        timeout: Some(std::time::Duration::from_secs(5)),
        ..UsageRequest::default()
    })
    .expect("the layered files name what to probe");
    let json = serde_json::to_value(&report).unwrap();
    let bins: Vec<(&str, &str)> = json["identities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            assert_eq!(i["availability"]["reason"]["kind"], "binary_missing", "{i}");
            (
                i["harness"].as_str().unwrap(),
                i["availability"]["reason"]["bin"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(bins, EXPECTED_BINS);
}

#[test]
fn a_sync_request_layers_its_config_files() {
    let _env = hermetic();
    // `denied_tools` is set only in the earlier file; `allowed_tools` in both.
    let (dir, d, j) = two_configs(
        "sync",
        "allowed_tools = [\"Bash(ls)\"]\ndenied_tools = [\"Bash(rm:*)\"]\n",
        "allowed_tools = [\"Bash(echo layered)\"]\n",
    );
    sync::sync(&SyncRequest {
        config: vec![d, j],
        cwd: Some(dir.to_path_buf()),
        harness: vec!["claude-code".to_string()],
        ..SyncRequest::default()
    })
    .expect("the layered policy syncs");
    let written: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join(".claude").join("settings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        written["permissions"],
        serde_json::json!({"allow": ["Bash(echo layered)"], "deny": ["Bash(rm:*)"]})
    );
}
