// llmlint: ignore-file[comments_earn_their_place] Contradictory rules leave no arrangement that passes both: run 20260730T015003Z-03f66 rejected usage prose duplicated from `docs/harness-usage.md`, and run 20260730T022546Z-ea300 rejected the requested deferral to that document.
//! End-to-end tests that drive the real `oneharness` binary the way a consumer
//! does, asserting on exit codes and the JSON contract. The subprocess path is
//! exercised hermetically through the `oneharness-mock-harness` fixture (a fake
//! harness wired in via `--bin`/env overrides), so these are deterministic,
//! network-free, and run identically on every platform.

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use oneharness_core::domain::events::{ActionEvent, TimingSource, ToolCallStatus};
use oneharness_core::domain::history::{HistoryLabels, HistoryLine, HistoryStreamEnvelope};
use oneharness_core::domain::report::{RunStreamEnvelope, Status};
use oneharness_core::domain::session;
use oneharness_core::domain::signals::FailureKind;
use oneharness_core::domain::usage::{UsageProbe, UsageSupport};
use oneharness_core::io::history::HistoryWriter;
use serde_json::Value;

const ALL_IDS: &[&str] = &[
    "claude-code",
    "codex",
    "opencode",
    "goose",
    "qwen",
    "crush",
    "copilot",
    "cursor",
];

fn oneharness_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_oneharness"))
}

/// The mock harness is built beside the main binary when the `mock-harness`
/// feature is enabled (which `just test` / `just check` and CI do).
fn mock_bin() -> PathBuf {
    let mut path = oneharness_bin();
    path.set_file_name(format!(
        "oneharness-mock-harness{}",
        std::env::consts::EXE_SUFFIX
    ));
    path
}

fn run(args: &[&str], envs: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(oneharness_bin());
    // Hermetic by default: the developer's real user/project config files must
    // never shape these assertions. Config behavior itself is tested through
    // `run_with_config`, which opts back in.
    cmd.env("ONEHARNESS_NO_CONFIG", "1");
    let redirect = mock_profile_redirect();
    cmd.args(with_mock_profile_redirect(args, &redirect));
    for (key, value) in envs {
        cmd.env(key, value);
    }
    cmd.output().expect("failed to run oneharness")
}

/// `--env` for a mock-harness child this test is going to have killed.
///
/// A cancelled or timed-out run tears its harness process down with `SIGKILL`
/// after the TERM grace, so that process never flushes its coverage profile and
/// leaves a truncated `.profraw`. `just coverage` collects every such file from
/// the target directory, and one bad header fails the whole `llvm-profdata
/// merge` — taking the gate down over a *fixture* whose coverage nobody wanted.
///
/// This targets the harness child only (`--env` sets the environment of each
/// harness process, not of oneharness), so the binary under test keeps
/// contributing its own coverage exactly as before. Harmless when the fixture is
/// not instrumented, which ignores the variable.
fn mock_profile_redirect() -> String {
    format!(
        "LLVM_PROFILE_FILE={}",
        std::env::temp_dir()
            .join("oneharness-killed-mock-%p.profraw")
            .display()
    )
}

/// `args` with that `--env` added, for any `run` invocation.
///
/// Applied to every run rather than only the tests that cancel on purpose: a
/// harness process is also torn down by a timeout, and under a loaded parallel
/// suite the TERM grace expires often enough that *which* run leaves the
/// truncated profile is a race rather than a property of one test. Inserted
/// before a raw `--` so a passthrough argument list keeps its meaning.
fn with_mock_profile_redirect<'a>(args: &[&'a str], redirect: &'a str) -> Vec<&'a str> {
    if args.first() != Some(&"run") {
        return args.to_vec();
    }
    let at = args.iter().position(|a| *a == "--").unwrap_or(args.len());
    let mut out = args[..at].to_vec();
    out.push("--env");
    out.push(redirect);
    out.extend_from_slice(&args[at..]);
    out
}

/// The `ONEHARNESS_*` env overrides recognized as a config layer. Cleared in
/// `run_with_config` so a developer's ambient value can never reshape a
/// config-layering assertion; a test that wants one passes it via `envs`.
const ENV_OVERRIDE_VARS: &[&str] = &[
    "ONEHARNESS_ALL",
    "ONEHARNESS_HARNESSES",
    "ONEHARNESS_EXCLUDE",
    "ONEHARNESS_MODEL",
    "ONEHARNESS_MODELS",
    "ONEHARNESS_SYSTEM",
    "ONEHARNESS_REASONING",
    "ONEHARNESS_MODE",
    "ONEHARNESS_BYPASS",
    "ONEHARNESS_TIMEOUT",
    "ONEHARNESS_OUTPUT_FORMAT",
    "ONEHARNESS_SCHEMA_FILE",
    "ONEHARNESS_SCHEMA_MAX_RETRIES",
    "ONEHARNESS_MAX_PARALLEL",
    "ONEHARNESS_RUN_MODE",
    "ONEHARNESS_REQUIRE_AVAILABLE",
    "ONEHARNESS_HISTORY",
    "ONEHARNESS_HISTORY_DIR",
    "ONEHARNESS_HISTORY_LABELS",
    "ONEHARNESS_STREAM",
];

/// Run with config loading enabled but still hermetic: the user-level config is
/// pinned to `user_config` via ONEHARNESS_CONFIG (so the developer's real one is
/// never read), ambient `ONEHARNESS_*` overrides are stripped, and project
/// discovery is steered with `--cwd` by the caller.
fn run_with_config(args: &[&str], envs: &[(&str, &str)], user_config: &std::path::Path) -> Output {
    let mut cmd = Command::new(oneharness_bin());
    cmd.env("ONEHARNESS_CONFIG", user_config);
    for var in ENV_OVERRIDE_VARS {
        cmd.env_remove(var);
    }
    let redirect = mock_profile_redirect();
    cmd.args(with_mock_profile_redirect(args, &redirect));
    for (key, value) in envs {
        cmd.env(key, value);
    }
    cmd.output().expect("failed to run oneharness")
}

/// A unique temp dir holding a project `oneharness.toml` plus an (empty unless
/// stated) user-level config to pin ONEHARNESS_CONFIG to.
struct ConfigFixture {
    dir: PathBuf,
}

impl ConfigFixture {
    fn new(tag: &str, project_toml: &str, user_toml: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("oneharness-cfgtest-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("oneharness.toml"), project_toml).unwrap();
        std::fs::write(dir.join("user-config.toml"), user_toml).unwrap();
        Self { dir }
    }

    fn cwd(&self) -> String {
        self.dir.display().to_string()
    }

    fn user_config(&self) -> PathBuf {
        self.dir.join("user-config.toml")
    }
}

impl Drop for ConfigFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn command_of(value: &Value, index: usize) -> Vec<String> {
    value["results"][index]["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect()
}

fn json_stdout(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|err| {
        panic!(
            "stdout was not JSON: {err}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn bin_override(id: &str) -> String {
    format!("{id}={}", mock_bin().display())
}

#[cfg(any(unix, windows))]
fn native_tick_file(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "oneharness-{label}-{}-{:?}.ticks",
        std::process::id(),
        std::thread::current().id()
    ))
}

#[cfg(any(unix, windows))]
fn assert_native_descendant_stopped(path: &std::path::Path) {
    let witness_deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let witnessed = loop {
        let length = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
        if length > 0 {
            break length;
        }
        assert!(
            std::time::Instant::now() < witness_deadline,
            "native descendant never wrote its durable tick witness"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    };

    // A live fixture ticks at least every 50 ms. Poll for a sustained quiet
    // interval instead of assuming one fixed sleep is enough on a busy runner.
    let stop_deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let mut last_length = witnessed;
    let mut quiet_since = std::time::Instant::now();
    loop {
        std::thread::sleep(std::time::Duration::from_millis(20));
        let length = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
        if length != last_length {
            last_length = length;
            quiet_since = std::time::Instant::now();
        }
        if quiet_since.elapsed() >= std::time::Duration::from_millis(750) {
            return;
        }
        assert!(
            std::time::Instant::now() < stop_deadline,
            "native descendant kept ticking after process-tree teardown"
        );
    }
}

#[test]
fn mock_fixture_is_built() {
    assert!(
        mock_bin().exists(),
        "mock harness not found at {}; run tests with `--features mock-harness` (e.g. `just test`)",
        mock_bin().display()
    );
}

#[test]
fn mock_fixture_only_exposes_session_ids_in_session_bearing_formats() {
    let invoke = |args: &[&str], stdout: &str| {
        Command::new(mock_bin())
            .args(args)
            .env("MOCK_STDOUT", stdout)
            .output()
            .expect("failed to run mock harness")
    };

    // Real Codex emits `thread.started.thread_id` only under `exec --json`.
    let codex_text = invoke(&["exec", "hi"], r#"{"thread_id":"th-1","result":"hi"}"#);
    assert!(codex_text.status.success());
    assert!(json_stdout(&codex_text).get("thread_id").is_none());
    let codex_json = invoke(
        &["exec", "--json", "hi"],
        r#"{"thread_id":"th-1","result":"hi"}"#,
    );
    assert_eq!(json_stdout(&codex_json)["thread_id"], "th-1");

    // Qwen likewise omits `session_id` in text mode and surfaces it in its
    // machine-readable output modes.
    let qwen_text = invoke(
        &["--approval-mode", "default", "-p", "hi"],
        r#"{"session_id":"q-1","result":"hi"}"#,
    );
    assert!(json_stdout(&qwen_text).get("session_id").is_none());
    let qwen_stream = invoke(
        &[
            "--approval-mode",
            "default",
            "--output-format",
            "stream-json",
            "-p",
            "hi",
        ],
        r#"{"session_id":"q-1","result":"hi"}"#,
    );
    assert_eq!(json_stdout(&qwen_stream)["session_id"], "q-1");
}

#[test]
fn every_report_carries_the_shared_schema_version() {
    // `run`, `list`, `detect`, `sync`, and `config` share one contract version,
    // so a consumer reads any of them with one number — and a bump must move
    // every surface at once. Pinned literally on purpose: asserting against the
    // constant would pass through a bump nobody intended.
    let version = "0.6";
    let printed = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--print-command",
        ],
        &[],
    );
    assert_eq!(json_stdout(&printed)["schema_version"], version);
    assert_eq!(json_stdout(&run(&["list"], &[]))["schema_version"], version);
    assert_eq!(
        json_stdout(&run(&["detect"], &[]))["schema_version"],
        version
    );
    let fx = ConfigFixture::new("shared-schema-version", "allowed_tools = [\"Read\"]\n", "");
    let synced = run_with_config(
        &["sync", "--harness", "claude-code", "--cwd", &fx.cwd()],
        &[],
        &fx.user_config(),
    );
    assert!(
        synced.status.success(),
        "{}",
        String::from_utf8_lossy(&synced.stderr)
    );
    assert_eq!(json_stdout(&synced)["schema_version"], version);
    let config = run_with_config(
        &["config", "--cwd", &fx.cwd(), "--compact"],
        &[],
        &fx.user_config(),
    );
    assert_eq!(json_stdout(&config)["schema_version"], version);
}

#[test]
fn list_describes_every_harness() {
    let output = run(&["list"], &[]);
    assert!(output.status.success());
    let value = json_stdout(&output);
    assert_eq!(value["schema_version"], "0.6");
    let ids: Vec<&str> = value["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["id"].as_str().unwrap())
        .collect();
    for id in ALL_IDS {
        assert!(ids.contains(id), "list is missing harness `{id}`");
    }
}

#[test]
fn list_rejects_a_malformed_loaded_config() {
    let fx = ConfigFixture::new("list-malformed-config", "", "not valid = [");
    let output = run_with_config(&["list", "--compact"], &[], &fx.user_config());
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid config"));
    assert!(output.stdout.is_empty());
}

#[test]
fn print_command_pins_argv_for_every_harness() {
    // The deterministic, network-free proof that each adapter builds the right
    // invocation. argv[0] is the default binary; we pin argv[1..].
    let expected: &[(&str, &[&str])] = &[
        (
            "claude-code",
            &[
                "-p",
                "hi",
                "--permission-mode",
                "bypassPermissions",
                "--output-format",
                "json",
            ],
        ),
        (
            "codex",
            &[
                "exec",
                "--dangerously-bypass-approvals-and-sandbox",
                "--json",
                "hi",
            ],
        ),
        (
            "opencode",
            &[
                "run",
                "--dangerously-skip-permissions",
                "--format",
                "json",
                "hi",
            ],
        ),
        ("goose", &["run", "--with-builtin", "developer", "-t", "hi"]),
        ("qwen", &["--yolo", "-p", "hi"]),
        ("crush", &["run", "-q", "hi"]),
        (
            "copilot",
            &[
                "-p",
                "hi",
                "--allow-all-tools",
                "--allow-all-paths",
                "--no-ask-user",
            ],
        ),
        (
            "cursor",
            &["-p", "hi", "--force", "--output-format", "stream-json"],
        ),
    ];

    let output = run(
        &[
            "run",
            "--all",
            "--prompt",
            "hi",
            "--print-command",
            // Pin the bypass argvs explicitly (the global default is now
            // `default`); each harness's default-mode argv is unit-pinned in
            // `domain::harness`.
            "--mode",
            "bypass",
            "--compact",
        ],
        &[],
    );
    assert!(output.status.success());
    let value = json_stdout(&output);
    assert_eq!(value["dry_run"], true);
    assert_eq!(value["permission_mode"], "bypass");

    let results = value["results"].as_array().unwrap();
    assert_eq!(results.len(), ALL_IDS.len());

    for (id, want_tail) in expected {
        let result = results
            .iter()
            .find(|r| r["harness"] == *id)
            .unwrap_or_else(|| panic!("no result for {id}"));
        assert_eq!(result["status"], "planned", "{id} status");
        let command: Vec<&str> = result["command"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap())
            .collect();
        assert_eq!(&command[1..], *want_tail, "{id} argv tail");
    }
}

#[test]
fn no_bypass_switches_claude_to_default_mode() {
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--print-command",
            "--no-bypass",
            "--compact",
        ],
        &[],
    );
    assert!(output.status.success());
    let value = json_stdout(&output);
    let command = value["results"][0]["command"].to_string();
    // --no-bypass is shorthand for `--mode default`, which Claude expresses as
    // `dontAsk` (deny-and-continue) so a headless run never aborts on a prompt.
    assert!(command.contains("dontAsk"), "{command}");
    assert!(!command.contains("bypassPermissions"), "{command}");
    assert_eq!(value["bypass_permissions"], false);
    assert_eq!(value["permission_mode"], "default");
}

#[test]
fn mode_flag_selects_a_permission_mode() {
    // `--mode plan` reaches the harness's native plan flag and is echoed.
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--print-command",
            "--mode",
            "plan",
            "--compact",
        ],
        &[],
    );
    assert!(output.status.success());
    let value = json_stdout(&output);
    assert_eq!(value["permission_mode"], "plan");
    assert_eq!(value["bypass_permissions"], false);
    let command = value["results"][0]["command"].to_string();
    assert!(command.contains("plan"), "{command}");
}

#[test]
fn read_only_is_distinct_from_plan_and_enforced_where_possible() {
    // read-only on codex is the OS-enforced read-only sandbox; codex has no plan
    // workflow, so `--mode plan` is refused for it (use read-only instead).
    let ro = run(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "hi",
            "--print-command",
            "--mode",
            "read-only",
            "--compact",
        ],
        &[],
    );
    assert!(ro.status.success());
    let value = json_stdout(&ro);
    assert_eq!(value["permission_mode"], "read-only");
    assert_eq!(value["bypass_permissions"], false);
    let command = value["results"][0]["command"].to_string();
    assert!(command.contains("read-only"), "{command}");
    // Codex read-only is the bare sandbox — no plan instruction (that's what
    // distinguishes it from `plan`, which adds the instruction).
    assert!(
        !command.contains("PLAN MODE"),
        "read-only must not plan: {command}"
    );

    // On Claude, read-only and plan are genuinely different invocations.
    let claude_ro = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--print-command",
            "--mode",
            "read-only",
            "--compact",
        ],
        &[],
    );
    let c = json_stdout(&claude_ro)["results"][0]["command"].to_string();
    assert!(c.contains("disallowedTools"), "{c}");
}

#[test]
fn mode_delivered_via_env_reaches_the_child() {
    // OpenCode's `edit` mode has no argv flag — it rides the OPENCODE_CONFIG_CONTENT
    // inline-config env var, which must reach the spawned harness.
    let output = run(
        &[
            "run",
            "--harness",
            "opencode",
            "--prompt",
            "hi",
            "--mode",
            "edit",
            "--bin",
            &bin_override("opencode"),
            "--compact",
        ],
        &[("MOCK_ECHO_ENV", "OPENCODE_CONFIG_CONTENT")],
    );
    assert!(output.status.success());
    let value = json_stdout(&output);
    assert_eq!(value["permission_mode"], "edit");
    // The mock echoes the requested env var as `NAME=value`.
    assert_eq!(
        value["results"][0]["stdout"],
        r#"OPENCODE_CONFIG_CONTENT={"permission":{"edit":"allow","bash":"deny"}}"#
    );
    // Goose carries the whole spectrum in GOOSE_MODE; bypass = auto.
    let output = run(
        &[
            "run",
            "--harness",
            "goose",
            "--prompt",
            "hi",
            "--mode",
            "bypass",
            "--bin",
            &bin_override("goose"),
            "--compact",
        ],
        &[("MOCK_ECHO_ENV", "GOOSE_MODE")],
    );
    assert_eq!(
        json_stdout(&output)["results"][0]["stdout"],
        "GOOSE_MODE=auto"
    );
}

#[test]
fn codex_plan_is_read_only_sandbox_plus_a_plan_instruction() {
    // Codex has no native exec plan mode; oneharness synthesizes it as the
    // read-only sandbox (enforcement) + a plan instruction prepended to the
    // prompt (behavior) — reproducing Codex's interactive Plan mode.
    let output = run(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "refactor auth",
            "--mode",
            "plan",
            "--print-command",
            "--compact",
        ],
        &[],
    );
    assert!(output.status.success());
    let value = json_stdout(&output);
    assert_eq!(value["permission_mode"], "plan");
    let command: Vec<&str> = value["results"][0]["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap())
        .collect();
    assert!(
        command.windows(2).any(|w| w == ["--sandbox", "read-only"]),
        "{command:?}"
    );
    let joined = command.join(" ");
    assert!(
        joined.contains("PLAN MODE"),
        "plan instruction missing: {joined}"
    );
    assert!(joined.contains("refactor auth"), "task missing: {joined}");
}

#[test]
fn unsupported_mode_for_a_harness_is_refused() {
    // crush has no plan mode; asking for it is a loud usage error, not a run.
    let output = run(
        &[
            "run",
            "--harness",
            "crush",
            "--prompt",
            "hi",
            "--print-command",
            "--mode",
            "plan",
            "--compact",
        ],
        &[],
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("does not support"), "{stderr}");
    assert!(stderr.contains("crush"), "{stderr}");
}

#[test]
fn hang_prone_mode_warns_but_runs_and_permit_prompts_silences_it() {
    // cursor's `default` could block on an approval prompt headlessly, so
    // oneharness warns — but still runs it (the --timeout is the backstop),
    // rather than refusing. `--permit-prompts` silences the warning.
    let base = [
        "run",
        "--harness",
        "cursor",
        "--prompt",
        "hi",
        "--print-command",
        "--mode",
        "default",
        "--compact",
    ];
    let output = run(&base, &[]);
    assert!(output.status.success(), "hang-prone mode should still run");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("may block on an interactive"), "{stderr}");
    assert_eq!(json_stdout(&output)["permission_mode"], "default");
    // --permit-prompts silences the warning.
    let mut with_permit = base.to_vec();
    with_permit.push("--permit-prompts");
    let output = run(&with_permit, &[]);
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("may block"),
        "warning should be silenced: {stderr}"
    );
}

#[test]
fn default_is_the_global_default_mode() {
    // With no --mode/--bypass, the resolved mode is `default` (not bypass).
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--print-command",
            "--compact",
        ],
        &[],
    );
    assert!(output.status.success());
    let value = json_stdout(&output);
    assert_eq!(value["permission_mode"], "default");
    assert_eq!(value["bypass_permissions"], false);
    // Claude's `default` is `dontAsk` (deny-and-continue), not bypass.
    let command = value["results"][0]["command"].to_string();
    assert!(command.contains("dontAsk"), "{command}");
}

#[test]
fn model_flag_is_passed_through() {
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--model",
            "haiku",
            "--print-command",
            "--compact",
        ],
        &[],
    );
    let value = json_stdout(&output);
    let command: Vec<String> = value["results"][0]["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect();
    assert!(
        command.windows(2).any(|w| w == ["--model", "haiku"]),
        "{command:?}"
    );
    assert_eq!(value["model"], "haiku");
}

#[test]
fn output_format_override_is_emitted_and_drives_extraction() {
    // The emitted flag changes...
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--output-format",
            "stream-json",
            "--print-command",
            "--compact",
        ],
        &[],
    );
    let value = json_stdout(&output);
    let command = value["results"][0]["command"].to_string();
    assert!(command.contains("stream-json"), "{command}");
    assert!(!command.contains("\"json\""), "{command}");

    // ...and so does extraction: forcing `text` returns the raw stdout verbatim.
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--output-format",
            "text",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[("MOCK_STDOUT", r#"{"result":"x"}"#)],
    );
    let value = json_stdout(&output);
    assert_eq!(value["results"][0]["text"], r#"{"result":"x"}"#);
    assert_eq!(value["results"][0]["text_source"], "raw");
}

#[test]
fn system_prompt_maps_to_append_system_prompt_for_claude() {
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--system",
            "be terse",
            "--print-command",
            "--compact",
        ],
        &[],
    );
    let value = json_stdout(&output);
    let command: Vec<String> = value["results"][0]["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect();
    assert!(
        command
            .windows(2)
            .any(|w| w == ["--append-system-prompt", "be terse"]),
        "{command:?}"
    );
}

#[test]
fn system_prompt_maps_to_gooses_native_system_flag() {
    // Goose exposes its own `--system`, so `--system` maps to it (not prepended).
    let output = run(
        &[
            "run",
            "--harness",
            "goose",
            "--prompt",
            "hi",
            "--system",
            "be terse",
            "--print-command",
            "--compact",
        ],
        &[],
    );
    let value = json_stdout(&output);
    let command: Vec<String> = value["results"][0]["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect();
    assert!(
        command.windows(2).any(|w| w == ["--system", "be terse"]),
        "{command:?}"
    );
    // The prompt is delivered via -t and left un-prefixed.
    assert!(command.windows(2).any(|w| w == ["-t", "hi"]), "{command:?}");
}

#[test]
fn system_prompt_is_prepended_for_harness_without_a_flag() {
    // Codex has no system-prompt flag, so `--system` is prepended to the prompt
    // (it must reach the model, not be dropped).
    let output = run(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "hi",
            "--system",
            "be terse",
            "--print-command",
            "--compact",
        ],
        &[],
    );
    let value = json_stdout(&output);
    let command: Vec<String> = value["results"][0]["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect();
    assert!(command.iter().any(|t| t == "be terse\n\nhi"), "{command:?}");
}

#[test]
fn hyphenated_system_and_prompt_values_are_accepted() {
    // skilltest passes a skill as --system; a value that begins with `-`/`---`
    // (YAML front matter) must be taken as the value, not parsed as a flag.
    let output = run(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "--look-like-a-flag",
            "--system",
            "---\nname: x",
            "--print-command",
            "--compact",
        ],
        &[],
    );
    let value = json_stdout(&output);
    let command: Vec<String> = value["results"][0]["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect();
    assert!(
        command
            .iter()
            .any(|t| t == "---\nname: x\n\n--look-like-a-flag"),
        "{command:?}"
    );
}

#[test]
fn normalizes_usage_and_session_id_from_claude_json() {
    // A Claude-shaped result document carries cost, token usage, and a session id
    // buried in stdout; oneharness lifts them into the envelope, best-effort.
    let stdout = r#"{"type":"result","result":"pong","session_id":"sess-xyz",
        "total_cost_usd":0.0095,"usage":{"input_tokens":1200,"output_tokens":8,
        "cache_read_input_tokens":34,"cache_creation_input_tokens":56}}"#;
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[("MOCK_STDOUT", stdout)],
    );
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let value = json_stdout(&output);
    let result = &value["results"][0];
    assert_eq!(result["usage"]["input_tokens"], 1200);
    assert_eq!(result["usage"]["output_tokens"], 8);
    assert_eq!(result["usage"]["cache_read_tokens"], 34);
    assert_eq!(result["usage"]["cache_write_tokens"], 56);
    assert_eq!(result["usage"]["cost_usd"], 0.0095);
    assert_eq!(result["usage_source"], "json");
    assert_eq!(result["session_id"], "sess-xyz");
    // No failure on a clean run.
    assert!(result["failure_kind"].is_null());
}

#[test]
fn plain_codex_run_extracts_final_text_from_its_default_json_stream() {
    // Codex's session-bearing default is `exec --json`; its user-visible answer
    // remains the final agent_message text, so changing the transport must not
    // regress the plain `oneharness run` contract.
    let stdout = concat!(
        "{\"type\":\"thread.started\",\"thread_id\":\"th-plain\"}\n",
        "{\"type\":\"item.completed\",\"item\":{\"id\":\"item-1\",\"type\":\"agent_message\",\"text\":\"same final answer\"}}\n",
        "{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":4,\"output_tokens\":3}}\n",
    );
    let output = run(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("codex"),
            "--compact",
        ],
        &[("MOCK_STDOUT", stdout)],
    );
    assert!(output.status.success(), "{output:?}");
    let value = json_stdout(&output);
    let result = &value["results"][0];
    assert_eq!(result["output_format"], "json");
    assert_eq!(result["text"], "same final answer");
    assert_eq!(result["text_source"], "json:codex-agent-message");
    assert!(
        result["command"]
            .as_array()
            .unwrap()
            .iter()
            .any(|arg| arg == "--json"),
        "plain codex must request its JSON stream: {}",
        result["command"]
    );
}

#[test]
fn usage_fields_are_null_when_harness_reports_none() {
    // A plain result with no usage/session still yields a stable, null-filled shape.
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[("MOCK_STDOUT", r#"{"result":"hi"}"#)],
    );
    let value = json_stdout(&output);
    let result = &value["results"][0];
    assert!(result["usage"]["cost_usd"].is_null());
    assert!(result["usage"]["input_tokens"].is_null());
    assert!(result["usage"]["cache_read_tokens"].is_null());
    assert!(result["usage"]["cache_write_tokens"].is_null());
    assert!(result["usage_source"].is_null());
    assert!(result["session_id"].is_null());
}

#[test]
fn classifies_failure_kind_on_nonzero_exit() {
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[
            ("MOCK_EXIT", "1"),
            (
                "MOCK_STDERR",
                "Error: 401 Unauthorized — please authenticate",
            ),
            ("MOCK_STDOUT", ""),
        ],
    );
    assert_eq!(output.status.code(), Some(1));
    let value = json_stdout(&output);
    let result = &value["results"][0];
    assert_eq!(result["status"], "nonzero");
    assert_eq!(result["failure_kind"], "auth");
    assert_eq!(result["failure_kind_source"], "stderr");
}

#[test]
fn deferred_tool_dead_end_is_classified_not_silent() {
    // A Claude Code bridge deployment defers a builtin tool instead of running it:
    // the process exits 0 with an empty result and a `deferred_tool_use`. Without
    // detection this masquerades as an empty/invalid answer (issue #1114). It must
    // instead surface a distinct `tool_deferred` failure_kind, an actionable error
    // naming the tool, and a non-zero exit — a clean dead-end is still a failure.
    let stdout = r#"{"type":"result","num_turns":1,"stop_reason":"tool_deferred",
        "terminal_reason":"tool_deferred","result":"","permission_denials":[],
        "deferred_tool_use":{"name":"Read","input":{"file_path":"/x/usage.rs"}}}"#;
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "Read ./src/usage.rs and tell me about it",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[("MOCK_STDOUT", stdout)],
    );
    // The run dead-ended: a clean exit-0 process, but no useful work → exit 1.
    assert_eq!(
        output.status.code(),
        Some(1),
        "a dead-end must fail the run"
    );
    let value = json_stdout(&output);
    let result = &value["results"][0];
    // The process itself exited 0 — status reflects that; the failure is carried
    // by the typed signal, distinct from `status` per the contract.
    assert_eq!(result["status"], "ok");
    assert_eq!(result["failure_kind"], "tool_deferred");
    assert_eq!(result["failure_kind_source"], "stdout");
    let error = result["error"].as_str().unwrap();
    assert!(
        error.contains("`Read`"),
        "error names the deferred tool: {error}"
    );
    assert!(
        error.contains("deferred") && error.contains("inline"),
        "error is actionable: {error}"
    );
}

#[test]
fn deferred_tool_stops_a_fallback_chain_and_fails() {
    // A deferred-tool dead-end is a harness that *ran* (exit 0), so a fallback
    // chain must STOP at it — not fall through to the next candidate — and the
    // run must still fail, since the harness that ran did no useful work.
    let deferred = r#"{"type":"result","stop_reason":"tool_deferred","result":"","deferred_tool_use":{"name":"Bash"}}"#;
    let output = run(
        &[
            "run",
            "--run-mode",
            "fallback",
            "--harness",
            "claude-code,codex",
            "--prompt",
            "run echo hi",
            "--bin",
            &bin_override("claude-code"),
            "--bin",
            &bin_override("codex"),
            "--compact",
        ],
        &[("MOCK_STDOUT", deferred)],
    );
    assert_eq!(output.status.code(), Some(1), "a dead-end fails the run");
    let v = json_stdout(&output);
    // Stopped at the first harness — it ran (and dead-ended), so it is the answer;
    // codex was never attempted.
    assert_eq!(v["fallback"]["ran"], "claude-code");
    assert_eq!(v["fallback"]["fell_through"].as_array().unwrap().len(), 0);
    let results = v["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["harness"], "claude-code");
    assert_eq!(results[0]["status"], "ok");
    assert_eq!(results[0]["failure_kind"], "tool_deferred");
}

#[test]
fn deferred_tool_is_classified_on_the_streaming_path() {
    // `--stream` is a separate execution path (stream_one_harness), but it funnels
    // through the same result assembly: a deferral must be classified there too,
    // surfaced in the terminal report line, and fail the run.
    let deferred = r#"{"type":"result","stop_reason":"tool_deferred","result":"","deferred_tool_use":{"name":"Read"}}"#;
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "read a file",
            "--bin",
            &bin_override("claude-code"),
            "--stream",
        ],
        &[("MOCK_STDOUT", deferred)],
    );
    assert_eq!(output.status.code(), Some(1), "a dead-end fails the stream");
    // The terminal `{"type":"result","report":{…}}` line carries the classified
    // result (there are no tool_call event lines to emit for a deferred turn).
    let text = String::from_utf8_lossy(&output.stdout);
    let terminal: Value = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("each stream line is JSON"))
        .find(|v: &Value| v["type"] == "result")
        .expect("a terminal result line");
    let result = &terminal["report"]["results"][0];
    assert_eq!(result["status"], "ok");
    assert_eq!(result["failure_kind"], "tool_deferred");
    assert!(result["error"].as_str().unwrap().contains("`Read`"));
}

#[test]
fn deferred_tool_counts_toward_the_batch_failure_summary() {
    // A batch (one harness, N prompts) whose prompts all dead-end must count every
    // deferred result as failed in the run's stderr summary and exit non-zero —
    // the multi-result failure-count path, not just the single-result case.
    let deferred = r#"{"type":"result","stop_reason":"tool_deferred","result":"","deferred_tool_use":{"name":"Read"}}"#;
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "read file a",
            "--prompt",
            "read file b",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[("MOCK_STDOUT", deferred)],
    );
    assert_eq!(output.status.code(), Some(1));
    let value = json_stdout(&output);
    let results = value["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);
    for r in results {
        assert_eq!(r["status"], "ok");
        assert_eq!(r["failure_kind"], "tool_deferred");
    }
    // The summary counts both deferred results as failures.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("2/2 harness run(s) did not succeed"),
        "batch failure count in stderr: {stderr}"
    );
}

#[test]
fn resume_maps_to_resume_flag_and_echoes_session() {
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "continue please",
            "--resume",
            "sess-abc",
            "--print-command",
            "--compact",
        ],
        &[],
    );
    assert!(output.status.success());
    let value = json_stdout(&output);
    assert_eq!(value["resume"], "sess-abc");
    let command: Vec<String> = value["results"][0]["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect();
    assert!(
        command.windows(2).any(|w| w == ["--resume", "sess-abc"]),
        "{command:?}"
    );
}

/// A private, per-test session-store directory (removed and recreated), so the
/// uniform-handle tests never collide with each other or a real store.
fn session_store_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "oh-session-{tag}-{}-{}",
        std::process::id(),
        tag.len()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn session_create_then_continue_round_trips_via_the_store() {
    // `--session <name>` maps a caller-owned handle to the harness's native
    // session id: the first run starts fresh and captures the id; the second run
    // with the same name resumes it — no native-id threading by the caller.
    let store = session_store_dir("roundtrip");
    let store_arg = store.display().to_string();
    // A stable project dir so both runs key the same store file.
    let cwd = session_store_dir("roundtrip-cwd");
    let cwd_arg = cwd.display().to_string();
    let argv_file = store.join("argv.txt");
    let argv_arg = argv_file.display().to_string();

    // Run 1 — create. The mock emits a Claude-shaped result with a session id.
    let first = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--session",
            "greet",
            "--session-dir",
            &store_arg,
            "--cwd",
            &cwd_arg,
            "--prompt",
            "hi",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[
            (
                "MOCK_STDOUT",
                r#"{"type":"result","result":"hi","session_id":"sess-1"}"#,
            ),
            ("MOCK_ARGV_FILE", &argv_arg),
        ],
    );
    assert!(first.status.success(), "{first:?}");
    let v1 = json_stdout(&first);
    assert_eq!(v1["session"]["name"], "greet");
    assert_eq!(v1["session"]["phase"], "create");
    assert_eq!(v1["session"]["token"], "sess-1");
    // A create builds a fresh argv — no --resume.
    let argv1 = std::fs::read_to_string(&argv_file).unwrap();
    assert!(
        !argv1.lines().any(|l| l == "--resume"),
        "create should not resume: {argv1:?}"
    );

    // Run 2 — continue. Same name; oneharness resolves it to `sess-1`.
    let second = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--session",
            "greet",
            "--session-dir",
            &store_arg,
            "--cwd",
            &cwd_arg,
            "--prompt",
            "and again",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[
            (
                "MOCK_STDOUT",
                r#"{"type":"result","result":"ok","session_id":"sess-1"}"#,
            ),
            ("MOCK_ARGV_FILE", &argv_arg),
        ],
    );
    assert!(second.status.success(), "{second:?}");
    let v2 = json_stdout(&second);
    assert_eq!(v2["session"]["phase"], "continue");
    assert_eq!(v2["session"]["token"], "sess-1");
    // A continue reuses the harness's verified --resume mapping.
    let argv2 = std::fs::read_to_string(&argv_file).unwrap();
    assert!(
        argv2
            .lines()
            .collect::<Vec<_>>()
            .windows(2)
            .any(|w| w == ["--resume", "sess-1"]),
        "continue should resume the stored token: {argv2:?}"
    );

    let _ = std::fs::remove_dir_all(&store);
    let _ = std::fs::remove_dir_all(&cwd);
}

#[test]
fn session_on_an_unsupported_harness_is_a_usage_error() {
    // Goose exposes no session id headlessly, so a uniform handle cannot bind to
    // it — a loud usage error (exit 2), never a silent fresh start.
    let store = session_store_dir("unsupported");
    let output = run(
        &[
            "run",
            "--harness",
            "goose",
            "--session",
            "x",
            "--session-dir",
            &store.display().to_string(),
            "--prompt",
            "hi",
            "--print-command",
            "--compact",
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("does not support --session"),
        "stderr: {stderr}"
    );
    let _ = std::fs::remove_dir_all(&store);
}

#[test]
fn session_rejects_an_explicit_format_that_cannot_emit_an_id() {
    for id in ["codex", "qwen"] {
        let store = session_store_dir(&format!("bad-format-{id}"));
        let output = run(
            &[
                "run",
                "--harness",
                id,
                "--session",
                "chat",
                "--session-dir",
                &store.display().to_string(),
                "--output-format",
                "text",
                "--prompt",
                "hi",
                "--print-command",
                "--compact",
            ],
            &[],
        );
        assert_eq!(output.status.code(), Some(2), "{id}: {output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("--session")
                && stderr.contains("output format `text`")
                && stderr.contains("session id"),
            "{id}: stderr was {stderr}"
        );
        let _ = std::fs::remove_dir_all(&store);
    }
}

#[test]
fn session_needs_exactly_one_harness() {
    let store = session_store_dir("multi");
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code,codex",
            "--session",
            "x",
            "--session-dir",
            &store.display().to_string(),
            "--prompt",
            "hi",
            "--print-command",
            "--compact",
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--session needs exactly one harness"),
        "stderr: {stderr}"
    );
    let _ = std::fs::remove_dir_all(&store);
}

#[test]
fn session_bound_to_one_harness_rejects_a_different_one() {
    // A named session created on one harness cannot be continued on another.
    let store = session_store_dir("conflict");
    let store_arg = store.display().to_string();
    let cwd = session_store_dir("conflict-cwd");
    let cwd_arg = cwd.display().to_string();

    // Create `chat` on codex (its session handle is `thread_id`).
    let create = run(
        &[
            "run",
            "--harness",
            "codex",
            "--session",
            "chat",
            "--session-dir",
            &store_arg,
            "--cwd",
            &cwd_arg,
            "--prompt",
            "hi",
            "--bin",
            &bin_override("codex"),
            "--compact",
        ],
        &[("MOCK_STDOUT", r#"{"thread_id":"th-1","result":"hi"}"#)],
    );
    assert!(create.status.success(), "{create:?}");
    assert_eq!(json_stdout(&create)["session"]["token"], "th-1");

    // Reusing the name on claude-code is a loud conflict, not a silent migration.
    let conflict = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--session",
            "chat",
            "--session-dir",
            &store_arg,
            "--cwd",
            &cwd_arg,
            "--prompt",
            "hi",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[],
    );
    assert_eq!(conflict.status.code(), Some(2), "{conflict:?}");
    let stderr = String::from_utf8_lossy(&conflict.stderr);
    assert!(
        stderr.contains("bound to one harness") || stderr.contains("created on harness"),
        "stderr: {stderr}"
    );

    let _ = std::fs::remove_dir_all(&store);
    let _ = std::fs::remove_dir_all(&cwd);
}

#[test]
fn qwen_session_without_events_captures_and_resumes() {
    let store = session_store_dir("qwen-roundtrip");
    let store_arg = store.display().to_string();
    let cwd = session_store_dir("qwen-roundtrip-cwd");
    let cwd_arg = cwd.display().to_string();
    let argv_file = store.join("argv.txt");
    let argv_arg = argv_file.display().to_string();
    let stdout = concat!(
        "{\"type\":\"system\",\"session_id\":\"q-1\"}\n",
        "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"hi\"}]}}\n",
    );
    let bin = bin_override("qwen");
    let args = |prompt: &str| {
        vec![
            "run".to_string(),
            "--harness".to_string(),
            "qwen".to_string(),
            "--session".to_string(),
            "chat".to_string(),
            "--session-dir".to_string(),
            store_arg.clone(),
            "--cwd".to_string(),
            cwd_arg.clone(),
            "--prompt".to_string(),
            prompt.to_string(),
            "--bin".to_string(),
            bin.clone(),
            "--compact".to_string(),
        ]
    };

    let first_args = args("first");
    let first_refs = first_args.iter().map(String::as_str).collect::<Vec<_>>();
    let first = run(
        &first_refs,
        &[("MOCK_STDOUT", stdout), ("MOCK_ARGV_FILE", &argv_arg)],
    );
    assert!(first.status.success(), "{first:?}");
    let first_report = json_stdout(&first);
    assert_eq!(first_report["session"]["token"], "q-1");
    assert_eq!(first_report["results"][0]["output_format"], "stream-json");
    let first_argv = std::fs::read_to_string(&argv_file).unwrap();
    assert!(
        first_argv
            .lines()
            .collect::<Vec<_>>()
            .windows(2)
            .any(|pair| pair == ["--output-format", "stream-json"]),
        "qwen session create must select stream-json: {first_argv:?}"
    );

    let second_args = args("second");
    let second_refs = second_args.iter().map(String::as_str).collect::<Vec<_>>();
    let second = run(
        &second_refs,
        &[("MOCK_STDOUT", stdout), ("MOCK_ARGV_FILE", &argv_arg)],
    );
    assert!(second.status.success(), "{second:?}");
    assert_eq!(json_stdout(&second)["session"]["phase"], "continue");
    let second_argv = std::fs::read_to_string(&argv_file).unwrap();
    assert!(
        second_argv
            .lines()
            .collect::<Vec<_>>()
            .windows(2)
            .any(|pair| pair == ["--resume", "q-1"]),
        "qwen session continue must resume q-1: {second_argv:?}"
    );

    let _ = std::fs::remove_dir_all(&store);
    let _ = std::fs::remove_dir_all(&cwd);
}

#[test]
fn session_cannot_combine_with_a_batch() {
    // A named session is one continued conversation, not a fan-out over prompts.
    let store = session_store_dir("batch");
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--session",
            "x",
            "--session-dir",
            &store.display().to_string(),
            "--prompt",
            "one",
            "--prompt",
            "two",
            "--print-command",
            "--compact",
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--session cannot be combined with a batch"),
        "stderr: {stderr}"
    );
    let _ = std::fs::remove_dir_all(&store);
}

#[test]
fn session_in_fallback_mode_anchors_to_the_first_session_capable_harness() {
    // Unlike parallel (which is single-harness), `--session` is allowed on a
    // multi-harness fallback chain: it binds to the anchor — the first
    // session-capable harness in priority order. Here `goose` (not session-capable
    // and not installed) falls through, and `codex` (the anchor) runs; its native
    // id is captured. This also proves the token is read from the ANCHOR's result,
    // not `results.first()` (the fell-through `goose`, which exposes no id).
    let store = session_store_dir("fallback");
    let store_arg = store.display().to_string();
    let cwd = session_store_dir("fallback-cwd");
    let cwd_arg = cwd.display().to_string();

    // Run 1 — create. goose is not installed (falls through); codex runs.
    let first = run(
        &[
            "run",
            "--run-mode",
            "fallback",
            "--harness",
            "goose,codex",
            "--session",
            "triage",
            "--session-dir",
            &store_arg,
            "--cwd",
            &cwd_arg,
            "--prompt",
            "hi",
            "--bin",
            &missing_bin("goose"),
            "--bin",
            &bin_override("codex"),
            "--compact",
        ],
        &[("MOCK_STDOUT", r#"{"thread_id":"th-1","result":"hi"}"#)],
    );
    assert!(first.status.success(), "{first:?}");
    let v1 = json_stdout(&first);
    assert_eq!(v1["fallback"]["ran"], "codex");
    assert_eq!(v1["session"]["name"], "triage");
    assert_eq!(v1["session"]["phase"], "create");
    // The captured token is the anchor's (codex's) id, not the fell-through goose's.
    assert_eq!(v1["session"]["token"], "th-1");

    // Run 2 — continue. Same name resolves to codex's stored token (the anchor is
    // stable across runs given stable availability).
    let second = run(
        &[
            "run",
            "--run-mode",
            "fallback",
            "--harness",
            "goose,codex",
            "--session",
            "triage",
            "--session-dir",
            &store_arg,
            "--cwd",
            &cwd_arg,
            "--prompt",
            "again",
            "--bin",
            &missing_bin("goose"),
            "--bin",
            &bin_override("codex"),
            "--compact",
        ],
        &[("MOCK_STDOUT", r#"{"thread_id":"th-1","result":"ok"}"#)],
    );
    assert!(second.status.success(), "{second:?}");
    let v2 = json_stdout(&second);
    assert_eq!(v2["fallback"]["ran"], "codex");
    assert_eq!(v2["session"]["phase"], "continue");
    assert_eq!(v2["session"]["token"], "th-1");

    let _ = std::fs::remove_dir_all(&store);
    let _ = std::fs::remove_dir_all(&cwd);
}

/// The stored record for a named session, read back off the report's own
/// `session.store_file` handle (the path a consumer is told to look at).
///
/// Every record the CLI writes is asserted to carry the current store schema
/// version here rather than in each caller: the version is what tells a reader
/// whether `harness` is a variant-qualified id or a pre-`0.2` base name it must
/// decline to resume, so a record written without the bump would be read back as
/// legacy and silently start fresh forever.
fn stored_session(report: &Value) -> Value {
    let path = report["session"]["store_file"]
        .as_str()
        .expect("a session run reports its store file");
    let record: Value = serde_json::from_str(
        &std::fs::read_to_string(path).unwrap_or_else(|err| panic!("reading {path}: {err}")),
    )
    .expect("the session store holds one JSON record");
    assert_eq!(
        record["schema_version"],
        session::SCHEMA_VERSION,
        "a CLI-written record must carry the current store schema version"
    );
    record
}

/// Two identities of one harness (mocked): the first refuses, the second serves.
/// `first_env` / `second_env` are TOML env tables for the two variants.
fn variant_fallback_project(first_env: &str, second_env: &str) -> String {
    let mock = mock_bin().display().to_string();
    format!(
        r#"
        harnesses = ["claude-code:primary", "claude-code:alternate"]
        run_mode = "fallback"

        [harness.claude-code.variant.primary]
        bin = '{mock}'
        env = {{ {first_env} }}

        [harness.claude-code.variant.alternate]
        bin = '{mock}'
        env = {{ {second_env} }}
        "#
    )
}

#[test]
fn session_binds_to_the_variant_that_ran_not_the_one_that_fell_through() {
    // A fallback chain of two identities of the SAME harness. The anchor (the
    // first) is out of quota and falls through; the second does the turn and
    // exposes its own native id. The handle must persist *that* token, bound to
    // the variant-qualified id that minted it — a record keyed on the base
    // `claude-code` cannot say which identity's namespace the token lives in, and
    // matching results on the base id picks the fell-through candidate, which
    // exposed no id at all.
    let store = session_store_dir("variant-anchor");
    let project = variant_fallback_project(
        r#"MOCK_EXIT = "1", MOCK_STDERR = "Error: insufficient_quota""#,
        r#"MOCK_EXIT = "0", MOCK_STDOUT = '{"session_id":"sess-alt","result":"served-by-alternate"}'"#,
    );
    let fx = ConfigFixture::new("session-variant-anchor", &project, "");
    let output = run_with_config(
        &[
            "run",
            "--prompt",
            "hi",
            "--cwd",
            &fx.cwd(),
            "--session",
            "triage",
            "--session-dir",
            &store.display().to_string(),
            "--compact",
        ],
        &[],
        &fx.user_config(),
    );
    assert!(
        output.status.success(),
        "exit {:?}, stderr {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let value = json_stdout(&output);
    assert_eq!(value["fallback"]["fell_through"][0]["reason"], "quota");
    assert_eq!(value["fallback"]["ran"], "claude-code:alternate");
    assert_eq!(value["results"][1]["text"], "served-by-alternate");
    // The token the winner minted, not the anchor's (absent) one.
    assert_eq!(value["session"]["token"], "sess-alt");
    let record = stored_session(&value);
    assert_eq!(record["token"], "sess-alt");
    assert_eq!(
        record["harness"], "claude-code:alternate",
        "the record binds to the identity whose namespace holds the token"
    );

    let _ = std::fs::remove_dir_all(&store);
}

#[test]
fn a_resume_the_identity_cannot_resolve_falls_through_to_the_next_candidate() {
    // The condition that collapsed a five-identity chain, driven as the two turns
    // an operator actually runs. Turn one binds `triage` to the first identity.
    // Turn two hands that identity its own token back, and it refuses — the shape
    // a real `claude` prints when the conversation is not in its config dir: exit
    // 1, empty stdout, no usage, one line on stderr. Read as a plain task failure
    // that stops the chain dead; classified as `session_not_found` it falls
    // through, and the second authenticated identity does the work.
    let store = session_store_dir("stale-resume");
    let store_arg = store.display().to_string();
    let argv_file = std::env::temp_dir().join(format!("oh-stale-argv-{}", std::process::id()));
    let _ = std::fs::remove_file(&argv_file);
    let alternate_serves = r#"MOCK_EXIT = "0", MOCK_STDOUT = '{"session_id":"sess-alt","result":"served-by-alternate"}'"#;
    // One project directory across both turns (it is the key the session store
    // partitions on); the mocks are scripted apart by rewriting its config.
    let fx = ConfigFixture::new(
        "session-stale-resume",
        &variant_fallback_project(
            r#"MOCK_EXIT = "0", MOCK_STDOUT = '{"session_id":"sess-primary","result":"served-by-primary"}'"#,
            alternate_serves,
        ),
        "",
    );
    // History is on so the refusal's own record can be read back: a
    // `session_not_found` record is legible only to a v1.5 reader, and it must
    // say so.
    let history = hist_dir("stale-resume");
    let args = [
        "run",
        "--prompt",
        "hi",
        "--cwd",
        &fx.cwd(),
        "--session",
        "triage",
        "--session-dir",
        &store_arg,
        "--history",
        "--history-dir",
        &history.display().to_string(),
        "--compact",
    ]
    .map(str::to_string);
    let turn = || {
        run_with_config(
            &args.iter().map(String::as_str).collect::<Vec<_>>(),
            &[],
            &fx.user_config(),
        )
    };

    let first = turn();
    assert!(first.status.success(), "{first:?}");
    let created = json_stdout(&first);
    assert_eq!(created["fallback"]["ran"], "claude-code:primary");
    assert_eq!(created["session"]["token"], "sess-primary");

    std::fs::write(
        PathBuf::from(fx.cwd()).join("oneharness.toml"),
        variant_fallback_project(
            &format!(
                r#"MOCK_EXIT = "1", MOCK_ARGV_FILE = '{}', MOCK_STDERR = "No conversation found with session ID: sess-primary""#,
                argv_file.display()
            ),
            alternate_serves,
        ),
    )
    .unwrap();
    let second = turn();
    assert!(
        second.status.success(),
        "a chain with a healthy identity must still succeed: exit {:?}, stderr {}",
        second.status.code(),
        String::from_utf8_lossy(&second.stderr)
    );
    let value = json_stdout(&second);
    // The anchor was handed its own token back and could not resolve it...
    let resumed = std::fs::read_to_string(&argv_file).unwrap();
    assert!(
        resumed
            .lines()
            .collect::<Vec<_>>()
            .windows(2)
            .any(|pair| pair == ["--resume", "sess-primary"]),
        "the anchor must be the candidate that resumes: {resumed}"
    );
    assert_eq!(value["results"][0]["status"], "nonzero");
    assert_eq!(value["results"][0]["failure_kind"], "session_not_found");
    assert_eq!(value["results"][0]["failure_kind_source"], "stderr");
    assert_eq!(
        value["fallback"]["fell_through"][0]["reason"],
        "session-not-found"
    );
    // ...so the next identity ran the turn, and the handle follows it.
    assert_eq!(value["fallback"]["ran"], "claude-code:alternate");
    assert_eq!(value["results"][1]["text"], "served-by-alternate");
    assert_eq!(value["session"]["token"], "sess-alt");
    assert_eq!(
        stored_session(&value)["harness"],
        "claude-code:alternate",
        "a session that moved identities says so"
    );
    // A move is not silent: the handle now continues somewhere else, and an
    // operator reading only the report would not know the thread was dropped.
    let moved = String::from_utf8_lossy(&second.stderr);
    assert!(
        moved.contains("session `triage` was bound to `claude-code:primary`")
            && moved.contains("`claude-code:alternate` ran this turn"),
        "the rebind must be announced: {moved}"
    );
    // The refusal's history record declares the version that first understood it.
    let refused = first_history_run(Path::new(value["history_file"].as_str().unwrap()));
    assert_eq!(refused["harness_id"], "claude-code:primary");
    assert_eq!(refused["failure_kind"], "session_not_found");
    assert_eq!(refused["schema_version"], "1.5");

    // Turn three: the handle lives on `alternate` now, so *it* is the anchor and
    // resumes `sess-alt` — even though `primary` still leads the priority chain.
    // Binding to the head of the chain instead would hand the token back to the
    // identity that cannot resolve it, every turn from here on.
    let alternate_argv = std::env::temp_dir().join(format!("oh-stale-alt-{}", std::process::id()));
    let _ = std::fs::remove_file(&alternate_argv);
    std::fs::write(
        PathBuf::from(fx.cwd()).join("oneharness.toml"),
        variant_fallback_project(
            r#"MOCK_EXIT = "1", MOCK_STDERR = "Error: insufficient_quota""#,
            &format!(
                r#"MOCK_EXIT = "0", MOCK_ARGV_FILE = '{}', MOCK_STDOUT = '{{"session_id":"sess-alt","result":"served-by-alternate"}}'"#,
                alternate_argv.display()
            ),
        ),
    )
    .unwrap();
    let third = turn();
    assert!(third.status.success(), "{third:?}");
    let continued = json_stdout(&third);
    assert_eq!(continued["session"]["phase"], "continue");
    assert_eq!(continued["fallback"]["ran"], "claude-code:alternate");
    let alternate_resumed = std::fs::read_to_string(&alternate_argv).unwrap();
    assert!(
        alternate_resumed
            .lines()
            .collect::<Vec<_>>()
            .windows(2)
            .any(|pair| pair == ["--resume", "sess-alt"]),
        "the identity holding the session must be the one that resumes it: {alternate_resumed}"
    );

    let _ = std::fs::remove_file(&alternate_argv);
    let _ = std::fs::remove_file(&argv_file);
    let _ = std::fs::remove_dir_all(&history);
    let _ = std::fs::remove_dir_all(&store);
}

#[test]
fn every_sourced_unknown_session_refusal_falls_through_its_own_chain() {
    // One case per CLI whose refusal was captured from a real invocation (a bogus
    // session id resumed against the installed binary). Each drives its own
    // harness's dialect through a real fallback chain, so a phrase that stops
    // matching stops the chain here rather than in a dispatch: exit 1, empty
    // stdout, the message on stderr — the exact shape each CLI produces.
    let mock = mock_bin().display().to_string();
    let served =
        r#"{"type":"item.completed","item":{"type":"agent_message","text":"served-by-codex"}}"#;
    let cases = [
        (
            "claude-code",
            "No conversation found with session ID: 019f-0000",
        ),
        (
            "codex",
            "Error: thread/resume: thread/resume failed: no rollout found for thread id 019f-0000 \
             (code -32600)",
        ),
        ("opencode", "Error: Session not found"),
        ("qwen", "No saved session found with title \"019f-0000\"."),
    ];

    for (first, refusal) in cases {
        // codex is the healthy tail for every chain, so the refusing harness is
        // always a different one.
        let tail = if first == "codex" {
            "opencode"
        } else {
            "codex"
        };
        let tail_stdout = if tail == "codex" {
            served.to_string()
        } else {
            r#"{"type":"text","part":{"type":"text","text":"served-by-codex"}}"#.to_string()
        };
        let project = format!(
            r#"
            harnesses = ["{first}", "{tail}"]
            run_mode = "fallback"

            [harness.{first}]
            bin = '{mock}'
            env = {{ MOCK_EXIT = "1", MOCK_STDERR = '{refusal}' }}

            [harness.{tail}]
            bin = '{mock}'
            env = {{ MOCK_EXIT = "0", MOCK_STDOUT = '{tail_stdout}' }}
            "#
        );
        let fx = ConfigFixture::new(&format!("unknown-session-{first}"), &project, "");
        let output = run_with_config(
            &["run", "--prompt", "hi", "--cwd", &fx.cwd(), "--compact"],
            &[],
            &fx.user_config(),
        );
        assert!(
            output.status.success(),
            "{first}: exit {:?}, stderr {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
        let value = json_stdout(&output);
        assert_eq!(
            value["results"][0]["failure_kind"], "session_not_found",
            "{first} must classify its own refusal"
        );
        assert_eq!(
            value["fallback"]["fell_through"][0]["reason"], "session-not-found",
            "{first}"
        );
        assert_eq!(value["fallback"]["ran"], tail, "{first}");
        assert_eq!(value["results"][1]["text"], "served-by-codex", "{first}");
    }
}

#[test]
fn a_named_session_refuses_to_migrate_between_identities_of_one_harness() {
    // The record binds to the whole identity, so continuing it on a sibling
    // variant is the same loud usage error as continuing it on another harness —
    // never a silent resume with a token that identity's store has never seen.
    let mock = mock_bin().display().to_string();
    let store = session_store_dir("variant-conflict");
    let store_arg = store.display().to_string();
    let project = format!(
        r#"
        [harness.claude-code.variant.primary]
        bin = '{mock}'

        [harness.claude-code.variant.alternate]
        bin = '{mock}'
        "#
    );
    let fx = ConfigFixture::new("session-variant-conflict", &project, "");
    let session_args = |variant: &str| {
        [
            "run".to_string(),
            "--prompt".to_string(),
            "hi".to_string(),
            "--cwd".to_string(),
            fx.cwd(),
            "--harness".to_string(),
            format!("claude-code:{variant}"),
            "--session".to_string(),
            "triage".to_string(),
            "--session-dir".to_string(),
            store_arg.clone(),
            "--compact".to_string(),
        ]
    };
    let first_args = session_args("primary");
    let first = run_with_config(
        &first_args.iter().map(String::as_str).collect::<Vec<_>>(),
        &[(
            "MOCK_STDOUT",
            r#"{"session_id":"sess-primary","result":"ok"}"#,
        )],
        &fx.user_config(),
    );
    assert!(first.status.success(), "{first:?}");
    let created = json_stdout(&first);
    assert_eq!(created["session"]["token"], "sess-primary");
    assert_eq!(stored_session(&created)["harness"], "claude-code:primary");

    let second_args = session_args("alternate");
    let second = run_with_config(
        &second_args.iter().map(String::as_str).collect::<Vec<_>>(),
        &[("MOCK_STDOUT", r#"{"session_id":"sess-alt","result":"ok"}"#)],
        &fx.user_config(),
    );
    assert_eq!(second.status.code(), Some(2), "{second:?}");
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(
        stderr.contains("claude-code:primary") && stderr.contains("claude-code:alternate"),
        "the error must name both identities: {stderr}"
    );

    let _ = std::fs::remove_dir_all(&store);
}

#[test]
fn a_sibling_variant_is_never_handed_the_anchors_resume_token() {
    // The other half of the fall-through: the candidate that picks up the turn
    // must run FRESH. Its config directory has never seen the anchor's token, so
    // passing it along would only reproduce the refusal one identity down the
    // chain — the token is scoped to the namespace that minted it, and the
    // report's own `phase` must not claim a continuation nobody performed.
    let store = session_store_dir("sibling-no-resume");
    let store_arg = store.display().to_string();
    let alternate_argv =
        std::env::temp_dir().join(format!("oh-sibling-argv-{}", std::process::id()));
    let _ = std::fs::remove_file(&alternate_argv);
    let alternate_serves = format!(
        r#"MOCK_EXIT = "0", MOCK_ARGV_FILE = '{}', MOCK_STDOUT = '{{"session_id":"sess-alt","result":"served-by-alternate"}}'"#,
        alternate_argv.display()
    );
    let fx = ConfigFixture::new(
        "session-sibling-no-resume",
        &variant_fallback_project(
            r#"MOCK_EXIT = "0", MOCK_STDOUT = '{"session_id":"sess-primary","result":"served-by-primary"}'"#,
            &alternate_serves,
        ),
        "",
    );
    let args = [
        "run",
        "--prompt",
        "hi",
        "--cwd",
        &fx.cwd(),
        "--session",
        "triage",
        "--session-dir",
        &store_arg,
        "--compact",
    ]
    .map(str::to_string);
    let turn = || {
        run_with_config(
            &args.iter().map(String::as_str).collect::<Vec<_>>(),
            &[],
            &fx.user_config(),
        )
    };

    // Turn one binds the handle to `primary`.
    let first = turn();
    assert!(first.status.success(), "{first:?}");
    assert_eq!(json_stdout(&first)["session"]["token"], "sess-primary");

    // Turn two: the anchor is out of quota, so `alternate` takes the turn.
    std::fs::write(
        PathBuf::from(fx.cwd()).join("oneharness.toml"),
        variant_fallback_project(
            r#"MOCK_EXIT = "1", MOCK_STDERR = "Error: insufficient_quota""#,
            &alternate_serves,
        ),
    )
    .unwrap();
    let second = turn();
    assert!(second.status.success(), "{second:?}");
    let value = json_stdout(&second);
    assert_eq!(value["fallback"]["ran"], "claude-code:alternate");

    let argv = std::fs::read_to_string(&alternate_argv).unwrap();
    assert!(
        !argv.lines().any(|arg| arg == "--resume"),
        "the sibling must run fresh, never carrying the anchor's token: {argv}"
    );
    assert!(
        !argv.contains("sess-primary"),
        "the anchor's token must not reach a sibling's argv at all: {argv}"
    );

    let _ = std::fs::remove_file(&alternate_argv);
    let _ = std::fs::remove_dir_all(&store);
}

#[test]
fn a_lossy_output_format_is_refused_by_the_variant_qualified_identity() {
    // `--session` needs a format that actually carries the native id. An explicit
    // pin that drops it is refused BEFORE spawning (accepting it would leave the
    // handle silently unstorable), and the error names the whole identity — a
    // bare `claude-code` would not tell an operator running several which of
    // their configured variants to re-run.
    let mock = mock_bin().display().to_string();
    let store = session_store_dir("variant-format");
    let fx = ConfigFixture::new(
        "session-variant-format",
        &format!(
            r#"
        [harness.claude-code.variant.alternate]
        bin = '{mock}'
        "#
        ),
        "",
    );
    let output = run_with_config(
        &[
            "run",
            "--prompt",
            "hi",
            "--cwd",
            &fx.cwd(),
            "--harness",
            "claude-code:alternate",
            "--session",
            "triage",
            "--session-dir",
            &store.display().to_string(),
            "--output-format",
            "text",
            "--compact",
        ],
        &[],
        &fx.user_config(),
    );
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("claude-code:alternate"),
        "the error must name the variant-qualified identity: {stderr}"
    );
    assert!(
        stderr.contains("text") && stderr.contains("json"),
        "the error must name the refused format and the ones that work: {stderr}"
    );
    // Nothing spawned, so nothing was stored.
    assert_eq!(
        std::fs::read_dir(&store).unwrap().count(),
        0,
        "a refused run must leave the store untouched"
    );

    let _ = std::fs::remove_dir_all(&store);
}

#[test]
fn a_legacy_session_record_starts_fresh_instead_of_resuming_a_guessed_identity() {
    // A record written before the store bound sessions to a variant-qualified id
    // says only `claude-code` — which identity minted its token is unrecoverable.
    // Guessing is what strands a chain, so the run creates a new session instead.
    let store = session_store_dir("legacy-record");
    let store_arg = store.display().to_string();
    let cwd = session_store_dir("legacy-record-cwd");
    let argv_file = std::env::temp_dir().join(format!("oh-legacy-argv-{}", std::process::id()));
    let _ = std::fs::remove_file(&argv_file);
    let args = [
        "run",
        "--harness",
        "claude-code",
        "--session",
        "triage",
        "--session-dir",
        &store_arg,
        "--cwd",
        &cwd.display().to_string(),
        "--prompt",
        "hi",
        "--bin",
        &bin_override("claude-code"),
        "--compact",
    ];

    // Learn the store path the way a consumer does, then plant a v0.1 record.
    let dry = run(
        &[args.as_slice(), &["--print-command"]].concat(),
        &[("MOCK_STDOUT", r#"{"session_id":"sess-new","result":"ok"}"#)],
    );
    assert!(dry.status.success(), "{dry:?}");
    let path = PathBuf::from(
        json_stdout(&dry)["session"]["store_file"]
            .as_str()
            .expect("the handle reports its store file"),
    );
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        r#"{"schema_version":"0.1","name":"triage","project":"/p","harness":"claude-code",
            "token":"sess-legacy","created":"2026-07-10T00:00:00Z","updated":"2026-07-10T00:00:00Z"}"#,
    )
    .unwrap();

    let output = run(
        &args,
        &[
            ("MOCK_STDOUT", r#"{"session_id":"sess-new","result":"ok"}"#),
            ("MOCK_ARGV_FILE", &argv_file.display().to_string()),
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let value = json_stdout(&output);
    assert_eq!(value["session"]["phase"], "create");
    assert_eq!(value["session"]["token"], "sess-new");
    let argv = std::fs::read_to_string(&argv_file).unwrap();
    assert!(
        !argv.contains("sess-legacy"),
        "the legacy token must never be resumed: {argv}"
    );
    // The replacement record is written at the current shape.
    let record = stored_session(&value);
    assert_eq!(record["token"], "sess-new");
    assert_eq!(record["harness"], "claude-code");

    let _ = std::fs::remove_file(&argv_file);
    let _ = std::fs::remove_dir_all(&store);
    let _ = std::fs::remove_dir_all(&cwd);
}

#[test]
fn session_print_command_reports_the_handle_without_writing_the_store() {
    // `--print-command` builds argv but runs nothing, so a create must not write
    // a store record, yet still report the (as-yet tokenless) handle.
    let store = session_store_dir("dry");
    let store_arg = store.display().to_string();
    let cwd = session_store_dir("dry-cwd");
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--session",
            "greet",
            "--session-dir",
            &store_arg,
            "--cwd",
            &cwd.display().to_string(),
            "--prompt",
            "hi",
            "--print-command",
            "--compact",
        ],
        &[],
    );
    assert!(output.status.success(), "{output:?}");
    let v = json_stdout(&output);
    assert_eq!(v["session"]["phase"], "create");
    assert!(v["session"]["token"].is_null());
    // Nothing ran, so no record (nor its project subdir) was persisted.
    assert_eq!(
        std::fs::read_dir(&store).unwrap().count(),
        0,
        "print-command must not write the store"
    );
    let _ = std::fs::remove_dir_all(&store);
    let _ = std::fs::remove_dir_all(&cwd);
}

#[test]
fn session_create_without_a_session_id_warns_and_stores_nothing() {
    // If a session-capable harness happens to expose no id on a given run, the
    // handle cannot be continued: oneharness warns and stores nothing rather than
    // persisting an unusable record.
    let store = session_store_dir("noid");
    let store_arg = store.display().to_string();
    let cwd = session_store_dir("noid-cwd");
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--session",
            "greet",
            "--session-dir",
            &store_arg,
            "--cwd",
            &cwd.display().to_string(),
            "--prompt",
            "hi",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        // A result document with no session id.
        &[("MOCK_STDOUT", r#"{"type":"result","result":"hi"}"#)],
    );
    assert!(output.status.success(), "{output:?}");
    let v = json_stdout(&output);
    assert_eq!(v["session"]["phase"], "create");
    assert!(v["session"]["token"].is_null());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("exposed no session id"), "stderr: {stderr}");
    // Best-effort: nothing written when there is no token to persist.
    assert_eq!(std::fs::read_dir(&store).unwrap().count(), 0);
    let _ = std::fs::remove_dir_all(&store);
    let _ = std::fs::remove_dir_all(&cwd);
}

#[test]
fn session_continue_persists_a_rotated_token() {
    // A harness may return a fresh session id on a continue; the store must be
    // updated to the new token (so the next turn resumes it) while keeping the
    // original `created`.
    let store = session_store_dir("rotate");
    let store_arg = store.display().to_string();
    let cwd = session_store_dir("rotate-cwd");
    let cwd_arg = cwd.display().to_string();
    let bin = bin_override("claude-code");
    let args = |prompt: &'static str| {
        [
            "run",
            "--harness",
            "claude-code",
            "--session",
            "chat",
            "--session-dir",
            store_arg.as_str(),
            "--cwd",
            cwd_arg.as_str(),
            "--prompt",
            prompt,
            "--bin",
            bin.as_str(),
            "--compact",
        ]
    };

    let first = run(
        &args("first"),
        &[(
            "MOCK_STDOUT",
            r#"{"type":"result","result":"a","session_id":"sess-1"}"#,
        )],
    );
    assert!(first.status.success(), "{first:?}");
    let v1 = json_stdout(&first);
    assert_eq!(v1["session"]["token"], "sess-1");
    let created: String = serde_json::from_str::<Value>(
        &std::fs::read_to_string(v1["session"]["store_file"].as_str().unwrap()).unwrap(),
    )
    .unwrap()["created"]
        .as_str()
        .unwrap()
        .to_string();

    let second = run(
        &args("second"),
        &[(
            "MOCK_STDOUT",
            r#"{"type":"result","result":"b","session_id":"sess-2"}"#,
        )],
    );
    assert!(second.status.success(), "{second:?}");
    let v2 = json_stdout(&second);
    assert_eq!(v2["session"]["phase"], "continue");
    assert_eq!(v2["session"]["token"], "sess-2");
    // The stored record now holds the rotated token.
    let record: Value = serde_json::from_str(
        &std::fs::read_to_string(v2["session"]["store_file"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(record["token"], "sess-2");
    // The original creation time is preserved across the rotation.
    assert_eq!(record["created"], created);

    let _ = std::fs::remove_dir_all(&store);
    let _ = std::fs::remove_dir_all(&cwd);
}

#[test]
fn session_survives_an_unwritable_store() {
    // The store is best-effort: if the store path can't be written (here the
    // --session-dir is a regular file, so its project subdir can't be created),
    // the run still succeeds and warns rather than aborting.
    let file = std::env::temp_dir().join(format!("oh-session-file-{}", std::process::id()));
    std::fs::write(&file, b"not a dir").unwrap();
    let cwd = session_store_dir("unwritable-cwd");
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--session",
            "greet",
            "--session-dir",
            &file.display().to_string(),
            "--cwd",
            &cwd.display().to_string(),
            "--prompt",
            "hi",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[(
            "MOCK_STDOUT",
            r#"{"type":"result","result":"hi","session_id":"sess-1"}"#,
        )],
    );
    assert!(output.status.success(), "best-effort store: {output:?}");
    // The token is still captured and reported, even though it couldn't persist.
    assert_eq!(json_stdout(&output)["session"]["token"], "sess-1");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("could not write session store"),
        "stderr: {stderr}"
    );
    let _ = std::fs::remove_file(&file);
    let _ = std::fs::remove_dir_all(&cwd);
}

#[test]
fn session_without_a_resolvable_store_is_a_usage_error() {
    let mock_profile = mock_profile_redirect();
    // With no --session-dir and no platform state dir (HOME / XDG / LOCALAPPDATA
    // all unset), the store can't be located — a loud usage error up front.
    let output = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        .env_remove("HOME")
        .env_remove("XDG_STATE_HOME")
        .env_remove("LOCALAPPDATA")
        .env_remove("USERPROFILE")
        .args([
            "run",
            "--harness",
            "claude-code",
            "--session",
            "x",
            "--prompt",
            "hi",
            "--print-command",
            "--compact",
            "--env",
            mock_profile.as_str(),
        ])
        .output()
        .expect("failed to run oneharness");
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("session store directory"),
        "stderr: {stderr}"
    );
}

#[test]
fn session_composes_with_structured_output() {
    // `--schema` runs a validate/retry loop; `--session` must still capture the
    // token from the (final) result and report a valid structured value.
    let store = session_store_dir("schema");
    let cwd = session_store_dir("schema-cwd");
    let schema = temp_file("session-schema", PERSON_SCHEMA);
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--session",
            "s",
            "--session-dir",
            &store.display().to_string(),
            "--cwd",
            &cwd.display().to_string(),
            "--prompt",
            "describe ada",
            "--schema",
            &schema,
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[(
            "MOCK_STDOUT",
            r#"{"type":"result","result":"Here is Ada.","structured_output":{"name":"Ada","age":36},"session_id":"sess-1"}"#,
        )],
    );
    assert!(output.status.success(), "{output:?}");
    let v = json_stdout(&output);
    assert_eq!(v["results"][0]["schema_valid"], true);
    assert_eq!(v["results"][0]["structured"]["name"], "Ada");
    assert_eq!(v["session"]["phase"], "create");
    assert_eq!(v["session"]["token"], "sess-1");
    let _ = std::fs::remove_dir_all(&store);
    let _ = std::fs::remove_dir_all(&cwd);
}

#[test]
fn session_composes_with_history() {
    // `--session` and `--history` write independent stores; both must be honored
    // in one run.
    let store = session_store_dir("hist-sess");
    let hist = session_store_dir("hist-dir");
    let cwd = session_store_dir("hist-cwd");
    let output = run(
        &[
            "run",
            "--harness",
            "codex",
            "--session",
            "s",
            "--session-dir",
            &store.display().to_string(),
            "--history",
            "--history-dir",
            &hist.display().to_string(),
            "--cwd",
            &cwd.display().to_string(),
            "--prompt",
            "hi",
            "--bin",
            &bin_override("codex"),
            "--compact",
        ],
        &[(
            "MOCK_STDOUT",
            "{\"type\":\"thread.started\",\"thread_id\":\"sess-1\"}\n{\"type\":\"turn.started\"}\n{\"type\":\"item.completed\",\"item\":{\"id\":\"m1\",\"type\":\"agent_message\",\"text\":\"hi\"}}\n{\"type\":\"turn.completed\"}\n",
        )],
    );
    assert!(output.status.success(), "{output:?}");
    let v = json_stdout(&output);
    assert_eq!(v["session"]["token"], "sess-1");
    assert!(!v["history_file"].is_null(), "history should be recorded");
    // Both stores landed on disk.
    assert!(std::fs::read_dir(&store).unwrap().count() > 0);
    assert!(std::fs::read_dir(&hist).unwrap().count() > 0);
    let _ = std::fs::remove_dir_all(&store);
    let _ = std::fs::remove_dir_all(&hist);
    let _ = std::fs::remove_dir_all(&cwd);
}

#[test]
fn session_composes_with_mock_rules() {
    // `--session` and the ephemeral `--mock-rules` delivery must coexist: the
    // session token is captured and the mock ruleset is echoed on the report.
    let dir = session_store_dir("mock-sess");
    let store = session_store_dir("mock-store");
    let rules = dir.join("rules.json");
    std::fs::write(
        &rules,
        r#"{"rules":[{"match":{"tool":"Bash","event_contains":"MARK"},"action":{"deny":{"message":"m"}}}]}"#,
    )
    .unwrap();
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--session",
            "s",
            "--session-dir",
            &store.display().to_string(),
            "--cwd",
            dir.to_str().unwrap(),
            "--mock-rules",
            rules.to_str().unwrap(),
            "--prompt",
            "hi",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[(
            "MOCK_STDOUT",
            r#"{"type":"result","result":"hi","session_id":"sess-1"}"#,
        )],
    );
    assert!(output.status.success(), "{output:?}");
    let v = json_stdout(&output);
    assert_eq!(v["session"]["token"], "sess-1");
    assert!(!v["mock_rules"].is_null(), "mock rules should be echoed");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&store);
}

#[test]
fn session_works_with_streaming() {
    // The streaming path has its own finalize step: `--session --stream` must
    // still capture the token and persist the store, and surface the session
    // block on the terminal report line.
    let store = session_store_dir("stream");
    let store_arg = store.display().to_string();
    let cwd = session_store_dir("stream-cwd");
    let stdout = concat!(
        "{\"type\":\"step_start\",\"sessionID\":\"ses_stream\",\"part\":{}}\n",
        "{\"type\":\"tool_use\",\"part\":{\"type\":\"tool\",\"tool\":\"bash\",\"state\":{\"status\":\"completed\",\"input\":{\"command\":\"echo hi\"},\"output\":\"hi\"}}}\n",
        "{\"type\":\"step_finish\",\"sessionID\":\"ses_stream\",\"part\":{}}\n",
    );
    let output = run(
        &[
            "run",
            "--harness",
            "opencode",
            "--session",
            "live",
            "--session-dir",
            &store_arg,
            "--cwd",
            &cwd.display().to_string(),
            "--prompt",
            "hi",
            "--bin",
            &bin_override("opencode"),
            "--stream",
        ],
        &[("MOCK_STDOUT", stdout)],
    );
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8_lossy(&output.stdout);
    let terminal: Value = text
        .lines()
        .rfind(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .unwrap();
    assert_eq!(terminal["type"], "result");
    let session = &terminal["report"]["session"];
    assert_eq!(session["name"], "live");
    assert_eq!(session["phase"], "create");
    assert_eq!(session["token"], "ses_stream");
    // The store was persisted through the streaming path, so a later run resumes.
    let record: Value = serde_json::from_str(
        &std::fs::read_to_string(session["store_file"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(record["token"], "ses_stream");
    assert_eq!(record["harness"], "opencode");

    let _ = std::fs::remove_dir_all(&store);
    let _ = std::fs::remove_dir_all(&cwd);
}

#[test]
fn session_works_with_streaming_in_fallback_mode() {
    // The two single-outcome handles compose now that a fallback chain can
    // stream: the named session still binds to the anchor (codex, since goose is
    // not session-capable *and* not installed) and its token is captured from the
    // streaming candidate that ran.
    let store = session_store_dir("stream-fallback-session");
    let cwd = session_store_dir("stream-fallback-session-cwd");
    let output = run(
        &[
            "run",
            "--run-mode",
            "fallback",
            "--harness",
            "goose,codex",
            "--session",
            "triage",
            "--session-dir",
            &store.display().to_string(),
            "--cwd",
            &cwd.display().to_string(),
            "--prompt",
            "hi",
            "--bin",
            &missing_bin("goose"),
            "--bin",
            &bin_override("codex"),
            "--stream",
        ],
        &[(
            "MOCK_STDOUT",
            concat!(
                r#"{"type":"thread.started","thread_id":"th-stream"}"#,
                "\n",
                r#"{"type":"item.completed","item":{"type":"command_execution","command":"echo hi","aggregated_output":"hi"}}"#,
                "\n",
            ),
        )],
    );
    assert!(
        output.status.success(),
        "exit {:?}: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let envelopes = stream_envelopes(&output);
    assert_eq!(envelopes[0]["type"], "event", "{envelopes:#?}");
    let report = &envelopes.last().unwrap()["report"];
    assert_eq!(report["fallback"]["ran"], "codex");
    assert_eq!(report["session"]["phase"], "create");
    assert_eq!(report["session"]["token"], "th-stream");
    let record: Value = serde_json::from_str(
        &std::fs::read_to_string(report["session"]["store_file"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(record["harness"], "codex");

    let _ = std::fs::remove_dir_all(&store);
    let _ = std::fs::remove_dir_all(&cwd);
}

/// Build a `--print-command` argv for one harness with the given extra args, and
/// return its `results[0].command` as a Vec<String>.
fn print_command_for(extra: &[&str]) -> Vec<String> {
    let mut argv = vec!["run", "--prompt", "go", "--print-command", "--compact"];
    argv.extend_from_slice(extra);
    let output = run(&argv, &[]);
    assert!(output.status.success(), "{:?}", output);
    let value = json_stdout(&output);
    value["results"][0]["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn resume_maps_for_the_text_output_harnesses() {
    // codex: `exec resume <id> <prompt>` (subcommand, id before prompt).
    let codex = print_command_for(&["--harness", "codex", "--resume", "th-1"]);
    assert!(
        codex.windows(2).any(|w| w == ["exec", "resume"]),
        "{codex:?}"
    );
    assert!(codex.iter().any(|t| t == "th-1"), "{codex:?}");
    // goose: caller-named session via `--resume --name <name>`.
    let goose = print_command_for(&["--harness", "goose", "--resume", "my-name"]);
    assert!(goose.iter().any(|t| t == "--resume"), "{goose:?}");
    assert!(
        goose.windows(2).any(|w| w == ["--name", "my-name"]),
        "{goose:?}"
    );
    // qwen / copilot: `--resume <id>`.
    for id in ["qwen", "copilot"] {
        let argv = print_command_for(&["--harness", id, "--resume", "uuid-x"]);
        assert!(
            argv.windows(2).any(|w| w == ["--resume", "uuid-x"]),
            "{id}: {argv:?}"
        );
    }
    // crush: `--session <id>`.
    let crush = print_command_for(&["--harness", "crush", "--resume", "s-9"]);
    assert!(
        crush.windows(2).any(|w| w == ["--session", "s-9"]),
        "{crush:?}"
    );
}

#[test]
fn reasoning_maps_to_each_capable_harness_native_flag() {
    // claude-code: `--effort <value>`; value forwarded verbatim.
    let claude = print_command_for(&["--harness", "claude-code", "--reasoning", "high"]);
    assert!(
        claude.windows(2).any(|w| w == ["--effort", "high"]),
        "{claude:?}"
    );
    // codex: `-c model_reasoning_effort=<value>` (bare value, Codex's `-c`
    // override — the same mechanism the codex mock phase exercises live).
    let codex = print_command_for(&["--harness", "codex", "--reasoning", "xhigh"]);
    assert!(
        codex
            .windows(2)
            .any(|w| w == ["-c", "model_reasoning_effort=xhigh"]),
        "{codex:?}"
    );
    // copilot: `--reasoning-effort <value>` (its documented headless flag).
    let copilot = print_command_for(&["--harness", "copilot", "--reasoning", "medium"]);
    assert!(
        copilot
            .windows(2)
            .any(|w| w == ["--reasoning-effort", "medium"]),
        "{copilot:?}"
    );
    // cursor: effort is a `-<tier>` suffix on the model id — `claude-opus-4-8`
    // + `high` → `--model claude-opus-4-8-high` (cursor-agent rejects a bracketed
    // `model[effort=…]`; the tier is baked into the id — verified live).
    let cursor = print_command_for(&[
        "--harness",
        "cursor",
        "--model",
        "claude-opus-4-8",
        "--reasoning",
        "high",
    ]);
    assert!(
        cursor
            .windows(2)
            .any(|w| w == ["--model", "claude-opus-4-8-high"]),
        "{cursor:?}"
    );
}

#[test]
fn cursor_reasoning_without_a_model_is_a_usage_error() {
    // Cursor's effort rides the model id, so `--reasoning` with no model has
    // nothing to attach to — a loud usage error, not a bare `[effort=…]`.
    let output = run(
        &[
            "run",
            "--harness",
            "cursor",
            "--prompt",
            "hi",
            "--reasoning",
            "high",
            "--print-command",
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("needs a model"), "{stderr}");
}

#[test]
fn cursor_reasoning_leaves_the_recorded_model_clean() {
    // The effort suffix rides the argv `--model` only; the result's recorded
    // `model` stays the plain id (effort is a separate concept, not the model).
    let output = run(
        &[
            "run",
            "--harness",
            "cursor",
            "--model",
            "sonnet",
            "--reasoning",
            "high",
            "--prompt",
            "hi",
            "--print-command",
        ],
        &[],
    );
    assert!(output.status.success(), "{output:?}");
    let value = json_stdout(&output);
    assert_eq!(value["results"][0]["model"], "sonnet");
}

#[test]
fn reasoning_leads_the_override_args_so_passthrough_can_win() {
    // Reasoning args precede a raw `--` passthrough, so an explicit override wins.
    let claude = print_command_for(&[
        "--harness",
        "claude-code",
        "--reasoning",
        "high",
        "--",
        "--effort",
        "max",
    ]);
    let first = claude.iter().position(|t| t == "--effort").unwrap();
    let last = claude.iter().rposition(|t| t == "--effort").unwrap();
    assert_ne!(first, last, "expected two --effort occurrences: {claude:?}");
    assert_eq!(
        claude[last + 1],
        "max",
        "passthrough must come last: {claude:?}"
    );
}

#[test]
fn reasoning_without_the_flag_leaves_argv_untouched() {
    // The common case is byte-identical to before the feature (no --effort/-c).
    let claude = print_command_for(&["--harness", "claude-code"]);
    assert!(!claude.iter().any(|t| t == "--effort"), "{claude:?}");
    let codex = print_command_for(&["--harness", "codex"]);
    assert!(
        !codex
            .iter()
            .any(|t| t == "model_reasoning_effort=high" || t.starts_with("model_reasoning_effort")),
        "{codex:?}"
    );
}

#[test]
fn reasoning_on_a_harness_without_a_flag_is_a_usage_error() {
    // Every harness but claude-code/codex sets effort via config, not argv, so a
    // reasoning request is refused loudly (never silently dropped) and points at
    // the harnesses that can take it.
    let output = run(
        &[
            "run",
            "--harness",
            "opencode",
            "--prompt",
            "hi",
            "--reasoning",
            "high",
            "--print-command",
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cannot take a reasoning"), "{stderr}");
    assert!(
        stderr.contains("claude-code, codex, copilot, cursor"),
        "should list capable harnesses: {stderr}"
    );
}

#[test]
fn reasoning_scoped_to_a_capable_harness_does_not_trip_a_mixed_selection() {
    // A value scoped per harness (`[harness.codex].reasoning`) reaches codex and
    // leaves claude-code untouched — the mixed selection is NOT refused, because
    // the incapable-in-selection harness has no effective reasoning value.
    let fx = ConfigFixture::new(
        "reasoning-scoped",
        "[harness.codex]\nreasoning = \"high\"\n",
        "",
    );
    let output = run_with_config(
        &[
            "run",
            "--harness",
            "codex",
            "--harness",
            "claude-code",
            "--prompt",
            "go",
            "--print-command",
            "--compact",
            "--cwd",
            &fx.cwd(),
        ],
        &[],
        &fx.user_config(),
    );
    assert!(output.status.success(), "{output:?}");
    let value = json_stdout(&output);
    let by_harness = |id: &str| -> Vec<String> {
        let results = value["results"].as_array().unwrap();
        let idx = results.iter().position(|r| r["harness"] == id).unwrap();
        command_of(&value, idx)
    };
    assert!(
        by_harness("codex")
            .windows(2)
            .any(|w| w == ["-c", "model_reasoning_effort=high"]),
        "codex should carry the scoped effort"
    );
    assert!(
        !by_harness("claude-code").iter().any(|t| t == "--effort"),
        "claude-code must be untouched by a codex-scoped value"
    );
}

#[test]
fn config_command_reports_reasoning_provenance() {
    // `oneharness config` attributes both the top-level and per-harness reasoning
    // to their winning layer, so a consumer can see the effective effort/source.
    let fx = ConfigFixture::new(
        "reasoning-provenance",
        "reasoning = \"medium\"\n[harness.codex]\nreasoning = \"high\"\n",
        "",
    );
    let output = run_with_config(&["config", "--cwd", &fx.cwd()], &[], &fx.user_config());
    assert!(output.status.success(), "{output:?}");
    let value = json_stdout(&output);
    assert_eq!(value["reasoning"]["value"], "medium");
    assert_eq!(value["harness"]["codex"]["reasoning"]["value"], "high");
}

#[test]
fn fork_on_a_resume_only_harness_is_a_usage_error() {
    // Codex supports --resume but resumes linearly; --fork is a loud error, not a
    // silent linear resume.
    let output = run(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "hi",
            "--resume",
            "th-1",
            "--fork",
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("does not support --fork"), "{stderr}");
    assert!(stderr.contains("claude-code"), "{stderr}");
}

#[test]
fn fork_without_resume_is_rejected_by_clap() {
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--fork",
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn fork_maps_to_native_flag_and_is_echoed() {
    // claude-code: `--resume <id> --fork-session`, with the report echoing fork.
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "go",
            "--resume",
            "sess-abc",
            "--fork",
            "--print-command",
            "--compact",
        ],
        &[],
    );
    assert!(output.status.success());
    let value = json_stdout(&output);
    assert_eq!(value["resume"], "sess-abc");
    assert_eq!(value["fork"], true);
    let command: Vec<String> = value["results"][0]["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect();
    assert!(
        command.windows(2).any(|w| w == ["--resume", "sess-abc"]),
        "{command:?}"
    );
    assert!(command.iter().any(|t| t == "--fork-session"), "{command:?}");
    // opencode forks with `--fork`.
    let opencode = print_command_for(&["--harness", "opencode", "--resume", "ses_x", "--fork"]);
    assert!(opencode.iter().any(|t| t == "--fork"), "{opencode:?}");
}

#[test]
fn resume_with_multiple_harnesses_is_a_usage_error() {
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code,codex",
            "--prompt",
            "hi",
            "--resume",
            "sess-abc",
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("exactly one harness"), "{stderr}");
}

#[test]
fn resume_with_all_is_rejected_by_clap() {
    let output = run(
        &["run", "--all", "--prompt", "hi", "--resume", "sess-abc"],
        &[],
    );
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn list_exposes_resume_and_fork_capabilities() {
    let output = run(&["list", "--compact"], &[]);
    let value = json_stdout(&output);
    let harnesses = value["harnesses"].as_array().unwrap();
    // Every harness now exposes a headless continuation flag.
    for h in harnesses {
        assert_eq!(h["supports_resume"], true, "{} resume", h["id"]);
    }
    let fork = |id: &str| {
        harnesses.iter().find(|h| h["id"] == id).unwrap()["supports_fork"]
            .as_bool()
            .unwrap()
    };
    // Fork is claude-code + opencode only; the rest resume linearly.
    assert!(fork("claude-code"));
    assert!(fork("opencode"));
    assert!(!fork("codex"));
    assert!(!fork("cursor"));
    assert!(!fork("goose"));
    // Cache-reusing fork (the min-tokens batch saving) is Claude Code only:
    // OpenCode can fork but its fork re-sends the prefix cold.
    let reuses = |id: &str| {
        harnesses.iter().find(|h| h["id"] == id).unwrap()["fork_reuses_cache"]
            .as_bool()
            .unwrap()
    };
    assert!(reuses("claude-code"));
    assert!(!reuses("opencode"));
    assert!(!reuses("codex"));

    // `--session` (the uniform handle) is supported exactly for harnesses that
    // expose a session id headlessly — the `extract_session` sources.
    let session_capable = |id: &str| {
        harnesses.iter().find(|h| h["id"] == id).unwrap()["session_capable"]
            .as_bool()
            .unwrap()
    };
    for id in ["claude-code", "opencode", "codex", "cursor", "qwen"] {
        assert!(session_capable(id), "{id} should be session_capable");
    }
    for id in ["goose", "copilot", "crush"] {
        assert!(!session_capable(id), "{id} should not be session_capable");
    }
}

#[test]
fn resume_maps_to_session_flag_for_opencode() {
    let output = run(
        &[
            "run",
            "--harness",
            "opencode",
            "--prompt",
            "continue",
            "--resume",
            "ses_abc",
            "--print-command",
            "--compact",
        ],
        &[],
    );
    assert!(output.status.success());
    let value = json_stdout(&output);
    let command: Vec<String> = value["results"][0]["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect();
    assert!(
        command.windows(2).any(|w| w == ["--session", "ses_abc"]),
        "{command:?}"
    );
}

#[test]
fn resume_maps_to_resume_flag_for_cursor() {
    let output = run(
        &[
            "run",
            "--harness",
            "cursor",
            "--prompt",
            "continue",
            "--resume",
            "chat-9",
            "--print-command",
            "--compact",
        ],
        &[],
    );
    assert!(output.status.success());
    let value = json_stdout(&output);
    let command: Vec<String> = value["results"][0]["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect();
    assert!(
        command.windows(2).any(|w| w == ["--resume", "chat-9"]),
        "{command:?}"
    );
}

#[test]
fn normalizes_usage_and_session_from_opencode_stream_json() {
    // OpenCode emits JSONL step events: a camelCase sessionID and per-step
    // tokens/cost under `part`, which oneharness sums into one usage reading and
    // records the method as `json:summed-steps`.
    let stdout = concat!(
        "{\"type\":\"step_start\",\"sessionID\":\"ses_abc\",\"part\":{}}\n",
        "{\"type\":\"step_finish\",\"sessionID\":\"ses_abc\",\"part\":{\"cost\":0.001,",
        "\"tokens\":{\"input\":671,\"output\":8,\"cache\":{\"read\":21415,\"write\":100}}}}\n",
        "{\"type\":\"step_finish\",\"sessionID\":\"ses_abc\",\"part\":{\"cost\":0.002,",
        "\"tokens\":{\"input\":12,\"output\":34}}}\n",
    );
    let output = run(
        &[
            "run",
            "--harness",
            "opencode",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("opencode"),
            "--compact",
        ],
        &[("MOCK_STDOUT", stdout)],
    );
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let value = json_stdout(&output);
    let result = &value["results"][0];
    assert_eq!(result["usage"]["input_tokens"], 683);
    assert_eq!(result["usage"]["output_tokens"], 42);
    assert_eq!(result["usage"]["cache_read_tokens"], 21415);
    assert_eq!(result["usage"]["cache_write_tokens"], 100);
    assert!((result["usage"]["cost_usd"].as_f64().unwrap() - 0.003).abs() < 1e-9);
    assert_eq!(result["usage_source"], "json:summed-steps");
    assert_eq!(result["session_id"], "ses_abc");
}

#[test]
fn codex_usage_and_known_model_cost_flow_into_history_while_unknown_cost_is_omitted() {
    let stdout = concat!(
        "{\"type\":\"turn.started\"}\n",
        "{\"type\":\"thread.started\",\"thread_id\":\"th-usage\"}\n",
        "{\"type\":\"item.completed\",\"item\":{\"id\":\"msg-1\",\"type\":\"agent_message\",\"text\":\"done\"}}\n",
        "{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":1000,\"cached_input_tokens\":400,\"output_tokens\":100}}\n",
    );
    for (model, expect_cost) in [("gpt-5-codex", true), ("private-alias", false)] {
        let dir = hist_dir(model);
        let ds = dir.display().to_string();
        let output = run(
            &[
                "run",
                "--harness",
                "codex",
                "--prompt",
                "usage",
                "--model",
                model,
                "--bin",
                &bin_override("codex"),
                "--history",
                "--history-dir",
                &ds,
                "--bypass",
                "--compact",
            ],
            &[("MOCK_STDOUT", stdout), ("MOCK_STREAM_DELAY_MS", "20")],
        );
        assert!(output.status.success());
        let report = json_stdout(&output);
        let result = &report["results"][0];
        assert_eq!(result["usage"]["input_tokens"], 1000);
        assert_eq!(result["usage"]["cache_read_tokens"], 400);
        assert_eq!(result["usage"]["output_tokens"], 100);
        assert_eq!(result["usage"]["cost_usd"].is_number(), expect_cost);
        let record = first_history_run(Path::new(report["history_file"].as_str().unwrap()));
        assert_eq!(record["schema_version"], "1.1");
        assert_eq!(record["usage"]["cost_usd"].is_number(), expect_cost);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn extracts_opencode_text_from_real_jsonl_transcript() {
    // OpenCode requests `--format json` but emits line-delimited events, not one
    // document — so naive single-document parsing left `text` null and consumers
    // had to fall back to raw stdout. The fixture is a real `opencode run --format
    // json` transcript (OpenCode 1.17.3); oneharness reconstructs the answer from
    // its `text` parts and records the method as `json:opencode-parts`.
    let stdout = include_str!("support/opencode_run.jsonl");
    let output = run(
        &[
            "run",
            "--harness",
            "opencode",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("opencode"),
            "--compact",
        ],
        &[("MOCK_STDOUT", stdout)],
    );
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let value = json_stdout(&output);
    let result = &value["results"][0];
    assert_eq!(result["text"], "PING-123");
    assert_eq!(result["text_source"], "json:opencode-parts");
}

#[test]
fn normalizes_tool_events_from_opencode_jsonl() {
    // Behavioral trace (issue #1096): OpenCode's `tool` parts become normalized
    // `tool_call` events carrying name/input/output, in order — so a consumer can
    // assert on what the harness *did*, not just its final text.
    let stdout = concat!(
        r#"{"type":"text","part":{"type":"text","text":"running it"}}"#,
        "\n",
        r#"{"type":"tool_use","part":{"type":"tool","tool":"bash","state":{"status":"completed","input":{"command":"git commit -m x"},"output":"OK"}}}"#,
        "\n",
        r#"{"type":"tool_use","part":{"type":"tool","tool":"edit","state":{"status":"completed","input":{"filePath":"config.yaml"}}}}"#,
        "\n",
        r#"{"type":"step_finish","part":{"type":"step-finish","cost":0.01}}"#,
        "\n",
    );
    let output = run(
        &[
            "run",
            "--harness",
            "opencode",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("opencode"),
            "--compact",
        ],
        &[("MOCK_STDOUT", stdout)],
    );
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let value = json_stdout(&output);
    let result = &value["results"][0];
    assert_eq!(result["events_source"], "json:opencode-parts");
    let events = result["events"].as_array().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["kind"], "tool_call");
    assert_eq!(events[0]["name"], "bash");
    assert_eq!(events[0]["input"]["command"], "git commit -m x");
    assert_eq!(events[0]["output"], "OK");
    assert_eq!(events[0]["index"], 0);
    assert_eq!(events[1]["name"], "edit");
    assert_eq!(events[1]["input"]["filePath"], "config.yaml");
    assert!(events[1]["output"].is_null());
    assert_eq!(events[1]["index"], 1);
}

#[test]
fn codex_collaboration_and_web_search_events_flow_through_stream_and_history() {
    let dir = hist_dir("codex-collab-web");
    let stdout = concat!(
        "{\"type\":\"turn.started\"}\n",
        "{\"type\":\"item.started\",\"item\":{\"id\":\"collab-1\",\"type\":\"collab_tool_call\",\"tool\":\"spawn_agent\",\"prompt\":\"inspect tests\",\"receiver_thread_ids\":[\"thread-2\"]}}\n",
        "{\"type\":\"item.completed\",\"item\":{\"id\":\"collab-1\",\"type\":\"collab_tool_call\",\"tool\":\"spawn_agent\",\"prompt\":\"inspect tests\",\"receiver_thread_ids\":[\"thread-2\"],\"status\":\"completed\"}}\n",
        "{\"type\":\"item.started\",\"item\":{\"id\":\"search-1\",\"type\":\"web_search\",\"query\":\"Rust docs\"}}\n",
        "{\"type\":\"item.completed\",\"item\":{\"id\":\"search-1\",\"type\":\"web_search\",\"query\":\"Rust docs\",\"status\":\"completed\"}}\n",
        "{\"type\":\"item.completed\",\"item\":{\"id\":\"m1\",\"type\":\"agent_message\",\"text\":\"done\"}}\n",
        "{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":8,\"output_tokens\":2}}\n",
    );
    let output = run(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "research",
            "--bin",
            &bin_override("codex"),
            "--stream",
            "--history",
            "--history-dir",
            &dir.display().to_string(),
            "--bypass",
        ],
        &[("MOCK_STDOUT", stdout), ("MOCK_STREAM_DELAY_MS", "1")],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelopes = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let streamed = envelopes
        .iter()
        .filter(|value| value["type"] == "event")
        .map(|value| &value["event"])
        .collect::<Vec<_>>();
    assert_eq!(streamed.len(), 4);
    assert_eq!(streamed[0]["name"], "spawn_agent");
    assert_eq!(streamed[0]["input"]["prompt"], "inspect tests");
    assert_eq!(streamed[0]["input"]["receiver_thread_ids"][0], "thread-2");
    assert_eq!(streamed[1]["name"], "spawn_agent");
    assert_eq!(streamed[2]["name"], "web_search");
    assert_eq!(streamed[2]["input"]["query"], "Rust docs");
    assert_eq!(streamed[3]["name"], "web_search");

    let report = &envelopes.last().unwrap()["report"];
    let raw_lines = std::fs::read_to_string(report["history_file"].as_str().unwrap()).unwrap();
    let raw_lines = raw_lines
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(raw_lines.len(), 5);
    for (index, line) in raw_lines.iter().take(4).enumerate() {
        assert_eq!(line["type"], "event");
        assert_eq!(line["event"]["index"], index);
    }
    assert_eq!(raw_lines[4]["type"], "run");
    assert!(raw_lines[4].get("events").is_none());
    let record = first_history_run(Path::new(report["history_file"].as_str().unwrap()));
    assert_eq!(record["events"][0]["name"], "spawn_agent");
    assert_eq!(record["events"][1]["name"], "spawn_agent");
    assert_eq!(record["events"][2]["name"], "web_search");
    assert_eq!(record["events"][3]["name"], "web_search");
    assert_eq!(record["usage"]["input_tokens"], 8);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn normalizes_tool_events_from_cursor_stream_json_content_blocks() {
    // The Anthropic content-block shape (Cursor / Claude Code under stream-json):
    // a `tool_use` assistant block and its `tool_result` observation normalize to
    // `tool_call` + `tool_result` events under `stream-json:content-blocks`.
    let stdout = concat!(
        r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}]}}"#,
        "\n",
        r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"a.txt"}]}}"#,
        "\n",
        r#"{"type":"result","subtype":"success","result":"done","session_id":"11111111-2222-3333-4444-555555555555"}"#,
        "\n",
    );
    let output = run(
        &[
            "run",
            "--harness",
            "cursor",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("cursor"),
            "--compact",
        ],
        &[("MOCK_STDOUT", stdout)],
    );
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let value = json_stdout(&output);
    let result = &value["results"][0];
    assert_eq!(result["events_source"], "stream-json:content-blocks");
    let events = result["events"].as_array().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["kind"], "tool_call");
    assert_eq!(events[0]["name"], "Bash");
    assert_eq!(events[0]["input"]["command"], "ls");
    assert_eq!(events[1]["kind"], "tool_result");
    assert_eq!(events[1]["output"], "a.txt");
    assert!(events[1]["name"].is_null());
}

#[test]
fn claude_stream_json_surfaces_content_block_events() {
    // The flagship path: Claude Code's default `json` result has no transcript,
    // but under `--output-format stream-json` it emits the Anthropic content-block
    // stream, which oneharness normalizes into `tool_call` / `tool_result` events.
    // (The real CLI needs `--verbose` for this, which oneharness adds — see the
    // build_argv test; the mock just emits the shape.)
    let stdout = concat!(
        r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"echo hi"}}]}}"#,
        "\n",
        r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"hi"}]}}"#,
        "\n",
        r#"{"type":"result","subtype":"success","result":"done","session_id":"sess-1"}"#,
        "\n",
    );
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("claude-code"),
            "--output-format",
            "stream-json",
            "--compact",
        ],
        &[("MOCK_STDOUT", stdout)],
    );
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let value = json_stdout(&output);
    let result = &value["results"][0];
    // oneharness added `--verbose` to the built command (required by the real CLI).
    let command = result["command"].as_array().unwrap();
    assert!(
        command.iter().any(|t| t == "--verbose"),
        "stream-json command should carry --verbose: {command:?}"
    );
    assert_eq!(result["events_source"], "stream-json:content-blocks");
    let events = result["events"].as_array().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["kind"], "tool_call");
    assert_eq!(events[0]["name"], "Bash");
    assert_eq!(events[0]["input"]["command"], "echo hi");
    assert_eq!(events[1]["kind"], "tool_result");
    assert_eq!(events[1]["output"], "hi");
}

#[test]
fn events_flag_upgrades_claude_to_stream_json_and_surfaces_events() {
    // `--events` selects the harness's events-capable format when its default
    // carries no transcript: claude-code's default `json` becomes `stream-json`
    // (with the required `--verbose`), so tool events surface without the caller
    // knowing the quirk or passing --output-format.
    let stdout = concat!(
        r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}]}}"#,
        "\n",
        r#"{"type":"result","result":"done"}"#,
        "\n",
    );
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("claude-code"),
            "--events",
            "--compact",
        ],
        &[("MOCK_STDOUT", stdout)],
    );
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let value = json_stdout(&output);
    let result = &value["results"][0];
    let command = result["command"].as_array().unwrap();
    assert!(
        command
            .windows(2)
            .any(|w| w[0] == "--output-format" && w[1] == "stream-json"),
        "--events should upgrade claude to stream-json: {command:?}"
    );
    assert!(command.iter().any(|t| t == "--verbose"), "{command:?}");
    assert_eq!(result["events_source"], "stream-json:content-blocks");
    assert_eq!(result["events"][0]["name"], "Bash");
}

#[test]
fn events_flag_respects_explicit_output_format() {
    // An explicit --output-format always wins over the --events upgrade, so a
    // caller can still pin the format (here json, which for claude has no
    // transcript → events null) even with --events.
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("claude-code"),
            "--events",
            "--output-format",
            "json",
            "--compact",
        ],
        &[("MOCK_STDOUT", r#"{"type":"result","result":"hi"}"#)],
    );
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let value = json_stdout(&output);
    let command = value["results"][0]["command"].as_array().unwrap();
    assert!(
        !command.iter().any(|t| t == "stream-json"),
        "explicit --output-format json must win: {command:?}"
    );
    assert!(value["results"][0]["events"].is_null());
}

#[test]
fn stream_mode_emits_event_lines_then_a_terminal_report() {
    // `--stream` writes one NDJSON line per normalized event as it arrives, then a
    // terminal `{"type":"result","report":{…}}` line carrying the full envelope.
    let stdout = concat!(
        r#"{"type":"text","part":{"type":"text","text":"working"}}"#,
        "\n",
        r#"{"type":"tool_use","part":{"type":"tool","tool":"bash","state":{"status":"completed","input":{"command":"echo hi"},"output":"hi"}}}"#,
        "\n",
        r#"{"type":"tool_use","part":{"type":"tool","tool":"edit","state":{"input":{"filePath":"a.txt"}}}}"#,
        "\n",
    );
    let output = run(
        &[
            "run",
            "--harness",
            "opencode",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("opencode"),
            "--stream",
        ],
        &[("MOCK_STDOUT", stdout)],
    );
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let text = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<Value> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("each stream line is JSON"))
        .collect();
    let typed: Vec<RunStreamEnvelope> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("each stream line matches the Rust contract"))
        .collect();
    // Two event lines, then one result line.
    assert_eq!(lines.len(), 3, "lines: {text}");
    assert_eq!(lines[0]["type"], "event");
    assert_eq!(lines[0]["event"]["kind"], "tool_call");
    assert_eq!(lines[0]["event"]["name"], "bash");
    assert_eq!(lines[0]["event"]["index"], 0);
    assert_eq!(lines[1]["type"], "event");
    assert_eq!(lines[1]["event"]["name"], "edit");
    assert_eq!(lines[1]["event"]["index"], 1);
    // The terminal line is the full report; its single result carries the same
    // events array and the extracted text.
    assert_eq!(lines[2]["type"], "result");
    let result = &lines[2]["report"]["results"][0];
    assert_eq!(result["events_source"], "json:opencode-parts");
    assert_eq!(result["events"].as_array().unwrap().len(), 2);
    assert_eq!(result["text"], "working");
    assert!(matches!(typed[0], RunStreamEnvelope::Event { .. }));
    assert!(matches!(typed[1], RunStreamEnvelope::Event { .. }));
    assert!(matches!(typed[2], RunStreamEnvelope::Result { .. }));

    // Stream envelopes are producer output: new additive fields from a newer
    // oneharness remain readable by this Rust contract.
    let mut future = lines[2].clone();
    future["future_output_field"] = Value::Bool(true);
    assert!(serde_json::from_value::<RunStreamEnvelope>(future).is_ok());
    assert!(serde_json::from_value::<RunStreamEnvelope>(
        serde_json::json!({ "type": "future_variant" })
    )
    .is_err());
    for malformed in [
        serde_json::json!({}),
        serde_json::json!({ "type": "event" }),
        serde_json::json!({ "type": "result" }),
        serde_json::json!({ "type": "event", "event": {} }),
        serde_json::json!({ "type": "result", "report": {} }),
    ] {
        assert!(serde_json::from_value::<RunStreamEnvelope>(malformed).is_err());
    }
}

#[test]
fn stream_buffers_partial_provider_records_until_newline() {
    let stdout = concat!(
        "{\"type\":\"tool_use\",\"part\":{\"type\":\"tool\",\"tool\":\"bash\",\"state\":{\"status\":\"completed\",\"input\":{\"command\":\"pwd\"},\"output\":\"/repo\"}}}\n",
        "{\"type\":\"text\",\"part\":{\"type\":\"text\",\"text\":\"done\"}}\n",
    );
    let output = run(
        &[
            "run",
            "--harness",
            "opencode",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("opencode"),
            "--stream",
        ],
        &[
            ("MOCK_STDOUT", stdout),
            ("MOCK_STREAM_CHUNK_BYTES", "7"),
            ("MOCK_STREAM_DELAY_MS", "1"),
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelopes = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| serde_json::from_str::<RunStreamEnvelope>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(envelopes.len(), 2);
    match &envelopes[0] {
        RunStreamEnvelope::Event { event } => {
            assert_eq!(event.name.as_deref(), Some("bash"));
            assert_eq!(event.input.as_ref().unwrap()["command"], "pwd");
        }
        RunStreamEnvelope::Result { .. } => panic!("partial JSON emitted a result early"),
    }
    match &envelopes[1] {
        RunStreamEnvelope::Result { report } => {
            assert_eq!(report.results[0].text.as_deref(), Some("done"));
            assert_eq!(report.results[0].events.as_ref().unwrap().len(), 1);
        }
        RunStreamEnvelope::Event { .. } => panic!("missing terminal report"),
    }
}

#[test]
fn stream_config_key_streams_and_an_explicit_flag_wins() {
    // `stream = true` in config is exactly `--stream`, so a consumer that always
    // reads events declares it once instead of injecting the flag per invocation.
    // `--no-stream` takes it back for a single call.
    let bin = mock_bin().display().to_string().replace('\\', "\\\\");
    let fx = ConfigFixture::new(
        "stream-config",
        &format!(
            r#"
harnesses = ["opencode"]
stream = true
[harness.opencode]
bin = "{bin}"
"#
        ),
        "",
    );
    let stdout = concat!(
        r#"{"type":"tool_use","part":{"type":"tool","tool":"bash","state":{"status":"completed","input":{"command":"echo hi"},"output":"hi"}}}"#,
        "\n",
        r#"{"type":"text","part":{"type":"text","text":"done"}}"#,
        "\n",
    );
    let streamed = run_with_config(
        &["run", "--prompt", "hi", "--cwd", &fx.cwd()],
        &[("MOCK_STDOUT", stdout)],
        &fx.user_config(),
    );
    assert!(
        streamed.status.success(),
        "{}",
        String::from_utf8_lossy(&streamed.stderr)
    );
    let text = String::from_utf8_lossy(&streamed.stdout);
    let envelopes: Vec<RunStreamEnvelope> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("each stream line matches the contract"))
        .collect();
    assert_eq!(envelopes.len(), 2, "lines: {text}");
    match &envelopes[0] {
        RunStreamEnvelope::Event { event } => assert_eq!(event.name.as_deref(), Some("bash")),
        RunStreamEnvelope::Result { .. } => panic!("config `stream` did not publish events"),
    }
    match &envelopes[1] {
        RunStreamEnvelope::Result { report } => {
            assert_eq!(report.results[0].text.as_deref(), Some("done"));
        }
        RunStreamEnvelope::Event { .. } => panic!("missing terminal report"),
    }

    // The explicit flag wins over the config value.
    let buffered = run_with_config(
        &["run", "--prompt", "hi", "--cwd", &fx.cwd(), "--no-stream"],
        &[("MOCK_STDOUT", stdout)],
        &fx.user_config(),
    );
    assert!(
        buffered.status.success(),
        "{}",
        String::from_utf8_lossy(&buffered.stderr)
    );
    let report = json_stdout(&buffered);
    assert_eq!(report["results"][0]["text"], "done");
    assert!(
        !String::from_utf8_lossy(&buffered.stdout).contains("\"type\":\"event\""),
        "--no-stream still streamed"
    );

    // The environment layer is the same value by another name: it streams on its
    // own, from a config that says nothing about streaming...
    let env_only = ConfigFixture::new(
        "stream-env",
        &format!(
            r#"
harnesses = ["opencode"]
[harness.opencode]
bin = "{bin}"
"#
        ),
        "",
    );
    let from_env = run_with_config(
        &["run", "--prompt", "hi", "--cwd", &env_only.cwd()],
        &[("MOCK_STDOUT", stdout), ("ONEHARNESS_STREAM", "true")],
        &env_only.user_config(),
    );
    assert!(
        from_env.status.success(),
        "{}",
        String::from_utf8_lossy(&from_env.stderr)
    );
    let envelopes: Vec<RunStreamEnvelope> = String::from_utf8_lossy(&from_env.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("each stream line matches the contract"))
        .collect();
    assert_eq!(envelopes.len(), 2);
    assert!(matches!(envelopes[0], RunStreamEnvelope::Event { .. }));
    // ...and still loses to the explicit flag.
    let env_overridden = run_with_config(
        &[
            "run",
            "--prompt",
            "hi",
            "--cwd",
            &env_only.cwd(),
            "--no-stream",
        ],
        &[("MOCK_STDOUT", stdout), ("ONEHARNESS_STREAM", "true")],
        &env_only.user_config(),
    );
    assert!(
        !String::from_utf8_lossy(&env_overridden.stdout).contains("\"type\":\"event\""),
        "--no-stream lost to ONEHARNESS_STREAM"
    );
    let config = json_stdout(&run_with_config(
        &["config", "--cwd", &fx.cwd(), "--compact"],
        &[("ONEHARNESS_STREAM", "true")],
        &fx.user_config(),
    ));
    assert_eq!(config["stream"]["value"], true);
    assert_eq!(config["stream"]["source"], "environment");
    // A file value is attributed to that file, like any other scalar...
    let from_file = json_stdout(&run_with_config(
        &["config", "--cwd", &fx.cwd(), "--compact"],
        &[],
        &fx.user_config(),
    ));
    assert_eq!(from_file["stream"]["value"], true);
    assert!(
        from_file["stream"]["source"]
            .as_str()
            .unwrap()
            .ends_with("oneharness.toml"),
        "{from_file}"
    );

    // ...and the flag wins over an explicit `stream = false` too, not just over
    // an absent value.
    let off = ConfigFixture::new(
        "stream-off",
        &format!(
            r#"
harnesses = ["opencode"]
stream = false
[harness.opencode]
bin = "{bin}"
"#
        ),
        "",
    );
    let forced = run_with_config(
        &["run", "--prompt", "hi", "--cwd", &off.cwd(), "--stream"],
        &[("MOCK_STDOUT", stdout)],
        &off.user_config(),
    );
    assert!(
        forced.status.success(),
        "{}",
        String::from_utf8_lossy(&forced.stderr)
    );
    assert!(
        String::from_utf8_lossy(&forced.stdout).contains("\"type\":\"event\""),
        "--stream lost to `stream = false`"
    );
}

#[test]
fn stream_config_key_refuses_a_selection_it_cannot_stream() {
    // A config value is the flag, validation and all: a parallel multi-harness
    // selection is the same loud usage error `--stream` raises, never a silent
    // downgrade to a buffered report.
    let bin = mock_bin().display().to_string().replace('\\', "\\\\");
    let fx = ConfigFixture::new(
        "stream-config-refused",
        &format!(
            r#"
harnesses = ["opencode", "codex"]
stream = true
[harness.opencode]
bin = "{bin}"
[harness.codex]
bin = "{bin}"
"#
        ),
        "",
    );
    let output = run_with_config(
        &["run", "--prompt", "hi", "--cwd", &fx.cwd()],
        &[],
        &fx.user_config(),
    );
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--stream runs a single harness"),
        "{stderr}"
    );
    // ...and `--no-stream` is how one call opts out of the inherited config.
    let allowed = run_with_config(
        &["run", "--prompt", "hi", "--cwd", &fx.cwd(), "--no-stream"],
        &[("MOCK_STDOUT", "hello")],
        &fx.user_config(),
    );
    assert!(
        allowed.status.success(),
        "{}",
        String::from_utf8_lossy(&allowed.stderr)
    );
    assert_eq!(
        json_stdout(&allowed)["results"].as_array().unwrap().len(),
        2
    );

    // Every other shape streaming cannot serve is refused the same way, whether
    // the value came from the flag or from config: a batch, structured output,
    // and a parallel model fan-out.
    let schema = std::path::Path::new(&fx.cwd()).join("schema.json");
    std::fs::write(&schema, r#"{"type":"object"}"#).unwrap();
    let single = ["run", "--harness", "opencode", "--cwd"];
    for (args, needle) in [
        (
            vec![
                "--prompt",
                "one",
                "--prompt",
                "two",
                "--batch-strategy",
                "speed",
            ],
            "--stream is incompatible with a batch",
        ),
        (
            vec!["--prompt", "hi", "--schema", &schema.display().to_string()],
            "--stream is incompatible with --schema",
        ),
        (
            vec!["--prompt", "hi", "--model", "a", "--model", "b"],
            "--stream",
        ),
    ] {
        let mut argv: Vec<&str> = single.to_vec();
        let cwd = fx.cwd();
        argv.push(&cwd);
        argv.extend(args.iter().copied());
        let refused = run_with_config(&argv, &[], &fx.user_config());
        assert_eq!(refused.status.code(), Some(2), "{argv:?}");
        let stderr = String::from_utf8_lossy(&refused.stderr);
        assert!(stderr.contains(needle), "{argv:?}: {stderr}");
    }

    // A malformed environment value is the same loud usage error any other
    // ONEHARNESS_* boolean raises, never a silent "not true, so off".
    let malformed = run_with_config(
        &[
            "run",
            "--harness",
            "opencode",
            "--prompt",
            "hi",
            "--cwd",
            &fx.cwd(),
        ],
        &[("ONEHARNESS_STREAM", "maybe")],
        &fx.user_config(),
    );
    assert_eq!(malformed.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&malformed.stderr);
    assert!(stderr.contains("ONEHARNESS_STREAM"), "{stderr}");

    // Asking for both directions at once is refused by the parser.
    let both = run_with_config(
        &[
            "run",
            "--harness",
            "opencode",
            "--prompt",
            "hi",
            "--stream",
            "--no-stream",
        ],
        &[],
        &fx.user_config(),
    );
    assert!(!both.status.success());
    let stderr = String::from_utf8_lossy(&both.stderr);
    assert!(stderr.contains("--no-stream"), "{stderr}");
}

#[test]
fn stream_short_circuit_tears_down_the_child_when_the_consumer_closes() {
    let mock_profile = mock_profile_redirect();
    // The flagship streaming behavior: when the consumer stops reading (closes
    // oneharness's stdout), oneharness's next event write fails (broken pipe) and
    // it tears the harness child down — so a bad turn is cut off, not paid for in
    // full. Proven deterministically: the mock streams several event lines with a
    // delay and only writes a COMPLETE sentinel if it runs to the end. We read the
    // first event, drop the pipe, and assert COMPLETE was never written.
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};

    let sentinel = std::env::temp_dir().join(format!("oh-sc-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&sentinel);
    // Five opencode tool-part lines → five tool_call events, 300ms apart.
    let lines: Vec<String> = (0..5)
        .map(|i| {
            format!(
                r#"{{"type":"tool_use","part":{{"type":"tool","tool":"bash","state":{{"input":{{"command":"step {i}"}}}}}}}}"#
            )
        })
        .collect();
    let stdout_script = lines.join("\n");

    let mut child = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        .env("MOCK_STDOUT", &stdout_script)
        .env("MOCK_STREAM_DELAY_MS", "300")
        .env("MOCK_LOG_FILE", &sentinel)
        .args([
            "run",
            "--harness",
            "opencode",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("opencode"),
            "--stream",
            "--env",
            mock_profile.as_str(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn oneharness --stream");

    // Read exactly the first event line, then drop the reader → close the read
    // end of oneharness's stdout (the consumer short-circuiting).
    let mut reader = BufReader::new(child.stdout.take().expect("piped stdout"));
    let mut first = String::new();
    reader.read_line(&mut first).expect("read first event line");
    let first: Value = serde_json::from_str(first.trim()).expect("first line is JSON");
    assert_eq!(first["type"], "event");
    assert_eq!(first["event"]["kind"], "tool_call");
    drop(reader); // close the pipe — the short-circuit signal

    let status = child
        .wait()
        .expect("oneharness should exit after the pipe closes");
    // The mock only writes COMPLETE if it streamed every line; a torn-down child
    // never reaches it. (If short-circuit regressed, oneharness would drain all
    // lines, the mock would finish, and COMPLETE would be present.)
    let log = std::fs::read_to_string(&sentinel).unwrap_or_default();
    assert!(
        !log.contains("COMPLETE"),
        "child was not torn down on consumer close (mock ran to completion): {log:?}"
    );
    // oneharness itself exits cleanly (a consumer-driven stop is not a failure).
    assert!(
        status.success() || status.code().is_none(),
        "unexpected exit: {status:?}"
    );
    let _ = std::fs::remove_file(&sentinel);
}

#[test]
fn stream_with_multiple_harnesses_is_a_usage_error() {
    let output = run(
        &[
            "run",
            "--harness",
            "opencode",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--stream",
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--stream runs a single harness"),
        "{stderr}"
    );
    assert!(stderr.contains("--run-mode fallback"), "{stderr}");
}

#[test]
fn stream_with_schema_is_a_usage_error() {
    // A schema file is needed to reach the --stream/--schema conflict check.
    let dir = std::env::temp_dir().join(format!("oh-stream-schema-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let schema_path = dir.join("s.json");
    std::fs::write(&schema_path, r#"{"type":"object"}"#).unwrap();
    let output = run(
        &[
            "run",
            "--harness",
            "opencode",
            "--prompt",
            "hi",
            "--stream",
            "--schema",
            schema_path.to_str().unwrap(),
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--stream is incompatible with --schema"),
        "{stderr}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn events_flag_is_a_safe_noop_for_a_text_only_harness() {
    // The skilltest safety property: `--events` must never break text extraction.
    // A text-only harness (goose) has no events_format, so `--events` leaves its
    // format (and text extraction) untouched, and `events` is honestly null — the
    // reply still comes through. (A blanket `--output-format json` once broke
    // exactly this; `--events` is the safe path.)
    let output = run(
        &[
            "run",
            "--harness",
            "goose",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("goose"),
            "--events",
            "--compact",
        ],
        &[("MOCK_STDOUT", "plain text reply")],
    );
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let value = json_stdout(&output);
    let result = &value["results"][0];
    // Format stayed the harness default (text) — no format flag was injected.
    let command = result["command"].as_array().unwrap();
    assert!(
        !command.iter().any(|t| t == "json" || t == "stream-json"),
        "text-only harness must not be format-upgraded by --events: {command:?}"
    );
    assert_eq!(result["text"], "plain text reply");
    assert!(result["events"].is_null());
    assert!(result["events_source"].is_null());
}

#[test]
fn events_are_extracted_from_a_nonzero_run() {
    // Events are best-effort over whatever output a run produced — including a
    // non-zero exit (a harness that used tools then failed). The tool trace is
    // still lifted, so a consumer can see what ran before the failure.
    let stdout = concat!(
        r#"{"type":"tool_use","part":{"type":"tool","tool":"bash","state":{"input":{"command":"boom"},"output":"err"}}}"#,
        "\n",
    );
    let output = run(
        &[
            "run",
            "--harness",
            "opencode",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("opencode"),
            "--compact",
        ],
        &[("MOCK_STDOUT", stdout), ("MOCK_EXIT", "1")],
    );
    let value = json_stdout(&output);
    let result = &value["results"][0];
    assert_eq!(result["status"], "nonzero");
    assert_eq!(result["events_source"], "json:opencode-parts");
    assert_eq!(result["events"][0]["name"], "bash");
    assert_eq!(result["events"][0]["input"]["command"], "boom");
}

#[test]
fn events_absent_when_harness_exposes_no_trace() {
    // Claude Code's single-document `json` result carries no transcript, so
    // `events`/`events_source` stay null (absent), distinct from an empty array —
    // never fabricated, mirroring the `text`/`usage` best-effort contract.
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[("MOCK_STDOUT", r#"{"type":"result","result":"hi"}"#)],
    );
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let value = json_stdout(&output);
    let result = &value["results"][0];
    assert!(result["events"].is_null());
    assert!(result["events_source"].is_null());
}

#[test]
fn qwen_gets_yolo_suppression_env_injected() {
    // oneharness injects the harness's declared `default_env` into the child, so
    // qwen's startup YOLO warning is silenced without the caller doing anything.
    // The mock echoes its inherited value of the named variable to stdout.
    let output = run(
        &[
            "run",
            "--harness",
            "qwen",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("qwen"),
            "--compact",
        ],
        &[("MOCK_ECHO_ENV", "QWEN_CODE_SUPPRESS_YOLO_WARNING")],
    );
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let value = json_stdout(&output);
    assert_eq!(
        value["results"][0]["stdout"],
        "QWEN_CODE_SUPPRESS_YOLO_WARNING=1"
    );
}

#[test]
fn default_env_is_per_harness_and_overridable_by_explicit_env() {
    // A harness with no declared default sees the variable empty (no leakage)...
    let without = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[("MOCK_ECHO_ENV", "QWEN_CODE_SUPPRESS_YOLO_WARNING")],
    );
    assert_eq!(
        json_stdout(&without)["results"][0]["stdout"],
        "QWEN_CODE_SUPPRESS_YOLO_WARNING="
    );

    // ...and an explicit `--env` wins over the harness default on a key collision.
    let overridden = run(
        &[
            "run",
            "--harness",
            "qwen",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("qwen"),
            "--env",
            "QWEN_CODE_SUPPRESS_YOLO_WARNING=0",
            "--compact",
        ],
        &[("MOCK_ECHO_ENV", "QWEN_CODE_SUPPRESS_YOLO_WARNING")],
    );
    assert_eq!(
        json_stdout(&overridden)["results"][0]["stdout"],
        "QWEN_CODE_SUPPRESS_YOLO_WARNING=0"
    );
}

#[test]
fn passthrough_args_are_appended_verbatim() {
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--print-command",
            "--compact",
            "--",
            "--max-turns",
            "6",
            "--verbose",
        ],
        &[],
    );
    let value = json_stdout(&output);
    let command: Vec<String> = value["results"][0]["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect();
    let tail = &command[command.len() - 3..];
    assert_eq!(tail, ["--max-turns", "6", "--verbose"]);
}

#[test]
fn output_dir_writes_raw_streams_to_files() {
    let dir = std::env::temp_dir().join(format!("oneharness-od-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("claude-code"),
            "--output-dir",
            &dir.display().to_string(),
            "--compact",
        ],
        &[
            ("MOCK_STDOUT", "raw-stdout-bytes"),
            ("MOCK_STDERR", "raw-stderr-bytes"),
        ],
    );
    assert!(output.status.success());

    let out = std::fs::read_to_string(dir.join("claude-code.stdout")).unwrap();
    let err = std::fs::read_to_string(dir.join("claude-code.stderr")).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(out, "raw-stdout-bytes");
    assert_eq!(err, "raw-stderr-bytes");
}

#[test]
fn cwd_sets_pwd_env_for_the_child() {
    // --cwd must keep $PWD consistent with the working directory (like a shell
    // `cd`), not just chdir() and leave the inherited $PWD stale: Bun-based CLIs
    // such as OpenCode trust $PWD to locate the project, so a stale value sends
    // their gate to the wrong directory. Regression test for that.
    let dir = std::env::temp_dir().join(format!("oneharness-pwd-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("claude-code"),
            "--cwd",
            &dir.display().to_string(),
            "--compact",
        ],
        &[("MOCK_ECHO_PWD", "1")],
    );
    assert!(output.status.success());
    let report = json_stdout(&output);
    let stdout = report["results"][0]["stdout"].as_str().unwrap_or("");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        stdout,
        format!("PWD={}", dir.display()),
        "child $PWD should match --cwd; got {stdout:?}"
    );
}

#[test]
fn no_selection_is_a_usage_error() {
    let output = run(&["run", "--prompt", "hi"], &[]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--all or --harness"), "{stderr}");
}

#[test]
fn unknown_harness_is_a_usage_error() {
    let output = run(&["run", "--harness", "bogus", "--prompt", "hi"], &[]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown harness"), "{stderr}");
}

#[test]
fn missing_prompt_is_a_usage_error() {
    let output = run(&["run", "--harness", "claude-code"], &[]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no prompt"), "{stderr}");
}

#[test]
fn executes_mock_and_extracts_json_result() {
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[("MOCK_STDOUT", r#"{"result":"hello from mock"}"#)],
    );
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let value = json_stdout(&output);
    let result = &value["results"][0];
    assert_eq!(result["status"], "ok");
    assert_eq!(result["exit_code"], 0);
    assert_eq!(result["available"], true);
    assert_eq!(result["text"], "hello from mock");
    assert_eq!(result["text_source"], "json:result");
    assert!(result["duration_ms"].is_number());
}

#[test]
fn shipped_mock_harness_runs_without_fixture_binary() {
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--mock-harness",
            "claude-code",
            "--prompt",
            "hi",
            "--compact",
        ],
        &[
            ("MOCK_STDOUT", r#"{"result":"shipped responder"}"#),
            ("MOCK_EXIT", "0"),
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result = &json_stdout(&output)["results"][0];
    assert_eq!(result["status"], "ok");
    assert_eq!(result["text"], "shipped responder");
}

#[test]
fn nonzero_exit_is_reported_and_fails_the_run() {
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[("MOCK_EXIT", "3"), ("MOCK_STDOUT", "boom")],
    );
    assert_eq!(output.status.code(), Some(1));
    let value = json_stdout(&output);
    let result = &value["results"][0];
    assert_eq!(result["status"], "nonzero");
    assert_eq!(result["exit_code"], 3);
}

#[test]
fn slow_harness_times_out() {
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--timeout",
            "1",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[("MOCK_SLEEP_MS", "5000")],
    );
    assert_eq!(output.status.code(), Some(1));
    let value = json_stdout(&output);
    let result = &value["results"][0];
    assert_eq!(result["status"], "timeout");
    assert!(result["error"].as_str().unwrap().contains("timeout"));
}

#[cfg(any(unix, windows))]
#[test]
fn timeout_preserves_partial_telemetry_in_report_and_history() {
    // Reproduce an npm-like launcher plus a long-lived descendant (TERM-ignoring
    // on Unix, parent-independent on Windows). The descendant emits a real
    // OpenCode-shaped transcript (including a task_complete record), leaves a
    // truncated final JSONL record, then keeps both pipes open beyond deadline.
    let history = hist_dir("timeout-telemetry");
    let history_arg = history.display().to_string();
    let ticks = native_tick_file("timeout-cli");
    let _ = std::fs::remove_file(&ticks);
    let transcript = concat!(
        "{\"type\":\"step_start\",\"sessionID\":\"ses-timeout\",\"part\":{\"type\":\"step-start\"}}\n",
        "{\"type\":\"text\",\"sessionID\":\"ses-timeout\",\"part\":",
        "{\"type\":\"text\",\"text\":\"partial answer\"}}\n",
        "{\"type\":\"tool_use\",\"sessionID\":\"ses-timeout\",\"part\":",
        "{\"id\":\"call-timeout\",\"type\":\"tool\",\"tool\":\"bash\",\"state\":",
        "{\"input\":{\"command\":\"echo hi\"},\"output\":\"hi\",\"time\":{\"start\":1773878400000}}}}\n",
        "{\"type\":\"step_finish\",\"sessionID\":\"ses-timeout\",\"part\":",
        "{\"cost\":0.01,\"tokens\":{\"input\":12,\"output\":3,",
        "\"cache\":{\"read\":9,\"write\":4}}}}\n",
        "{\"type\":\"task_complete\",\"text\":\"emitted before exit\"}\n",
        "{\"type\":\"incomplete\"",
    );

    let started = std::time::Instant::now();
    let output = run(
        &[
            "run",
            "--harness",
            "opencode",
            "--prompt",
            "capture timeout evidence",
            "--timeout",
            "1",
            "--bin",
            &bin_override("opencode"),
            "--history",
            "--history-dir",
            &history_arg,
            "--compact",
        ],
        &[
            ("MOCK_STDOUT", transcript),
            ("MOCK_STDERR", "native child stderr\n"),
            ("MOCK_NATIVE_GRANDCHILD_MS", "5000"),
            ("MOCK_TICK_FILE", &ticks.display().to_string()),
        ],
    );

    assert_eq!(output.status.code(), Some(1));
    assert!(
        started.elapsed() >= std::time::Duration::from_millis(900),
        "timeout returned before its configured deadline: {:?}",
        started.elapsed()
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(3),
        "timeout should return near deadline plus teardown grace, took {:?}",
        started.elapsed()
    );
    let value = json_stdout(&output);
    let result = &value["results"][0];
    assert_eq!(result["status"], "timeout");
    assert_eq!(result["text"], "partial answer");
    assert_eq!(result["text_source"], "json:opencode-parts");
    assert_eq!(result["usage"]["input_tokens"], 12);
    assert_eq!(result["usage"]["output_tokens"], 3);
    assert_eq!(result["usage"]["cache_read_tokens"], 9);
    assert_eq!(result["usage"]["cache_write_tokens"], 4);
    assert_eq!(result["usage"]["cost_usd"], 0.01);
    assert_eq!(result["session_id"], "ses-timeout");
    assert_eq!(result["events_source"], "json:opencode-parts");
    assert_eq!(result["events"][0]["name"], "bash");
    assert!(result["stderr"]
        .as_str()
        .unwrap()
        .contains("native child stderr"));
    assert!(
        result["stdout"]
            .as_str()
            .unwrap()
            .ends_with("{\"type\":\"incomplete\""),
        "raw capture retains the truncated tail"
    );

    // History freezes the same normalized evidence while omitting raw streams.
    let history_file = value["history_file"].as_str().expect("history file");
    // The trace completed before the process was killed, so the measurement is
    // provider-measured — and the report carries it, not just history.
    assert_eq!(result["telemetry"]["source"], "provider_measured");
    assert!(result["telemetry"]["started_at"].is_string());

    let record = first_history_run(Path::new(history_file));
    assert_eq!(record["status"], "timeout");
    assert_eq!(record["text"], "partial answer");
    assert_eq!(record["usage"]["input_tokens"], 12);
    assert_eq!(record["session_id"], "ses-timeout");
    assert_eq!(record["events"][0]["name"], "bash");
    assert!(record.get("stdout").is_none());

    // The native-like descendant is gone when oneharness returns; it cannot keep
    // doing work under PID 1 after the report/history have been emitted.
    assert_native_descendant_stopped(&ticks);

    let _ = std::fs::remove_file(ticks);
    let _ = std::fs::remove_dir_all(history);
}

#[cfg(unix)]
#[test]
fn a_host_signal_cancels_the_run_and_terminates_a_silent_harness() {
    let mock_profile = mock_profile_redirect();
    // The CLI face of cancellation. The harness writes nothing at all, so the run
    // has no line to react to, and its descendant outlives a launcher kill. A
    // SIGTERM to oneharness must therefore tear the whole tree down through the
    // ordinary terminate path and still emit the machine-readable report, rather
    // than dying and orphaning a harness that keeps working (and billing).
    let history = hist_dir("cancel-signal");
    let history_arg = history.display().to_string();
    let ticks = native_tick_file("cancel-signal");
    let _ = std::fs::remove_file(&ticks);

    let child = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        // No MOCK_STDOUT: the fixture is completely silent.
        .env("MOCK_NATIVE_GRANDCHILD_MS", "20000")
        .env("MOCK_TICK_FILE", ticks.display().to_string())
        .args([
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "cancel me",
            "--bin",
            &bin_override("claude-code"),
            // Far beyond the teardown this test measures, so a run that only
            // ended at its own deadline could never pass.
            "--timeout",
            "60",
            "--history",
            "--history-dir",
            &history_arg,
            "--compact",
            "--env",
            mock_profile.as_str(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn oneharness");

    // Cancel only once the descendant has proven it is really running, so the
    // teardown assertion below is about a live tree rather than a spawn race.
    let alive_deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while std::fs::metadata(&ticks).map(|m| m.len()).unwrap_or(0) == 0 {
        assert!(
            std::time::Instant::now() < alive_deadline,
            "the silent harness never started its descendant"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    let signalled = std::time::Instant::now();
    let sent = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .expect("failed to signal oneharness");
    assert!(sent.success(), "kill -TERM did not succeed");

    let output = child.wait_with_output().expect("failed to reap oneharness");
    assert!(
        signalled.elapsed() < std::time::Duration::from_secs(15),
        "the signalled run did not tear down promptly: {:?}",
        signalled.elapsed()
    );
    assert_eq!(
        output.status.code(),
        Some(1),
        "oneharness did not exit through its own reporting path: {:?}",
        output.status
    );

    // The report is still the contract: a cancelled run is a value a consumer
    // reads, not a process that vanished.
    let value = json_stdout(&output);
    assert_eq!(value["schema_version"], "0.6");
    let result = &value["results"][0];
    assert_eq!(result["status"], "cancelled");
    assert_eq!(result["exit_code"], Value::Null);
    assert!(
        result["error"].as_str().unwrap().contains("cancelled"),
        "{}",
        result["error"]
    );

    let record = first_history_run(Path::new(value["history_file"].as_str().unwrap()));
    assert_eq!(record["status"], "cancelled");
    assert_eq!(record["schema_version"], "1.4");

    assert_native_descendant_stopped(&ticks);
    let _ = std::fs::remove_file(ticks);
    let _ = std::fs::remove_dir_all(history);
}

#[cfg(unix)]
#[test]
fn a_cancelled_batch_claims_no_invocation_for_its_queued_prompts() {
    let mock_profile = mock_profile_redirect();
    // The queued half of cancellation, end to end. A batch of three prompts runs
    // one at a time, so when the signal lands only the first prompt has ever been
    // invoked — the other two are still queued. They are still the caller's
    // prompts, so the report stays one entry per prompt and each queued entry is
    // `cancelled`; but never having run, they must claim no invocation bounds.
    // The harness is opencode because it declares a provider trace: on a
    // trace-capable harness a cancelled run *does* report the instant it began,
    // so `telemetry` here separates the prompt that ran from the ones that never
    // did — a queued entry reporting a start time would be measuring nothing.
    let history = hist_dir("cancel-queued");
    let history_arg = history.display().to_string();
    let log = batch_log_path("cancel-queued");

    let child = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        // Silent — no MOCK_STDOUT — and long-running, so the run has no line to
        // react to and only the cancel re-check can end it.
        .env("MOCK_SLEEP_MS", "60000")
        // Every invocation that gets as far as running appends `S` here.
        .env("MOCK_LOG_FILE", log.display().to_string())
        .args([
            "run",
            "--harness",
            "opencode",
            "--prompt",
            "first",
            "--prompt",
            "second",
            "--prompt",
            "third",
            "--bin",
            &bin_override("opencode"),
            // One at a time, so prompts two and three are unambiguously queued
            // behind the first when the signal arrives.
            "--max-parallel",
            "1",
            "--timeout",
            "60",
            "--history",
            "--history-dir",
            &history_arg,
            "--compact",
            "--env",
            mock_profile.as_str(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn oneharness");

    // Cancel only once the first prompt has really spawned, so the queued state
    // below is a fact about scheduling rather than a spawn race.
    let alive_deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while std::fs::read_to_string(&log).unwrap_or_default().is_empty() {
        assert!(
            std::time::Instant::now() < alive_deadline,
            "the batch never spawned its first prompt"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    let signalled = std::time::Instant::now();
    let sent = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .expect("failed to signal oneharness");
    assert!(sent.success(), "kill -TERM did not succeed");

    let output = child.wait_with_output().expect("failed to reap oneharness");
    assert!(
        signalled.elapsed() < std::time::Duration::from_secs(15),
        "the signalled batch did not tear down promptly: {:?}",
        signalled.elapsed()
    );
    assert_eq!(
        output.status.code(),
        Some(1),
        "oneharness did not exit through its own reporting path: {:?}",
        output.status
    );

    let value = json_stdout(&output);
    assert_eq!(value["batch"]["prompt_count"], 3);
    let results = value["results"].as_array().unwrap();
    assert_eq!(
        results.len(),
        3,
        "a cancelled batch still reports one entry per prompt: {results:?}"
    );
    assert_eq!(results[0]["prompt"], "first");
    assert_eq!(results[1]["prompt"], "second");
    assert_eq!(results[2]["prompt"], "third");
    for result in results {
        assert_eq!(result["status"], "cancelled", "{result}");
        assert!(
            result["error"].as_str().unwrap().contains("cancelled"),
            "{}",
            result["error"]
        );
    }

    // The prompt that ran was cut short mid-invocation, so the instant it began
    // is a real measurement the report keeps.
    assert_eq!(results[0]["telemetry"]["source"], "partial_invocation");
    assert!(results[0]["telemetry"]["started_at"].is_string());

    // The two queued prompts were never invoked: no exit code, no measured
    // duration, and — the contrast that matters — no telemetry claiming when a
    // run that never began started.
    for result in &results[1..] {
        assert_eq!(result["exit_code"], Value::Null, "{result}");
        assert_eq!(result["duration_ms"], 0, "{result}");
        assert_eq!(result["telemetry"], Value::Null, "{result}");
    }

    // And exactly one invocation ever reached the harness itself.
    let spawns = std::fs::read_to_string(&log).unwrap_or_default();
    assert_eq!(
        spawns.matches('S').count(),
        1,
        "more than one prompt reached the harness: {spawns:?}"
    );

    // History keeps the same one-record-per-prompt shape, cancelled included.
    let records = materialized_history(Path::new(value["history_file"].as_str().unwrap()));
    assert_eq!(records.len(), 3);
    for record in &records {
        assert_eq!(record["status"], "cancelled", "{record}");
    }

    let _ = std::fs::remove_file(log);
    let _ = std::fs::remove_dir_all(history);
}

#[cfg(unix)]
#[test]
fn a_cancelled_run_keeps_the_output_it_had_already_produced() {
    let mock_profile = mock_profile_redirect();
    // Cancelling is not a reason to throw away evidence. The harness emits a
    // complete transcript and then keeps its descendant alive, so the run is cut
    // short *after* it produced parseable output — which must still normalize
    // into text/usage/session/events, exactly as it does for a timeout.
    let history = hist_dir("cancel-normalized");
    let history_arg = history.display().to_string();
    let ticks = native_tick_file("cancel-normalized");
    let _ = std::fs::remove_file(&ticks);
    let transcript = concat!(
        "{\"type\":\"text\",\"sessionID\":\"ses-cancel\",\"part\":",
        "{\"type\":\"text\",\"text\":\"partial answer\"}}\n",
        "{\"type\":\"tool_use\",\"sessionID\":\"ses-cancel\",\"part\":",
        "{\"id\":\"call-cancel\",\"type\":\"tool\",\"tool\":\"bash\",\"state\":",
        "{\"input\":{\"command\":\"echo hi\"},\"output\":\"hi\"}}}\n",
        "{\"type\":\"step_finish\",\"sessionID\":\"ses-cancel\",\"part\":",
        "{\"cost\":0.01,\"tokens\":{\"input\":12,\"output\":3,",
        "\"cache\":{\"read\":9,\"write\":4}}}}\n",
    );

    let child = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        .env("MOCK_STDOUT", transcript)
        .env("MOCK_NATIVE_GRANDCHILD_MS", "20000")
        .env("MOCK_TICK_FILE", ticks.display().to_string())
        .args([
            "run",
            "--harness",
            "opencode",
            "--prompt",
            "cancel me after output",
            "--bin",
            &bin_override("opencode"),
            "--timeout",
            "60",
            "--history",
            "--history-dir",
            &history_arg,
            "--compact",
            "--env",
            mock_profile.as_str(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn oneharness");

    // The fixture writes its transcript before its first tick, so a tick proves
    // the output oneharness must preserve has already been produced.
    let alive_deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while std::fs::metadata(&ticks).map(|m| m.len()).unwrap_or(0) == 0 {
        assert!(
            std::time::Instant::now() < alive_deadline,
            "the harness never produced its transcript"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    assert!(Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .expect("failed to signal oneharness")
        .success());

    let output = child.wait_with_output().expect("failed to reap oneharness");
    let value = json_stdout(&output);
    let result = &value["results"][0];
    assert_eq!(result["status"], "cancelled");
    assert_eq!(result["text"], "partial answer");
    assert_eq!(result["text_source"], "json:opencode-parts");
    assert_eq!(result["usage"]["input_tokens"], 12);
    assert_eq!(result["usage"]["output_tokens"], 3);
    assert_eq!(result["usage"]["cache_read_tokens"], 9);
    assert_eq!(result["usage"]["cost_usd"], 0.01);
    assert_eq!(result["session_id"], "ses-cancel");
    assert_eq!(result["events"][0]["name"], "bash");

    // History freezes the same normalized evidence under the cancelled status.
    let record = first_history_run(Path::new(value["history_file"].as_str().unwrap()));
    assert_eq!(record["status"], "cancelled");
    assert_eq!(record["text"], "partial answer");
    assert_eq!(record["session_id"], "ses-cancel");
    assert_eq!(record["events"][0]["name"], "bash");

    assert_native_descendant_stopped(&ticks);
    let _ = std::fs::remove_file(ticks);
    let _ = std::fs::remove_dir_all(history);
}

#[cfg(unix)]
#[test]
fn a_cancelled_fallback_candidate_stops_the_chain() {
    let mock_profile = mock_profile_redirect();
    // Falling through a cancellation would spawn the very next harness the
    // cancellation was meant to prevent — so the chain stops, and `results`
    // holds only the candidate that was actually attempted.
    let ticks = native_tick_file("cancel-signal-fallback");
    let _ = std::fs::remove_file(&ticks);

    let child = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        .env("MOCK_NATIVE_GRANDCHILD_MS", "20000")
        .env("MOCK_TICK_FILE", ticks.display().to_string())
        .args([
            "run",
            "--run-mode",
            "fallback",
            "--harness",
            "claude-code",
            "--harness",
            "codex",
            "--prompt",
            "cancel me",
            "--bin",
            &bin_override("claude-code"),
            "--bin",
            &bin_override("codex"),
            "--timeout",
            "60",
            "--compact",
            "--env",
            mock_profile.as_str(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn oneharness");

    let alive_deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while std::fs::metadata(&ticks).map(|m| m.len()).unwrap_or(0) == 0 {
        assert!(
            std::time::Instant::now() < alive_deadline,
            "the silent harness never started its descendant"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    assert!(Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .expect("failed to signal oneharness")
        .success());

    let output = child.wait_with_output().expect("failed to reap oneharness");
    let value = json_stdout(&output);
    let results = value["results"].as_array().expect("results array");
    assert_eq!(
        results.len(),
        1,
        "the chain continued past a cancellation: {results:?}"
    );
    assert_eq!(results[0]["harness"], "claude-code");
    assert_eq!(results[0]["status"], "cancelled");
    assert!(
        value["fallback"]["fell_through"]
            .as_array()
            .expect("fell_through array")
            .is_empty(),
        "a cancellation is not a startup failure: {}",
        value["fallback"]
    );

    assert_native_descendant_stopped(&ticks);
    let _ = std::fs::remove_file(ticks);
}

/// A "harness" that ignores SIGTERM and stays alive, so oneharness's teardown
/// takes its full TERM→KILL grace rather than finishing in a few milliseconds.
/// `$OH_READY_FILE` gets a byte as soon as it is running. The inner `sleep` dies
/// with the process group; the loop is what keeps the script itself alive
/// through the grace.
#[cfg(unix)]
fn term_ignoring_harness(label: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = std::env::temp_dir().join(format!(
        "oneharness-{label}-{}-{:?}.sh",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(
        &path,
        "#!/bin/sh\ntrap '' TERM\nprintf x >> \"$OH_READY_FILE\"\ni=0\nwhile [ $i -lt 600 ]; do sleep 0.05; i=$((i + 1)); done\n",
    )
    .expect("failed to write the fixture harness");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        .expect("failed to make the fixture harness executable");
    path
}

#[cfg(unix)]
#[test]
fn a_second_host_signal_stops_waiting_for_teardown() {
    let mock_profile = mock_profile_redirect();
    // Teardown is bounded but not instant, and an operator pressing Ctrl-C twice
    // is asking to stop waiting for it: the second signal exits 130 straight from
    // the handler, without the report the first one would have produced.
    //
    // The harness ignores TERM on purpose. That is what makes the window this
    // test aims at deterministic: oneharness must spend its whole TERM→KILL
    // grace on the tree, so a follow-up signal reliably lands while the first
    // one's teardown is still in progress.
    let harness = term_ignoring_harness("cancel-signal-twice");
    let ready = native_tick_file("cancel-signal-twice");
    let _ = std::fs::remove_file(&ready);

    let child = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        .env("OH_READY_FILE", ready.display().to_string())
        .args([
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "cancel me twice",
            "--bin",
            &format!("claude-code={}", harness.display()),
            "--timeout",
            "60",
            "--compact",
            "--env",
            mock_profile.as_str(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn oneharness");

    let alive_deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while std::fs::metadata(&ready).map(|m| m.len()).unwrap_or(0) == 0 {
        assert!(
            std::time::Instant::now() < alive_deadline,
            "the fixture harness never started"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    // Signal across the teardown window rather than exactly twice: a standard
    // signal does not queue, so two sends the process has not been scheduled to
    // handle yet collapse into a single delivery. Repeating guarantees a second
    // *delivery* once the first handler has returned.
    let pid = child.id().to_string();
    let signal = || {
        Command::new("kill")
            .args(["-TERM", &pid])
            .status()
            .expect("failed to signal oneharness")
            .success()
    };
    assert!(signal(), "kill -TERM did not reach oneharness");
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        signal();
    }

    let output = child.wait_with_output().expect("failed to reap oneharness");
    assert_eq!(
        output.status.code(),
        Some(130),
        "a second signal must exit immediately: {:?}",
        output.status
    );
    let _ = std::fs::remove_file(ready);
    let _ = std::fs::remove_file(harness);
}

#[cfg(unix)]
#[test]
fn a_host_signal_cancels_a_streaming_run_and_still_terminates_the_stream() {
    let mock_profile = mock_profile_redirect();
    // Streaming is where a silent harness is hardest to stop: the loop reacts to
    // lines, and there are none. The consumer must still get its terminal
    // `result` envelope — a stream that simply stops is indistinguishable from a
    // stalled one.
    let ticks = native_tick_file("cancel-signal-stream");
    let _ = std::fs::remove_file(&ticks);

    let child = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        .env("MOCK_NATIVE_GRANDCHILD_MS", "20000")
        .env("MOCK_TICK_FILE", ticks.display().to_string())
        .args([
            "run",
            "--harness",
            "opencode",
            "--prompt",
            "cancel me",
            "--bin",
            &bin_override("opencode"),
            "--timeout",
            "60",
            "--stream",
            "--env",
            mock_profile.as_str(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn oneharness");

    let alive_deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while std::fs::metadata(&ticks).map(|m| m.len()).unwrap_or(0) == 0 {
        assert!(
            std::time::Instant::now() < alive_deadline,
            "the silent harness never started its descendant"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    let signalled = std::time::Instant::now();
    assert!(Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .expect("failed to signal oneharness")
        .success());

    let output = child.wait_with_output().expect("failed to reap oneharness");
    assert!(
        signalled.elapsed() < std::time::Duration::from_secs(15),
        "the signalled stream did not tear down promptly: {:?}",
        signalled.elapsed()
    );
    assert_eq!(output.status.code(), Some(1));

    let last = String::from_utf8_lossy(&output.stdout)
        .lines()
        .last()
        .expect("the stream ended without any line")
        .to_string();
    match serde_json::from_str::<RunStreamEnvelope>(&last).expect("terminal envelope is typed") {
        RunStreamEnvelope::Result { report } => {
            assert_eq!(report.results[0].status, Status::Cancelled);
        }
        RunStreamEnvelope::Event { .. } => panic!("the stream never reached its terminal result"),
    }

    assert_native_descendant_stopped(&ticks);
    let _ = std::fs::remove_file(ticks);
}

#[cfg(windows)]
#[test]
fn windows_streaming_stop_terminates_native_descendant() {
    use oneharness_core::domain::report::Status;
    use oneharness_core::io::runner::{run_job_streaming, Job, StreamStep};

    let ticks = native_tick_file("windows-stream");
    let _ = std::fs::remove_file(&ticks);
    let job = Job {
        argv: vec![mock_bin().display().to_string()],
        cwd: None,
        env: vec![
            ("MOCK_NATIVE_GRANDCHILD_MS".to_string(), "10000".to_string()),
            ("MOCK_TICK_FILE".to_string(), ticks.display().to_string()),
            (
                "MOCK_STDOUT".to_string(),
                "native stream marker\n".to_string(),
            ),
            (
                "MOCK_STDERR".to_string(),
                "native stream stderr\n".to_string(),
            ),
        ],
        env_remove: Vec::new(),
        timeout: std::time::Duration::from_secs(10),
        stdin: None,
    };

    let started = std::time::Instant::now();
    let mut observed = Vec::new();
    let capture = run_job_streaming(&job, |line| {
        observed.push(line.to_string());
        StreamStep::Stop
    });

    assert_eq!(capture.status, Status::Ok);
    assert_eq!(observed, ["native stream marker"]);
    assert!(capture.stdout.contains("native stream marker"));
    assert!(capture.stderr.contains("native stream stderr"));
    assert!(
        started.elapsed() < std::time::Duration::from_secs(3),
        "streaming short-circuit teardown took {:?}",
        started.elapsed()
    );
    assert_native_descendant_stopped(&ticks);
    let _ = std::fs::remove_file(ticks);
}

#[test]
fn missing_binary_is_skipped_not_failed() {
    let args = [
        "run",
        "--harness",
        "codex",
        "--prompt",
        "hi",
        "--bin",
        "codex=/no/such/oneharness-binary-xyz",
        "--compact",
    ];
    let output = run(&args, &[]);
    assert_eq!(output.status.code(), Some(0));
    let value = json_stdout(&output);
    let result = &value["results"][0];
    assert_eq!(result["status"], "skipped");
    assert_eq!(result["available"], false);

    // Same selection becomes a failure under --require-available.
    let mut strict: Vec<&str> = args.to_vec();
    strict.push("--require-available");
    let output = run(&strict, &[]);
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn runs_all_harnesses_in_parallel_via_env_overrides() {
    let mock = mock_bin();
    let mock = mock.display().to_string();
    let envs: Vec<(&str, &str)> = vec![
        ("ONEHARNESS_BIN_CLAUDE_CODE", mock.as_str()),
        ("ONEHARNESS_BIN_CODEX", mock.as_str()),
        ("ONEHARNESS_BIN_OPENCODE", mock.as_str()),
        ("ONEHARNESS_BIN_GOOSE", mock.as_str()),
        ("ONEHARNESS_BIN_QWEN", mock.as_str()),
        ("ONEHARNESS_BIN_CRUSH", mock.as_str()),
        ("ONEHARNESS_BIN_COPILOT", mock.as_str()),
        ("ONEHARNESS_BIN_CURSOR", mock.as_str()),
    ];
    let output = run(&["run", "--all", "--prompt", "hi", "--compact"], &envs);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = json_stdout(&output);
    let results = value["results"].as_array().unwrap();
    assert_eq!(results.len(), ALL_IDS.len());
    for result in results {
        assert_eq!(
            result["status"], "ok",
            "harness {} not ok",
            result["harness"]
        );
        assert_eq!(result["available"], true);
    }
}

#[test]
fn built_argv_actually_reaches_the_binary() {
    let argv_file = std::env::temp_dir().join(format!(
        "oneharness-argv-{}-{}.txt",
        std::process::id(),
        "claude"
    ));
    let _ = std::fs::remove_file(&argv_file);

    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "unique-prompt-marker",
            "--mode",
            "bypass",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[("MOCK_ARGV_FILE", &argv_file.display().to_string())],
    );
    assert!(output.status.success());

    let received = std::fs::read_to_string(&argv_file).expect("mock did not record argv");
    let _ = std::fs::remove_file(&argv_file);
    let argv: Vec<&str> = received.lines().collect();
    assert!(argv.contains(&"-p"), "argv: {argv:?}");
    assert!(argv.contains(&"unique-prompt-marker"), "argv: {argv:?}");
    assert!(argv.contains(&"bypassPermissions"), "argv: {argv:?}");
}

#[cfg(windows)]
#[test]
fn cmd_shim_spawns_with_a_multiline_argument() {
    // Regression (Windows): oneharness resolves an npm harness to a `claude.cmd`
    // shim, and since Rust 1.77 std refuses to spawn a `.cmd` with a multi-line
    // argument ("batch file arguments are invalid"). oneharness must bypass the
    // shim and invoke its interpreter directly. Stand in a real npm-style `.cmd`
    // shim whose `_prog` is the mock harness exe, drive a multi-line `--system`
    // through it, and assert it spawned cleanly AND the multi-line value reached
    // the child intact. The hermetic suite's other harnesses are real `.exe`s, so
    // only this `.cmd`-shim path exercises the rewrite end to end.
    let dir = std::env::temp_dir().join(format!("oneharness-cmdshim-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let cmd_path = dir.join("claude.cmd");
    // npm-style shim: name the interpreter via `_prog`, invoke it with a `%dp0%`
    // script ahead of the forwarded `%*`. Here `_prog` is the mock exe and the
    // script is an arg it ignores, so a successful run proves the bypass spawned.
    let shim = format!(
        "SET \"_prog={}\"\r\n\"%_prog%\" \"%dp0%\\ignored.js\" %*\r\n",
        mock_bin().display()
    );
    std::fs::write(&cmd_path, shim).unwrap();

    let argv_file = dir.join("argv.txt");
    let system = "line-a\nline-b\nline-c";
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--system",
            system,
            "--bin",
            &format!("claude-code={}", cmd_path.display()),
            "--compact",
        ],
        &[("MOCK_ARGV_FILE", &argv_file.display().to_string())],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = json_stdout(&output);
    assert_eq!(
        value["results"][0]["status"], "ok",
        "the `.cmd` shim should spawn despite the multi-line arg: {value}"
    );

    let received =
        std::fs::read_to_string(&argv_file).expect("mock recorded no argv — the spawn failed");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        received.contains("--append-system-prompt"),
        "argv: {received:?}"
    );
    assert!(
        received.contains(system),
        "the multi-line --system value must reach the child intact: {received:?}"
    );
}

#[cfg(windows)]
#[test]
fn native_exe_cmd_shim_spawns_with_a_multiline_argument() {
    // The real claude-code shape: its npm bin is `bin/claude.exe`, so the
    // `claude.cmd` shim forwards straight to that exe — no `_prog`, no script,
    // an *empty* prefix. This is the layout that defeated the first fix attempt.
    // Stand in a `%dp0%`-rooted exe shim pointing at a colocated copy of the mock
    // harness, drive a multi-line `--system`, and assert it spawns and the value
    // arrives intact — the end-to-end proof of the native-exe rewrite branch.
    let dir = std::env::temp_dir().join(format!("oneharness-exeshim-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Colocate the target exe beside the shim so `%dp0%\claude.exe` resolves.
    let exe_path = dir.join("claude.exe");
    std::fs::copy(mock_bin(), &exe_path).unwrap();
    let cmd_path = dir.join("claude.cmd");
    std::fs::write(
        &cmd_path,
        "@ECHO off\r\nSET dp0=%~dp0\r\n\"%dp0%\\claude.exe\"   %*\r\n",
    )
    .unwrap();

    let argv_file = dir.join("argv.txt");
    let system = "alpha\nbeta\ngamma";
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--system",
            system,
            "--bin",
            &format!("claude-code={}", cmd_path.display()),
            "--compact",
        ],
        &[("MOCK_ARGV_FILE", &argv_file.display().to_string())],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = json_stdout(&output);
    assert_eq!(
        value["results"][0]["status"], "ok",
        "the native-exe `.cmd` shim should spawn despite the multi-line arg: {value}"
    );

    let received =
        std::fs::read_to_string(&argv_file).expect("mock recorded no argv — the spawn failed");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        received.contains(system),
        "the multi-line --system value must reach the child intact: {received:?}"
    );
}

#[test]
fn project_config_supplies_selection_model_and_is_reported() {
    let fx = ConfigFixture::new(
        "project",
        "harnesses = [\"claude-code\"]\nmodel = \"cfg-model\"\n",
        "",
    );
    // No --harness/--all and no --model: both come from the project file.
    let output = run_with_config(
        &[
            "run",
            "--prompt",
            "hi",
            "--cwd",
            &fx.cwd(),
            "--print-command",
            "--compact",
        ],
        &[],
        &fx.user_config(),
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = json_stdout(&output);
    assert_eq!(value["model"], "cfg-model");
    let results = value["results"].as_array().unwrap();
    assert_eq!(results.len(), 1, "config selection should pick one harness");
    assert_eq!(results[0]["harness"], "claude-code");
    let command = command_of(&value, 0);
    assert!(
        command.windows(2).any(|w| w == ["--model", "cfg-model"]),
        "{command:?}"
    );
    // The report records which files shaped the run (project file last).
    let files = value["config_files"].as_array().unwrap();
    assert!(
        files
            .last()
            .unwrap()
            .as_str()
            .unwrap()
            .ends_with("oneharness.toml"),
        "{files:?}"
    );
}

#[test]
fn user_config_applies_and_project_file_wins_per_field() {
    let fx = ConfigFixture::new(
        "layering",
        "model = \"project-model\"\n",
        "model = \"user-model\"\nsystem = \"from user\"\n",
    );
    let output = run_with_config(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--cwd",
            &fx.cwd(),
            "--print-command",
            "--compact",
        ],
        &[],
        &fx.user_config(),
    );
    let value = json_stdout(&output);
    let command = command_of(&value, 0);
    // The project file's model wins; the user file's system still applies.
    assert!(
        command
            .windows(2)
            .any(|w| w == ["--model", "project-model"]),
        "{command:?}"
    );
    assert!(
        command
            .windows(2)
            .any(|w| w == ["--append-system-prompt", "from user"]),
        "{command:?}"
    );
}

#[test]
fn cli_flags_beat_config() {
    let fx = ConfigFixture::new(
        "cli-wins",
        "model = \"cfg-model\"\nharnesses = [\"codex\"]\n",
        "",
    );
    let output = run_with_config(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--model",
            "cli-model",
            "--cwd",
            &fx.cwd(),
            "--print-command",
            "--compact",
        ],
        &[],
        &fx.user_config(),
    );
    let value = json_stdout(&output);
    let results = value["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["harness"], "claude-code");
    let command = command_of(&value, 0);
    assert!(
        command.windows(2).any(|w| w == ["--model", "cli-model"]),
        "{command:?}"
    );
}

#[test]
fn per_harness_config_beats_top_level_and_appends_args() {
    let fx = ConfigFixture::new(
        "per-harness",
        concat!(
            "model = \"global-model\"\n",
            "[harness.claude-code]\n",
            "model = \"claude-model\"\n",
            "args = [\"--max-turns\", \"6\"]\n",
        ),
        "",
    );
    let output = run_with_config(
        &[
            "run",
            "--harness",
            "claude-code,codex",
            "--prompt",
            "hi",
            "--cwd",
            &fx.cwd(),
            "--print-command",
            "--compact",
        ],
        &[],
        &fx.user_config(),
    );
    let value = json_stdout(&output);
    let claude = command_of(&value, 0);
    let codex = command_of(&value, 1);
    assert!(
        claude.windows(2).any(|w| w == ["--model", "claude-model"]),
        "{claude:?}"
    );
    // The per-harness args land on claude only, after the built argv.
    assert_eq!(&claude[claude.len() - 2..], ["--max-turns", "6"]);
    assert!(
        codex.windows(2).any(|w| w == ["--model", "global-model"]),
        "{codex:?}"
    );
    assert!(!codex.iter().any(|t| t == "--max-turns"), "{codex:?}");
}

#[test]
fn config_bypass_false_applies_and_cli_bypass_reenables() {
    let fx = ConfigFixture::new("bypass", "bypass = false\n", "");
    let base = [
        "run",
        "--harness",
        "claude-code",
        "--prompt",
        "hi",
        "--print-command",
        "--compact",
        "--cwd",
    ];
    let mut args: Vec<&str> = base.to_vec();
    let cwd = fx.cwd();
    args.push(&cwd);
    let output = run_with_config(&args, &[], &fx.user_config());
    let value = json_stdout(&output);
    assert_eq!(value["bypass_permissions"], false);
    let command = command_of(&value, 0);
    assert!(
        command
            .windows(2)
            .any(|w| w == ["--permission-mode", "dontAsk"]),
        "{command:?}"
    );

    // --bypass overrides the config back on.
    args.push("--bypass");
    let output = run_with_config(&args, &[], &fx.user_config());
    let value = json_stdout(&output);
    assert_eq!(value["bypass_permissions"], true);
}

#[test]
fn config_env_reaches_the_child_and_explicit_env_wins() {
    let fx = ConfigFixture::new(
        "env",
        concat!(
            "[env]\n",
            "ONEHARNESS_TEST_VAR = \"from-config\"\n",
            "[harness.claude-code.env]\n",
            "ONEHARNESS_TEST_VAR = \"from-harness-config\"\n",
        ),
        "",
    );
    let bin = bin_override("claude-code");
    let base = [
        "run",
        "--harness",
        "claude-code",
        "--prompt",
        "hi",
        "--bin",
        &bin,
        "--compact",
        "--cwd",
    ];
    let mut args: Vec<&str> = base.to_vec();
    let cwd = fx.cwd();
    args.push(&cwd);
    let output = run_with_config(
        &args,
        &[("MOCK_ECHO_ENV", "ONEHARNESS_TEST_VAR")],
        &fx.user_config(),
    );
    let value = json_stdout(&output);
    // The per-harness [harness.<id>.env] beats the top-level [env]...
    assert_eq!(
        value["results"][0]["stdout"],
        "ONEHARNESS_TEST_VAR=from-harness-config"
    );

    // ...and an explicit --env beats both.
    args.push("--env");
    args.push("ONEHARNESS_TEST_VAR=from-cli");
    let output = run_with_config(
        &args,
        &[("MOCK_ECHO_ENV", "ONEHARNESS_TEST_VAR")],
        &fx.user_config(),
    );
    let value = json_stdout(&output);
    assert_eq!(
        value["results"][0]["stdout"],
        "ONEHARNESS_TEST_VAR=from-cli"
    );
}

#[test]
fn variants_spawn_concurrently_with_isolated_environment_and_identity() {
    let bin = mock_bin().display().to_string().replace('\\', "\\\\");
    let fx = ConfigFixture::new(
        "variant-env",
        &format!(
            r#"
harnesses = ["claude-code:masked", "claude-code:file", "claude-code:sourced"]
max_parallel = 2
[harness.claude-code.variant.masked]
bin = "{bin}"
model = "sonnet"
args = ["--variant-arg"]
unset_env = ["ANTHROPIC_API_KEY"]
[harness.claude-code.variant.masked.env]
VARIANT_LABEL = "masked"
[harness.claude-code.variant.sourced]
bin = "{bin}"
[harness.claude-code.variant.sourced.env_from]
ANTHROPIC_API_KEY = "ANTHROPIC_API_KEY_WORK"
[harness.claude-code.variant.file]
bin = "{bin}"
env_file = "variant.env"
"#
        ),
        "",
    );
    let variant_env = std::path::Path::new(&fx.cwd()).join("variant.env");
    std::fs::write(&variant_env, "ANTHROPIC_API_KEY=file-only\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&variant_env, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let history_dir = std::path::Path::new(&fx.cwd()).join("history");
    let output = run_with_config(
        &[
            "run",
            "--prompt",
            "hi",
            "--history",
            "--history-dir",
            &history_dir.display().to_string(),
            "--cwd",
            &fx.cwd(),
            "--compact",
        ],
        &[
            ("MOCK_ECHO_ENV", "ANTHROPIC_API_KEY"),
            ("ANTHROPIC_API_KEY", "ambient-must-not-leak"),
            ("ANTHROPIC_API_KEY_WORK", "work-only"),
        ],
        &fx.user_config(),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = json_stdout(&output);
    assert_eq!(value["results"][0]["harness"], "claude-code");
    assert_eq!(value["results"][0]["variant"], "masked");
    assert_eq!(value["results"][0]["harness_id"], "claude-code:masked");
    assert_eq!(value["results"][0]["stdout"], "ANTHROPIC_API_KEY=");
    assert_eq!(value["results"][1]["variant"], "file");
    assert_eq!(value["results"][1]["stdout"], "ANTHROPIC_API_KEY=file-only");
    assert_eq!(value["results"][2]["variant"], "sourced");
    assert_eq!(value["results"][2]["stdout"], "ANTHROPIC_API_KEY=work-only");
    let records: Vec<Value> = std::fs::read_to_string(value["history_file"].as_str().unwrap())
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .filter(|line: &Value| line["type"] == "run")
        .collect();
    assert_eq!(records[0]["variant"], "masked");
    assert_eq!(records[1]["harness_id"], "claude-code:file");
    let config = json_stdout(&run_with_config(
        &["config", "--cwd", &fx.cwd(), "--compact"],
        &[],
        &fx.user_config(),
    ));
    assert_eq!(
        config["harness"]["claude-code"]["variant"]["file"]["env_file"]["value"],
        "variant.env"
    );
    let masked_config = &config["harness"]["claude-code"]["variant"]["masked"];
    assert_eq!(masked_config["model"]["value"], "sonnet");
    assert_eq!(masked_config["args"]["value"][0], "--variant-arg");
    assert_eq!(masked_config["env"]["VARIANT_LABEL"]["value"], "masked");
    assert_eq!(masked_config["unset_env"]["value"][0], "ANTHROPIC_API_KEY");
    assert!(masked_config["model"]["source"]
        .as_str()
        .unwrap()
        .ends_with("oneharness.toml"));
    assert_eq!(
        config["harness"]["claude-code"]["variant"]["sourced"]["env_from"]["ANTHROPIC_API_KEY"]
            ["value"],
        "ANTHROPIC_API_KEY_WORK"
    );
    let listed = json_stdout(&run_with_config(
        &["list", "--compact"],
        &[],
        &std::path::Path::new(&fx.cwd()).join("oneharness.toml"),
    ));
    let claude = listed["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == "claude-code")
        .unwrap();
    assert_eq!(claude["variants"].as_array().unwrap().len(), 3);
    let masked = claude["variants"]
        .as_array()
        .unwrap()
        .iter()
        .find(|variant| variant["name"] == "masked")
        .unwrap();
    assert_eq!(masked["harness_id"], "claude-code:masked");
    assert_eq!(masked["model"], "sonnet");
    assert_eq!(masked["bin"], mock_bin().display().to_string());
    assert_eq!(masked["args"][0], "--variant-arg");
    assert_eq!(masked["env_keys"][0], "VARIANT_LABEL");
    assert_eq!(masked["unset_env"][0], "ANTHROPIC_API_KEY");
    let sourced = claude["variants"]
        .as_array()
        .unwrap()
        .iter()
        .find(|variant| variant["name"] == "sourced")
        .unwrap();
    assert_eq!(
        sourced["env_from"]["ANTHROPIC_API_KEY"],
        "ANTHROPIC_API_KEY_WORK"
    );
    let file = claude["variants"]
        .as_array()
        .unwrap()
        .iter()
        .find(|variant| variant["name"] == "file")
        .unwrap();
    assert_eq!(file["env_file"], "variant.env");

    let history = json_stdout(&run_with_config(
        &[
            "history",
            "list",
            "--all-projects",
            "--history-dir",
            &history_dir.display().to_string(),
            "--variant",
            "file",
            "--compact",
        ],
        &[],
        &fx.user_config(),
    ));
    assert_eq!(history.as_array().unwrap().len(), 1);
    assert_eq!(history[0]["harnesses"][1], "claude-code:file");

    let detected = json_stdout(&run_with_config(
        &["detect", "--harness", "claude-code:file", "--compact"],
        &[("MOCK_STDOUT", "mock-harness variant")],
        &std::path::Path::new(&fx.cwd()).join("oneharness.toml"),
    ));
    assert_eq!(detected["detected"][0]["id"], "claude-code:file");
    assert_eq!(detected["detected"][0]["available"], true);
}

#[test]
fn variant_masking_wins_over_explicit_cli_env_at_spawn() {
    let bin = mock_bin().display().to_string().replace('\\', "\\\\");
    let fx = ConfigFixture::new(
        "variant-mask-cli-env",
        &format!(
            r#"
[harness.claude-code.variant.subscription]
bin = "{bin}"
unset_env = ["ANTHROPIC_API_KEY"]
"#
        ),
        "",
    );
    let output = run_with_config(
        &[
            "run",
            "--harness",
            "claude-code:subscription",
            "--prompt",
            "hi",
            "--env",
            "ANTHROPIC_API_KEY=cli-must-be-masked",
            "--cwd",
            &fx.cwd(),
            "--compact",
        ],
        &[("MOCK_ECHO_ENV", "ANTHROPIC_API_KEY")],
        &fx.user_config(),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        json_stdout(&output)["results"][0]["stdout"],
        "ANTHROPIC_API_KEY="
    );
}

#[test]
fn variant_absolute_env_file_reaches_the_spawned_process() {
    let bin = mock_bin().display().to_string().replace('\\', "\\\\");
    let fx = ConfigFixture::new("variant-absolute-env-file", "", "");
    let env_file = std::path::Path::new(&fx.cwd()).join("absolute.env");
    std::fs::write(&env_file, "VARIANT_ABSOLUTE=from-absolute-file\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&env_file, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let escaped_env_file = env_file.display().to_string().replace('\\', "\\\\");
    std::fs::write(
        std::path::Path::new(&fx.cwd()).join("oneharness.toml"),
        format!(
            r#"
[harness.claude-code.variant.absolute]
bin = "{bin}"
env_file = "{escaped_env_file}"
"#
        ),
    )
    .unwrap();

    let output = run_with_config(
        &[
            "run",
            "--harness",
            "claude-code:absolute",
            "--prompt",
            "hi",
            "--cwd",
            &fx.cwd(),
            "--compact",
        ],
        &[("MOCK_ECHO_ENV", "VARIANT_ABSOLUTE")],
        &fx.user_config(),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        json_stdout(&output)["results"][0]["stdout"],
        "VARIANT_ABSOLUTE=from-absolute-file"
    );
}

#[test]
fn variant_identity_is_reported_by_print_command() {
    let fx = ConfigFixture::new(
        "variant-print-command",
        "[harness.claude-code.variant.work]\nmodel = \"sonnet\"\n",
        "",
    );
    let output = run_with_config(
        &[
            "run",
            "--harness",
            "claude-code:work",
            "--prompt",
            "hi",
            "--print-command",
            "--cwd",
            &fx.cwd(),
            "--compact",
        ],
        &[],
        &fx.user_config(),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result = json_stdout(&output)["results"][0].clone();
    assert_eq!(result["harness"], "claude-code");
    assert_eq!(result["variant"], "work");
    assert_eq!(result["harness_id"], "claude-code:work");
    assert_eq!(result["model"], "sonnet");
    assert_eq!(result["status"], "planned");
}

#[test]
fn unknown_and_malformed_variants_are_usage_errors() {
    let fx = ConfigFixture::new(
        "variant-errors",
        "[harness.claude-code.variant.work]\nmodel = \"sonnet\"\n",
        "",
    );
    for id in ["claude-code:missing", "claude-code:-bad"] {
        let output = run_with_config(
            &["run", "--harness", id, "--prompt", "hi", "--cwd", &fx.cwd()],
            &[],
            &fx.user_config(),
        );
        assert_eq!(output.status.code(), Some(2), "{id}");
    }
    std::fs::write(
        std::path::Path::new(&fx.cwd()).join("oneharness.toml"),
        "[harness.claude-code.variant.\"bad.name\"]\nmodel = \"x\"\n",
    )
    .unwrap();
    let malformed_config = run_with_config(&["config", "--cwd", &fx.cwd()], &[], &fx.user_config());
    assert_eq!(malformed_config.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&malformed_config.stderr)
        .contains("invalid harness variant name `bad.name`"));
}

#[test]
fn detect_applies_a_composed_id_cli_bin_override() {
    let bin = mock_bin().display().to_string();
    let fx = ConfigFixture::new(
        "variant-detect-cli-bin",
        "[harness.claude-code.variant.work]\nmodel = \"sonnet\"\n",
        "",
    );
    let project_config = std::path::Path::new(&fx.cwd())
        .join("oneharness.toml")
        .display()
        .to_string();
    let output = run_with_config(
        &[
            "detect",
            "--config",
            &project_config,
            "--harness",
            "claude-code:work",
            "--bin",
            &format!("claude-code:work={bin}"),
        ],
        &[],
        &fx.user_config(),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let detected = &json_stdout(&output)["detected"][0];
    assert_eq!(detected["id"], "claude-code:work");
    assert_eq!(detected["bin"], bin);
    assert_eq!(detected["available"], true);
}

#[test]
fn hook_harness_filters_reject_variant_selectors_at_the_cli_boundary() {
    let fx = ConfigFixture::new(
        "hook-variant-filter",
        "[[hooks]]\ncommand = \"oneharness gate {harness}\"\nharnesses = [\"claude-code:-bad\"]\n",
        "",
    );
    let output = run_with_config(&["config", "--cwd", &fx.cwd()], &[], &fx.user_config());
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("accepts base harness ids"), "{stderr}");
    assert!(stderr.contains("claude-code:-bad"), "{stderr}");
}

#[test]
fn variant_external_source_errors_are_loud_at_cli_boundaries() {
    let bin = mock_bin().display().to_string().replace('\\', "\\\\");
    let fx = ConfigFixture::new(
        "variant-source-errors",
        &format!(
            r#"
[harness.claude-code.variant.work]
bin = "{bin}"
[harness.claude-code.variant.missing-file]
bin = "{bin}"
env_file = "absent.env"
[harness.claude-code.variant.bad-line]
bin = "{bin}"
env_file = "bad.env"
[harness.claude-code.variant.bad-name]
bin = "{bin}"
env_file = "bad-name.env"
[harness.claude-code.variant.nul-value]
bin = "{bin}"
env_file = "nul-value.env"
[harness.claude-code.variant.insecure]
bin = "{bin}"
env_file = "insecure.env"
[harness.claude-code.variant.missing-parent]
bin = "{bin}"
[harness.claude-code.variant.missing-parent.env_from]
ANTHROPIC_API_KEY = "ONEHARNESS_TEST_PARENT_IS_ABSENT"
"#
        ),
        "",
    );
    let bad_env = std::path::Path::new(&fx.cwd()).join("bad.env");
    std::fs::write(&bad_env, "not-key-value\n").unwrap();
    let bad_name_env = std::path::Path::new(&fx.cwd()).join("bad-name.env");
    std::fs::write(&bad_name_env, "BAD-NAME=value\n").unwrap();
    let nul_value_env = std::path::Path::new(&fx.cwd()).join("nul-value.env");
    std::fs::write(&nul_value_env, b"VALID_NAME=before\0after\n").unwrap();
    let insecure_env = std::path::Path::new(&fx.cwd()).join("insecure.env");
    std::fs::write(&insecure_env, "ANTHROPIC_API_KEY=not-a-real-key\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bad_env, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::set_permissions(&bad_name_env, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::set_permissions(&nul_value_env, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::set_permissions(&insecure_env, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
    #[cfg(unix)]
    {
        let output = run_with_config(
            &[
                "run",
                "--harness",
                "claude-code:insecure",
                "--prompt",
                "hi",
                "--cwd",
                &fx.cwd(),
            ],
            &[],
            &fx.user_config(),
        );
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stderr).contains("mode 0600 or stricter"));
    }
    for (variant, expected) in [
        ("missing-file", "could not read variant environment file"),
        ("bad-line", "expected KEY=VALUE"),
        ("bad-name", "expected KEY=VALUE"),
        ("nul-value", "expected KEY=VALUE"),
        ("missing-parent", "is not set in the parent process"),
    ] {
        let output = run_with_config(
            &[
                "run",
                "--harness",
                &format!("claude-code:{variant}"),
                "--prompt",
                "hi",
                "--cwd",
                &fx.cwd(),
            ],
            &[],
            &fx.user_config(),
        );
        assert_eq!(output.status.code(), Some(2), "{variant}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected),
            "{variant}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let project_config = std::path::Path::new(&fx.cwd()).join("oneharness.toml");
    let detect = run_with_config(
        &["detect", "--harness", "claude-code:unknown"],
        &[],
        &project_config,
    );
    assert_eq!(detect.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&detect.stderr).contains("unknown harness variant"));

    let detect_exclude = run_with_config(
        &["detect", "--all", "--exclude", "claude-code:unknown"],
        &[],
        &project_config,
    );
    assert_eq!(detect_exclude.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&detect_exclude.stderr).contains("unknown harness variant"));

    let sync = run_with_config(
        &[
            "sync",
            "--harness",
            "claude-code:unknown",
            "--cwd",
            &fx.cwd(),
        ],
        &[],
        &fx.user_config(),
    );
    assert_eq!(sync.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&sync.stderr).contains("unknown harness variant"));
}

#[test]
fn fallback_treats_same_harness_variants_as_distinct_auth_candidates() {
    let bin = mock_bin().display().to_string().replace('\\', "\\\\");
    let fx = ConfigFixture::new(
        "variant-fallback",
        &format!(
            r#"
harnesses = ["claude-code:bad", "claude-code:good"]
run_mode = "fallback"
[harness.claude-code.variant.bad]
bin = "{bin}"
[harness.claude-code.variant.bad.env]
MOCK_STDERR = "401 Unauthorized: invalid API key"
MOCK_EXIT = "1"
[harness.claude-code.variant.good]
bin = "{bin}"
[harness.claude-code.variant.good.env]
MOCK_STDOUT = "{{\"type\":\"result\",\"result\":\"served-by-good\"}}"
"#
        ),
        "",
    );
    let output = run_with_config(
        &["run", "--prompt", "hi", "--cwd", &fx.cwd(), "--compact"],
        &[],
        &fx.user_config(),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = json_stdout(&output);
    assert_eq!(value["results"][0]["harness_id"], "claude-code:bad");
    assert_eq!(value["results"][0]["failure_kind"], "auth");
    assert_eq!(value["results"][1]["harness_id"], "claude-code:good");
    assert_eq!(value["results"][1]["text"], "served-by-good");
    assert_eq!(value["fallback"]["ran"], "claude-code:good");
    assert_eq!(
        value["fallback"]["fell_through"][0]["harness"],
        "claude-code:bad"
    );
}

#[test]
fn an_absent_env_from_home_falls_through_as_auth_like_an_empty_one() {
    // A variant's `env_from` points a child at one account's home directory. A
    // directory that is not on disk holds no credentials — the same "not set up
    // yet" state an EMPTY one is in, which the harness itself already reports as
    // `auth`. Both must therefore fall through a chain: the absent one is
    // classified without spawning (so no config directory is created for an
    // account nobody has logged into), the empty one still runs and is judged by
    // what the harness says.
    let bin = mock_bin().display().to_string().replace('\\', "\\\\");
    let fx = ConfigFixture::new(
        "variant-absent-home",
        &format!(
            r#"
harnesses = ["claude-code:absent", "claude-code:empty"]
run_mode = "fallback"
[harness.claude-code.variant.absent]
bin = "{bin}"
[harness.claude-code.variant.absent.env_from]
CLAUDE_CONFIG_DIR = "OH_TEST_ABSENT_HOME"
[harness.claude-code.variant.empty]
bin = "{bin}"
[harness.claude-code.variant.empty.env_from]
CLAUDE_CONFIG_DIR = "OH_TEST_EMPTY_HOME"
"#
        ),
        "",
    );
    let empty_home = std::path::Path::new(&fx.cwd()).join("empty-home");
    std::fs::create_dir_all(&empty_home).unwrap();
    let absent_home = std::path::Path::new(&fx.cwd()).join("absent-home");
    let absent = absent_home.display().to_string();
    let empty = empty_home.display().to_string();
    let envs: Vec<(&str, &str)> = vec![
        ("OH_TEST_ABSENT_HOME", absent.as_str()),
        ("OH_TEST_EMPTY_HOME", empty.as_str()),
        (
            "MOCK_STDOUT",
            r#"{"type":"result","result":"served-by-empty"}"#,
        ),
    ];
    let output = run_with_config(
        &["run", "--prompt", "hi", "--cwd", &fx.cwd(), "--compact"],
        &envs,
        &fx.user_config(),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = json_stdout(&output);
    let refused = &value["results"][0];
    assert_eq!(refused["harness_id"], "claude-code:absent");
    assert_eq!(refused["status"], "skipped");
    // Installed — this is a credential problem, not a missing binary.
    assert_eq!(refused["available"], true);
    assert_eq!(refused["failure_kind"], "auth");
    assert_eq!(refused["failure_kind_source"], "config:env_from");
    assert_eq!(refused["command"][0], mock_bin().display().to_string());
    // Never spawned: the mock's stdout would have surfaced as `text`.
    assert!(refused["text"].is_null(), "{refused}");
    let error = refused["error"].as_str().unwrap();
    for needle in ["CLAUDE_CONFIG_DIR", "OH_TEST_ABSENT_HOME", &absent] {
        assert!(error.contains(needle), "{error}");
    }
    assert!(!absent_home.exists(), "the absent home was created");
    // The empty home is left to the harness, and the chain advances past the
    // absent candidate to it.
    assert_eq!(value["results"][1]["harness_id"], "claude-code:empty");
    assert_eq!(value["results"][1]["text"], "served-by-empty");
    assert_eq!(value["fallback"]["ran"], "claude-code:empty");
    assert_eq!(
        value["fallback"]["fell_through"][0]["harness"],
        "claude-code:absent"
    );
    assert_eq!(value["fallback"]["fell_through"][0]["reason"], "auth");

    // A streamed chain selects the same candidate a buffered one did: the
    // unprovisioned candidate publishes nothing and the chain advances past it.
    let streamed = run_with_config(
        &["run", "--prompt", "hi", "--cwd", &fx.cwd(), "--stream"],
        &envs,
        &fx.user_config(),
    );
    assert!(
        streamed.status.success(),
        "{}",
        String::from_utf8_lossy(&streamed.stderr)
    );
    let envelopes: Vec<RunStreamEnvelope> = String::from_utf8_lossy(&streamed.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("each stream line matches the contract"))
        .collect();
    match envelopes.last().expect("a terminal result line") {
        RunStreamEnvelope::Result { report } => {
            let fallback = report.fallback.as_ref().expect("a fallback report");
            assert_eq!(fallback.ran.as_deref(), Some("claude-code:empty"));
            assert_eq!(fallback.fell_through[0].harness, "claude-code:absent");
            assert_eq!(fallback.fell_through[0].reason, "auth");
            assert_eq!(report.results[0].status, Status::Skipped);
            assert_eq!(report.results[0].failure_kind, Some(FailureKind::Auth));
        }
        RunStreamEnvelope::Event { .. } => panic!("missing terminal report"),
    }

    // Outside a chain there is nothing to fall through to, so the run fails —
    // with the same classification, not an unreadable harness-side refusal.
    let alone = run_with_config(
        &[
            "run",
            "--prompt",
            "hi",
            "--harness",
            "claude-code:absent",
            "--run-mode",
            "parallel",
            "--cwd",
            &fx.cwd(),
            "--compact",
        ],
        &envs,
        &fx.user_config(),
    );
    assert_eq!(alone.status.code(), Some(1));
    assert_eq!(json_stdout(&alone)["results"][0]["failure_kind"], "auth");

    // An opaque credential is never probed on disk: `env_from` values that are
    // not absolute paths reach the child untouched.
    let opaque = run_with_config(
        &[
            "run",
            "--prompt",
            "hi",
            "--harness",
            "claude-code:absent",
            "--run-mode",
            "parallel",
            "--cwd",
            &fx.cwd(),
            "--compact",
        ],
        &[
            ("OH_TEST_ABSENT_HOME", "sk-ant-not-a-path"),
            ("MOCK_ECHO_ENV", "CLAUDE_CONFIG_DIR"),
        ],
        &fx.user_config(),
    );
    assert!(
        opaque.status.success(),
        "{}",
        String::from_utf8_lossy(&opaque.stderr)
    );
    assert_eq!(json_stdout(&opaque)["results"][0]["status"], "ok");
}

#[test]
fn sync_rejects_conflicting_variants_sharing_one_native_config() {
    let fx = ConfigFixture::new(
        "variant-sync-conflict",
        r#"
[harness.claude-code.variant.work]
allowed_tools = ["Read"]
[harness.claude-code.variant.personal]
allowed_tools = ["Bash(git status --short)"]
"#,
        "",
    );
    let output = run_with_config(
        &[
            "sync",
            "--harness",
            "claude-code:work",
            "--harness",
            "claude-code:personal",
            "--cwd",
            &fx.cwd(),
        ],
        &[],
        &fx.user_config(),
    );
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "harness selections `claude-code:work` and `claude-code:personal` resolve to conflicting sync settings"
        ),
        "{stderr}"
    );
}

#[test]
fn sync_rejects_conflicting_base_and_variant_sharing_one_native_config() {
    let fx = ConfigFixture::new(
        "base-variant-sync-conflict",
        r#"
[harness.claude-code]
allowed_tools = ["Read"]
[harness.claude-code.variant.work]
allowed_tools = ["Bash(git status --short)"]
"#,
        "",
    );
    let output = run_with_config(
        &[
            "sync",
            "--harness",
            "claude-code",
            "--harness",
            "claude-code:work",
            "--cwd",
            &fx.cwd(),
        ],
        &[],
        &fx.user_config(),
    );
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "harness selections `claude-code` and `claude-code:work` resolve to conflicting sync settings"
        ),
        "{stderr}"
    );
}

#[test]
fn sync_applies_a_selected_variants_settings_to_the_shared_native_file() {
    let fx = ConfigFixture::new(
        "variant-sync-success",
        "[harness.claude-code.variant.work]\nallowed_tools = [\"Read\"]\n",
        "",
    );
    let output = run_with_config(
        &[
            "sync",
            "--harness",
            "claude-code:work",
            "--cwd",
            &fx.cwd(),
            "--compact",
        ],
        &[],
        &fx.user_config(),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(json_stdout(&output)["results"][0]["harness"], "claude-code");
    let settings = read_json(&std::path::Path::new(&fx.cwd()).join(".claude/settings.json"));
    assert_eq!(settings["permissions"]["allow"][0], "Read");
}

#[test]
fn duplicate_variant_selectors_keep_following_detect_and_sync_associations() {
    let bin = mock_bin().display().to_string().replace('\\', "\\\\");
    let fx = ConfigFixture::new(
        "variant-duplicate-selector-association",
        &format!(
            r#"
[harness.codex.variant.work]
bin = "{bin}"
[harness.claude-code.variant.personal]
bin = "{bin}"
allowed_tools = ["Bash(git status --short)"]
"#
        ),
        "",
    );
    let project_config = std::path::Path::new(&fx.cwd()).join("oneharness.toml");
    let detect = run_with_config(
        &[
            "detect",
            "--harness",
            "codex:work",
            "--harness",
            "codex:work",
            "--harness",
            "claude-code:personal",
            "--compact",
        ],
        &[],
        &project_config,
    );
    assert!(
        detect.status.success(),
        "{}",
        String::from_utf8_lossy(&detect.stderr)
    );
    let detected = json_stdout(&detect);
    assert_eq!(detected["detected"].as_array().unwrap().len(), 2);
    assert_eq!(detected["detected"][0]["id"], "codex:work");
    assert_eq!(detected["detected"][1]["id"], "claude-code:personal");

    let sync = run_with_config(
        &[
            "sync",
            "--harness",
            "codex:work",
            "--harness",
            "codex:work",
            "--harness",
            "claude-code:personal",
            "--cwd",
            &fx.cwd(),
            "--compact",
        ],
        &[],
        &fx.user_config(),
    );
    assert!(
        sync.status.success(),
        "{}",
        String::from_utf8_lossy(&sync.stderr)
    );
    let synced = json_stdout(&sync);
    assert_eq!(synced["results"].as_array().unwrap().len(), 2);
    assert_eq!(synced["results"][0]["harness"], "codex");
    assert_eq!(synced["results"][0]["status"], "skipped");
    assert_eq!(synced["results"][1]["harness"], "claude-code");
    assert_eq!(synced["results"][1]["status"], "created");
    let settings = read_json(&std::path::Path::new(&fx.cwd()).join(".claude/settings.json"));
    assert_eq!(
        settings["permissions"]["allow"][0],
        "Bash(git status --short)"
    );
}

#[test]
fn sync_uses_a_variant_selected_by_config() {
    let fx = ConfigFixture::new(
        "variant-sync-config-selection",
        r#"
harnesses = ["claude-code:work"]
[harness.claude-code.variant.work]
allowed_tools = ["Read"]
"#,
        "",
    );
    let output = run_with_config(
        &["sync", "--cwd", &fx.cwd(), "--compact"],
        &[],
        &fx.user_config(),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json_stdout(&output);
    assert_eq!(report["results"].as_array().unwrap().len(), 1);
    assert_eq!(report["results"][0]["harness"], "claude-code");
    let settings = read_json(&std::path::Path::new(&fx.cwd()).join(".claude/settings.json"));
    assert_eq!(settings["permissions"]["allow"][0], "Read");
}

#[test]
fn config_bin_override_is_used_to_execute() {
    let fx = ConfigFixture::new(
        "bin",
        &format!("[harness.claude-code]\nbin = '{}'\n", mock_bin().display()),
        "",
    );
    // No --bin: the configured binary is resolved and actually spawned.
    let output = run_with_config(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--cwd",
            &fx.cwd(),
            "--compact",
        ],
        &[("MOCK_STDOUT", r#"{"result":"via config bin"}"#)],
        &fx.user_config(),
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = json_stdout(&output);
    assert_eq!(value["results"][0]["status"], "ok");
    assert_eq!(value["results"][0]["text"], "via config bin");
}

#[test]
fn config_timeout_applies() {
    let fx = ConfigFixture::new("timeout", "timeout = 1\n", "");
    let bin = bin_override("claude-code");
    let output = run_with_config(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--bin",
            &bin,
            "--cwd",
            &fx.cwd(),
            "--compact",
        ],
        &[("MOCK_SLEEP_MS", "5000")],
        &fx.user_config(),
    );
    assert_eq!(output.status.code(), Some(1));
    let value = json_stdout(&output);
    assert_eq!(value["results"][0]["status"], "timeout");
}

#[test]
fn no_config_ignores_files_and_env_var_does_too() {
    let fx = ConfigFixture::new(
        "no-config",
        "model = \"cfg-model\"\nharnesses = [\"claude-code\"]\n",
        "",
    );
    // --no-config: the project file is ignored, so no selection exists.
    let output = run_with_config(
        &[
            "run",
            "--prompt",
            "hi",
            "--cwd",
            &fx.cwd(),
            "--no-config",
            "--print-command",
        ],
        &[],
        &fx.user_config(),
    );
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no harness selected"), "{stderr}");

    // ONEHARNESS_NO_CONFIG=1 behaves identically (this is what keeps wrapper
    // scripts and this very test suite hermetic).
    let output = run_with_config(
        &[
            "run",
            "--prompt",
            "hi",
            "--cwd",
            &fx.cwd(),
            "--print-command",
        ],
        &[("ONEHARNESS_NO_CONFIG", "1")],
        &fx.user_config(),
    );
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn env_override_beats_config_file_and_cli_beats_env() {
    // The standard `ONEHARNESS_<FIELD>` override sits between the config files
    // and the CLI flags: it beats a project `model`, and `--model` beats it.
    let fx = ConfigFixture::new("env-precedence", "model = \"cfg-model\"\n", "");
    let cwd = fx.cwd();
    let base = [
        "run",
        "--harness",
        "claude-code",
        "--prompt",
        "hi",
        "--print-command",
        "--compact",
        "--cwd",
        &cwd,
    ];
    // No --model: the env override wins over the project file.
    let output = run_with_config(
        &base,
        &[("ONEHARNESS_MODEL", "env-model")],
        &fx.user_config(),
    );
    let value = json_stdout(&output);
    assert_eq!(value["model"], "env-model");
    assert!(
        command_of(&value, 0)
            .windows(2)
            .any(|w| w == ["--model", "env-model"]),
        "{:?}",
        command_of(&value, 0)
    );

    // --model on the CLI beats the env override.
    let mut args: Vec<&str> = base.to_vec();
    args.extend(["--model", "cli-model"]);
    let output = run_with_config(
        &args,
        &[("ONEHARNESS_MODEL", "env-model")],
        &fx.user_config(),
    );
    let value = json_stdout(&output);
    assert_eq!(value["model"], "cli-model");
}

#[test]
fn env_override_supplies_selection_bypass_and_timeout() {
    // Selection, a boolean, and a number all flow from the environment with no
    // CLI flag or file in play (the user file is empty, no --harness given).
    let fx = ConfigFixture::new("env-misc", "", "");
    let cwd = fx.cwd();
    let output = run_with_config(
        &[
            "run",
            "--prompt",
            "hi",
            "--print-command",
            "--compact",
            "--cwd",
            &cwd,
        ],
        &[
            ("ONEHARNESS_HARNESSES", "claude-code"),
            ("ONEHARNESS_BYPASS", "false"),
            ("ONEHARNESS_TIMEOUT", "45"),
        ],
        &fx.user_config(),
    );
    let value = json_stdout(&output);
    let results = value["results"].as_array().unwrap();
    assert_eq!(results.len(), 1, "env selection should pick one harness");
    assert_eq!(results[0]["harness"], "claude-code");
    assert_eq!(value["bypass_permissions"], false);
    // bypass=false from the env reaches the built command (mapped to dontAsk).
    assert!(
        command_of(&value, 0)
            .windows(2)
            .any(|w| w == ["--permission-mode", "dontAsk"]),
        "{:?}",
        command_of(&value, 0)
    );
}

#[test]
fn env_override_is_ignored_under_no_config() {
    // --no-config (and ONEHARNESS_NO_CONFIG) must disable the env overrides too,
    // or the hermetic guarantee the suite relies on would leak. A selection set
    // only via the environment therefore does not apply.
    let fx = ConfigFixture::new("env-nocfg", "", "");
    let cwd = fx.cwd();
    let output = run_with_config(
        &[
            "run",
            "--prompt",
            "hi",
            "--no-config",
            "--print-command",
            "--cwd",
            &cwd,
        ],
        &[("ONEHARNESS_HARNESSES", "claude-code")],
        &fx.user_config(),
    );
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no harness selected"), "{stderr}");
}

#[test]
fn invalid_env_override_is_a_usage_error() {
    let fx = ConfigFixture::new("env-bad", "", "");
    let cwd = fx.cwd();
    let output = run_with_config(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--print-command",
            "--cwd",
            &cwd,
        ],
        &[("ONEHARNESS_TIMEOUT", "soon")],
        &fx.user_config(),
    );
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ONEHARNESS_TIMEOUT") && stderr.contains("environment"),
        "{stderr}"
    );
}

#[test]
fn invalid_config_environment_names_are_usage_errors() {
    for (name, project) in [
        ("top-level", "[env]\nBAD-NAME = \"value\"\n"),
        (
            "base-harness",
            "[harness.claude-code.env]\nBAD-NAME = \"value\"\n",
        ),
    ] {
        let fx = ConfigFixture::new(&format!("invalid-config-env-{name}"), project, "");
        let output = run_with_config(
            &[
                "run",
                "--harness",
                "claude-code",
                "--prompt",
                "hi",
                "--print-command",
                "--cwd",
                &fx.cwd(),
            ],
            &[],
            &fx.user_config(),
        );
        assert_eq!(output.status.code(), Some(2), "{name}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("environment variable names must match"),
            "{name}: {stderr}"
        );
    }
}

#[test]
fn config_command_attributes_env_overrides() {
    // The `config` provenance surface shows an env-sourced value as coming from
    // `environment`, beating the file it overrides.
    let fx = ConfigFixture::new("env-config-cmd", "model = \"cfg-model\"\n", "");
    let cwd = fx.cwd();
    let output = run_with_config(
        &["config", "--cwd", &cwd, "--compact"],
        &[("ONEHARNESS_MODEL", "env-model")],
        &fx.user_config(),
    );
    let value = json_stdout(&output);
    assert_eq!(value["model"]["value"], "env-model");
    assert_eq!(value["model"]["source"], "environment");
    assert!(
        value["config_files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f == "environment"),
        "{:?}",
        value["config_files"]
    );
}

#[test]
fn env_override_beats_an_explicit_config_file() {
    // `--config <path>` skips discovery but the env overrides still layer on
    // top, so the explicit file's model loses to ONEHARNESS_MODEL — and both
    // the file and `environment` show up as sources.
    let fx = ConfigFixture::new("env-explicit", "model = \"file-model\"\n", "");
    let explicit = fx.dir.join("oneharness.toml").display().to_string();
    let output = run_with_config(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--config",
            &explicit,
            "--print-command",
            "--compact",
        ],
        &[("ONEHARNESS_MODEL", "env-model")],
        &fx.user_config(),
    );
    let value = json_stdout(&output);
    assert_eq!(value["model"], "env-model");
    let files = value["config_files"].as_array().unwrap();
    assert!(files.iter().any(|f| f == "environment"), "{files:?}");
    assert!(
        files
            .iter()
            .any(|f| f.as_str().unwrap().ends_with("oneharness.toml")),
        "{files:?}"
    );
}

#[test]
fn oneharness_no_config_env_disables_env_overrides() {
    // The env form of --no-config must suppress the ONEHARNESS_* overrides too,
    // not just the files — the property the whole suite's hermeticity rests on.
    // A selection set only via ONEHARNESS_HARNESSES therefore does not apply.
    let fx = ConfigFixture::new("noconfig-disables-env", "", "");
    let output = run_with_config(
        &[
            "run",
            "--prompt",
            "hi",
            "--cwd",
            &fx.cwd(),
            "--print-command",
        ],
        &[
            ("ONEHARNESS_NO_CONFIG", "1"),
            ("ONEHARNESS_HARNESSES", "claude-code"),
        ],
        &fx.user_config(),
    );
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no harness selected"), "{stderr}");
}

#[test]
fn env_override_sets_output_format() {
    // A non-default field (output_format) flows from the env into the result
    // envelope (claude-code's default is json; the override forces stream-json).
    let fx = ConfigFixture::new("env-format", "", "");
    let output = run_with_config(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--cwd",
            &fx.cwd(),
            "--print-command",
            "--compact",
        ],
        &[("ONEHARNESS_OUTPUT_FORMAT", "stream-json")],
        &fx.user_config(),
    );
    let value = json_stdout(&output);
    assert_eq!(value["results"][0]["output_format"], "stream-json");
    assert!(
        command_of(&value, 0)
            .windows(2)
            .any(|w| w == ["--output-format", "stream-json"]),
        "{:?}",
        command_of(&value, 0)
    );
}

#[test]
fn explicit_config_flag_loads_exactly_that_file() {
    let fx = ConfigFixture::new("explicit", "model = \"project-model\"\n", "");
    let only = fx.dir.join("only.toml");
    std::fs::write(&only, "model = \"explicit-model\"\n").unwrap();
    let output = run_with_config(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--cwd",
            &fx.cwd(),
            "--config",
            &only.display().to_string(),
            "--print-command",
            "--compact",
        ],
        &[],
        &fx.user_config(),
    );
    let value = json_stdout(&output);
    let command = command_of(&value, 0);
    // The named file wins; the project file in --cwd is not even read.
    assert!(
        command
            .windows(2)
            .any(|w| w == ["--model", "explicit-model"]),
        "{command:?}"
    );
    // And a missing explicit file is a loud usage error.
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--config",
            "/no/such/oneharness-config.toml",
            "--no-config",
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn invalid_config_is_a_usage_error_with_the_path() {
    for (tag, toml, want) in [
        ("syntax", "not = valid = toml", "invalid config file"),
        ("typo", "modle = \"x\"", "modle"),
        ("bad-id", "[harness.bogus]\nmodel = \"x\"", "bogus"),
        (
            "conflict",
            "all = true\nharnesses = [\"codex\"]",
            "mutually exclusive",
        ),
    ] {
        let fx = ConfigFixture::new(tag, toml, "");
        let output = run_with_config(
            &[
                "run",
                "--prompt",
                "hi",
                "--harness",
                "claude-code",
                "--cwd",
                &fx.cwd(),
            ],
            &[],
            &fx.user_config(),
        );
        assert_eq!(output.status.code(), Some(2), "case {tag}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(want), "case {tag}: {stderr}");
        assert!(stderr.contains("oneharness.toml"), "case {tag}: {stderr}");
    }
}

#[test]
fn config_all_true_selection_with_exclude() {
    let fx = ConfigFixture::new(
        "all-true",
        "all = true\nexclude = [\"codex\", \"goose\"]\n",
        "",
    );
    let output = run_with_config(
        &[
            "run",
            "--prompt",
            "hi",
            "--cwd",
            &fx.cwd(),
            "--print-command",
            "--compact",
        ],
        &[],
        &fx.user_config(),
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = json_stdout(&output);
    let ids: Vec<&str> = value["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["harness"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), ALL_IDS.len() - 2, "{ids:?}");
    assert!(
        !ids.contains(&"codex") && !ids.contains(&"goose"),
        "{ids:?}"
    );
}

#[test]
fn config_exclude_applies_to_cli_all_and_cli_exclude_replaces_it() {
    let fx = ConfigFixture::new("exclude", "exclude = [\"codex\", \"goose\"]\n", "");
    // CLI --all with no --exclude: the config exclusions still apply.
    let output = run_with_config(
        &[
            "run",
            "--all",
            "--prompt",
            "hi",
            "--cwd",
            &fx.cwd(),
            "--print-command",
            "--compact",
        ],
        &[],
        &fx.user_config(),
    );
    let value = json_stdout(&output);
    assert_eq!(
        value["results"].as_array().unwrap().len(),
        ALL_IDS.len() - 2
    );

    // An explicit --exclude replaces the config's list entirely.
    let output = run_with_config(
        &[
            "run",
            "--all",
            "--exclude",
            "cursor",
            "--prompt",
            "hi",
            "--cwd",
            &fx.cwd(),
            "--print-command",
            "--compact",
        ],
        &[],
        &fx.user_config(),
    );
    let value = json_stdout(&output);
    let ids: Vec<&str> = value["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["harness"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), ALL_IDS.len() - 1, "{ids:?}");
    assert!(
        ids.contains(&"codex") && !ids.contains(&"cursor"),
        "{ids:?}"
    );
}

#[test]
fn config_output_format_applies_and_cli_beats_it() {
    let fx = ConfigFixture::new("format", "output_format = \"stream-json\"\n", "");
    let base = [
        "run",
        "--harness",
        "claude-code",
        "--prompt",
        "hi",
        "--print-command",
        "--compact",
        "--cwd",
    ];
    let mut args: Vec<&str> = base.to_vec();
    let cwd = fx.cwd();
    args.push(&cwd);
    let value = json_stdout(&run_with_config(&args, &[], &fx.user_config()));
    let command = command_of(&value, 0);
    assert!(
        command
            .windows(2)
            .any(|w| w == ["--output-format", "stream-json"]),
        "{command:?}"
    );

    args.extend(["--output-format", "text"]);
    let value = json_stdout(&run_with_config(&args, &[], &fx.user_config()));
    let command = command_of(&value, 0);
    assert!(
        command.windows(2).any(|w| w == ["--output-format", "text"]),
        "{command:?}"
    );
}

#[test]
fn config_require_available_fails_a_missing_harness() {
    let fx = ConfigFixture::new("require", "require_available = true\n", "");
    let output = run_with_config(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "hi",
            "--bin",
            "codex=/no/such/oneharness-binary-xyz",
            "--cwd",
            &fx.cwd(),
            "--compact",
        ],
        &[],
        &fx.user_config(),
    );
    assert_eq!(output.status.code(), Some(1));
    let value = json_stdout(&output);
    assert_eq!(value["results"][0]["status"], "skipped");
}

#[test]
fn cli_timeout_beats_config_timeout() {
    // Config says 1s; the CLI raises it to 20s, so a 2.5s mock must succeed.
    let fx = ConfigFixture::new("timeout-cli", "timeout = 1\n", "");
    let bin = bin_override("claude-code");
    let output = run_with_config(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--bin",
            &bin,
            "--timeout",
            "20",
            "--cwd",
            &fx.cwd(),
            "--compact",
        ],
        &[
            ("MOCK_SLEEP_MS", "2500"),
            ("MOCK_STDOUT", r#"{"result":"slow but fine"}"#),
        ],
        &fx.user_config(),
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = json_stdout(&output);
    assert_eq!(value["results"][0]["status"], "ok");
}

#[test]
fn env_var_bin_override_beats_config_bin() {
    // ONEHARNESS_BIN_<ID> must win over a configured bin: config points at a
    // nonexistent binary, the env var at the mock — the run must execute.
    let fx = ConfigFixture::new(
        "bin-env",
        "[harness.claude-code]\nbin = \"/no/such/oneharness-binary-xyz\"\n",
        "",
    );
    let mock = mock_bin().display().to_string();
    let output = run_with_config(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--cwd",
            &fx.cwd(),
            "--compact",
        ],
        &[
            ("ONEHARNESS_BIN_CLAUDE_CODE", mock.as_str()),
            ("MOCK_STDOUT", r#"{"result":"env wins"}"#),
        ],
        &fx.user_config(),
    );
    assert!(output.status.success());
    let value = json_stdout(&output);
    assert_eq!(value["results"][0]["text"], "env wins");
}

#[test]
fn project_config_is_discovered_walking_up_and_dotted_name_works() {
    let fx = ConfigFixture::new("discovery", "model = \"outer\"\n", "");
    // A nested dir with no config of its own walks up to the fixture root...
    let plain = fx.dir.join("nested").join("deeper");
    std::fs::create_dir_all(&plain).unwrap();
    // ...while a sibling with its own .oneharness.toml shadows the outer file.
    let dotted = fx.dir.join("dotted");
    std::fs::create_dir_all(&dotted).unwrap();
    std::fs::write(dotted.join(".oneharness.toml"), "model = \"inner\"\n").unwrap();

    for (cwd, want) in [(plain, "outer"), (dotted, "inner")] {
        let output = run_with_config(
            &[
                "run",
                "--harness",
                "claude-code",
                "--prompt",
                "hi",
                "--cwd",
                &cwd.display().to_string(),
                "--print-command",
                "--compact",
            ],
            &[],
            &fx.user_config(),
        );
        let value = json_stdout(&output);
        let command = command_of(&value, 0);
        assert!(
            command.windows(2).any(|w| w == ["--model", want]),
            "cwd {cwd:?}: {command:?}"
        );
    }
}

#[test]
fn missing_oneharness_config_env_path_is_a_usage_error() {
    // ONEHARNESS_CONFIG names a file explicitly, so its absence must be loud.
    let output = run_with_config(
        &["run", "--harness", "claude-code", "--prompt", "hi"],
        &[],
        std::path::Path::new("/no/such/oneharness-user-config.toml"),
    );
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ONEHARNESS_CONFIG"), "{stderr}");
}

#[test]
fn resume_works_with_a_config_driven_selection() {
    let fx = ConfigFixture::new("resume", "harnesses = [\"claude-code\"]\n", "");
    let output = run_with_config(
        &[
            "run",
            "--prompt",
            "continue",
            "--resume",
            "sess-cfg",
            "--cwd",
            &fx.cwd(),
            "--print-command",
            "--compact",
        ],
        &[],
        &fx.user_config(),
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = json_stdout(&output);
    let command = command_of(&value, 0);
    assert!(
        command.windows(2).any(|w| w == ["--resume", "sess-cfg"]),
        "{command:?}"
    );

    // A config selection of several harnesses still rejects --resume.
    let fx = ConfigFixture::new(
        "resume-multi",
        "harnesses = [\"claude-code\", \"opencode\"]\n",
        "",
    );
    let output = run_with_config(
        &[
            "run",
            "--prompt",
            "continue",
            "--resume",
            "sess-cfg",
            "--cwd",
            &fx.cwd(),
            "--print-command",
        ],
        &[],
        &fx.user_config(),
    );
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn config_max_parallel_is_accepted_and_runs_succeed() {
    // max_parallel only caps concurrency, which a black-box test can't time
    // reliably; this pins that a configured cap is parsed, wired, and still
    // yields a complete report for more harnesses than the cap.
    let fx = ConfigFixture::new("parallel", "max_parallel = 1\n", "");
    let claude = bin_override("claude-code");
    let codex = bin_override("codex");
    let output = run_with_config(
        &[
            "run",
            "--harness",
            "claude-code,codex",
            "--prompt",
            "hi",
            "--bin",
            &claude,
            "--bin",
            &codex,
            "--cwd",
            &fx.cwd(),
            "--compact",
        ],
        &[("MOCK_STDOUT", "fine")],
        &fx.user_config(),
    );
    assert!(output.status.success());
    let value = json_stdout(&output);
    let results = value["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r["status"] == "ok"));
}

#[test]
fn config_command_shows_values_with_sources() {
    let fx = ConfigFixture::new(
        "cmd",
        "model = \"project-model\"\n[harness.claude-code]\nmodel = \"sonnet\"\n",
        "model = \"user-model\"\ntimeout = 30\n[env]\nFOO = \"bar\"\n",
    );
    let output = run_with_config(
        &["config", "--cwd", &fx.cwd(), "--compact"],
        &[],
        &fx.user_config(),
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = json_stdout(&output);
    assert_eq!(value["schema_version"], "0.6");
    assert_eq!(value["config_files"].as_array().unwrap().len(), 2);

    // The project file wins for model and is named as the source...
    assert_eq!(value["model"]["value"], "project-model");
    let model_src = value["model"]["source"].as_str().unwrap();
    assert!(model_src.ends_with("oneharness.toml"), "{model_src}");
    // ...the user file's timeout and env survive with their own attribution...
    assert_eq!(value["timeout"]["value"], 30);
    let timeout_src = value["timeout"]["source"].as_str().unwrap();
    assert!(timeout_src.ends_with("user-config.toml"), "{timeout_src}");
    assert_eq!(value["env"]["FOO"]["value"], "bar");
    // ...untouched fields fall to their built-in defaults...
    assert_eq!(value["bypass"]["value"], false);
    assert_eq!(value["bypass"]["source"], "default");
    // `stream` is the field schema 0.4 added; it reports like any other scalar.
    assert_eq!(value["stream"]["value"], false);
    assert_eq!(value["stream"]["source"], "default");
    // `mode` has no built-in default (it derives from `bypass` when unset).
    assert!(value["mode"]["value"].is_null());
    assert!(value["mode"]["source"].is_null());
    assert!(value["system"]["value"].is_null());
    assert!(value["system"]["source"].is_null());
    // ...and per-harness overrides are attributed too.
    assert_eq!(value["harness"]["claude-code"]["model"]["value"], "sonnet");
}

#[test]
fn config_command_no_config_shows_pure_defaults() {
    let fx = ConfigFixture::new("cmd-none", "model = \"ignored\"\n", "model = \"ignored\"\n");
    for envs in [
        vec![],
        vec![("ONEHARNESS_NO_CONFIG", "1")], // env form, --no-config dropped below
    ] {
        let mut args = vec!["config", "--cwd"];
        let cwd = fx.cwd();
        args.push(&cwd);
        args.push("--compact");
        if envs.is_empty() {
            args.push("--no-config");
        }
        let output = run_with_config(&args, &envs, &fx.user_config());
        assert!(output.status.success());
        let value = json_stdout(&output);
        assert!(value["config_files"].as_array().unwrap().is_empty());
        assert!(value["model"]["value"].is_null());
        assert_eq!(value["timeout"]["value"], 120);
        assert_eq!(value["timeout"]["source"], "default");
    }
}

#[test]
fn config_command_explicit_file_and_invalid_config_error() {
    let fx = ConfigFixture::new("cmd-explicit", "model = \"project\"\n", "");
    let only = fx.dir.join("only.toml");
    std::fs::write(&only, "model = \"explicit\"\n").unwrap();
    let output = run_with_config(
        &[
            "config",
            "--cwd",
            &fx.cwd(),
            "--config",
            &only.display().to_string(),
            "--compact",
        ],
        &[],
        &fx.user_config(),
    );
    let value = json_stdout(&output);
    assert_eq!(value["model"]["value"], "explicit");
    assert_eq!(value["config_files"].as_array().unwrap().len(), 1);

    // A broken config file fails `config` the same way it fails `run`.
    std::fs::write(fx.dir.join("oneharness.toml"), "modle = \"typo\"").unwrap();
    let output = run_with_config(&["config", "--cwd", &fx.cwd()], &[], &fx.user_config());
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("modle"), "{stderr}");
}

#[test]
fn detect_uses_configured_bin() {
    let fx = ConfigFixture::new(
        "detect-bin",
        &format!("[harness.claude-code]\nbin = '{}'\n", mock_bin().display()),
        "",
    );
    // detect discovers project config from its own cwd, so run it from there.
    let mut cmd = Command::new(oneharness_bin());
    cmd.current_dir(&fx.dir)
        .env("ONEHARNESS_CONFIG", fx.user_config())
        .env("MOCK_STDOUT", "mock-harness 9.9.9")
        .args(["detect", "--harness", "claude-code", "--compact"]);
    let output = cmd.output().expect("failed to run oneharness");
    assert!(output.status.success());
    let value = json_stdout(&output);
    let entry = &value["detected"][0];
    assert_eq!(entry["available"], true);
    assert!(entry["version"].as_str().unwrap().contains("9.9.9"));
}

#[test]
fn detect_reports_availability_and_version() {
    let mock = mock_bin();
    let mock = mock.display().to_string();
    let output = run(
        &["detect", "--harness", "claude-code", "--compact"],
        &[
            ("ONEHARNESS_BIN_CLAUDE_CODE", mock.as_str()),
            ("MOCK_STDOUT", "mock-harness 1.2.3"),
        ],
    );
    assert!(output.status.success());
    let value = json_stdout(&output);
    let entry = &value["detected"][0];
    assert_eq!(entry["id"], "claude-code");
    assert_eq!(entry["available"], true);
    assert!(entry["version"].as_str().unwrap().contains("1.2.3"));
}

#[test]
fn detect_marks_missing_binary_unavailable() {
    let output = run(
        &[
            "detect",
            "--harness",
            "codex",
            "--bin",
            "codex=/no/such/oneharness-binary-xyz",
            "--compact",
        ],
        &[],
    );
    assert!(output.status.success());
    let value = json_stdout(&output);
    let entry = &value["detected"][0];
    assert_eq!(entry["available"], false);
    assert!(entry["version"].is_null());
}

#[test]
fn config_hooks_on_incapable_harness_fail_at_parse() {
    let fx = ConfigFixture::new("hooks-bad", "[harness.codex.hooks]\nPreToolUse = []\n", "");
    let output = run_with_config(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--cwd",
            &fx.cwd(),
        ],
        &[],
        &fx.user_config(),
    );
    // Loud even though codex isn't selected: the config itself is invalid
    // (codex has no config file oneharness could sync hooks into).
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("hooks"), "{stderr}");
    assert!(stderr.contains("codex"), "{stderr}");
    assert!(stderr.contains("cannot deliver"), "{stderr}");
}

#[test]
fn config_command_attributes_rules_and_hooks() {
    let fx = ConfigFixture::new(
        "cmd-rules",
        concat!(
            "allowed_tools = [\"Bash(git:*)\"]\n",
            "[harness.claude-code.hooks]\n",
            "PreToolUse = []\n",
        ),
        "denied_tools = [\"Bash(rm:*)\"]\n",
    );
    let output = run_with_config(
        &["config", "--cwd", &fx.cwd(), "--compact"],
        &[],
        &fx.user_config(),
    );
    let value = json_stdout(&output);
    assert_eq!(value["allowed_tools"]["value"][0], "Bash(git:*)");
    assert!(value["allowed_tools"]["source"]
        .as_str()
        .unwrap()
        .ends_with("oneharness.toml"));
    assert_eq!(value["denied_tools"]["value"][0], "Bash(rm:*)");
    assert!(value["denied_tools"]["source"]
        .as_str()
        .unwrap()
        .ends_with("user-config.toml"));
    assert!(value["harness"]["claude-code"]["hooks"]["value"]["PreToolUse"].is_array());
}

/// The full-feature sync fixture: top-level rules, claude hooks, opencode raw
/// settings — exercising every mapping kind at once.
const SYNC_TOML: &str = concat!(
    "allowed_tools = [\"Bash(git log:*)\", \"Read\"]\n",
    "denied_tools = [\"Bash(rm:*)\"]\n",
    "[harness.claude-code.hooks]\n",
    "PreToolUse = [{ matcher = \"Bash\", hooks = [{ type = \"command\", command = \"./check.sh\" }] }]\n",
    "[harness.opencode.settings.permission]\n",
    "edit = \"deny\"\n",
);

fn read_json(path: &std::path::Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap_or_else(|e| {
        panic!("could not read {}: {e}", path.display());
    }))
    .unwrap_or_else(|e| panic!("{} is not JSON: {e}", path.display()))
}

fn sync_result<'a>(value: &'a Value, id: &str) -> &'a Value {
    value["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["harness"] == id)
        .unwrap_or_else(|| panic!("no sync result for {id}"))
}

#[test]
fn sync_materializes_every_harness_config_file() {
    let fx = ConfigFixture::new("sync-all", SYNC_TOML, "");
    let output = run_with_config(
        &["sync", "--cwd", &fx.cwd(), "--compact"],
        &[],
        &fx.user_config(),
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = json_stdout(&output);
    assert_eq!(value["check"], false);

    // claude-code: rules at permissions.allow/deny, hooks at the top level.
    assert_eq!(sync_result(&value, "claude-code")["status"], "created");
    let claude = read_json(&fx.dir.join(".claude/settings.json"));
    assert_eq!(
        claude["permissions"]["allow"],
        serde_json::json!(["Bash(git log:*)", "Read"])
    );
    assert_eq!(
        claude["permissions"]["deny"],
        serde_json::json!(["Bash(rm:*)"])
    );
    assert_eq!(claude["hooks"]["PreToolUse"][0]["matcher"], "Bash");
    assert_eq!(
        claude["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
        "./check.sh"
    );

    // qwen and cursor share the permissions.allow/deny shape.
    let qwen = read_json(&fx.dir.join(".qwen/settings.json"));
    assert_eq!(qwen["permissions"]["allow"][0], "Bash(git log:*)");
    assert_eq!(qwen["permissions"]["deny"][0], "Bash(rm:*)");
    let cursor = read_json(&fx.dir.join(".cursor/cli.json"));
    assert_eq!(cursor["permissions"]["allow"][0], "Bash(git log:*)");

    // crush: allow at permissions.allowed_tools, deny at options.disabled_tools.
    let crush = read_json(&fx.dir.join("crush.json"));
    assert_eq!(crush["permissions"]["allowed_tools"][0], "Bash(git log:*)");
    assert_eq!(crush["options"]["disabled_tools"][0], "Bash(rm:*)");

    // opencode: only the raw settings table (its permission shape is a map);
    // the top-level rule lists are reported unmapped, loudly.
    let opencode = read_json(&fx.dir.join("opencode.json"));
    assert_eq!(opencode["permission"]["edit"], "deny");
    let oc = sync_result(&value, "opencode");
    assert_eq!(oc["status"], "created");
    assert_eq!(
        oc["unmapped"],
        serde_json::json!(["allowed_tools", "denied_tools"])
    );

    // codex/goose/copilot: nothing to write, rules unmapped, warned on stderr.
    for id in ["codex", "goose", "copilot"] {
        let entry = sync_result(&value, id);
        assert_eq!(entry["status"], "skipped", "{id}");
        assert!(entry["file"].is_null(), "{id}");
        assert_eq!(
            entry["unmapped"],
            serde_json::json!(["allowed_tools", "denied_tools"]),
            "{id}"
        );
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("NOT applied") && stderr.contains("codex"),
        "{stderr}"
    );
}

#[test]
fn sync_merges_without_touching_unrelated_keys_and_is_idempotent() {
    let fx = ConfigFixture::new(
        "sync-merge",
        "allowed_tools = [\"Bash(ls *)\", \"Edit\"]\n",
        "",
    );
    let claude_dir = fx.dir.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(
        claude_dir.join("settings.json"),
        r#"{
  "permissions": { "allow": ["Read", "Bash(ls *)"], "defaultMode": "plan" },
  "env": { "FOO": "bar" },
  "model": "opus"
}"#,
    )
    .unwrap();

    let args = ["sync", "--harness", "claude-code", "--cwd"];
    let mut argv: Vec<&str> = args.to_vec();
    let cwd = fx.cwd();
    argv.push(&cwd);
    argv.push("--compact");
    let output = run_with_config(&argv, &[], &fx.user_config());
    let value = json_stdout(&output);
    assert_eq!(sync_result(&value, "claude-code")["status"], "updated");

    let merged = read_json(&claude_dir.join("settings.json"));
    // Union: existing entries keep their place, new ones are appended once.
    assert_eq!(
        merged["permissions"]["allow"],
        serde_json::json!(["Read", "Bash(ls *)", "Edit"])
    );
    // Unrelated keys at every level are untouched.
    assert_eq!(merged["permissions"]["defaultMode"], "plan");
    assert_eq!(merged["env"]["FOO"], "bar");
    assert_eq!(merged["model"], "opus");

    // Re-syncing is a no-op.
    let output = run_with_config(&argv, &[], &fx.user_config());
    let value = json_stdout(&output);
    assert_eq!(sync_result(&value, "claude-code")["status"], "unchanged");
}

#[test]
fn sync_check_reports_and_writes_nothing() {
    let fx = ConfigFixture::new("sync-check", "allowed_tools = [\"Read\"]\n", "");
    let output = run_with_config(
        &[
            "sync",
            "--harness",
            "claude-code",
            "--check",
            "--cwd",
            &fx.cwd(),
            "--compact",
        ],
        &[],
        &fx.user_config(),
    );
    // Out of sync: exit 1, nothing written, status says what would happen.
    assert_eq!(output.status.code(), Some(1));
    let value = json_stdout(&output);
    assert_eq!(value["check"], true);
    assert_eq!(sync_result(&value, "claude-code")["status"], "created");
    assert!(!fx.dir.join(".claude/settings.json").exists());

    // After a real sync, --check passes with exit 0.
    let output = run_with_config(
        &["sync", "--harness", "claude-code", "--cwd", &fx.cwd()],
        &[],
        &fx.user_config(),
    );
    assert!(output.status.success());
    let output = run_with_config(
        &[
            "sync",
            "--harness",
            "claude-code",
            "--check",
            "--cwd",
            &fx.cwd(),
            "--compact",
        ],
        &[],
        &fx.user_config(),
    );
    assert_eq!(output.status.code(), Some(0));
    let value = json_stdout(&output);
    assert_eq!(sync_result(&value, "claude-code")["status"], "unchanged");
}

#[test]
fn sync_refuses_a_file_it_cannot_parse_and_leaves_it_alone() {
    let fx = ConfigFixture::new("sync-jsonc", "allowed_tools = [\"Read\"]\n", "");
    let claude_dir = fx.dir.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    let original = "{ // hand-written, with comments\n  \"model\": \"opus\" }";
    std::fs::write(claude_dir.join("settings.json"), original).unwrap();

    let output = run_with_config(
        &["sync", "--harness", "claude-code", "--cwd", &fx.cwd()],
        &[],
        &fx.user_config(),
    );
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not valid JSON"), "{stderr}");
    assert!(stderr.contains("settings.json"), "{stderr}");
    assert_eq!(
        std::fs::read_to_string(claude_dir.join("settings.json")).unwrap(),
        original,
        "the unparseable file must be left untouched"
    );
}

#[test]
fn sync_prefers_crushs_existing_dotted_config() {
    let fx = ConfigFixture::new("sync-crush-alt", "allowed_tools = [\"bash\"]\n", "");
    std::fs::write(fx.dir.join(".crush.json"), "{\"options\": {\"tui\": {}}}").unwrap();
    let output = run_with_config(
        &[
            "sync",
            "--harness",
            "crush",
            "--cwd",
            &fx.cwd(),
            "--compact",
        ],
        &[],
        &fx.user_config(),
    );
    let value = json_stdout(&output);
    // `.crush.json` beats `crush.json` for crush, so oneharness merges into it
    // rather than creating a second, shadowed file.
    let entry = sync_result(&value, "crush");
    assert_eq!(entry["status"], "updated");
    assert!(
        entry["file"].as_str().unwrap().ends_with(".crush.json"),
        "{entry}"
    );
    assert!(!fx.dir.join("crush.json").exists());
    let merged = read_json(&fx.dir.join(".crush.json"));
    assert_eq!(merged["permissions"]["allowed_tools"][0], "bash");
    assert!(merged["options"]["tui"].is_object(), "unrelated key kept");
}

#[test]
fn sync_with_nothing_configured_skips_everything() {
    let fx = ConfigFixture::new("sync-noop", "model = \"x\"\n", "");
    let output = run_with_config(
        &["sync", "--cwd", &fx.cwd(), "--compact"],
        &[],
        &fx.user_config(),
    );
    assert!(output.status.success());
    let value = json_stdout(&output);
    for entry in value["results"].as_array().unwrap() {
        assert_eq!(entry["status"], "skipped", "{entry}");
        assert!(entry["unmapped"].as_array().unwrap().is_empty(), "{entry}");
    }
    // And nothing was created.
    assert!(!fx.dir.join(".claude").exists());
    assert!(!fx.dir.join("opencode.json").exists());
}

#[test]
fn sync_installs_normalized_hooks_across_harness_shapes() {
    // One `[[hooks]]` entry fans across harnesses with structurally different
    // hook formats: a shared-file merge (claude-code), a dedicated file
    // (codex), and a JS plugin shim (opencode). `{harness}` is substituted.
    let project =
        "[[hooks]]\ncommand = \"mygate hook {harness}\"\nmatcher = \"Bash\"\ntimeout = 10\n";
    let fx = ConfigFixture::new("sync-hooks", project, "");
    let output = run_with_config(
        &[
            "sync",
            "--harness",
            "claude-code",
            "--harness",
            "codex",
            "--harness",
            "opencode",
            "--cwd",
            &fx.cwd(),
            "--compact",
        ],
        &[],
        &fx.user_config(),
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = json_stdout(&output);

    // claude-code: merged into its settings file, with `{harness}` resolved.
    let claude = sync_result(&value, "claude-code");
    assert_eq!(claude["hooks"][0]["status"], "created");
    assert!(claude["hooks"][0]["file"]
        .as_str()
        .unwrap()
        .ends_with(".claude/settings.json"));
    let settings = read_json(&fx.dir.join(".claude/settings.json"));
    assert_eq!(
        settings["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
        "mygate hook claude-code"
    );

    // codex: a dedicated hooks file was created.
    assert!(fx.dir.join(".codex/hooks.json").is_file());
    assert_eq!(
        read_json(&fx.dir.join(".codex/hooks.json"))["hooks"]["PreToolUse"][0]["hooks"][0]
            ["command"],
        "mygate hook codex"
    );

    // opencode: a JS shim with the command wired in as an argv array.
    let shim = std::fs::read_to_string(fx.dir.join(".opencode/plugin/oneharness.js")).unwrap();
    assert!(
        shim.contains(r#"["mygate","hook","opencode"]"#),
        "command must be wired into the shim:\n{shim}"
    );

    // Re-syncing changes nothing: --check passes with exit 0.
    let output = run_with_config(
        &[
            "sync",
            "--harness",
            "claude-code",
            "--harness",
            "codex",
            "--harness",
            "opencode",
            "--check",
            "--cwd",
            &fx.cwd(),
            "--compact",
        ],
        &[],
        &fx.user_config(),
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "re-sync should be idempotent"
    );
}

#[test]
fn list_exposes_sync_capabilities() {
    let output = run(&["list", "--compact"], &[]);
    let value = json_stdout(&output);
    let harnesses = value["harnesses"].as_array().unwrap();
    let by_id = |id: &str| harnesses.iter().find(|h| h["id"] == id).unwrap();
    let claude = by_id("claude-code");
    assert_eq!(claude["sync_file"], ".claude/settings.json");
    assert_eq!(claude["supports_allowed_tools"], true);
    assert_eq!(claude["supports_denied_tools"], true);
    assert_eq!(claude["supports_hooks"], true);
    // qwen and crush gained deny support via their files; opencode is
    // settings-table only; codex/goose/copilot have no sync file at all.
    assert_eq!(by_id("qwen")["supports_denied_tools"], true);
    assert_eq!(by_id("crush")["supports_denied_tools"], true);
    let opencode = by_id("opencode");
    assert_eq!(opencode["sync_file"], "opencode.json");
    assert_eq!(opencode["supports_allowed_tools"], false);
    for id in ["codex", "goose", "copilot"] {
        assert!(by_id(id)["sync_file"].is_null(), "{id}");
        assert_eq!(by_id(id)["supports_allowed_tools"], false, "{id}");
    }
}

#[test]
fn list_exposes_mock_capabilities() {
    let output = run(&["list", "--compact"], &[]);
    let value = json_stdout(&output);
    let harnesses = value["harnesses"].as_array().unwrap();
    let by_id = |id: &str| harnesses.iter().find(|h| h["id"] == id).unwrap();
    // Every harness can express the mock deny (the gate protocol).
    for h in harnesses {
        assert_eq!(h["supports_mock_deny"], true, "{}", h["id"]);
    }
    // The rewrite shape is present only where verified live (oh_mock_enforce /
    // the explore-hooks probe) and absent where the harness can't express one
    // (goose), never fires hooks headlessly (copilot), or had its documented
    // shape live-refuted (qwen) — see docs/mock-spy-design.md.
    assert_eq!(by_id("claude-code")["mock_rewrite"], "claude-nested");
    assert_eq!(by_id("codex")["mock_rewrite"], "claude-nested");
    assert_eq!(by_id("crush")["mock_rewrite"], "crush-flat");
    assert_eq!(by_id("opencode")["mock_rewrite"], "opencode-shim");
    assert_eq!(by_id("cursor")["mock_rewrite"], "cursor-permission");
    for id in ["goose", "qwen", "copilot"] {
        assert!(by_id(id)["mock_rewrite"].is_null(), "{id}");
    }
}

#[test]
fn run_never_emits_policy_flags_from_sync_settings() {
    // The file-only guarantee: rules/hooks/settings are delivered by `sync`,
    // never injected into a run's argv. A config full of policy must leave
    // every built command untouched (and the run must not error).
    let fx = ConfigFixture::new("run-clean", SYNC_TOML, "");
    let output = run_with_config(
        &[
            "run",
            "--harness",
            "claude-code,opencode",
            "--prompt",
            "hi",
            "--cwd",
            &fx.cwd(),
            "--print-command",
            "--compact",
        ],
        &[],
        &fx.user_config(),
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = json_stdout(&output);
    for index in 0..2 {
        let command = command_of(&value, index);
        for token in &command {
            assert!(
                !token.contains("allowedTools")
                    && !token.contains("allow-tool")
                    && !token.contains("--settings")
                    && !token.contains("hooks"),
                "policy leaked into argv: {command:?}"
            );
        }
    }
}

#[test]
fn sync_is_add_only_across_config_edits() {
    // Documented semantics: editing the unified config and re-syncing adds
    // and updates, but never removes — the old rule survives in the file.
    let fx = ConfigFixture::new("sync-edit", "allowed_tools = [\"RuleA\"]\n", "");
    let argv = |cwd: &str| {
        vec![
            "sync".to_string(),
            "--harness".to_string(),
            "claude-code".to_string(),
            "--cwd".to_string(),
            cwd.to_string(),
            "--compact".to_string(),
        ]
    };
    let args: Vec<String> = argv(&fx.cwd());
    let args_ref: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = run_with_config(&args_ref, &[], &fx.user_config());
    assert!(output.status.success());

    // Edit the config: RuleA replaced by RuleB at the oneharness level...
    std::fs::write(
        fx.dir.join("oneharness.toml"),
        "allowed_tools = [\"RuleB\"]\n",
    )
    .unwrap();
    let output = run_with_config(&args_ref, &[], &fx.user_config());
    let value = json_stdout(&output);
    assert_eq!(sync_result(&value, "claude-code")["status"], "updated");

    // ...but the harness file unions: RuleA is kept, RuleB appended.
    let merged = read_json(&fx.dir.join(".claude/settings.json"));
    assert_eq!(
        merged["permissions"]["allow"],
        serde_json::json!(["RuleA", "RuleB"])
    );
}

#[test]
fn sync_selection_edges_nothing_to_sync_and_unknown_id() {
    // Naming a harness with nothing configured is a clean skip (exit 0)...
    let fx = ConfigFixture::new("sync-edges", "model = \"x\"\n", "");
    let output = run_with_config(
        &[
            "sync",
            "--harness",
            "codex",
            "--cwd",
            &fx.cwd(),
            "--compact",
        ],
        &[],
        &fx.user_config(),
    );
    assert!(output.status.success());
    let value = json_stdout(&output);
    assert_eq!(value["results"].as_array().unwrap().len(), 1);
    assert_eq!(sync_result(&value, "codex")["status"], "skipped");

    // ...while an unknown id is the usual loud usage error.
    let output = run_with_config(
        &["sync", "--harness", "bogus", "--cwd", &fx.cwd()],
        &[],
        &fx.user_config(),
    );
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown harness"), "{stderr}");
}

#[test]
fn sync_honors_no_config_and_writes_nothing() {
    // ONEHARNESS_NO_CONFIG must neutralize sync exactly like run — a hermetic
    // wrapper can never have its project files rewritten by ambient config.
    let fx = ConfigFixture::new("sync-no-config", SYNC_TOML, "");
    for extra in [&["--no-config"][..], &[][..]] {
        let mut args = vec!["sync", "--cwd"];
        let cwd = fx.cwd();
        args.push(&cwd);
        args.push("--compact");
        args.extend_from_slice(extra);
        let envs: &[(&str, &str)] = if extra.is_empty() {
            &[("ONEHARNESS_NO_CONFIG", "1")]
        } else {
            &[]
        };
        let output = run_with_config(&args, envs, &fx.user_config());
        assert!(output.status.success());
        let value = json_stdout(&output);
        assert!(value["config_files"].as_array().unwrap().is_empty());
        for entry in value["results"].as_array().unwrap() {
            assert_eq!(entry["status"], "skipped", "{entry}");
        }
        assert!(!fx.dir.join(".claude").exists());
        assert!(!fx.dir.join("opencode.json").exists());
    }
}

#[test]
fn sync_defaults_to_the_current_directory() {
    // Without --cwd, sync targets (and discovers config from) the process cwd.
    let fx = ConfigFixture::new("sync-cwd", "allowed_tools = [\"Read\"]\n", "");
    let mut cmd = Command::new(oneharness_bin());
    cmd.current_dir(&fx.dir)
        .env("ONEHARNESS_CONFIG", fx.user_config())
        .args(["sync", "--harness", "claude-code", "--compact"]);
    let output = cmd.output().expect("failed to run oneharness");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let merged = read_json(&fx.dir.join(".claude/settings.json"));
    assert_eq!(merged["permissions"]["allow"][0], "Read");
}

#[test]
fn non_table_hooks_or_settings_are_loud_parse_errors() {
    for (tag, toml) in [
        ("hooks-scalar", "[harness.claude-code]\nhooks = \"oops\"\n"),
        ("settings-scalar", "[harness.opencode]\nsettings = 3\n"),
    ] {
        let fx = ConfigFixture::new(tag, toml, "");
        let output = run_with_config(&["sync", "--cwd", &fx.cwd()], &[], &fx.user_config());
        assert_eq!(output.status.code(), Some(2), "case {tag}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("must be a table"), "case {tag}: {stderr}");
    }
}

/// Pipe `stdin` into `oneharness <args>` and capture the output. Used for the
/// `gate` verb, which reads the harness's hook event from stdin.
fn run_with_stdin(args: &[&str], stdin: &str) -> Output {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn oneharness");
    // Best-effort: a gate that exits before reading (e.g. unknown harness) closes
    // the pipe, and that broken-pipe write is expected, not a test failure.
    let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
    child
        .wait_with_output()
        .expect("failed to wait on oneharness")
}

#[test]
fn gate_blocks_on_match_and_allows_otherwise() {
    let event = r#"{"tool_name":"Bash","tool_input":{"command":"touch BLOCK-9.txt"}}"#;
    // The marker is in the command -> the harness's native deny verdict.
    let out = run_with_stdin(
        &[
            "gate",
            "claude-code",
            "--deny-if-contains",
            "BLOCK-9",
            "--reason",
            "oneharness: nope",
        ],
        event,
    );
    assert!(out.status.success(), "gate must always exit 0");
    let v = json_stdout(&out);
    assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "deny");
    assert_eq!(
        v["hookSpecificOutput"]["permissionDecisionReason"],
        "oneharness: nope"
    );

    // Marker absent -> empty stdout (the universal fall-through, never a block).
    let out = run_with_stdin(
        &["gate", "claude-code", "--deny-if-contains", "BLOCK-9"],
        r#"{"tool_input":{"command":"touch ok.txt"}}"#,
    );
    assert!(out.status.success());
    assert!(
        out.stdout.is_empty(),
        "a non-match must emit nothing, got: {}",
        String::from_utf8_lossy(&out.stdout)
    );

    // No deny marker at all -> the inert default allows everything.
    let out = run_with_stdin(&["gate", "goose"], event);
    assert!(out.status.success() && out.stdout.is_empty());
}

#[test]
fn gate_renders_each_harness_native_verdict() {
    // The marker appears in whichever field a harness reads (command / tool_input).
    let event = r#"{"command":"BLOCKME","tool_input":{"command":"BLOCKME"}}"#;
    let deny = |id: &str| {
        json_stdout(&run_with_stdin(
            &["gate", id, "--deny-if-contains", "BLOCKME"],
            event,
        ))
    };

    // Claude / Codex / Qwen: nested under hookSpecificOutput.
    for id in ["claude-code", "codex", "qwen"] {
        assert_eq!(
            deny(id)["hookSpecificOutput"]["permissionDecision"],
            "deny",
            "{id}"
        );
    }
    // Copilot: the same field, flat.
    let copilot = deny("copilot");
    assert_eq!(copilot["permissionDecision"], "deny");
    assert!(copilot.get("hookSpecificOutput").is_none());
    // Cursor: `permission`, carrying both message spellings.
    let cursor = deny("cursor");
    assert_eq!(cursor["permission"], "deny");
    assert!(cursor["agentMessage"].is_string() && cursor["agent_message"].is_string());
    // Crush / OpenCode: flat `decision: "deny"`; Goose: `decision: "block"`.
    assert_eq!(deny("crush")["decision"], "deny");
    assert_eq!(deny("opencode")["decision"], "deny");
    assert_eq!(deny("goose")["decision"], "block");
}

#[test]
fn gate_unknown_harness_is_a_usage_error() {
    let out = run_with_stdin(&["gate", "nope", "--deny-if-contains", "x"], "{}");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown harness"));
}

#[test]
fn init_scaffolds_and_refuses_overwrite() {
    let dir = std::env::temp_dir().join(format!("oneharness-inittest-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("oneharness.judge.toml");
    let path_str = path.display().to_string();

    // First write: exit 0, a plain confirmation line on stdout, real file on disk.
    let out = run(&["init", &path_str], &[]);
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("wrote"));
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(written.contains("run_mode = \"fallback\""));
    assert!(written.contains("[harness.claude-code]"));

    // Second write without --force: refused (exit 2), original file untouched.
    let out = run(&["init", &path_str], &[]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("already exists"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), written);

    // With --force: overwrites and succeeds.
    let out = run(&["init", &path_str, "--force"], &[]);
    assert_eq!(out.status.code(), Some(0));

    let _ = std::fs::remove_dir_all(&dir);
}

/// A throwaway directory for the mock responder's rules/spy files, plus the
/// standard ruleset the tests share: rule 0 denies `git push`, rule 1 rewrites
/// `git status` to a stub that prints canned output.
fn mock_fixture(tag: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("oneharness-mock-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let rules = dir.join("rules.json");
    std::fs::write(
        &rules,
        r#"{"rules":[
            {"match":{"tool":"Bash","event_contains":"git push"},
             "action":{"deny":{"message":"pushes are mocked"}}},
            {"match":{"event_contains":"git status"},
             "action":{"rewrite":{"input":{"command":"printf clean"},"message":"mocked status"}}}
        ]}"#,
    )
    .unwrap();
    (dir, rules)
}

#[test]
fn mock_rewrites_denies_and_falls_through() {
    let (dir, rules) = mock_fixture("verbs");
    let rules = rules.to_str().unwrap();

    // A matched rewrite emits the harness's native allow+updatedInput verdict.
    let out = run_with_stdin(
        &["mock", "claude-code", "--rules", rules],
        r#"{"tool_name":"Bash","tool_input":{"command":"git status"}}"#,
    );
    assert!(
        out.status.success(),
        "mock must always exit 0 after startup"
    );
    let v = json_stdout(&out);
    assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "allow");
    assert_eq!(
        v["hookSpecificOutput"]["updatedInput"]["command"],
        "printf clean"
    );
    assert_eq!(
        v["hookSpecificOutput"]["permissionDecisionReason"],
        "mocked status"
    );

    // A matched deny speaks the same protocol the gate does.
    let out = run_with_stdin(
        &["mock", "claude-code", "--rules", rules],
        r#"{"tool_name":"Bash","tool_input":{"command":"git push origin"}}"#,
    );
    let v = json_stdout(&out);
    assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "deny");
    assert_eq!(
        v["hookSpecificOutput"]["permissionDecisionReason"],
        "pushes are mocked"
    );

    // No matching rule -> empty stdout, the universal fall-through.
    let out = run_with_stdin(
        &["mock", "claude-code", "--rules", rules],
        r#"{"tool_name":"Bash","tool_input":{"command":"ls"}}"#,
    );
    assert!(out.status.success());
    assert!(
        out.stdout.is_empty(),
        "a non-match must emit nothing, got: {}",
        String::from_utf8_lossy(&out.stdout)
    );

    // No rules at all -> spy-only responder: everything falls through.
    let out = run_with_stdin(&["mock", "claude-code"], r#"{"tool_name":"Bash"}"#);
    assert!(out.status.success() && out.stdout.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn mock_renders_each_harness_rewrite_shape() {
    let (dir, rules) = mock_fixture("shapes");
    let rules = rules.to_str().unwrap();
    let event = r#"{"tool_name":"bash","tool_input":{"command":"git status"}}"#;
    let rewrite = |id: &str| json_stdout(&run_with_stdin(&["mock", id, "--rules", rules], event));

    // Claude Code: nested under hookSpecificOutput (updatedInput). (Qwen's
    // docs describe the same shape, but it was live-refuted — see the registry
    // — so a rewrite for qwen is now a loud refusal, covered below.)
    let v = rewrite("claude-code");
    assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "allow");
    assert_eq!(
        v["hookSpecificOutput"]["updatedInput"]["command"],
        "printf clean"
    );
    let out = run_with_stdin(&["mock", "qwen", "--rules", rules], event);
    assert_eq!(out.status.code(), Some(2), "qwen rewrite must be refused");
    // Crush: flat versioned shape with a shallow-merge updated_input.
    let v = rewrite("crush");
    assert_eq!(v["version"], 1);
    assert_eq!(v["decision"], "allow");
    assert_eq!(v["updated_input"]["command"], "printf clean");
    // OpenCode: the flat decision the oneharness plugin shim applies.
    let v = rewrite("opencode");
    assert_eq!(v["decision"], "allow");
    assert_eq!(v["updated_input"]["command"], "printf clean");
    assert!(v.get("version").is_none());
    // Codex speaks the same claude-nested protocol (probe-verified).
    let v = rewrite("codex");
    assert_eq!(
        v["hookSpecificOutput"]["updatedInput"]["command"],
        "printf clean"
    );
    // Cursor: permission + updated_input ONLY (the probe-verified reply).
    let v = rewrite("cursor");
    assert_eq!(
        v,
        serde_json::json!({"permission": "allow", "updated_input": {"command": "printf clean"}})
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn mock_spy_log_records_every_event_including_fall_throughs() {
    let (dir, rules) = mock_fixture("spy");
    let rules = rules.to_str().unwrap();
    let spy = dir.join("spy.jsonl");
    let spy_arg = spy.to_str().unwrap();

    // One deny, one fall-through (with rules), one spy-only (no rules).
    for (args, event) in [
        (
            vec![
                "mock",
                "claude-code",
                "--rules",
                rules,
                "--spy-file",
                spy_arg,
            ],
            r#"{"tool_name":"Bash","tool_input":{"command":"git push"}}"#,
        ),
        (
            vec![
                "mock",
                "claude-code",
                "--rules",
                rules,
                "--spy-file",
                spy_arg,
            ],
            r#"{"tool_name":"Bash","tool_input":{"command":"ls"}}"#,
        ),
        (
            vec!["mock", "goose", "--spy-file", spy_arg],
            "not even json",
        ),
    ] {
        assert!(run_with_stdin(&args, event).status.success());
    }

    let log = std::fs::read_to_string(&spy).unwrap();
    let lines: Vec<serde_json::Value> = log
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(
        lines.len(),
        3,
        "every invocation appends exactly one record"
    );
    assert_eq!(lines[0]["harness"], "claude-code");
    assert_eq!(lines[0]["action"], "deny");
    assert_eq!(lines[0]["rule"], 0);
    // The original event is embedded as parsed JSON — the pre-rewrite intent.
    assert_eq!(lines[0]["event"]["tool_input"]["command"], "git push");
    assert_eq!(lines[1]["action"], "allow");
    assert!(lines[1]["rule"].is_null());
    // A non-JSON event is kept verbatim as a string, never dropped.
    assert_eq!(lines[2]["harness"], "goose");
    assert_eq!(lines[2]["event"], "not even json");

    // The env-var channel (how a `run --env` setting reaches every hook
    // invocation) selects the same spy file; the flag would win over it.
    let env_spy = dir.join("spy-env.jsonl");
    let out = {
        use std::io::Write;
        use std::process::Stdio;
        let mut child = Command::new(oneharness_bin())
            .env("ONEHARNESS_NO_CONFIG", "1")
            .env("ONEHARNESS_SPY_FILE", &env_spy)
            .args(["mock", "claude-code"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to spawn oneharness");
        let _ = child
            .stdin
            .take()
            .unwrap()
            .write_all(br#"{"tool_name":"Bash"}"#);
        child.wait_with_output().expect("failed to wait")
    };
    assert!(out.status.success());
    let log = std::fs::read_to_string(&env_spy).unwrap();
    let line: serde_json::Value = serde_json::from_str(log.lines().next().unwrap()).unwrap();
    assert_eq!(line["action"], "allow");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `run --mock-rules` in an "original" workspace: the hook is layered onto a
/// pre-existing project config non-destructively for the duration of the run
/// (the mock harness cats the config file mid-run, proving the merged state),
/// and the file is restored byte-identically afterwards. The spy path rides
/// the same hook command; the report records both.
#[test]
fn run_mock_rules_layers_onto_existing_config_and_restores_it() {
    let dir = std::env::temp_dir().join(format!("oneharness-mockrun-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // A real project: crush config with unrelated keys AND an existing hook.
    let config = dir.join(".crush.json");
    let original =
        r#"{"unrelated":{"keep":true},"hooks":{"PreToolUse":[{"command":"existing-hook"}]}}"#;
    std::fs::write(&config, original).unwrap();
    let rules = dir.join("rules.json");
    std::fs::write(
        &rules,
        r#"{"rules":[{"match":{"event_contains":"MARK"},"action":{"deny":{"message":"m"}}}]}"#,
    )
    .unwrap();
    let spy = dir.join("spy.jsonl");

    let out = run(
        &[
            "run",
            "--harness",
            "crush",
            "--prompt",
            "hi",
            "--cwd",
            dir.to_str().unwrap(),
            "--mock-rules",
            rules.to_str().unwrap(),
            "--spy-file",
            spy.to_str().unwrap(),
            "--bin",
            &bin_override("crush"),
            "--compact",
        ],
        &[("MOCK_CAT_FILE", config.to_str().unwrap())],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report = json_stdout(&out);
    // Mid-run state (the mock harness's stdout IS the config file): the mock
    // hook was installed AND the pre-existing config survived beside it.
    let mid_run = report["results"][0]["stdout"].as_str().unwrap();
    assert!(mid_run.contains("mock crush --rules"), "{mid_run}");
    assert!(mid_run.contains("--spy-file"), "{mid_run}");
    assert!(
        mid_run.contains("existing-hook"),
        "layering must preserve the existing hook: {mid_run}"
    );
    assert!(
        mid_run.contains("unrelated"),
        "layering must preserve unrelated keys: {mid_run}"
    );
    // Afterwards: byte-identical restore.
    assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
    // The report records the mock inputs.
    assert_eq!(
        report["mock_rules"]["rules"][0]["action"]["deny"]["message"],
        "m"
    );
    assert!(report["spy_file"].as_str().unwrap().ends_with("spy.jsonl"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Codex delivery: the hooks file is created in a fresh `.codex/`, its
/// hook-engine opt-in flags are appended to the argv automatically, and the
/// created file AND directory are removed afterwards (nothing left behind in
/// a workspace that had no `.codex`).
#[test]
fn run_mock_rules_codex_appends_opt_in_flags_and_removes_created_files() {
    let dir = std::env::temp_dir().join(format!("oneharness-mockcodex-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let rules = dir.join("rules.json");
    std::fs::write(
        &rules,
        r#"{"rules":[{"match":{"event_contains":"MARK"},"action":{"rewrite":{"input":{"command":"printf ok"}}}}]}"#,
    )
    .unwrap();
    let argv_file = dir.join("argv.txt");

    let out = run(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "hi",
            "--cwd",
            dir.to_str().unwrap(),
            "--mock-rules",
            rules.to_str().unwrap(),
            "--bin",
            &bin_override("codex"),
            "--compact",
        ],
        &[
            ("MOCK_ARGV_FILE", argv_file.to_str().unwrap()),
            (
                "MOCK_CAT_FILE",
                dir.join(".codex/hooks.json").to_str().unwrap(),
            ),
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report = json_stdout(&out);
    // The opt-in flags rode the argv (also visible in the reported command).
    let argv = std::fs::read_to_string(&argv_file).unwrap();
    assert!(argv.contains("features.hooks=true"), "{argv}");
    assert!(argv.contains("--dangerously-bypass-hook-trust"), "{argv}");
    // Mid-run the hooks file existed and carried the mock command...
    let mid_run = report["results"][0]["stdout"].as_str().unwrap();
    assert!(mid_run.contains("mock codex --rules"), "{mid_run}");
    // ...and afterwards both the created file and its created dir are gone.
    assert!(!dir.join(".codex/hooks.json").exists());
    assert!(!dir.join(".codex").exists(), "created dir must be pruned");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Claude Code delivery: zero workspace mutation — the hook rides a per-run
/// `--settings <tempfile>` argument (contents proven mid-run by catting the
/// file the argv names), and the temp file is deleted afterwards.
#[test]
fn run_mock_rules_claude_rides_settings_flag_with_no_workspace_files() {
    let dir = std::env::temp_dir().join(format!("oneharness-mockclaude-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let rules = dir.join("rules.json");
    std::fs::write(
        &rules,
        r#"{"rules":[{"match":{"tool":"Bash","event_contains":"MARK"},"action":{"deny":{"message":"m"}}}]}"#,
    )
    .unwrap();
    let argv_file = dir.join("argv.txt");

    let out = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--cwd",
            dir.to_str().unwrap(),
            "--mock-rules",
            rules.to_str().unwrap(),
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[
            ("MOCK_ARGV_FILE", argv_file.to_str().unwrap()),
            ("MOCK_CAT_ARG_AFTER", "--settings"),
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report = json_stdout(&out);
    // Mid-run, the file the argv's --settings named carried the hook JSON.
    let mid_run = report["results"][0]["stdout"].as_str().unwrap();
    assert!(mid_run.contains("PreToolUse"), "{mid_run}");
    assert!(mid_run.contains("mock claude-code --rules"), "{mid_run}");
    // The temp settings file the argv referenced is deleted afterwards.
    let argv = std::fs::read_to_string(&argv_file).unwrap();
    let lines: Vec<&str> = argv.lines().collect();
    let settings_path = lines
        .iter()
        .position(|a| *a == "--settings")
        .and_then(|i| lines.get(i + 1))
        .expect("argv must carry --settings <path>");
    assert!(
        !std::path::Path::new(settings_path).exists(),
        "temp settings must be deleted: {settings_path}"
    );
    // No config files were created in the workspace.
    assert!(!dir.join(".claude").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Spy-only mode: `--spy-file` without `--mock-rules` installs a pure
/// observer hook (no `--rules` on the hook command), and the report reflects
/// spy-without-rules.
#[test]
fn run_spy_file_alone_installs_a_pure_observer() {
    let dir = std::env::temp_dir().join(format!("oneharness-spyonly-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let spy = dir.join("spy.jsonl");
    let out = run(
        &[
            "run",
            "--harness",
            "crush",
            "--prompt",
            "hi",
            "--cwd",
            dir.to_str().unwrap(),
            "--spy-file",
            spy.to_str().unwrap(),
            "--bin",
            &bin_override("crush"),
            "--compact",
        ],
        &[("MOCK_CAT_FILE", dir.join("crush.json").to_str().unwrap())],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report = json_stdout(&out);
    let mid_run = report["results"][0]["stdout"].as_str().unwrap();
    assert!(mid_run.contains("mock crush --spy-file"), "{mid_run}");
    assert!(
        !mid_run.contains("--rules"),
        "spy-only must carry no ruleset: {mid_run}"
    );
    assert!(report["mock_rules"].is_null());
    assert!(report["spy_file"].as_str().is_some());
    // The created config file is removed afterwards (fresh workspace).
    assert!(!dir.join("crush.json").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Everything refusable is refused loudly BEFORE any file is touched or
/// process spawned: no one-shot delivery (qwen, copilot), an action the
/// harness can't express, and print-command combination (clap).
#[test]
fn run_mock_rules_refusals_are_loud_and_touch_nothing() {
    let dir = std::env::temp_dir().join(format!("oneharness-mockrefuse-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let rules = dir.join("rules.json");
    std::fs::write(
        &rules,
        r#"{"rules":[{"match":{"event_contains":"MARK"},"action":{"rewrite":{"input":{"command":"printf ok"}}}}]}"#,
    )
    .unwrap();
    let base = |id: &str| {
        run(
            &[
                "run",
                "--harness",
                id,
                "--prompt",
                "hi",
                "--cwd",
                dir.to_str().unwrap(),
                "--mock-rules",
                rules.to_str().unwrap(),
                "--bin",
                &bin_override(id),
            ],
            &[],
        )
    };
    // qwen/copilot: no one-shot delivery.
    for id in ["qwen", "copilot"] {
        let out = base(id);
        assert_eq!(out.status.code(), Some(2), "{id}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("cannot take a one-shot mock hook"),
            "{id}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    // goose: delivery exists but the ruleset asks for a rewrite it can't express.
    let out = base("goose");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot express the mock action"));
    // Nothing was installed by any refusal.
    assert!(!dir.join(".agents").exists());
    // --print-command is refused by clap (there is nothing to install for a dry run).
    let out = run(
        &[
            "run",
            "--harness",
            "crush",
            "--prompt",
            "hi",
            "--mock-rules",
            rules.to_str().unwrap(),
            "--print-command",
        ],
        &[],
    );
    assert_eq!(out.status.code(), Some(2));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The `stub` action: declare only the output; oneharness generates the
/// safely-quoted printf rewrite itself (nothing user-authored executes).
#[test]
fn mock_stub_action_compiles_to_a_safe_printf_rewrite() {
    let dir = std::env::temp_dir().join(format!("oneharness-stub-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let rules = dir.join("rules.json");
    std::fs::write(
        &rules,
        r#"{"rules":[{"match":{"event_contains":"git status"},"action":{"stub":{"output":"nothing to commit, it's clean"}}},{"match":{"event_contains":"flaky"},"action":{"stub":{"output":"boom","exit_code":3}}}]}"#,
    )
    .unwrap();
    let rules = rules.to_str().unwrap();

    // The verdict is an input rewrite whose command prints the declared output
    // verbatim (single-quote escaping included) with a trailing newline.
    let out = run_with_stdin(
        &["mock", "crush", "--rules", rules],
        r#"{"tool_name":"bash","tool_input":{"command":"git status"}}"#,
    );
    let v = json_stdout(&out);
    assert_eq!(v["decision"], "allow");
    assert_eq!(
        v["updated_input"]["command"],
        "printf '%s\\n' 'nothing to commit, it'\\''s clean'"
    );
    // exit_code fakes a failing command.
    let out = run_with_stdin(
        &["mock", "claude-code", "--rules", rules],
        r#"{"tool_name":"Bash","tool_input":{"command":"run the flaky thing"}}"#,
    );
    let v = json_stdout(&out);
    assert_eq!(
        v["hookSpecificOutput"]["updatedInput"]["command"],
        "printf '%s\\n' 'boom'; exit 3"
    );
    // The spy log records the action as `stub`.
    let spy = dir.join("spy.jsonl");
    let out = run_with_stdin(
        &[
            "mock",
            "crush",
            "--rules",
            rules,
            "--spy-file",
            spy.to_str().unwrap(),
        ],
        r#"{"tool_name":"bash","tool_input":{"command":"git status"}}"#,
    );
    assert!(out.status.success());
    let line: serde_json::Value = serde_json::from_str(
        std::fs::read_to_string(&spy)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(line["action"], "stub");
    // A stub is a rewrite underneath, so a rewrite-less harness refuses it loudly.
    let out = run_with_stdin(&["mock", "goose", "--rules", rules], "{}");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot express the mock action `stub`"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// `--stream` + `--mock-rules`: the ephemeral hook is delivered, the stream
/// path still emits NDJSON + a terminal result line (carrying `mock_rules`),
/// and the restore runs on the streaming exit path too — the created plugin
/// file and its directory are gone afterwards.
#[test]
fn run_stream_with_mock_rules_restores_on_the_streaming_path() {
    let dir = std::env::temp_dir().join(format!("oneharness-mockstream-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let rules = dir.join("rules.json");
    std::fs::write(
        &rules,
        r#"{"rules":[{"match":{"event_contains":"MARK"},"action":{"stub":{"output":"ok"}}}]}"#,
    )
    .unwrap();
    let out = run(
        &[
            "run",
            "--harness",
            "opencode",
            "--prompt",
            "hi",
            "--cwd",
            dir.to_str().unwrap(),
            "--mock-rules",
            rules.to_str().unwrap(),
            "--bin",
            &bin_override("opencode"),
            "--stream",
        ],
        &[(
            "MOCK_STDOUT",
            r#"{"type":"text","part":{"type":"text","text":"done"}}"#,
        )],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    let last: serde_json::Value =
        serde_json::from_str(text.lines().last().expect("a terminal line")).unwrap();
    assert_eq!(last["type"], "result");
    assert!(
        !last["report"]["mock_rules"].is_null(),
        "the streaming report must record the ruleset"
    );
    // The JS-plugin install (a created file) was removed on the streaming path.
    assert!(
        !dir.join(".opencode").exists(),
        "restore must run after a stream"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A multi-harness mocked run delivers per harness and restores per harness:
/// a pre-existing config is snapshotted BEFORE its own install (never after
/// another harness's) and comes back byte-identical, while a fresh harness's
/// created file and directory are removed.
#[test]
fn run_mock_rules_multi_harness_restores_each_config_independently() {
    let dir = std::env::temp_dir().join(format!("oneharness-mockmulti-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let crush_config = dir.join("crush.json");
    let original = r#"{"mine":{"keep":1}}"#;
    std::fs::write(&crush_config, original).unwrap();
    let rules = dir.join("rules.json");
    std::fs::write(
        &rules,
        r#"{"rules":[{"match":{"event_contains":"MARK"},"action":{"deny":{"message":"m"}}}]}"#,
    )
    .unwrap();
    let out = run(
        &[
            "run",
            "--harness",
            "crush,codex",
            "--prompt",
            "hi",
            "--cwd",
            dir.to_str().unwrap(),
            "--mock-rules",
            rules.to_str().unwrap(),
            "--bin",
            &bin_override("crush"),
            "--bin",
            &bin_override("codex"),
            "--compact",
        ],
        &[],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report = json_stdout(&out);
    assert_eq!(report["results"].as_array().unwrap().len(), 2);
    // Pre-existing config restored byte-identically; fresh one fully removed.
    assert_eq!(std::fs::read_to_string(&crush_config).unwrap(), original);
    assert!(!dir.join(".codex").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A batch run (one harness, N prompts) under --mock-rules installs once,
/// applies the hook args to every fanned-out job, and restores afterwards.
#[test]
fn run_mock_rules_works_with_a_batch_and_restores_once() {
    let dir = std::env::temp_dir().join(format!("oneharness-mockbatch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let rules = dir.join("rules.json");
    std::fs::write(
        &rules,
        r#"{"rules":[{"match":{"event_contains":"MARK"},"action":{"stub":{"output":"ok"}}}]}"#,
    )
    .unwrap();
    let out = run(
        &[
            "run",
            "--harness",
            "crush",
            "--prompt",
            "one",
            "--prompt",
            "two",
            "--cwd",
            dir.to_str().unwrap(),
            "--mock-rules",
            rules.to_str().unwrap(),
            "--bin",
            &bin_override("crush"),
            "--compact",
        ],
        &[],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report = json_stdout(&out);
    assert_eq!(report["results"].as_array().unwrap().len(), 2);
    assert_eq!(report["batch"]["prompt_count"], 2);
    assert!(!report["mock_rules"].is_null());
    // The (created) config file is gone after the batch completes.
    assert!(!dir.join("crush.json").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Regex + per-field input matching through the real `mock` binary: a rule
/// that fires only on a `bash` tool whose `command` argument matches a regex,
/// and an invalid regex is a loud usage error.
#[test]
fn mock_regex_and_input_matching_end_to_end() {
    let (dir, _rules) = mock_fixture("regexmatch");
    let rules = dir.join("rx.json");
    std::fs::write(
        &rules,
        r#"{"rules":[{"match":{"tool_regex":"^(?i)bash$","input":{"command":{"regex":"git\\s+push"}}},"action":{"deny":{"message":"no pushing"}}}]}"#,
    )
    .unwrap();
    let rules = rules.to_str().unwrap();

    // Matches: bash tool + command matching the regex → deny.
    let out = run_with_stdin(
        &["mock", "claude-code", "--rules", rules],
        r#"{"tool_name":"Bash","tool_input":{"command":"git   push origin"}}"#,
    );
    let v = json_stdout(&out);
    assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "deny");
    assert_eq!(
        v["hookSpecificOutput"]["permissionDecisionReason"],
        "no pushing"
    );

    // Right tool, non-matching command → fall through (empty stdout).
    let out = run_with_stdin(
        &["mock", "claude-code", "--rules", rules],
        r#"{"tool_name":"Bash","tool_input":{"command":"git status"}}"#,
    );
    assert!(out.status.success() && out.stdout.is_empty());

    // An invalid regex is a loud usage error before any event is read.
    let bad = dir.join("bad-rx.json");
    std::fs::write(
        &bad,
        r#"{"rules":[{"match":{"event_regex":"("},"action":{"deny":{"message":"m"}}}]}"#,
    )
    .unwrap();
    let out = run_with_stdin(
        &["mock", "claude-code", "--rules", bad.to_str().unwrap()],
        "{}",
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("invalid mock rules"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn mock_startup_faults_are_loud_usage_errors() {
    let (dir, rules) = mock_fixture("errors");
    let rules_arg = rules.to_str().unwrap();

    // Unknown harness.
    let out = run_with_stdin(&["mock", "nope", "--rules", rules_arg], "{}");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown harness"));

    // Missing rules file.
    let missing = dir.join("absent.json");
    let out = run_with_stdin(
        &["mock", "claude-code", "--rules", missing.to_str().unwrap()],
        "{}",
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("could not read mock rules"));

    // Invalid rules (empty match) — refused before any event is read.
    let bad = dir.join("bad.json");
    std::fs::write(
        &bad,
        r#"{"rules":[{"match":{},"action":{"deny":{"message":"m"}}}]}"#,
    )
    .unwrap();
    let out = run_with_stdin(
        &["mock", "claude-code", "--rules", bad.to_str().unwrap()],
        "{}",
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("invalid mock rules"));

    // A rewrite rule for a harness with no verified rewrite shape (Goose) is a
    // loud refusal, never a silent allow — the capability gap must be visible.
    let out = run_with_stdin(&["mock", "goose", "--rules", rules_arg], "{}");
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("cannot express the mock action `rewrite`"),
        "{stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// `sync --global` installs `[[hooks]]` at each harness's user-global location
/// (resolved from the injected HOME / XDG_CONFIG_HOME), not the project.
#[test]
fn sync_global_installs_hooks_at_user_locations() {
    let dir = std::env::temp_dir().join(format!("oneharness-global-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = dir.join("oh.toml");
    std::fs::write(
        &cfg,
        "[[hooks]]\ncommand = \"oneharness gate {harness} --deny-if-contains BLOCK\"\nplugin_name = \"ohg\"\n",
    )
    .unwrap();
    let home = dir.join("home");
    let xdg = dir.join("xdg");
    let out = Command::new(oneharness_bin())
        .env("HOME", &home)
        // On Windows the home base resolves from USERPROFILE (HOME is a Git Bash
        // ism), so set it too to keep this hermetic on every platform.
        .env("USERPROFILE", &home)
        .env("XDG_CONFIG_HOME", &xdg)
        .args([
            "sync",
            "--harness",
            "claude-code,opencode,copilot",
            "--global",
            "--config",
            cfg.to_str().unwrap(),
            "--cwd",
            dir.to_str().unwrap(),
            "--compact",
        ])
        .output()
        .expect("failed to run oneharness");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // HOME-anchored (claude, copilot's `.copilot/hooks`) and XDG-anchored (opencode).
    assert!(
        home.join(".claude/settings.json").is_file(),
        "claude global"
    );
    assert!(
        home.join(".copilot/hooks/ohg.json").is_file(),
        "copilot global"
    );
    assert!(
        xdg.join("opencode/plugin/ohg.js").is_file(),
        "opencode global"
    );
    // Nothing leaked into the project directory.
    assert!(!dir.join(".claude").exists(), "project must be untouched");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sync_global_refuses_project_only_settings() {
    let dir = std::env::temp_dir().join(format!("oneharness-global-bad-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = dir.join("oh.toml");
    std::fs::write(
        &cfg,
        "[harness.claude-code]\nallowed_tools = [\"Bash(ls)\"]\n",
    )
    .unwrap();
    let out = Command::new(oneharness_bin())
        .env("HOME", dir.join("home"))
        .args([
            "sync",
            "--harness",
            "claude-code",
            "--global",
            "--config",
            cfg.to_str().unwrap(),
            "--cwd",
            dir.to_str().unwrap(),
        ])
        .output()
        .expect("failed to run oneharness");
    assert_eq!(out.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("installs hooks only"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn config_command_attributes_settings_tables() {
    let fx = ConfigFixture::new(
        "cmd-settings",
        "[harness.opencode.settings.permission]\nedit = \"deny\"\n",
        "",
    );
    let output = run_with_config(
        &["config", "--cwd", &fx.cwd(), "--compact"],
        &[],
        &fx.user_config(),
    );
    let value = json_stdout(&output);
    let settings = &value["harness"]["opencode"]["settings"];
    assert_eq!(settings["value"]["permission"]["edit"], "deny");
    assert!(settings["source"]
        .as_str()
        .unwrap()
        .ends_with("oneharness.toml"));
}

#[test]
fn prompt_file_reads_the_prompt_from_a_file() {
    // `--prompt-file PATH` is the file-backed alternative to `--prompt`; the file
    // contents become the prompt verbatim and reach the harness argv.
    let dir = std::env::temp_dir().join(format!("oneharness-pf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("prompt.txt");
    std::fs::write(&file, "prompt-from-a-file").unwrap();

    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt-file",
            &file.display().to_string(),
            "--print-command",
            "--compact",
        ],
        &[],
    );
    let _ = std::fs::remove_dir_all(&dir);
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let value = json_stdout(&output);
    let command: Vec<String> = value["results"][0]["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect();
    assert!(
        command.iter().any(|t| t == "prompt-from-a-file"),
        "{command:?}"
    );
}

#[test]
fn prompt_file_dash_reads_the_prompt_from_stdin() {
    // `--prompt-file -` reads the prompt from stdin (how a pipeline feeds a long
    // or generated prompt without a temp file).
    let output = run_with_stdin(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt-file",
            "-",
            "--print-command",
            "--compact",
        ],
        "prompt-from-stdin",
    );
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let value = json_stdout(&output);
    let command: Vec<String> = value["results"][0]["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect();
    assert!(
        command.iter().any(|t| t == "prompt-from-stdin"),
        "{command:?}"
    );
}

#[test]
fn prompt_file_missing_path_is_a_usage_error() {
    // A `--prompt-file` pointing at a nonexistent path is a clean usage error
    // (exit 2) with the path surfaced, not a panic.
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt-file",
            "/no/such/oneharness-prompt-file-xyz",
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("oneharness-prompt-file-xyz"),
        "stderr should name the bad path: {stderr}"
    );
}

#[test]
fn system_file_reads_the_system_prompt_from_a_file() {
    // `--system-file PATH` is the file-backed alternative to `--system` (the
    // argv-limit escape hatch, mirroring `--prompt-file`): the file contents
    // become the system prompt and reach the harness argv identically.
    let dir = std::env::temp_dir().join(format!("oneharness-sf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("system.txt");
    std::fs::write(&file, "be terse").unwrap();

    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--system-file",
            &file.display().to_string(),
            "--print-command",
            "--compact",
        ],
        &[],
    );
    let _ = std::fs::remove_dir_all(&dir);
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let command = command_of(&json_stdout(&output), 0);
    assert!(
        command
            .windows(2)
            .any(|w| w == ["--append-system-prompt", "be terse"]),
        "{command:?}"
    );
}

#[test]
fn system_file_value_reaches_a_spawned_harness_intact() {
    // The positive `--system-file` tests above pin the *built* command; this one
    // actually SPAWNS (the mock harness via --bin) and asserts the file-sourced
    // system prompt arrives at the child argv byte-identically — the runtime proof
    // that `--system-file` behaves exactly like `--system`, not just in print.
    let dir = std::env::temp_dir().join(format!("oneharness-sfspawn-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("system.txt");
    // A leading `---` (YAML front matter) that would be misparsed as a flag on
    // argv is safe here because it comes from the file, not the command line.
    let system = "---\nname: reviewer\nbe terse";
    std::fs::write(&file, system).unwrap();
    let argv_file = dir.join("argv.txt");

    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--system-file",
            &file.display().to_string(),
            "--mode",
            "bypass",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[("MOCK_ARGV_FILE", &argv_file.display().to_string())],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(json_stdout(&output)["results"][0]["status"], "ok");
    let received =
        std::fs::read_to_string(&argv_file).expect("mock recorded no argv — the spawn failed");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        received.contains("--append-system-prompt"),
        "argv: {received:?}"
    );
    assert!(
        received.contains(system),
        "the file-sourced --system value must reach the child intact: {received:?}"
    );
}

#[test]
fn system_file_dash_reads_the_system_prompt_from_stdin() {
    // `--system-file -` reads the system prompt from stdin — how a pipeline feeds a
    // large or generated system prompt without a temp file.
    let output = run_with_stdin(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--system-file",
            "-",
            "--print-command",
            "--compact",
        ],
        "be terse",
    );
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let command = command_of(&json_stdout(&output), 0);
    assert!(
        command
            .windows(2)
            .any(|w| w == ["--append-system-prompt", "be terse"]),
        "{command:?}"
    );
}

#[test]
fn system_file_missing_path_is_a_usage_error() {
    // A `--system-file` pointing at a nonexistent path is a clean usage error
    // (exit 2) with the path surfaced, not a panic.
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--system-file",
            "/no/such/oneharness-system-file-xyz",
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("oneharness-system-file-xyz"),
        "stderr should name the bad path: {stderr}"
    );
}

#[test]
fn system_and_system_file_together_is_a_usage_error() {
    // `--system` and `--system-file` are two spellings of one input (clap
    // `conflicts_with`), so passing both is a clean usage error, not a silent
    // pick-one.
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--system",
            "a",
            "--system-file",
            "s.txt",
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--system-file") || stderr.contains("--system"),
        "{stderr}"
    );
}

#[test]
fn system_file_and_prompt_file_both_stdin_is_a_usage_error() {
    // stdin can be consumed only once, so `--prompt-file -` and `--system-file -`
    // together is a usage error caught before any read (never blocks on stdin).
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt-file",
            "-",
            "--system-file",
            "-",
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("stdin"), "{stderr}");
}

#[test]
fn large_system_file_avoids_the_argv_limit() {
    // The point of `--system-file`: a system prompt too large for a single argv
    // string (Linux's ~128 KiB MAX_ARG_STRLEN) would fail the caller's spawn of
    // oneharness with E2BIG if passed via `--system`. Delivered as a file path,
    // oneharness's own argv stays small and it reads the whole prompt — proven
    // here with a >128 KiB body that round-trips into the built command.
    let dir = std::env::temp_dir().join(format!("oneharness-sfbig-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("big-system.txt");
    let marker = "SYSTEM-MARKER-42";
    let big = format!("{marker}\n{}", "x".repeat(200 * 1024));
    std::fs::write(&file, &big).unwrap();

    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--system-file",
            &file.display().to_string(),
            "--print-command",
            "--compact",
        ],
        &[],
    );
    let _ = std::fs::remove_dir_all(&dir);
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let command = command_of(&json_stdout(&output), 0);
    assert!(
        command.iter().any(|t| t == &big),
        "the whole >128 KiB system prompt should reach the built command"
    );
    assert!(command.iter().any(|t| t.contains(marker)), "{marker}");
}

// --- Structured output (--schema): validate the final answer against a JSON
// Schema, delivering it natively where supported (Claude Code) or via the
// prompt otherwise, and re-prompting on a validation failure. All hermetic
// through the mock fixture.

const PERSON_SCHEMA: &str = r#"{"type":"object",
    "properties":{"name":{"type":"string"},"age":{"type":"integer"}},
    "required":["name","age"],"additionalProperties":false}"#;

/// Write `contents` to a unique temp file and return its path (string). Temp
/// files are fine to leave behind; the OS reclaims them.
fn temp_file(tag: &str, contents: &str) -> String {
    let path = std::env::temp_dir().join(format!(
        "oneharness-{tag}-{}-{}.json",
        std::process::id(),
        tag
    ));
    std::fs::write(&path, contents).unwrap();
    path.display().to_string()
}

/// A fresh, nonexistent counter path for MOCK_ATTEMPT_FILE.
fn temp_counter(tag: &str) -> String {
    let path =
        std::env::temp_dir().join(format!("oneharness-counter-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_file(&path);
    path.display().to_string()
}

#[test]
fn schema_prompt_based_validates_and_appends_instruction() {
    // A non-native harness (crush): the schema is appended to the prompt, and the
    // mock returns a conforming JSON object that oneharness validates.
    let schema = temp_file("schema-pb", PERSON_SCHEMA);
    let output = run(
        &[
            "run",
            "--harness",
            "crush",
            "--prompt",
            "describe ada",
            "--schema",
            &schema,
            "--bin",
            &bin_override("crush"),
            "--compact",
        ],
        &[("MOCK_STDOUT", r#"{"name":"Ada","age":36}"#)],
    );
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let value = json_stdout(&output);
    let result = &value["results"][0];
    assert_eq!(result["schema_valid"], true);
    assert_eq!(result["schema_attempts"], 1);
    assert!(result["schema_error"].is_null());
    assert_eq!(result["structured"]["name"], "Ada");
    assert_eq!(result["structured"]["age"], 36);
    // The schema instruction reached the prompt (non-native delivery)...
    let command = command_of(&value, 0);
    assert!(
        command.iter().any(|a| a.contains("JSON Schema")),
        "prompt should carry the schema instruction: {command:?}"
    );
    // ...and it stays newline-free, so the prompt argument survives being passed
    // to a `.cmd` harness shim on Windows.
    assert!(
        command.iter().all(|a| !a.contains('\n')),
        "schema prompt must be newline-free: {command:?}"
    );
    // The report echoes the applied schema and retry budget.
    assert_eq!(value["schema"]["required"][0], "name");
    assert_eq!(value["schema_max_retries"], 2);
}

#[test]
fn schema_native_claude_reads_structured_output_field() {
    // Claude Code's native path: `--json-schema` on the argv, the value read from
    // the result document's `structured_output`, and the prompt left untouched.
    let schema = temp_file("schema-native", PERSON_SCHEMA);
    let stdout = r#"{"type":"result","result":"Here is Ada.",
        "structured_output":{"name":"Ada","age":36}}"#;
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "describe ada",
            "--schema",
            &schema,
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[("MOCK_STDOUT", stdout)],
    );
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let value = json_stdout(&output);
    let result = &value["results"][0];
    assert_eq!(result["schema_valid"], true);
    assert_eq!(result["structured"]["age"], 36);
    let command = command_of(&value, 0);
    assert!(
        command.iter().any(|a| a == "--json-schema"),
        "native schema flag missing: {command:?}"
    );
    assert!(
        command.windows(2).any(|w| w == ["--output-format", "json"]),
        "native schema forces json output: {command:?}"
    );
    // Native delivery does NOT augment the prompt.
    assert!(
        !command.iter().any(|a| a.contains("JSON Schema")),
        "native delivery should not touch the prompt: {command:?}"
    );
}

#[test]
fn schema_retry_loop_recovers_on_a_later_attempt() {
    // First response misses `age` (invalid); the retry returns a conforming one.
    let schema = temp_file("schema-retry", PERSON_SCHEMA);
    let counter = temp_counter("retry");
    let output = run(
        &[
            "run",
            "--harness",
            "crush",
            "--prompt",
            "describe ada",
            "--schema",
            &schema,
            "--schema-max-retries",
            "3",
            "--bin",
            &bin_override("crush"),
            "--compact",
        ],
        &[
            ("MOCK_ATTEMPT_FILE", &counter),
            ("MOCK_STDOUT_1", r#"{"name":"Ada"}"#),
            ("MOCK_STDOUT_2", r#"{"name":"Ada","age":36}"#),
        ],
    );
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let value = json_stdout(&output);
    let result = &value["results"][0];
    assert_eq!(result["schema_valid"], true);
    assert_eq!(
        result["schema_attempts"], 2,
        "should have retried exactly once"
    );
    assert_eq!(result["structured"]["age"], 36);
}

#[test]
fn schema_invalid_after_retries_is_a_failure() {
    // Every attempt misses `age`; after the budget is spent the run fails with the
    // last invalid value and a validation error surfaced.
    let schema = temp_file("schema-fail", PERSON_SCHEMA);
    let output = run(
        &[
            "run",
            "--harness",
            "crush",
            "--prompt",
            "describe ada",
            "--schema",
            &schema,
            "--schema-max-retries",
            "1",
            "--bin",
            &bin_override("crush"),
            "--compact",
        ],
        &[("MOCK_STDOUT", r#"{"name":"Ada"}"#)],
    );
    // A structured-output run that never conforms is a failure (exit 1).
    assert_eq!(output.status.code(), Some(1));
    let value = json_stdout(&output);
    let result = &value["results"][0];
    assert_eq!(result["schema_valid"], false);
    assert_eq!(result["schema_attempts"], 2, "1 initial + 1 retry");
    assert!(!result["schema_error"].as_str().unwrap().is_empty());
    // The non-conforming value is still surfaced for inspection.
    assert_eq!(result["structured"]["name"], "Ada");
}

#[test]
fn schema_deferred_tool_does_not_burn_retries() {
    // Under `--schema`, a deferred-tool dead-end (issue #1114) is deterministic:
    // the deployment defers every time, so re-prompting only burns real model
    // calls. The retry loop must stop after the first attempt and classify it as
    // `tool_deferred` — not exhaust the whole `--schema-max-retries` budget.
    let schema = temp_file("schema-deferred", PERSON_SCHEMA);
    let counter = temp_counter("deferred");
    let deferred = r#"{"type":"result","stop_reason":"tool_deferred","result":"",
        "deferred_tool_use":{"name":"Read"}}"#;
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "describe ada",
            "--schema",
            &schema,
            "--schema-max-retries",
            "3",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[("MOCK_ATTEMPT_FILE", &counter), ("MOCK_STDOUT", deferred)],
    );
    assert_eq!(output.status.code(), Some(1));
    let value = json_stdout(&output);
    let result = &value["results"][0];
    assert_eq!(result["failure_kind"], "tool_deferred");
    assert_eq!(result["schema_valid"], false);
    assert_eq!(
        result["schema_attempts"], 1,
        "a deterministic deferral must not be retried"
    );
    // The harness binary was actually invoked exactly once (no wasted calls).
    let count = std::fs::read_to_string(&counter).unwrap();
    assert_eq!(count.trim(), "1", "harness should run exactly once");
}

#[test]
fn schema_no_json_in_response_is_invalid() {
    // A response with no extractable JSON is invalid (not a fabricated value), and
    // with zero retries it fails on the first attempt.
    let schema = temp_file("schema-nojson", PERSON_SCHEMA);
    let output = run(
        &[
            "run",
            "--harness",
            "crush",
            "--prompt",
            "describe ada",
            "--schema",
            &schema,
            "--schema-max-retries",
            "0",
            "--bin",
            &bin_override("crush"),
            "--compact",
        ],
        &[("MOCK_STDOUT", "I cannot help with that.")],
    );
    assert_eq!(output.status.code(), Some(1));
    let value = json_stdout(&output);
    let result = &value["results"][0];
    assert_eq!(result["schema_valid"], false);
    assert_eq!(result["schema_attempts"], 1);
    assert!(result["structured"].is_null());
    assert!(result["schema_error"].as_str().unwrap().contains("no JSON"));
}

#[test]
fn schema_fields_are_null_without_a_schema() {
    // A normal run carries the schema fields as nulls, so the shape is stable.
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[("MOCK_STDOUT", r#"{"result":"hi"}"#)],
    );
    let value = json_stdout(&output);
    assert!(value["schema"].is_null());
    assert!(value["schema_max_retries"].is_null());
    let result = &value["results"][0];
    assert!(result["schema_valid"].is_null());
    assert!(result["structured"].is_null());
    assert!(result["schema_attempts"].is_null());
}

#[test]
fn missing_schema_file_is_a_usage_error() {
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--schema",
            "/no/such/oneharness-schema-xyz.json",
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("could not read schema file"),
        "stderr should explain the missing schema: {stderr}"
    );
}

#[test]
fn invalid_schema_is_a_usage_error() {
    // A file that parses as JSON but is not a valid schema fails loudly before any
    // harness runs.
    let schema = temp_file("schema-bad", r#"{"type": 5}"#);
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--schema",
            &schema,
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("invalid --schema"),
        "stderr should reject the bad schema: {stderr}"
    );
}

#[test]
fn schema_file_from_config_is_loaded_relative_to_the_project() {
    // The schema path can come from `oneharness.toml` (resolved against the
    // project dir), not just `--schema`.
    let project = "harnesses = [\"crush\"]\nschema_file = \"person.json\"\n";
    let fx = ConfigFixture::new("schemacfg", project, "");
    std::fs::write(
        std::path::Path::new(&fx.cwd()).join("person.json"),
        PERSON_SCHEMA,
    )
    .unwrap();
    let output = run_with_config(
        &[
            "run",
            "--prompt",
            "hi",
            "--cwd",
            &fx.cwd(),
            "--bin",
            &bin_override("crush"),
            "--compact",
        ],
        &[("MOCK_STDOUT", r#"{"name":"Ada","age":36}"#)],
        &fx.user_config(),
    );
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let value = json_stdout(&output);
    assert_eq!(value["results"][0]["schema_valid"], true);
    assert_eq!(value["schema_max_retries"], 2);
}

#[test]
fn list_exposes_native_schema_capability() {
    let output = run(&["list", "--compact"], &[]);
    let value = json_stdout(&output);
    let harnesses = value["harnesses"].as_array().unwrap();
    let claude = harnesses.iter().find(|h| h["id"] == "claude-code").unwrap();
    assert_eq!(claude["supports_native_schema"], true);
    let crush = harnesses.iter().find(|h| h["id"] == "crush").unwrap();
    assert_eq!(crush["supports_native_schema"], false);
}

// --- same-prefix batch mode (one harness, N prompts) ------------------------

/// Count `S` (start) lines in the mock run log before the first `E` (end) line.
/// With a per-call sleep longer than spawn latency, this reveals scheduling: a
/// single wave that fires everything at once shows every `S` before any `E`; a
/// `min-tokens` warm-up shows exactly one `S` (and its `E`) before the rest start.
fn starts_before_first_end(log: &str) -> usize {
    let mut count = 0;
    for line in log.lines() {
        match line.trim() {
            "S" => count += 1,
            "E" => break,
            _ => {}
        }
    }
    count
}

/// A unique scratch path for a batch test's run log.
fn batch_log_path(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("oneharness-batchlog-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

#[test]
fn batch_speed_fires_every_prompt_in_one_wave() {
    let log = batch_log_path("speed");
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "first",
            "--prompt",
            "second",
            "--prompt",
            "third",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[
            ("MOCK_LOG_FILE", &log.display().to_string()),
            ("MOCK_SLEEP_MS", "500"),
        ],
    );
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let value = json_stdout(&output);

    // One harness fanned over three prompts → three results, each tagged with its
    // own prompt, in order; the batch block records the strategy.
    assert_eq!(value["batch"]["strategy"], "speed");
    assert_eq!(value["batch"]["prompt_count"], 3);
    let results = value["results"].as_array().unwrap();
    assert_eq!(results.len(), 3);
    assert_eq!(results[0]["prompt"], "first");
    assert_eq!(results[1]["prompt"], "second");
    assert_eq!(results[2]["prompt"], "third");
    // Top-level prompt repeats the first for back-compat.
    assert_eq!(value["prompt"], "first");
    for r in results {
        assert_eq!(r["harness"], "claude-code");
        assert_eq!(r["status"], "ok");
    }

    // Scheduling: all three start before any finishes — a single concurrent wave.
    let recorded = std::fs::read_to_string(&log).expect("mock wrote no log");
    let _ = std::fs::remove_file(&log);
    assert_eq!(
        starts_before_first_end(&recorded),
        3,
        "speed should fire all prompts at once; log: {recorded:?}"
    );
}

#[test]
fn batch_min_tokens_warms_one_then_fans_the_rest() {
    let log = batch_log_path("min-tokens");
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--batch-strategy",
            "min-tokens",
            "--prompt",
            "warm",
            "--prompt",
            "fan-a",
            "--prompt",
            "fan-b",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[
            ("MOCK_LOG_FILE", &log.display().to_string()),
            ("MOCK_SLEEP_MS", "500"),
        ],
    );
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let value = json_stdout(&output);
    assert_eq!(value["batch"]["strategy"], "min-tokens");
    let results = value["results"].as_array().unwrap();
    assert_eq!(results.len(), 3);
    // Results stay in prompt order regardless of the warm-then-fan scheduling.
    assert_eq!(results[0]["prompt"], "warm");
    assert_eq!(results[2]["prompt"], "fan-b");

    // Scheduling: exactly one call runs (and completes) before the rest begin —
    // the warm-up wave that writes the shared cache prefix.
    let recorded = std::fs::read_to_string(&log).expect("mock wrote no log");
    let _ = std::fs::remove_file(&log);
    assert_eq!(
        starts_before_first_end(&recorded),
        1,
        "min-tokens should issue exactly one warm-up call first; log: {recorded:?}"
    );
}

#[test]
fn single_prompt_run_is_not_a_batch() {
    // The ordinary path is unchanged: one prompt, no batch block, no per-result
    // prompt — even with --batch-strategy set (it only matters for many prompts).
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--batch-strategy",
            "min-tokens",
            "--prompt",
            "solo",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[],
    );
    assert!(output.status.success());
    let value = json_stdout(&output);
    assert!(value["batch"].is_null());
    assert!(value["results"][0]["prompt"].is_null());
    assert_eq!(value["prompt"], "solo");
}

#[test]
fn batch_combines_prompt_and_prompt_file_in_order() {
    let dir = std::env::temp_dir().join(format!("oneharness-batchpf-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("p.txt");
    std::fs::write(&file, "from-file").unwrap();

    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "from-flag",
            "--prompt-file",
            &file.display().to_string(),
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[],
    );
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let value = json_stdout(&output);
    let results = value["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);
    // --prompt values come first, then --prompt-file contents (read whole).
    assert_eq!(results[0]["prompt"], "from-flag");
    assert_eq!(results[1]["prompt"], "from-file");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn batch_with_multiple_harnesses_is_a_usage_error() {
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code,codex",
            "--prompt",
            "one",
            "--prompt",
            "two",
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("exactly one harness"), "{stderr}");
}

#[test]
fn batch_with_all_is_a_usage_error() {
    let output = run(&["run", "--all", "--prompt", "one", "--prompt", "two"], &[]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("exactly one harness"), "{stderr}");
}

#[test]
fn batch_with_resume_is_a_usage_error() {
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--resume",
            "sid-123",
            "--prompt",
            "one",
            "--prompt",
            "two",
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("batch"), "{stderr}");
}

#[test]
fn batch_output_dir_disambiguates_same_harness_results() {
    let dir = std::env::temp_dir().join(format!("oneharness-batchout-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "a",
            "--prompt",
            "b",
            "--output-dir",
            &dir.display().to_string(),
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[("MOCK_STDOUT", r#"{"result":"ok"}"#)],
    );
    assert!(output.status.success());
    // Same harness twice → indexed file stems, so neither overwrites the other.
    assert!(dir.join("claude-code-0.stdout").exists());
    assert!(dir.join("claude-code-1.stdout").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn print_command_works_for_a_batch() {
    // A dry-run batch builds one command per prompt without spawning anything.
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--print-command",
            "--prompt",
            "alpha",
            "--prompt",
            "beta",
        ],
        &[],
    );
    assert!(output.status.success());
    let value = json_stdout(&output);
    assert_eq!(value["dry_run"], true);
    assert_eq!(value["batch"]["prompt_count"], 2);
    let results = value["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);
    assert!(command_of(&value, 0).contains(&"alpha".to_string()));
    assert!(command_of(&value, 1).contains(&"beta".to_string()));
}

#[test]
fn batch_repeated_stdin_prompt_file_is_a_usage_error() {
    // `-` (stdin) can be consumed only once; two of them is a usage error caught
    // before any read, so it never blocks waiting on stdin.
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt-file",
            "-",
            "--prompt-file",
            "-",
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("stdin"), "{stderr}");
}

#[test]
fn batch_applies_the_schema_to_every_prompt() {
    // Structured output composes with batch: each prompt is validated
    // independently, so every result reports schema_valid.
    let dir = std::env::temp_dir().join(format!("oneharness-batchschema-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let schema = dir.join("s.json");
    std::fs::write(&schema, r#"{"type":"object","required":["a"]}"#).unwrap();

    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--schema",
            &schema.display().to_string(),
            "--prompt",
            "q1",
            "--prompt",
            "q2",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[("MOCK_STDOUT", r#"{"result":"{\"a\":1}"}"#)],
    );
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let value = json_stdout(&output);
    assert_eq!(value["batch"]["prompt_count"], 2);
    let results = value["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);
    for r in results {
        assert_eq!(r["schema_valid"], true, "result: {r}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn batch_with_an_unavailable_harness_skips_every_prompt() {
    // A missing binary in batch mode skips each prompt (one skipped result per
    // prompt), exits 0 by default, and still reports the batch block.
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "a",
            "--prompt",
            "b",
            "--bin",
            "claude-code=/no/such/oneharness-binary-xyz",
            "--compact",
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(0));
    let value = json_stdout(&output);
    assert_eq!(value["batch"]["prompt_count"], 2);
    let results = value["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);
    for r in results {
        assert_eq!(r["status"], "skipped");
    }
}

#[test]
fn batch_min_tokens_forks_the_warmed_session_for_the_fan_out() {
    // claude-code is fork-capable, so a min-tokens batch warms prompt[0] (a
    // session) then FORKS it for the rest, reusing the warmed cached prefix. The
    // mock emits a session_id so the fork wiring engages: the fan-out commands
    // must carry --resume <sid> --fork-session and drop --system (inherited from
    // the session), the warm-up must not, and the report marks the batch forked.
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--batch-strategy",
            "min-tokens",
            "--system",
            "shared context",
            "--prompt",
            "warm",
            "--prompt",
            "q1",
            "--prompt",
            "q2",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[("MOCK_STDOUT", r#"{"result":"ok","session_id":"SID-XYZ"}"#)],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = json_stdout(&output);
    assert_eq!(value["batch"]["strategy"], "min-tokens");
    assert_eq!(value["batch"]["forked"], true);

    let warm = command_of(&value, 0);
    // Warm-up establishes the session: no --resume, and it carries --system.
    assert!(!warm.contains(&"--resume".to_string()), "warm-up: {warm:?}");
    assert!(
        warm.contains(&"--append-system-prompt".to_string()),
        "warm-up should send --system: {warm:?}"
    );
    // Fan-out forks the warmed session and does not re-send --system.
    for i in [1, 2] {
        let fan = command_of(&value, i);
        assert!(
            fan.windows(2).any(|w| w == ["--resume", "SID-XYZ"]),
            "fan-out {i} should resume the warmed session: {fan:?}"
        );
        assert!(
            fan.contains(&"--fork-session".to_string()),
            "fan-out {i} should fork: {fan:?}"
        );
        assert!(
            !fan.contains(&"--append-system-prompt".to_string()),
            "fan-out {i} must not re-send --system (inherited from the session): {fan:?}"
        );
    }
}

#[test]
fn batch_min_tokens_without_a_cache_reusing_fork_warns_and_does_not_fork() {
    // min-tokens only saves on a harness with a *cache-reusing* fork. Codex can't
    // fork at all; OpenCode can fork but its fork re-sends the prefix cold
    // (fork_reuses_cache = false). Both must stay order-only: the report marks
    // them not forked, oneharness warns on stderr, and the fan-out is NOT forked,
    // while still producing one result per prompt.
    for (id, fork_flag) in [("codex", "--fork-session"), ("opencode", "--fork")] {
        let output = run(
            &[
                "run",
                "--harness",
                id,
                "--batch-strategy",
                "min-tokens",
                "--prompt",
                "a",
                "--prompt",
                "b",
                "--bin",
                &bin_override(id),
                "--compact",
            ],
            // A session_id the fork path *would* pick up if it engaged — proving it
            // does not for these harnesses.
            &[(
                "MOCK_STDOUT",
                r#"{"result":"ok","session_id":"SID-1","sessionID":"SID-1"}"#,
            )],
        );
        assert!(
            output.status.success(),
            "{id} exit {:?}",
            output.status.code()
        );
        let value = json_stdout(&output);
        assert_eq!(value["batch"]["forked"], false, "{id} should not fork");
        assert_eq!(value["results"].as_array().unwrap().len(), 2, "{id}");
        // The fan-out must not carry the fork flag.
        assert!(
            !command_of(&value, 1).contains(&fork_flag.to_string()),
            "{id} fan-out must not fork: {:?}",
            command_of(&value, 1)
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("only orders the calls"), "{id}: {stderr}");
    }
}

// ---------------------------------------------------------------------------
// History: opt-in, streamed, standardized run history + the `history` verb.
// ---------------------------------------------------------------------------

/// A unique, absent temp history dir for one test (removed on entry and by the
/// caller at the end).
fn hist_dir(tag: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("oneharness-histtest-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn materialized_history(path: &Path) -> Vec<Value> {
    oneharness_core::io::history::read_session(path)
        .unwrap()
        .into_iter()
        .map(|record| serde_json::to_value(record).unwrap())
        .collect()
}

fn first_history_run(path: &Path) -> Value {
    materialized_history(path).into_iter().next().unwrap()
}

const HISTORY_CODEX_TELEMETRY: &str = concat!(
    "{\"type\":\"turn.started\"}\n",
    "{\"type\":\"item.completed\",\"item\":{\"id\":\"m1\",\"type\":\"agent_message\",\"text\":\"x\"}}\n",
    "{\"type\":\"turn.completed\"}\n",
);
const HISTORY_BOTH_TRACES: &str = concat!(
    "{\"type\":\"turn.started\"}\n",
    "{\"type\":\"step_start\",\"part\":{\"type\":\"step-start\"}}\n",
    "{\"type\":\"item.completed\",\"item\":{\"id\":\"m1\",\"type\":\"agent_message\",\"text\":\"x\"}}\n",
    "{\"type\":\"text\",\"part\":{\"type\":\"text\",\"text\":\"x\"}}\n",
    "{\"type\":\"turn.completed\"}\n",
    "{\"type\":\"step_finish\",\"part\":{\"type\":\"step-finish\"}}\n",
);

#[test]
fn history_records_a_run_and_reports_the_file() {
    let dir = hist_dir("record");
    let ds = dir.display().to_string();
    let argv_file = dir.with_extension("argv");
    let argv = argv_file.display().to_string();
    let output = run(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "Fix the login bug",
            "--bin",
            &bin_override("codex"),
            "--history",
            "--history-dir",
            &ds,
            "--bypass",
            "--compact",
        ],
        &[
            (
                "MOCK_STDOUT",
                "{\"type\":\"turn.started\"}\n{\"type\":\"thread.started\",\"thread_id\":\"s1\"}\n{\"type\":\"item.completed\",\"item\":{\"id\":\"m1\",\"type\":\"agent_message\",\"text\":\"done\"}}\n{\"type\":\"turn.completed\"}\n",
            ),
            ("MOCK_ARGV_FILE", &argv),
        ],
    );
    assert!(output.status.success());
    let value = json_stdout(&output);
    // The report carries the absolute session-file path (the programmatic handle).
    let hf = value["history_file"].as_str().expect("history_file set");
    assert!(hf.ends_with(".jsonl"), "{hf}");
    // The file holds one normalized record with the prompt-derived name.
    let rec = first_history_run(Path::new(hf));
    assert_eq!(rec["harness"], "codex");
    assert_eq!(rec["name"], "fix-the-login-bug");
    assert_eq!(rec["status"], "ok");
    assert_eq!(rec["session_id"], "s1");
    assert_eq!(rec["permission_mode"], "bypass");
    assert!(
        std::fs::read_to_string(&argv_file)
            .unwrap()
            .lines()
            .any(|arg| arg == "--json"),
        "history must request telemetry without --events"
    );
    // Normalized only — no raw stdout/stderr leaks into history.
    assert!(rec.get("stdout").is_none());
    let _ = std::fs::remove_file(argv_file);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn history_rejects_unrecognized_or_incomplete_traces_without_fabricating_v03() {
    for (name, stdout) in [
        ("empty", ""),
        ("malformed", "not-json\n"),
        ("compact-in-stream", r#"{"result":"done"}"#),
    ] {
        let dir = hist_dir(name);
        let ds = dir.display().to_string();
        let output = run(
            &[
                "run",
                "--harness",
                "codex",
                "--prompt",
                "invalid trace",
                "--bin",
                &bin_override("codex"),
                "--history",
                "--history-dir",
                &ds,
                "--bypass",
                "--compact",
            ],
            &[("MOCK_STDOUT", stdout)],
        );
        assert!(output.status.success(), "{name}");
        let report = json_stdout(&output);
        let path = Path::new(report["history_file"].as_str().unwrap());
        assert!(
            !path.exists(),
            "{name} must not write an invalid or fabricated v1.0 run"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("lacks complete v1.0 telemetry"),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    let unsupported_dir = hist_dir("plain");
    let unsupported = run(
        &[
            "run",
            "--harness",
            "goose",
            "--prompt",
            "plain",
            "--bin",
            &bin_override("goose"),
            "--history",
            "--history-dir",
            &unsupported_dir.display().to_string(),
            "--compact",
        ],
        &[("MOCK_STDOUT", "plain text")],
    );
    assert!(unsupported.status.success());
    let report = json_stdout(&unsupported);
    assert_eq!(report["results"][0]["status"], "ok");
    let record = first_history_run(Path::new(report["history_file"].as_str().unwrap()));
    assert_eq!(record["schema_version"], "1.1");
    for field in [
        "started_at",
        "model_ms",
        "tool_ms",
        "time_to_first_token_ms",
        "observed_tool_ms",
    ] {
        assert!(
            record.get(field).is_none(),
            "{field} must not be fabricated"
        );
    }
    assert!(record["finished_at"].is_null());
    let _ = std::fs::remove_dir_all(&unsupported_dir);

    // Real Anthropic-style CLI envelopes expose init, assistant content/tool
    // blocks, and terminal result aggregation, but no provider-request start.
    let anthropic = concat!(
        "{\"type\":\"system\",\"subtype\":\"init\"}\n",
        "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"Bash\",\"input\":{}}]}}\n",
        "{\"type\":\"user\",\"message\":{\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t1\",\"content\":\"ok\"}]}}\n",
        "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"done\"}]}}\n",
        "{\"type\":\"result\",\"result\":\"done\"}\n",
    );
    for id in ["claude-code", "qwen", "cursor"] {
        let dir = hist_dir(id);
        let output = run(
            &[
                "run",
                "--harness",
                id,
                "--prompt",
                "unmeasured",
                "--bin",
                &bin_override(id),
                "--history",
                "--history-dir",
                &dir.display().to_string(),
                "--compact",
            ],
            &[("MOCK_STDOUT", anthropic), ("MOCK_STREAM_DELAY_MS", "60")],
        );
        assert!(
            output.status.success(),
            "{id}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report = json_stdout(&output);
        assert_eq!(report["results"][0]["status"], "ok", "{id}");
        let report_call = report["results"][0]["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event["kind"] == "tool_call")
            .unwrap();
        assert_eq!(
            report_call["timing_source"], "stdout_observed",
            "{id}: report event provenance"
        );
        // The aggregate reaches the report through `telemetry`, tagged with the
        // source that produced it — a consumer reads it here rather than
        // re-opening the history file the same run just wrote.
        let telemetry = &report["results"][0]["telemetry"];
        assert_eq!(telemetry["source"], "stdout_observed", "{id}");
        assert!(
            report["results"][0].get("observed_tool_ms").is_none(),
            "{id}: the flat field name stays a history-only spelling"
        );
        let history_path = Path::new(report["history_file"].as_str().unwrap());
        assert!(
            history_path.exists(),
            "{id}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let record = first_history_run(history_path);
        assert_eq!(record["schema_version"], "1.2", "{id}");
        assert!(record.get("model_ms").is_none(), "{id}");
        assert!(record.get("tool_ms").is_none(), "{id}");
        assert!(record.get("time_to_first_token_ms").is_none(), "{id}");
        let observed = record["observed_tool_ms"].as_u64().unwrap();
        assert!(observed >= 40, "{id}: {observed}");
        let call = record["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event["kind"] == "tool_call")
            .unwrap();
        assert_eq!(call["timing_source"], "stdout_observed", "{id}");
        assert!(call["started_at"].is_string(), "{id}");
        assert!(call["finished_at"].is_string(), "{id}");
        assert_eq!(call["status"], "completed", "{id}");
        assert_eq!(call["duration_ms"], record["observed_tool_ms"], "{id}");
        assert_eq!(
            report["results"][0]["telemetry"]["tool_ms"], record["observed_tool_ms"],
            "{id}: report and history must agree on one measurement"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    let dir = hist_dir("claude-incomplete-observed-tool");
    let incomplete = concat!(
        "{\"type\":\"system\",\"subtype\":\"init\"}\n",
        "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"tool_use\",\"id\":\"open\",\"name\":\"Bash\",\"input\":{}}]}}\n",
        "{\"type\":\"result\",\"result\":\"stopped\"}\n",
    );
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "incomplete",
            "--bin",
            &bin_override("claude-code"),
            "--history",
            "--history-dir",
            &dir.display().to_string(),
            "--compact",
        ],
        &[("MOCK_STDOUT", incomplete), ("MOCK_STREAM_DELAY_MS", "40")],
    );
    assert!(output.status.success());
    let report = json_stdout(&output);
    let record = first_history_run(Path::new(report["history_file"].as_str().unwrap()));
    let call = record["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["kind"] == "tool_call")
        .unwrap();
    assert_eq!(call["timing_source"], "stdout_observed");
    assert_eq!(call["status"], "interrupted");
    assert!(call["finished_at"].is_null());
    assert!(call["duration_ms"].is_null());
    assert!(record["observed_tool_ms"].as_u64().is_some());
    assert!(record.get("model_ms").is_none());
    let _ = std::fs::remove_dir_all(dir);
}

/// A telemetry/history-recording shortfall must never fail a run whose harness
/// actually succeeded. codex here returns a real answer, but the trace carries
/// no provider-request boundary (`turn.started`), so its v1.0 telemetry is
/// incomplete and no history record can be written. The run must still exit 0,
/// surface the harness's successful result and answer text, and only warn about
/// the skipped record — never discard the work (the 0.5.4 regression that took
/// down every codex worker and its orchestrator by exiting 1 here).
#[test]
fn incomplete_history_telemetry_warns_but_preserves_a_successful_run() {
    let dir = hist_dir("incomplete-telemetry-resilient");
    // A valid codex answer with no `turn.started`/`turn.completed` boundary: the
    // answer extracts fine, but v1.0 timing telemetry cannot be derived.
    let trace =
        "{\"type\":\"item.completed\",\"item\":{\"id\":\"m1\",\"type\":\"agent_message\",\"text\":\"the answer is 42\"}}\n";
    let output = run(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "q",
            "--bin",
            &bin_override("codex"),
            "--history",
            "--history-dir",
            &dir.display().to_string(),
            "--bypass",
            "--compact",
        ],
        &[("MOCK_STDOUT", trace)],
    );
    // The exit status reflects the harness result, not the history-write.
    assert!(
        output.status.success(),
        "an incomplete history record must not fail the run: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("could not write history record")
            && stderr.contains("lacks complete v1.0 telemetry"),
        "the shortfall is warned about: {stderr}"
    );
    let report = json_stdout(&output);
    // The harness's successful result is returned intact.
    assert_eq!(report["results"][0]["status"], "ok");
    assert_eq!(report["results"][0]["text"], "the answer is 42");
    // No partial/corrupt history record was written.
    assert!(!Path::new(report["history_file"].as_str().unwrap()).exists());
    let _ = std::fs::remove_dir_all(dir);
}

/// A run that fails before the provider produces an answer has no measured
/// telemetry *because* it failed — killed at launch, or cut short mid-turn. It
/// must still be recorded. An overnight orchestration whose codex workers were
/// all quota-killed left no history at all: every failed run was refused as
/// lacking complete v1.0 telemetry, so the only runs history could hold were the
/// ones an operator never needs to see.
#[test]
fn history_records_a_failure_whose_telemetry_could_not_be_measured() {
    for (tag, stdout, interrupted_tool, partial_answer) in [
        // Refused at launch: the provider never emitted a byte.
        ("launch", "", false, None),
        // Cut short mid-turn: a request boundary and a started tool call, but no
        // provider finish and no answer, so no timing can be derived.
        (
            "mid-turn",
            concat!(
                "{\"type\":\"turn.started\"}\n",
                "{\"type\":\"item.started\",\"item\":{\"id\":\"cmd1\",\"type\":\"command_execution\",\"command\":\"echo hi\",\"aggregated_output\":\"\",\"exit_code\":null,\"status\":\"in_progress\"}}\n",
            ),
            true,
            None,
        ),
        // Cut short *after* the model had begun answering: the transcript yields
        // partial assistant text but never a completed turn. The run's own
        // verdict is what says it failed — reading the salvaged text as success
        // would refuse exactly the record an operator needs.
        (
            "partial-answer",
            concat!(
                "{\"type\":\"turn.started\"}\n",
                "{\"type\":\"item.completed\",\"item\":{\"id\":\"m1\",\"type\":\"agent_message\",\"text\":\"I was still thinking when\"}}\n",
            ),
            false,
            Some("I was still thinking when"),
        ),
    ] {
        let dir = hist_dir(&format!("failed-{tag}"));
        let ds = dir.display().to_string();
        let output = run(
            &[
                "run",
                "--harness",
                "codex",
                "--prompt",
                "quota killed",
                "--bin",
                &bin_override("codex"),
                "--history",
                "--history-dir",
                &ds,
                "--bypass",
                "--compact",
            ],
            &[
                ("MOCK_STDOUT", stdout),
                ("MOCK_EXIT", "1"),
                (
                    "MOCK_STDERR",
                    "Error: insufficient_quota — your credit balance is too low",
                ),
            ],
        );
        // The run failed, so oneharness exits 1 — but history is not collateral.
        assert_eq!(output.status.code(), Some(1), "{tag}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !stderr.contains("could not write history record"),
            "{tag}: the failure must be recorded, not refused: {stderr}"
        );
        let report = json_stdout(&output);
        let record = first_history_run(Path::new(report["history_file"].as_str().unwrap()));
        // Exactly what an operator diagnosing the incident needs.
        assert_eq!(record["status"], "nonzero", "{tag}");
        assert_eq!(record["exit_code"], 1, "{tag}");
        assert_eq!(record["failure_kind"], "quota", "{tag}");
        assert!(record["duration_ms"].is_u64(), "{tag}");
        assert_eq!(record["prompt"], "quota killed", "{tag}");
        // The harness's own account of the failure, taken from the stderr
        // oneharness captured — the field a classified `failure_kind` alone
        // could never carry, and the reason the record declares v1.3.
        assert_eq!(
            record["error"],
            "Error: insufficient_quota — your credit balance is too low",
            "{tag}"
        );
        assert_eq!(record["schema_version"], "1.3", "{tag}");
        // Partial telemetry, preserved rather than discarded with the split: the
        // instant the runner itself watched the invocation start is measured, so
        // it survives; the provider/tool split a transcript that stopped mid-turn
        // could never yield is absent, never fabricated.
        assert!(record["started_at"].is_string(), "{tag}");
        assert!(record.get("model_ms").is_none(), "{tag}");
        assert!(record.get("tool_ms").is_none(), "{tag}");
        assert!(record["finished_at"].is_null(), "{tag}");
        // The report says the same thing, and says *which* kind of measurement
        // it is — so a consumer never reads a bare `started_at` as a full trace.
        let telemetry = &report["results"][0]["telemetry"];
        assert_eq!(telemetry["source"], "partial_invocation", "{tag}");
        assert_eq!(telemetry["started_at"], record["started_at"], "{tag}");
        assert!(telemetry.get("model_ms").is_none(), "{tag}");
        // Salvaged provider text is reported as-is; absent stays null, never
        // filled in from the error text.
        assert_eq!(
            record["text"],
            partial_answer.map_or(Value::Null, |text| Value::String(text.to_string())),
            "{tag}"
        );
        if interrupted_tool {
            // Whatever partial telemetry the failure left is kept.
            assert_eq!(record["events"][0]["kind"], "tool_call", "{tag}");
            assert_eq!(record["events"][0]["status"], "interrupted", "{tag}");
        } else {
            assert!(record["events"].is_null(), "{tag}");
        }
        // `history show` serves the failed run like any other record.
        let shown = json_stdout(&run(
            &[
                "history",
                "show",
                "--last",
                "--all-projects",
                "--history-dir",
                &ds,
                "--compact",
            ],
            &[],
        ));
        assert_eq!(shown[0]["history_id"], record["history_id"], "{tag}");
        assert_eq!(shown[0]["failure_kind"], "quota", "{tag}");
        assert_eq!(shown[0]["error"], record["error"], "{tag}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// A deferred-tool dead-end exits 0 having done no work (issue #1114), so its
/// history record is the one case where a *clean* exit still carries failure
/// text: oneharness's own actionable message, alongside the `tool_deferred` kind.
/// Refusing it here would hide the dead-end from the operator exactly as a
/// refused quota failure did.
#[test]
fn history_records_the_failure_text_of_a_clean_exit_dead_end() {
    let dir = hist_dir("deferred-dead-end");
    let ds = dir.display().to_string();
    // A real Claude Code bridge deployment's shape: exit 0, empty result, and a
    // deferred builtin tool instead of an executed one.
    let stdout = r#"{"type":"result","num_turns":1,"stop_reason":"tool_deferred",
        "terminal_reason":"tool_deferred","result":"","permission_denials":[],
        "deferred_tool_use":{"name":"Read","input":{"file_path":"/x/usage.rs"}}}"#;
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "read the file",
            "--bin",
            &bin_override("claude-code"),
            "--history",
            "--history-dir",
            &ds,
            "--bypass",
            "--compact",
        ],
        &[("MOCK_STDOUT", stdout)],
    );
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("could not write history record"),
        "the dead-end must be recorded: {stderr}"
    );
    let report = json_stdout(&output);
    let record = first_history_run(Path::new(report["history_file"].as_str().unwrap()));
    // The process exited 0, so `status` says so; the failure is the typed signal.
    assert_eq!(record["status"], "ok");
    assert_eq!(record["failure_kind"], "tool_deferred");
    let error = record["error"]
        .as_str()
        .expect("a dead-end records its actionable failure text");
    assert!(
        error.contains("`Read`") && error.contains("deferred"),
        "the recorded text names the deferred tool: {error}"
    );
    assert_eq!(record["schema_version"], "1.3");
    let shown = json_stdout(&run(
        &[
            "history",
            "show",
            "--last",
            "--all-projects",
            "--history-dir",
            &ds,
            "--compact",
        ],
        &[],
    ));
    assert_eq!(shown[0]["error"], record["error"]);
    let _ = std::fs::remove_dir_all(dir);
}

/// A run can fail silently: a non-zero exit with nothing on stderr and no
/// oneharness diagnostic of its own. There is then no failure text to record —
/// the field is omitted rather than invented — but the invocation the runner
/// watched is still measured, so the record carries invocation-bounds-only
/// timing and declares the version that shape arrived in.
#[test]
fn a_silent_failure_records_partial_timing_without_inventing_failure_text() {
    let dir = hist_dir("silent-failure");
    let ds = dir.display().to_string();
    let output = run(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "silent",
            "--bin",
            &bin_override("codex"),
            "--history",
            "--history-dir",
            &ds,
            "--bypass",
            "--compact",
        ],
        // Exits 1 having written nothing at all — no transcript, no stderr.
        &[("MOCK_EXIT", "1"), ("MOCK_STDOUT", ""), ("MOCK_STDERR", "")],
    );
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("could not write history record"),
        "a silent failure is still recorded: {stderr}"
    );
    let report = json_stdout(&output);
    let record = first_history_run(Path::new(report["history_file"].as_str().unwrap()));
    assert_eq!(record["status"], "nonzero");
    assert_eq!(record["exit_code"], 1);
    assert!(record["failure_kind"].is_null());
    assert!(
        record.get("error").is_none(),
        "nothing was reported, so nothing is invented: {record}"
    );
    // The partial measurement alone forces the version forward.
    assert!(record["started_at"].is_string());
    assert!(record.get("model_ms").is_none());
    assert_eq!(record["schema_version"], "1.3");
    let shown = json_stdout(&run(
        &[
            "history",
            "show",
            "--last",
            "--all-projects",
            "--history-dir",
            &ds,
            "--compact",
        ],
        &[],
    ));
    assert_eq!(shown[0]["started_at"], record["started_at"]);
    assert!(shown[0].get("error").is_none());
    let _ = std::fs::remove_dir_all(dir);
}

/// A harness that floods stderr must not flood history with it. The bound is a
/// property of the persisted record, so assert it there — on the file a consumer
/// reads — rather than only on the pure helper that applies it.
#[test]
fn a_flooding_failure_is_recorded_within_the_documented_bound() {
    let dir = hist_dir("bounded-failure-text");
    let bound = oneharness_core::domain::history::ERROR_MAX;
    // Leading whitespace to trim, then far more than the bound allows.
    let flood = format!("\n\n{}\n", "e".repeat(bound * 3));
    let output = run(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "flooded",
            "--bin",
            &bin_override("codex"),
            "--history",
            "--history-dir",
            &dir.display().to_string(),
            "--bypass",
            "--compact",
        ],
        &[("MOCK_EXIT", "1"), ("MOCK_STDERR", &flood)],
    );
    assert_eq!(output.status.code(), Some(1));
    let report = json_stdout(&output);
    let record = first_history_run(Path::new(report["history_file"].as_str().unwrap()));
    let error = record["error"]
        .as_str()
        .expect("the failure text is recorded");
    assert_eq!(
        error.chars().count(),
        bound,
        "the recorded failure text is bounded in characters"
    );
    assert!(
        error.starts_with('e') && error.ends_with('\u{2026}'),
        "surrounding whitespace is trimmed and the cut is marked: {:?}",
        &error[..8]
    );
    // Bounded or not, the record still reads back through the public contract.
    let shown = json_stdout(&run(
        &[
            "history",
            "show",
            "--last",
            "--all-projects",
            "--history-dir",
            &dir.display().to_string(),
            "--compact",
        ],
        &[],
    ));
    assert_eq!(shown[0]["error"], record["error"]);
    let _ = std::fs::remove_dir_all(dir);
}

/// The README states the failure text's bound and the version that introduced it
/// in prose, which is where a reader learns them — the same numbers the record
/// contract enforces. Tie the two together rather than letting a changed constant
/// leave the docs confidently wrong (the pattern
/// [`documented_usage_timeout_default_tracks_the_flag_constant`] uses).
#[test]
fn documented_history_failure_text_tracks_the_record_contract() {
    let readme = include_str!("../README.md");
    let bound = format!(
        "bounded to {} characters",
        oneharness_core::domain::history::ERROR_MAX
    );
    assert!(
        readme.contains(&bound),
        "README.md must state the failure-text bound as `{bound}`"
    );
    let introduced = format!(
        "both arrived in history schema **v{}**",
        oneharness_core::domain::history::FIRST_ERROR_SCHEMA_VERSION
    );
    assert_eq!(
        oneharness_core::domain::history::FIRST_PARTIAL_TIMING_SCHEMA_VERSION,
        oneharness_core::domain::history::FIRST_ERROR_SCHEMA_VERSION,
        "the README says `both`, so the two gates must name one version"
    );
    assert!(
        readme.contains(&introduced),
        "README.md must state where the failure text arrived as `{introduced}`"
    );
    let unchanged = format!(
        "a provider-measured success still declares `{}`",
        oneharness_core::domain::history::PREVIOUS_CURRENT_SCHEMA_VERSION
    );
    assert!(
        readme.contains(&unchanged),
        "README.md must state the unchanged provider-measured version as `{unchanged}`"
    );
}

/// The failure text is a *failure* signal: a run that succeeded never gets one,
/// so the field is absent from its record rather than present and empty, and its
/// record keeps the older version that carries no such field.
#[test]
fn a_successful_run_records_no_failure_text() {
    let dir = hist_dir("no-failure-text");
    let output = run(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "worked",
            "--bin",
            &bin_override("codex"),
            "--history",
            "--history-dir",
            &dir.display().to_string(),
            "--bypass",
            "--compact",
        ],
        &[
            ("MOCK_STDOUT", HISTORY_CODEX_TELEMETRY),
            // Harnesses chatter on stderr even when they work; none of it is a
            // failure, so none of it reaches history.
            ("MOCK_STDERR", "warning: using a deprecated flag"),
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json_stdout(&output);
    let record = first_history_run(Path::new(report["history_file"].as_str().unwrap()));
    assert_eq!(record["status"], "ok");
    assert!(
        record.get("error").is_none(),
        "an absent failure text is omitted, not written as null or empty: {record}"
    );
    assert_eq!(record["schema_version"], "1.1");
    let _ = std::fs::remove_dir_all(dir);
}

/// The carve-out is keyed on the run having failed, not on how it failed. A hang
/// the timeout killed, a binary the OS refuses to execute, and a harness that is
/// not installed each produce no provider output at all, and each must still
/// leave a record naming what happened rather than vanishing from history.
#[test]
fn history_records_every_shape_of_run_that_never_produced_output() {
    // Each case's `error` comes from oneharness's own diagnostic rather than the
    // child's stderr — the harness never got far enough to write one, so this is
    // the only account of what happened. `bounds` says which of the two honest
    // timing shapes to expect: a run that was spawned leaves the invocation start
    // the runner watched, a harness that was never spawned leaves nothing at all.
    for (tag, bin, extra, envs, status, exit, error_needle, bounds) in [
        (
            "timeout",
            bin_override("codex"),
            vec!["--timeout", "1"],
            vec![("MOCK_SLEEP_MS", "5000")],
            "timeout",
            Some(1),
            "timeout",
            true,
        ),
        (
            "spawn-error",
            unspawnable_bin("codex"),
            vec![],
            vec![],
            "spawn-error",
            Some(1),
            "failed to spawn",
            true,
        ),
        // Not installed: no run at all, so `oneharness` itself still exits 0.
        (
            "not-installed",
            missing_bin("codex"),
            vec![],
            vec![],
            "skipped",
            Some(0),
            "not found on PATH",
            false,
        ),
    ] {
        let dir = hist_dir(&format!("no-output-{tag}"));
        let ds = dir.display().to_string();
        let mut args = vec![
            "run",
            "--harness",
            "codex",
            "--prompt",
            "never answered",
            "--bin",
            &bin,
            "--history",
            "--history-dir",
            &ds,
            "--bypass",
            "--compact",
        ];
        args.extend(extra);
        let output = run(&args, &envs);
        assert_eq!(output.status.code(), exit, "{tag}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !stderr.contains("could not write history record"),
            "{tag}: the failure must be recorded, not refused: {stderr}"
        );
        let report = json_stdout(&output);
        let record = first_history_run(Path::new(report["history_file"].as_str().unwrap()));
        assert_eq!(record["status"], status, "{tag}");
        assert!(record["text"].is_null(), "{tag}");
        // No provider/tool split either way — the trace never got far enough.
        assert!(record.get("model_ms").is_none(), "{tag}");
        assert_eq!(
            record["started_at"].is_string(),
            bounds,
            "{tag}: a spawned run keeps its observed invocation start; an \
             unspawned one has none to keep"
        );
        let error = record["error"]
            .as_str()
            .unwrap_or_else(|| panic!("{tag}: the failure text must be recorded: {record}"));
        assert!(
            error.contains(error_needle),
            "{tag}: expected oneharness's own diagnostic to mention `{error_needle}`: {error}"
        );
        // `history show` serves it like any other record.
        let shown = json_stdout(&run(
            &[
                "history",
                "show",
                "--last",
                "--all-projects",
                "--history-dir",
                &ds,
                "--compact",
            ],
            &[],
        ));
        assert_eq!(shown[0]["status"], status, "{tag}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn history_accepts_each_advertised_provider_trace_shape() {
    let cases = [
        ("codex", "{\"type\":\"turn.started\"}\n{\"type\":\"item.completed\",\"item\":{\"id\":\"m1\",\"type\":\"agent_message\",\"text\":\"done\"}}\n{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":7,\"output_tokens\":2}}\n"),
        ("opencode", "{\"type\":\"step_start\",\"part\":{\"type\":\"step-start\"}}\n{\"type\":\"text\",\"part\":{\"type\":\"text\",\"text\":\"done\"}}\n{\"type\":\"step_finish\",\"part\":{\"type\":\"step-finish\",\"tokens\":{\"input\":7,\"output\":2,\"cache\":{\"read\":3}}}}\n"),
    ];
    for (id, stdout) in cases {
        let dir = hist_dir(id);
        let output = run(
            &[
                "run",
                "--harness",
                id,
                "--prompt",
                "trace",
                "--bin",
                &bin_override(id),
                "--history",
                "--history-dir",
                &dir.display().to_string(),
                "--bypass",
                "--compact",
            ],
            &[("MOCK_STDOUT", stdout), ("MOCK_STREAM_DELAY_MS", "5")],
        );
        assert!(
            output.status.success(),
            "{id}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report = json_stdout(&output);
        let record = first_history_run(Path::new(report["history_file"].as_str().unwrap()));
        assert_eq!(record["schema_version"], "1.1", "{id}");
        if matches!(id, "codex" | "opencode") {
            assert_eq!(record["usage"]["input_tokens"], 7, "{id}");
            assert_eq!(record["usage"]["output_tokens"], 2, "{id}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn history_preserves_format_contracts_and_composes_with_resume() {
    let explicit_dir = hist_dir("explicit-format");
    let explicit = run(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "text",
            "--bin",
            &bin_override("codex"),
            "--history",
            "--history-dir",
            &explicit_dir.display().to_string(),
            "--output-format",
            "text",
            "--compact",
        ],
        &[("MOCK_STDOUT", "plain text")],
    );
    assert_eq!(explicit.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&explicit.stderr)
        .contains("needs output format `json` for history telemetry, but `text` was selected"));

    let schema = temp_file("history-native-schema", PERSON_SCHEMA);
    let native_dir = hist_dir("native-schema");
    let native = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "schema",
            "--bin",
            &bin_override("claude-code"),
            "--history",
            "--history-dir",
            &native_dir.display().to_string(),
            "--schema",
            &schema,
            "--compact",
        ],
        &[(
            "MOCK_STDOUT",
            r#"{"structured_output":{"name":"Ada","age":36}}"#,
        )],
    );
    assert!(native.status.success());

    let resume_dir = hist_dir("resume-trace");
    let resumed = run(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "continue",
            "--resume",
            "sess-1",
            "--bin",
            &bin_override("codex"),
            "--history",
            "--history-dir",
            &resume_dir.display().to_string(),
            "--compact",
        ],
        &[("MOCK_STDOUT", HISTORY_CODEX_TELEMETRY)],
    );
    assert!(
        resumed.status.success(),
        "{}",
        String::from_utf8_lossy(&resumed.stderr)
    );
    let report = json_stdout(&resumed);
    assert_eq!(report["results"][0]["text"], "x");
    let record = first_history_run(Path::new(report["history_file"].as_str().unwrap()));
    assert_eq!(record["schema_version"], "1.1");
    // A run that worked still records complete telemetry — the failure carve-out
    // never relaxes what a successful resumed run must carry.
    assert!(record["started_at"].is_string());
    assert!(record["finished_at"].is_string());
    assert!(record["model_ms"].is_u64());
    assert!(record["tool_ms"].is_u64());
    // The report carries the same measurement, so a consumer never has to open
    // the history file to read what its own run already knew.
    let telemetry = &report["results"][0]["telemetry"];
    assert_eq!(telemetry["source"], "provider_measured");
    assert_eq!(telemetry["started_at"], record["started_at"]);
    assert_eq!(telemetry["finished_at"], record["finished_at"]);
    assert_eq!(telemetry["model_ms"], record["model_ms"]);
    assert_eq!(telemetry["tool_ms"], record["tool_ms"]);

    let _ = std::fs::remove_file(schema);
    let _ = std::fs::remove_dir_all(explicit_dir);
    let _ = std::fs::remove_dir_all(native_dir);
    let _ = std::fs::remove_dir_all(resume_dir);
}

#[test]
fn history_excludes_harness_startup_from_provider_model_time() {
    let dir = hist_dir("provider-overhead");
    let ds = dir.display().to_string();
    let stdout = concat!(
        "{\"type\":\"system\",\"subtype\":\"init\"}\n",
        "{\"type\":\"turn.started\"}\n",
        "{\"type\":\"item.completed\",\"item\":{\"id\":\"m1\",\"type\":\"agent_message\",\"text\":\"done\"}}\n",
        "{\"type\":\"turn.completed\"}\n",
    );
    let output = run(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "measure overhead",
            "--bin",
            &bin_override("codex"),
            "--history",
            "--history-dir",
            &ds,
            "--bypass",
            "--compact",
        ],
        &[("MOCK_STDOUT", stdout), ("MOCK_STREAM_DELAY_MS", "60")],
    );
    assert!(output.status.success());
    let report = json_stdout(&output);
    let record = first_history_run(Path::new(report["history_file"].as_str().unwrap()));
    let duration = record["duration_ms"].as_u64().unwrap();
    let model = record["model_ms"].as_u64().unwrap();
    assert!(
        model < duration,
        "startup leaked into model time: {model}/{duration}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn history_measures_overlapping_tool_intervals_from_provider_events() {
    let dir = hist_dir("timed-tools");
    let ds = dir.display().to_string();
    let stdout = concat!(
        "{\"type\":\"turn.started\"}\n",
        "{\"type\":\"item.started\",\"item\":{\"id\":\"call-a\",\"type\":\"command_execution\"}}\n",
        "{\"type\":\"item.started\",\"item\":{\"id\":\"call-b\",\"type\":\"command_execution\"}}\n",
        "{\"type\":\"item.completed\",\"item\":{\"id\":\"call-a\",\"type\":\"command_execution\",\"command\":\"a\",\"aggregated_output\":\"ok\",\"exit_code\":0}}\n",
        "{\"type\":\"item.completed\",\"item\":{\"id\":\"call-b\",\"type\":\"command_execution\",\"command\":\"b\",\"aggregated_output\":\"ok\",\"exit_code\":0}}\n",
        "{\"type\":\"item.completed\",\"item\":{\"id\":\"m1\",\"type\":\"agent_message\",\"text\":\"done\"}}\n",
        "{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":100,\"output_tokens\":20}}\n",
    );
    let output = run(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "measure",
            "--bin",
            &bin_override("codex"),
            "--events",
            "--history",
            "--history-dir",
            &ds,
            "--bypass",
            "--compact",
        ],
        &[("MOCK_STDOUT", stdout), ("MOCK_STREAM_DELAY_MS", "40")],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json_stdout(&output);
    let record = first_history_run(Path::new(report["history_file"].as_str().unwrap()));
    assert_eq!(record["schema_version"], "1.1");
    assert!(record["time_to_first_token_ms"].as_u64().is_some());
    assert!(record["time_to_first_token_ms"].as_u64().unwrap() > 0);
    let calls = record["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["kind"] == "tool_call")
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0]["tool_call_id"], "call-a");
    assert_eq!(calls[1]["tool_call_id"], "call-b");
    assert_eq!(calls[0]["status"], "completed");
    assert!(
        calls
            .iter()
            .all(|event| event.get("timing_source").is_none()),
        "provider-measured Codex events must retain their prior serialized shape"
    );
    assert!(
        record.get("observed_tool_ms").is_none(),
        "Codex provider telemetry must not acquire observed timing fields"
    );
    let individual = calls
        .iter()
        .map(|event| event["duration_ms"].as_u64().unwrap())
        .sum::<u64>();
    let union = record["tool_ms"].as_u64().unwrap();
    assert!(
        union < individual,
        "overlap must not be double-counted: {union} vs {individual}"
    );
    assert!(
        record["model_ms"].as_u64().unwrap() + union <= record["duration_ms"].as_u64().unwrap()
    );
    assert_ne!(calls[0]["started_at"], calls[1]["started_at"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn history_uses_codex_reasoning_for_first_and_last_model_boundaries() {
    let dir = hist_dir("codex-reasoning");
    let stdout = concat!(
        "{\"type\":\"turn.started\"}\n",
        "{\"type\":\"item.completed\",\"item\":{\"id\":\"r1\",\"type\":\"reasoning\",\"text\":\"Inspecting the request\"}}\n",
        "{\"type\":\"item.completed\",\"item\":{\"id\":\"m1\",\"type\":\"agent_message\",\"text\":\"done\"}}\n",
        "{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":10,\"output_tokens\":4}}\n",
    );
    let output = run(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "reason first",
            "--bin",
            &bin_override("codex"),
            "--history",
            "--history-dir",
            &dir.display().to_string(),
            "--bypass",
            "--compact",
        ],
        &[("MOCK_STDOUT", stdout), ("MOCK_STREAM_DELAY_MS", "50")],
    );
    assert!(output.status.success());
    let report = json_stdout(&output);
    let record = first_history_run(Path::new(report["history_file"].as_str().unwrap()));
    let ttft = record["time_to_first_token_ms"].as_u64().unwrap();
    let model = record["model_ms"].as_u64().unwrap();
    assert!(ttft > 0, "reasoning TTFT: {ttft}");
    assert!(model > ttft, "reasoning through answer: {model}");
    assert!(model < record["duration_ms"].as_u64().unwrap());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn history_normalizes_codex_mcp_failure_and_interruption() {
    let failed_dir = hist_dir("codex-mcp-failed");
    let failed_trace = concat!(
        "{\"type\":\"turn.started\"}\n",
        "{\"type\":\"item.completed\",\"item\":{\"id\":\"r1\",\"type\":\"reasoning\",\"text\":\"Calling MCP\"}}\n",
        "{\"type\":\"item.started\",\"item\":{\"id\":\"mcp-1\",\"type\":\"mcp_tool_call\",\"server\":\"minimal\",\"tool\":\"count\",\"arguments\":{\"n\":2},\"status\":\"in_progress\"}}\n",
        "{\"type\":\"item.completed\",\"item\":{\"id\":\"mcp-1\",\"type\":\"mcp_tool_call\",\"server\":\"minimal\",\"tool\":\"count\",\"arguments\":{\"n\":2},\"result\":null,\"error\":{\"message\":\"server failed\"},\"status\":\"failed\"}}\n",
        "{\"type\":\"item.completed\",\"item\":{\"id\":\"m1\",\"type\":\"agent_message\",\"text\":\"could not count\"}}\n",
        "{\"type\":\"turn.completed\"}\n",
    );
    let failed = run(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "mcp",
            "--bin",
            &bin_override("codex"),
            "--history",
            "--history-dir",
            &failed_dir.display().to_string(),
            "--bypass",
            "--compact",
        ],
        &[
            ("MOCK_STDOUT", failed_trace),
            ("MOCK_STREAM_DELAY_MS", "30"),
        ],
    );
    assert!(failed.status.success());
    let report = json_stdout(&failed);
    let record = first_history_run(Path::new(report["history_file"].as_str().unwrap()));
    let calls = record["events"].as_array().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["name"], "count");
    assert_eq!(calls[0]["tool_call_id"], "mcp-1");
    assert_eq!(calls[0]["status"], "failed");
    assert!(calls[0]["duration_ms"].as_u64().unwrap() > 0);

    let interrupted_dir = hist_dir("codex-mcp-interrupted");
    let interrupted_trace = concat!(
        "{\"type\":\"turn.started\"}\n",
        "{\"type\":\"item.completed\",\"item\":{\"id\":\"r1\",\"type\":\"reasoning\",\"text\":\"Calling MCP\"}}\n",
        "{\"type\":\"item.started\",\"item\":{\"id\":\"mcp-open\",\"type\":\"mcp_tool_call\",\"server\":\"minimal\",\"tool\":\"wait\",\"arguments\":{},\"status\":\"in_progress\"}}\n",
        "{\"type\":\"progress\"}\n",
        "{\"type\":\"progress\"}\n",
        "{\"type\":\"progress\"}\n",
    );
    let interrupted = run(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "interrupt",
            "--bin",
            &bin_override("codex"),
            "--history",
            "--history-dir",
            &interrupted_dir.display().to_string(),
            "--timeout",
            "1",
            "--bypass",
            "--compact",
        ],
        &[
            ("MOCK_STDOUT", interrupted_trace),
            ("MOCK_STREAM_DELAY_MS", "300"),
        ],
    );
    assert_eq!(interrupted.status.code(), Some(1));
    let report = json_stdout(&interrupted);
    let record = first_history_run(Path::new(report["history_file"].as_str().unwrap()));
    assert_eq!(record["status"], "timeout");
    assert!(record["time_to_first_token_ms"].as_u64().unwrap() >= 200);
    let call = &record["events"][0];
    assert_eq!(call["tool_call_id"], "mcp-open");
    assert_eq!(call["status"], "timeout");
    assert!(call["finished_at"].is_null());
    assert!(call["duration_ms"].is_null());
    let _ = std::fs::remove_dir_all(failed_dir);
    let _ = std::fs::remove_dir_all(interrupted_dir);
}

#[test]
fn history_validates_codex_terminal_tool_states_without_guessing() {
    for (tag, terminal_fields, expected) in [
        ("cancelled", r#""status":"cancelled""#, "interrupted"),
        (
            "error-no-status",
            r#""error":{"message":"provider error"}"#,
            "failed",
        ),
        ("timeout", r#""status":"timed_out""#, "timeout"),
    ] {
        let dir = hist_dir(tag);
        let trace = format!(
            concat!(
                "{{\"type\":\"turn.started\"}}\n",
                "{{\"type\":\"item.completed\",\"item\":{{\"id\":\"r1\",\"type\":\"reasoning\",\"text\":\"Calling tool\"}}}}\n",
                "{{\"type\":\"item.started\",\"item\":{{\"id\":\"mcp-state\",\"type\":\"mcp_tool_call\",\"server\":\"minimal\",\"tool\":\"state\",\"arguments\":{{}},\"status\":\"in_progress\"}}}}\n",
                "{{\"type\":\"item.completed\",\"item\":{{\"id\":\"mcp-state\",\"type\":\"mcp_tool_call\",\"server\":\"minimal\",\"tool\":\"state\",\"arguments\":{{}},{}}}}}\n",
                "{{\"type\":\"item.completed\",\"item\":{{\"id\":\"m1\",\"type\":\"agent_message\",\"text\":\"done\"}}}}\n",
                "{{\"type\":\"turn.completed\"}}\n",
            ),
            terminal_fields,
        );
        let output = run(
            &[
                "run",
                "--harness",
                "codex",
                "--prompt",
                tag,
                "--bin",
                &bin_override("codex"),
                "--history",
                "--history-dir",
                &dir.display().to_string(),
                "--bypass",
                "--compact",
            ],
            &[("MOCK_STDOUT", &trace), ("MOCK_STREAM_DELAY_MS", "20")],
        );
        assert!(output.status.success(), "{tag}");
        let report = json_stdout(&output);
        let record = first_history_run(Path::new(report["history_file"].as_str().unwrap()));
        assert_eq!(record["events"][0]["status"], expected, "{tag}");
        assert!(record["events"][0]["duration_ms"].as_u64().unwrap() > 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    for (tag, tool_records) in [
        (
            "unknown-status",
            concat!(
                "{\"type\":\"item.started\",\"item\":{\"id\":\"mcp-unknown\",\"type\":\"mcp_tool_call\",\"server\":\"minimal\",\"tool\":\"state\",\"arguments\":{},\"status\":\"in_progress\"}}\n",
                "{\"type\":\"item.completed\",\"item\":{\"id\":\"mcp-unknown\",\"type\":\"mcp_tool_call\",\"server\":\"minimal\",\"tool\":\"state\",\"arguments\":{},\"status\":\"mystery\"}}\n",
            ),
        ),
        (
            "completion-only-file-change",
            "{\"type\":\"item.completed\",\"item\":{\"id\":\"patch-1\",\"type\":\"file_change\",\"changes\":[{\"path\":\"src/lib.rs\",\"kind\":\"update\"}],\"status\":\"completed\"}}\n",
        ),
    ] {
        let dir = hist_dir(tag);
        let trace = format!(
            "{{\"type\":\"turn.started\"}}\n{{\"type\":\"item.completed\",\"item\":{{\"id\":\"r1\",\"type\":\"reasoning\",\"text\":\"working\"}}}}\n{tool_records}{{\"type\":\"item.completed\",\"item\":{{\"id\":\"m1\",\"type\":\"agent_message\",\"text\":\"done\"}}}}\n{{\"type\":\"turn.completed\"}}\n"
        );
        let output = run(
            &[
                "run", "--harness", "codex", "--prompt", tag, "--bin", &bin_override("codex"),
                "--history", "--history-dir", &dir.display().to_string(), "--bypass", "--compact",
            ],
            &[("MOCK_STDOUT", &trace), ("MOCK_STREAM_DELAY_MS", "20")],
        );
        assert!(output.status.success(), "{tag}");
        let report = json_stdout(&output);
        assert!(!Path::new(report["history_file"].as_str().unwrap()).exists(), "{tag}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("lacks complete v1.0 telemetry"),
            "{tag}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// codex-cli 0.145.0 emits `file_change` items with both an `item.started`
/// (status `in_progress`) and an `item.completed` (status `completed`), so a
/// normal file-editing turn now carries a real execution boundary and must
/// produce a complete v1.0 history record — no incomplete-telemetry warning,
/// with the `file_change` surfaced as a measured tool call. This is the exact
/// shape that broke every real codex run under history in 0.5.4.
#[test]
fn history_measures_codex_file_change_with_start_and_completion() {
    let dir = hist_dir("codex-file-change");
    // Mirrors a real `codex exec --json` file-editing turn: a message, a shell
    // command with started/completed boundaries, then a `file_change` with the
    // same boundary shape, a final message, and the terminal `turn.completed`.
    let trace = concat!(
        "{\"type\":\"turn.started\"}\n",
        "{\"type\":\"item.completed\",\"item\":{\"id\":\"m0\",\"type\":\"agent_message\",\"text\":\"I'll edit it.\"}}\n",
        "{\"type\":\"item.started\",\"item\":{\"id\":\"cmd1\",\"type\":\"command_execution\",\"command\":\"echo hi\",\"aggregated_output\":\"\",\"exit_code\":null,\"status\":\"in_progress\"}}\n",
        "{\"type\":\"item.completed\",\"item\":{\"id\":\"cmd1\",\"type\":\"command_execution\",\"command\":\"echo hi\",\"aggregated_output\":\"hi\\n\",\"exit_code\":0,\"status\":\"completed\"}}\n",
        "{\"type\":\"item.started\",\"item\":{\"id\":\"fc1\",\"type\":\"file_change\",\"changes\":[{\"path\":\"note.txt\",\"kind\":\"add\"}],\"status\":\"in_progress\"}}\n",
        "{\"type\":\"item.completed\",\"item\":{\"id\":\"fc1\",\"type\":\"file_change\",\"changes\":[{\"path\":\"note.txt\",\"kind\":\"add\"}],\"status\":\"completed\"}}\n",
        "{\"type\":\"item.completed\",\"item\":{\"id\":\"m1\",\"type\":\"agent_message\",\"text\":\"Done.\"}}\n",
        "{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":10,\"cached_input_tokens\":0,\"output_tokens\":5}}\n",
    );
    let output = run(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "edit a file",
            "--bin",
            &bin_override("codex"),
            "--history",
            "--history-dir",
            &dir.display().to_string(),
            "--bypass",
            "--compact",
        ],
        &[("MOCK_STDOUT", trace), ("MOCK_STREAM_DELAY_MS", "20")],
    );
    assert!(output.status.success());
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("lacks complete v1.0 telemetry"),
        "a boundaried file_change must not degrade telemetry: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json_stdout(&output);
    let record = first_history_run(Path::new(report["history_file"].as_str().unwrap()));
    // Provider-measured Codex records deliberately retain their pre-feature 1.1
    // wire shape; history 1.2 is reserved for stdout-observed timing.
    assert_eq!(record["schema_version"], "1.1");
    assert!(record["started_at"].is_string());
    assert!(record["model_ms"].is_u64());
    assert!(record["tool_ms"].is_u64());
    // The file_change is a measured tool call, not silently dropped.
    let file_change = record["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["name"] == "file_change")
        .expect("file_change surfaced as a tool call");
    assert_eq!(file_change["kind"], "tool_call");
    assert_eq!(file_change["status"], "completed");
    assert_eq!(file_change["tool_call_id"], "fc1");
    assert!(file_change["started_at"].is_string());
    assert!(file_change["finished_at"].is_string());
    assert!(file_change["duration_ms"].is_u64());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn history_preserves_an_unfinished_tool_interval_without_fabricating_an_end() {
    let dir = hist_dir("interrupted-tool");
    let ds = dir.display().to_string();
    let stdout = concat!(
        "{\"type\":\"step_start\",\"part\":{\"type\":\"step-start\"}}\n",
        "{\"type\":\"tool_use\",\"part\":{\"id\":\"call-open\",\"type\":\"tool\",\"tool\":\"bash\",\"state\":{\"status\":\"running\",\"input\":{},\"time\":{\"start\":1773878400000}}}}\n",
        "{\"type\":\"step_finish\",\"part\":{\"type\":\"step-finish\"}}\n",
    );
    let output = run(
        &[
            "run",
            "--harness",
            "opencode",
            "--prompt",
            "interrupt",
            "--bin",
            &bin_override("opencode"),
            "--events",
            "--history",
            "--history-dir",
            &ds,
            "--bypass",
            "--compact",
        ],
        &[("MOCK_STDOUT", stdout), ("MOCK_STREAM_DELAY_MS", "30")],
    );
    assert!(output.status.success());
    let report = json_stdout(&output);
    let record = first_history_run(Path::new(report["history_file"].as_str().unwrap()));
    assert_eq!(record["schema_version"], "1.1");
    let call = record["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["kind"] == "tool_call")
        .unwrap();
    assert_eq!(call["tool_call_id"], "call-open");
    assert_eq!(call["status"], "interrupted");
    assert!(call["finished_at"].is_null());
    assert!(call["duration_ms"].is_null());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn history_collapses_opencode_running_and_completed_call_updates() {
    let dir = hist_dir("opencode-tool-updates");
    // Captured OpenCode JSONL shape: one part/call identity is repeated as its
    // state advances from running to completed.
    let stdout = concat!(
        "{\"type\":\"step_start\",\"part\":{\"type\":\"step-start\"}}\n",
        "{\"type\":\"tool_use\",\"part\":{\"id\":\"part-1\",\"callID\":\"call-1\",\"type\":\"tool\",\"tool\":\"bash\",\"state\":{\"status\":\"running\",\"input\":{\"command\":\"pwd\"},\"time\":{\"start\":1773878400000}}}}\n",
        "{\"type\":\"tool_use\",\"part\":{\"id\":\"part-1\",\"callID\":\"call-1\",\"type\":\"tool\",\"tool\":\"bash\",\"state\":{\"status\":\"completed\",\"input\":{\"command\":\"pwd\"},\"output\":\"/repo\\n\",\"time\":{\"start\":1773878400000,\"end\":1773878400040}}}}\n",
        "{\"type\":\"text\",\"part\":{\"type\":\"text\",\"text\":\"done\"}}\n",
        "{\"type\":\"step_finish\",\"part\":{\"type\":\"step-finish\"}}\n",
    );
    let output = run(
        &[
            "run",
            "--harness",
            "opencode",
            "--prompt",
            "run pwd",
            "--bin",
            &bin_override("opencode"),
            "--history",
            "--history-dir",
            &dir.display().to_string(),
            "--bypass",
            "--compact",
        ],
        &[("MOCK_STDOUT", stdout), ("MOCK_STREAM_DELAY_MS", "30")],
    );
    assert!(output.status.success());
    let report = json_stdout(&output);
    let record = first_history_run(Path::new(report["history_file"].as_str().unwrap()));
    let calls = record["events"].as_array().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["tool_call_id"], "call-1");
    assert_eq!(calls[0]["status"], "completed");
    assert_eq!(calls[0]["output"], "/repo\n");
    assert!(calls[0]["duration_ms"].as_u64().unwrap() > 0);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn history_is_off_by_default_no_history_and_print_command() {
    let dir = hist_dir("off");
    let ds = dir.display().to_string();
    // Default (no --history): nothing recorded, no file, no dir created.
    let v = json_stdout(&run(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("codex"),
            "--history-dir",
            &ds,
            "--bypass",
            "--compact",
        ],
        &[("MOCK_STDOUT", r#"{"result":"ok"}"#)],
    ));
    assert!(v["history_file"].is_null());
    assert!(!dir.exists());
    // Explicit --no-history: still nothing (the override over config is proven in
    // `history_enabled_via_config_records_the_run`).
    let v = json_stdout(&run(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("codex"),
            "--no-history",
            "--history-dir",
            &ds,
            "--bypass",
            "--compact",
        ],
        &[("MOCK_STDOUT", HISTORY_CODEX_TELEMETRY)],
    ));
    assert!(v["history_file"].is_null());
    assert!(!dir.exists());
    // --print-command executes nothing, so history is never written even with --history.
    let v = json_stdout(&run(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("codex"),
            "--history",
            "--history-dir",
            &ds,
            "--print-command",
            "--compact",
        ],
        &[],
    ));
    assert!(v["history_file"].is_null());
    assert!(!dir.exists());
}

#[test]
fn history_name_overrides_the_prompt_derived_default() {
    let dir = hist_dir("name");
    let ds = dir.display().to_string();
    let v = json_stdout(&run(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "whatever",
            "--bin",
            &bin_override("codex"),
            "--history",
            "--history-dir",
            &ds,
            "--history-name",
            "My Release v2!",
            "--bypass",
            "--compact",
        ],
        &[("MOCK_STDOUT", HISTORY_CODEX_TELEMETRY)],
    ));
    let hf = v["history_file"].as_str().unwrap();
    // The label is slugified into the session id / filename.
    assert!(hf.contains("my-release-v2-"), "{hf}");
    let rec = first_history_run(Path::new(hf));
    assert_eq!(rec["name"], "my-release-v2");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn history_list_show_and_clear_round_trip() {
    let dir = hist_dir("view");
    let ds = dir.display().to_string();
    // Seed one session with an explicit name.
    let seeded = run(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "whatever",
            "--bin",
            &bin_override("codex"),
            "--history",
            "--history-dir",
            &ds,
            "--history-name",
            "My Session",
            "--bypass",
            "--compact",
        ],
        &[("MOCK_STDOUT", HISTORY_CODEX_TELEMETRY)],
    );
    assert!(seeded.status.success());

    // list (JSON, default) shows id + name.
    let list = json_stdout(&run(
        &[
            "history",
            "list",
            "--all-projects",
            "--history-dir",
            &ds,
            "--compact",
        ],
        &[],
    ));
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["name"], "my-session");
    assert_eq!(list[0]["harnesses"][0], "codex");

    // list --format text is human-readable.
    let text = run(
        &[
            "history",
            "list",
            "--all-projects",
            "--history-dir",
            &ds,
            "--format",
            "text",
        ],
        &[],
    );
    assert!(text.status.success());
    assert!(String::from_utf8_lossy(&text.stdout).contains("my-session"));

    // show resolves by name.
    let show = json_stdout(&run(
        &[
            "history",
            "show",
            "my-session",
            "--all-projects",
            "--history-dir",
            &ds,
            "--compact",
        ],
        &[],
    ));
    assert_eq!(show[0]["harness"], "codex");

    // show --last picks the newest session without naming it; --all is accepted
    // and returns every match (one here). --format text renders records.
    let last = json_stdout(&run(
        &[
            "history",
            "show",
            "--last",
            "--all",
            "--all-projects",
            "--history-dir",
            &ds,
            "--compact",
        ],
        &[],
    ));
    assert_eq!(last[0]["name"], "my-session");
    let last_text = run(
        &[
            "history",
            "show",
            "--last",
            "--all-projects",
            "--history-dir",
            &ds,
            "--format",
            "text",
        ],
        &[],
    );
    assert!(String::from_utf8_lossy(&last_text.stdout).contains("[codex]"));

    // clear without --yes is a dry run (nothing removed).
    let dry = json_stdout(&run(
        &[
            "history",
            "clear",
            "--all-projects",
            "--history-dir",
            &ds,
            "--compact",
        ],
        &[],
    ));
    assert_eq!(dry["dry_run"], true);
    assert_eq!(dry["would_remove"], 1);
    assert_eq!(
        json_stdout(&run(
            &[
                "history",
                "list",
                "--all-projects",
                "--history-dir",
                &ds,
                "--compact"
            ],
            &[],
        ))
        .as_array()
        .unwrap()
        .len(),
        1,
        "dry run must not delete"
    );

    // clear --yes deletes.
    let done = json_stdout(&run(
        &[
            "history",
            "clear",
            "--all-projects",
            "--history-dir",
            &ds,
            "--yes",
            "--compact",
        ],
        &[],
    ));
    assert_eq!(done["removed"], 1);
    assert!(json_stdout(&run(
        &[
            "history",
            "list",
            "--all-projects",
            "--history-dir",
            &ds,
            "--compact"
        ],
        &[],
    ))
    .as_array()
    .unwrap()
    .is_empty());

    // show of a missing session exits non-zero (not a crash).
    let missing = run(
        &[
            "history",
            "show",
            "nope",
            "--all-projects",
            "--history-dir",
            &ds,
            "--compact",
        ],
        &[],
    );
    assert_eq!(missing.status.code(), Some(1));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn history_readers_skip_unmigrated_files_with_a_migration_notice() {
    let dir = hist_dir("legacy-notice");
    let project = dir.join("legacy-project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("old-session.jsonl"),
        "{\"schema_version\":\"0.3\",\"name\":\"old\"}\n",
    )
    .unwrap();

    let output = run(
        &[
            "history",
            "list",
            "--all-projects",
            "--history-dir",
            &dir.display().to_string(),
            "--compact",
        ],
        &[],
    );
    assert!(output.status.success());
    assert!(json_stdout(&output).as_array().unwrap().is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("skipped unmigrated history lines"),
        "{stderr}"
    );
    assert!(stderr.contains("oneharness history migrate"), "{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn history_migrate_converts_every_legacy_store_and_is_idempotent() {
    let dir = hist_dir("migrate");
    let project = dir.join("legacy-project");
    std::fs::create_dir_all(&project).unwrap();
    for version in ["01", "02", "03"] {
        std::fs::copy(
            format!("tests/fixtures/history-v{version}.jsonl"),
            project.join(format!("legacy-{version}.jsonl")),
        )
        .unwrap();
    }
    // A stale index must be replaced, not merely appended to.
    std::fs::write(dir.join(".index.jsonl"), "not-json\n").unwrap();
    let ds = dir.display().to_string();

    let output = run(
        &["history", "migrate", "--history-dir", &ds, "--compact"],
        &[],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json_stdout(&output);
    assert_eq!(report["files_processed"], 3);
    for file in report["files"].as_array().unwrap() {
        assert_eq!(file["records_migrated"], 1);
        assert_eq!(file["skipped"], 0);
        assert_eq!(file["already_current"], 0);
    }

    for version in ["01", "02", "03"] {
        let text =
            std::fs::read_to_string(project.join(format!("legacy-{version}.jsonl"))).unwrap();
        let lines: Vec<HistoryLine> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.len(), 2);
        assert!(matches!(lines[0], HistoryLine::Event(_)));
        assert!(matches!(lines[1], HistoryLine::Run(_)));
    }
    let index_lines = std::fs::read_to_string(dir.join(".index.jsonl")).unwrap();
    assert_eq!(index_lines.lines().count(), 3);
    assert!(!index_lines.contains("not-json"));

    let listed = json_stdout(&run(
        &[
            "history",
            "list",
            "--all-projects",
            "--history-dir",
            &ds,
            "--compact",
        ],
        &[],
    ));
    assert_eq!(listed.as_array().unwrap().len(), 3);
    let shown = json_stdout(&run(
        &[
            "history",
            "show",
            "legacy-03",
            "--all-projects",
            "--history-dir",
            &ds,
            "--compact",
        ],
        &[],
    ));
    assert_eq!(shown[0]["events"][0]["input"]["command"], "echo 0.3");

    let rerun = json_stdout(&run(
        &["history", "migrate", "--history-dir", &ds, "--compact"],
        &[],
    ));
    for file in rerun["files"].as_array().unwrap() {
        assert_eq!(file["records_migrated"], 0);
        assert_eq!(file["already_current"], 2);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn history_enabled_via_config_records_the_run() {
    // Turning `history` on in a (user-level) config file — the per-user opt-in —
    // records a run with no CLI --history flag.
    let hdir = hist_dir("config");
    let hs = hdir.display().to_string();
    let fixture = ConfigFixture::new(
        "history",
        "", // project config empty
        &format!(
            "history = true\nhistory_dir = \"{}\"\n",
            hs.replace('\\', "\\\\")
        ),
    );
    let output = run_with_config(
        &[
            "run",
            "--cwd",
            &fixture.cwd(),
            "--harness",
            "codex",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("codex"),
            "--bypass",
            "--compact",
        ],
        &[("MOCK_STDOUT", HISTORY_CODEX_TELEMETRY)],
        &fixture.user_config(),
    );
    assert!(output.status.success());
    let value = json_stdout(&output);
    assert!(
        value["history_file"].as_str().is_some(),
        "config `history = true` should record: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // CLI --no-history overrides the config opt-in.
    let disabled = run_with_config(
        &[
            "run",
            "--cwd",
            &fixture.cwd(),
            "--harness",
            "codex",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("codex"),
            "--no-history",
            "--bypass",
            "--compact",
        ],
        &[("MOCK_STDOUT", HISTORY_CODEX_TELEMETRY)],
        &fixture.user_config(),
    );
    assert!(json_stdout(&disabled)["history_file"].is_null());

    // `history list` with no --history-dir resolves the store from the same
    // layered config, so it reads back the session the run recorded.
    let listed = json_stdout(&run_with_config(
        &["history", "list", "--all-projects", "--compact"],
        &[],
        &fixture.user_config(),
    ));
    assert_eq!(
        listed.as_array().unwrap().len(),
        1,
        "history list should resolve history_dir from config"
    );
    let _ = std::fs::remove_dir_all(&hdir);
}

#[test]
fn history_records_every_harness_in_one_session() {
    let dir = hist_dir("multi");
    let ds = dir.display().to_string();
    let out = run(
        &[
            "run",
            "--harness",
            "codex,opencode",
            "--bin",
            &bin_override("codex"),
            "--bin",
            &bin_override("opencode"),
            "--prompt",
            "multi",
            "--history",
            "--history-dir",
            &ds,
            "--bypass",
            "--compact",
        ],
        &[("MOCK_STDOUT", HISTORY_BOTH_TRACES)],
    );
    let hf = json_stdout(&out)["history_file"]
        .as_str()
        .unwrap()
        .to_string();
    let text = std::fs::read_to_string(&hf).unwrap();
    let harnesses: Vec<String> = text
        .lines()
        .map(|l| {
            serde_json::from_str::<Value>(l).unwrap()["harness"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(
        harnesses,
        ["codex", "opencode"],
        "one record per harness, in one file"
    );
    // The store sees a single session carrying both records.
    let list = json_stdout(&run(
        &[
            "history",
            "list",
            "--all-projects",
            "--history-dir",
            &ds,
            "--compact",
        ],
        &[],
    ));
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["record_count"], 2);
    assert_eq!(
        list[0]["harnesses"],
        serde_json::json!(["codex", "opencode"])
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn history_batch_records_one_record_per_prompt() {
    let dir = hist_dir("batch");
    let ds = dir.display().to_string();
    let out = run(
        &[
            "run",
            "--harness",
            "codex",
            "--bin",
            &bin_override("codex"),
            "--prompt",
            "first prompt",
            "--prompt",
            "second prompt",
            "--history",
            "--history-dir",
            &ds,
            "--bypass",
            "--compact",
        ],
        &[("MOCK_STDOUT", HISTORY_CODEX_TELEMETRY)],
    );
    let hf = json_stdout(&out)["history_file"]
        .as_str()
        .unwrap()
        .to_string();
    let text = std::fs::read_to_string(&hf).unwrap();
    let prompts: Vec<String> = text
        .lines()
        .map(|l| {
            serde_json::from_str::<Value>(l).unwrap()["prompt"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(
        prompts,
        ["first prompt", "second prompt"],
        "each batch prompt is its own record"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn history_records_a_streamed_run() {
    // --stream is a separate execution path with its own history append; prove it
    // records (asserting via the store, since --stream emits NDJSON to stdout).
    let dir = hist_dir("stream");
    let ds = dir.display().to_string();
    let out = run(
        &[
            "run",
            "--harness",
            "codex",
            "--bin",
            &bin_override("codex"),
            "--prompt",
            "stream test",
            "--stream",
            "--history",
            "--history-dir",
            &ds,
            "--bypass",
        ],
        &[("MOCK_STDOUT", HISTORY_CODEX_TELEMETRY)],
    );
    assert!(out.status.success());
    let list = json_stdout(&run(
        &[
            "history",
            "list",
            "--all-projects",
            "--history-dir",
            &ds,
            "--compact",
        ],
        &[],
    ));
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["record_count"], 1);
    assert_eq!(list[0]["harnesses"][0], "codex");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn history_cli_reads_v1_0_records_without_variant_identity_fields() {
    let dir = hist_dir("history-v1-0-identity");
    let output = run(
        &[
            "run",
            "--harness",
            "codex",
            "--bin",
            &bin_override("codex"),
            "--prompt",
            "legacy identity",
            "--history",
            "--history-dir",
            &dir.display().to_string(),
            "--bypass",
            "--compact",
        ],
        &[("MOCK_STDOUT", HISTORY_CODEX_TELEMETRY)],
    );
    assert!(output.status.success());
    let path = json_stdout(&output)["history_file"]
        .as_str()
        .unwrap()
        .to_string();
    let legacy = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| {
            panic!(
                "failed to read history file {path}: {error}; stderr: {}",
                String::from_utf8_lossy(&output.stderr)
            )
        })
        .lines()
        .map(|line| {
            let mut value: Value = serde_json::from_str(line).unwrap();
            value["schema_version"] = Value::String("1.0".to_string());
            value.as_object_mut().unwrap().remove("variant");
            value.as_object_mut().unwrap().remove("harness_id");
            serde_json::to_string(&value).unwrap()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let legacy_dir = hist_dir("history-v1-0-copy");
    let legacy_path = legacy_dir
        .join("legacy-project")
        .join(Path::new(&path).file_name().unwrap());
    std::fs::create_dir_all(legacy_path.parent().unwrap()).unwrap();
    std::fs::write(&legacy_path, format!("{legacy}\n")).unwrap();
    for line in legacy.lines() {
        serde_json::from_str::<HistoryLine>(line).unwrap();
    }
    let session = legacy_path.file_stem().unwrap().to_string_lossy();
    let shown = run(
        &[
            "history",
            "show",
            &session,
            "--all-projects",
            "--history-dir",
            &legacy_dir.display().to_string(),
            "--compact",
        ],
        &[],
    );
    assert!(
        shown.status.success(),
        "{}",
        String::from_utf8_lossy(&shown.stderr)
    );
    let shown_value = json_stdout(&shown);
    let record = shown_value
        .as_array()
        .unwrap()
        .iter()
        .find(|value| value["harness"] == "codex")
        .unwrap_or_else(|| panic!("no codex record in {shown_value}"));
    assert_eq!(record["harness"], "codex");
    assert_eq!(record["harness_id"], "codex");
    assert!(record["variant"].is_null());
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&legacy_dir);
}

#[test]
fn history_cli_rejects_mixed_provider_and_observed_timing() {
    let dir = hist_dir("history-mixed-timing");
    let output = run(
        &[
            "run",
            "--harness",
            "codex",
            "--bin",
            &bin_override("codex"),
            "--prompt",
            "mixed timing",
            "--history",
            "--history-dir",
            &dir.display().to_string(),
            "--bypass",
            "--compact",
        ],
        &[("MOCK_STDOUT", HISTORY_CODEX_TELEMETRY)],
    );
    assert!(output.status.success());
    let path = json_stdout(&output)["history_file"]
        .as_str()
        .unwrap()
        .to_string();
    let corrupted = std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|line| {
            let mut value: Value = serde_json::from_str(line).unwrap();
            if value["type"] == "run" {
                value["observed_tool_ms"] = serde_json::json!(1);
            }
            serde_json::to_string(&value).unwrap()
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&path, format!("{corrupted}\n")).unwrap();

    let listed = json_stdout(&run(
        &[
            "history",
            "list",
            "--all-projects",
            "--history-dir",
            &dir.display().to_string(),
            "--compact",
        ],
        &[],
    ));
    assert!(listed.as_array().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn streamed_variant_history_events_keep_the_composed_identity() {
    let bin = mock_bin().display().to_string().replace('\\', "\\\\");
    let fx = ConfigFixture::new(
        "stream-variant-history",
        &format!("history = true\n[harness.opencode.variant.work]\nbin = \"{bin}\"\n"),
        "",
    );
    let dir = hist_dir("stream-variant");
    let output = run_with_config(
        &[
            "run",
            "--harness",
            "opencode:work",
            "--prompt",
            "stream identity",
            "--stream",
            "--history-dir",
            &dir.display().to_string(),
            "--cwd",
            &fx.cwd(),
            "--bypass",
        ],
        &[(
            "MOCK_STDOUT",
            "{\"type\":\"tool_use\",\"part\":{\"type\":\"tool\",\"tool\":\"bash\",\"state\":{\"status\":\"completed\",\"input\":{\"command\":\"echo hi\"},\"output\":\"hi\"}}}\n",
        )],
        &fx.user_config(),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelopes: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        envelopes.last().unwrap()["report"]["results"][0]["harness_id"],
        "opencode:work"
    );
    let history_file = envelopes.last().unwrap()["report"]["history_file"]
        .as_str()
        .unwrap();
    let event = std::fs::read_to_string(history_file)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|line| line["type"] == "event")
        .unwrap();
    assert_eq!(event["harness"], "opencode");
    assert_eq!(event["variant"], "work");
    assert_eq!(event["harness_id"], "opencode:work");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn history_cli_rejects_inconsistent_variant_identities_in_run_and_event_lines() {
    let dir = hist_dir("invalid-variant-identities-source");
    let output = run(
        &[
            "run",
            "--harness",
            "opencode",
            "--bin",
            &bin_override("opencode"),
            "--prompt",
            "history identity",
            "--stream",
            "--history",
            "--history-dir",
            &dir.display().to_string(),
            "--bypass",
        ],
        &[(
            "MOCK_STDOUT",
            "{\"type\":\"tool_use\",\"part\":{\"type\":\"tool\",\"tool\":\"bash\",\"state\":{\"status\":\"completed\",\"input\":{\"command\":\"echo hi\"},\"output\":\"hi\"}}}\n",
        )],
    );
    assert!(output.status.success());
    let terminal: Value = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .next_back()
        .unwrap();
    let source = PathBuf::from(terminal["report"]["history_file"].as_str().unwrap());
    let source_lines: Vec<Value> = std::fs::read_to_string(&source)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();

    let write_mutated = |tag: &str, line_type: &str| {
        let target_dir = hist_dir(tag);
        let target = target_dir.join("project").join(source.file_name().unwrap());
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        let text = source_lines
            .iter()
            .cloned()
            .map(|mut line| {
                if line["type"] == line_type {
                    line["harness_id"] = Value::String("codex:work".to_string());
                }
                serde_json::to_string(&line).unwrap()
            })
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&target, format!("{text}\n")).unwrap();
        (target_dir, target)
    };

    let (bad_run_dir, _) = write_mutated("invalid-variant-run", "run");
    let listed = run(
        &[
            "history",
            "list",
            "--all-projects",
            "--history-dir",
            &bad_run_dir.display().to_string(),
            "--compact",
        ],
        &[],
    );
    assert!(listed.status.success());
    assert!(json_stdout(&listed).as_array().unwrap().is_empty());

    let (bad_event_dir, bad_event_path) = write_mutated("invalid-variant-event", "event");
    let shown = run(
        &[
            "history",
            "show",
            &bad_event_path.file_stem().unwrap().to_string_lossy(),
            "--all-projects",
            "--history-dir",
            &bad_event_dir.display().to_string(),
            "--compact",
        ],
        &[],
    );
    assert!(shown.status.success());
    let records = json_stdout(&shown);
    assert!(records.as_array().unwrap().is_empty());

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&bad_run_dir);
    let _ = std::fs::remove_dir_all(&bad_event_dir);
}

#[test]
fn streamed_history_falls_back_to_events_only_extractable_at_completion() {
    let dir = hist_dir("stream-completion-events");
    let ds = dir.display().to_string();
    let stdout = r#"{
  "type": "assistant",
  "message": {
    "content": [
      {
        "type": "tool_use",
        "id": "call-1",
        "name": "Bash",
        "input": {"command": "echo complete"}
      }
    ]
  }
}"#;
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--bin",
            &bin_override("claude-code"),
            "--prompt",
            "completion-only event",
            "--stream",
            "--history",
            "--history-dir",
            &ds,
            "--bypass",
        ],
        &[("MOCK_STDOUT", stdout), ("MOCK_PRESERVE_STDOUT", "1")],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelopes: Vec<Value> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(envelopes.iter().all(|line| line["type"] != "event"));
    let history_file = envelopes.last().unwrap()["report"]["history_file"]
        .as_str()
        .unwrap();
    let lines: Vec<HistoryLine> = std::fs::read_to_string(history_file)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 2);
    assert!(matches!(lines[0], HistoryLine::Event(_)));
    assert!(matches!(lines[1], HistoryLine::Run(_)));
    let record = first_history_run(Path::new(history_file));
    assert_eq!(record["events"][0]["name"], "Bash");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn interrupted_stream_preserves_events_without_a_closing_run() {
    let mock_profile = mock_profile_redirect();
    use std::io::BufReader;

    let dir = hist_dir("interrupted-stream");
    let ds = dir.display().to_string();
    let lines: Vec<String> = (0..5)
        .map(|i| format!(r#"{{"type":"item.started","item":{{"id":"call-{i}","type":"command_execution","command":"step {i}","status":"in_progress"}}}}"#))
        .collect();
    let mut child = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        // This child is SIGKILLed below, so it never flushes its coverage
        // profile and leaves a truncated `.profraw`. Under `just coverage` that
        // file is collected from the target directory and fails the whole
        // `llvm-profdata merge` ("file header is corrupt"), taking the gate down
        // for a process whose coverage was never wanted. Send it and its
        // inherited children somewhere the collector does not read. Harmless
        // when the binary is not instrumented, which ignores the variable.
        .env(
            "LLVM_PROFILE_FILE",
            std::env::temp_dir().join("oneharness-killed-%p.profraw"),
        )
        .env("MOCK_STDOUT", lines.join("\n"))
        .env("MOCK_STREAM_DELAY_MS", "300")
        .args([
            "run",
            "--harness",
            "codex",
            "--prompt",
            "interrupt me",
            "--bin",
            &bin_override("codex"),
            "--stream",
            "--history",
            "--history-dir",
            &ds,
            "--env",
            mock_profile.as_str(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn streaming history run");
    let mut first = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut first)
        .expect("read first streamed event");
    assert_eq!(
        serde_json::from_str::<Value>(&first).unwrap()["type"],
        "event"
    );
    child.kill().expect("interrupt oneharness");
    child.wait().expect("reap interrupted oneharness");

    let project_dir = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(Result::ok)
        .find(|entry| entry.path().is_dir())
        .expect("history project directory")
        .path();
    let session = std::fs::read_dir(project_dir)
        .unwrap()
        .filter_map(Result::ok)
        .find(|entry| entry.path().extension().is_some_and(|ext| ext == "jsonl"))
        .expect("history session file")
        .path();
    let session_id = session.file_stem().unwrap().to_string_lossy().to_string();
    let persisted: Vec<HistoryLine> = std::fs::read_to_string(&session)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(!persisted.is_empty());
    assert!(persisted
        .iter()
        .all(|line| matches!(line, HistoryLine::Event(_))));

    let shown = run(
        &[
            "history",
            "show",
            &session_id,
            "--all-projects",
            "--history-dir",
            &ds,
            "--compact",
        ],
        &[],
    );
    let displayed = json_stdout(&shown);
    let displayed = displayed.as_array().unwrap();
    assert_eq!(displayed.len(), 1);
    assert_eq!(displayed[0]["type"], "incomplete");
    assert_eq!(displayed[0]["harness"], "codex");
    assert_eq!(
        displayed[0]["events"].as_array().unwrap().len(),
        persisted.len()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn history_watch_event_mode_observes_event_before_stream_finishes() {
    let mock_profile = mock_profile_redirect();
    use std::io::BufReader;

    let dir = hist_dir("watch-live-events");
    let ds = dir.display().to_string();
    let project = std::env::current_dir().unwrap().display().to_string();
    let trace = [
        r#"{"type":"turn.started"}"#,
        r#"{"type":"item.started","item":{"id":"call-1","type":"command_execution","command":"first","status":"in_progress"}}"#,
        r#"{"type":"item.completed","item":{"id":"call-1","type":"command_execution","command":"first","aggregated_output":"ok","exit_code":0,"status":"completed"}}"#,
        r#"{"type":"turn.completed"}"#,
    ]
    .join("\n");
    let seeded = run(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "seed watch",
            "--bin",
            &bin_override("codex"),
            "--stream",
            "--history",
            "--history-dir",
            &ds,
        ],
        &[("MOCK_STDOUT", &trace)],
    );
    assert!(seeded.status.success());
    let listed = json_stdout(&run(
        &[
            "history",
            "list",
            "--project",
            &project,
            "--history-dir",
            &ds,
            "--compact",
        ],
        &[],
    ));
    let seeded_path = listed[0]["path"].as_str().unwrap().to_string();
    let after = first_history_run(Path::new(&seeded_path))["history_id"]
        .as_str()
        .unwrap()
        .to_string();
    let mut watcher = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        .args([
            "history",
            "watch",
            "--project",
            &project,
            "--events",
            "--after",
            &after,
            "--history-dir",
            &ds,
            "--format",
            "jsonl",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn event history watcher");
    let mut reader = BufReader::new(watcher.stdout.take().unwrap());
    let mut run_child = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        .env("MOCK_STDOUT", trace)
        .env("MOCK_STREAM_DELAY_MS", "600")
        .args([
            "run",
            "--harness",
            "codex",
            "--prompt",
            "watch live",
            "--bin",
            &bin_override("codex"),
            "--stream",
            "--history",
            "--history-dir",
            &ds,
            "--env",
            mock_profile.as_str(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn watched streaming run");

    let mut event_line = String::new();
    reader
        .read_line(&mut event_line)
        .expect("read live history event");
    let event: HistoryStreamEnvelope = serde_json::from_str(&event_line).unwrap();
    assert!(matches!(event, HistoryStreamEnvelope::Event { .. }));
    assert!(
        run_child.try_wait().unwrap().is_none(),
        "event must arrive before run completion"
    );
    assert!(run_child.wait().unwrap().success());

    let mut closing_line = String::new();
    loop {
        closing_line.clear();
        reader
            .read_line(&mut closing_line)
            .expect("read closing history record");
        if matches!(
            serde_json::from_str::<HistoryStreamEnvelope>(&closing_line).unwrap(),
            HistoryStreamEnvelope::Record { .. }
        ) {
            break;
        }
    }
    drop(reader);
    let trigger = run(
        &[
            "run",
            "--harness",
            "codex",
            "--bin",
            &bin_override("codex"),
            "--prompt",
            "close watcher",
            "--history",
            "--history-dir",
            &ds,
            "--bypass",
            "--compact",
        ],
        &[("MOCK_STDOUT", HISTORY_CODEX_TELEMETRY)],
    );
    assert!(trigger.status.success());
    assert!(watcher.wait().unwrap().success());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn history_watch_streams_stdout_observed_event_at_the_current_version() {
    let dir = hist_dir("watch-observed-event-version");
    let project = std::env::current_dir().unwrap();
    let writer =
        HistoryWriter::open(&dir, &project, "observed-event", HistoryLabels::default()).unwrap();
    let run_id = writer.begin_run();
    let event = ActionEvent {
        kind: "tool_call".to_string(),
        name: Some("Bash".to_string()),
        input: Some(serde_json::json!({"command": "true"})),
        output: Some("".to_string()),
        index: 0,
        tool_call_id: Some("tool-1".to_string()),
        started_at: Some("2026-07-26T12:00:00.000Z".to_string()),
        finished_at: Some("2026-07-26T12:00:00.010Z".to_string()),
        duration_ms: Some(10),
        status: Some(ToolCallStatus::Completed),
        timing_source: Some(TimingSource::StdoutObserved),
    };
    writer
        .append_event(run_id, "claude-code", event.clone())
        .unwrap();

    let mut watcher = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        .args([
            "history",
            "watch",
            "--all-projects",
            "--events",
            "--history-dir",
            &dir.display().to_string(),
            "--format",
            "jsonl",
        ])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut reader = std::io::BufReader::new(watcher.stdout.take().unwrap());
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let envelope: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(envelope["type"], "event");
    // Event lines are written by the current writer and read live, so they
    // always declare the current version (unlike a run record, whose version is
    // the oldest reader that can understand the fields it carries).
    assert_eq!(envelope["line"]["schema_version"], "1.5");
    assert_eq!(
        envelope["line"]["event"]["timing_source"],
        "stdout_observed"
    );

    drop(reader);
    writer.append_event(run_id, "claude-code", event).unwrap();
    assert!(watcher.wait().unwrap().success());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn history_enabled_and_dir_via_environment() {
    // ONEHARNESS_HISTORY / ONEHARNESS_HISTORY_DIR enable + place the store end to
    // end (config loading on, so the env layer applies).
    let dir = hist_dir("env");
    let ds = dir.display().to_string();
    let fixture = ConfigFixture::new("history-env", "", "");
    let out = run_with_config(
        &[
            "run",
            "--cwd",
            &fixture.cwd(),
            "--harness",
            "codex",
            "--bin",
            &bin_override("codex"),
            "--prompt",
            "env test",
            "--bypass",
            "--compact",
        ],
        &[
            ("MOCK_STDOUT", HISTORY_CODEX_TELEMETRY),
            ("ONEHARNESS_HISTORY", "1"),
            ("ONEHARNESS_HISTORY_DIR", &ds),
        ],
        &fixture.user_config(),
    );
    let hf = json_stdout(&out)["history_file"]
        .as_str()
        .map(str::to_string);
    let hf = hf.expect("env should enable history");
    let canonical_dir = std::fs::canonicalize(&dir).expect("history directory should exist");
    assert!(
        Path::new(&hf).starts_with(&canonical_dir),
        "ONEHARNESS_HISTORY_DIR should place the store: {hf}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn history_labels_layer_cli_over_environment_over_config_and_validate() {
    let dir = hist_dir("labels");
    let ds = dir.display().to_string();
    let fixture = ConfigFixture::new(
        "history-labels",
        "history_labels = { graph = \"project\", project = \"kept\" }",
        "history_labels = { graph = \"user\", user = \"kept\" }",
    );
    let out = run_with_config(
        &[
            "run",
            "--cwd",
            &fixture.cwd(),
            "--harness",
            "codex",
            "--bin",
            &bin_override("codex"),
            "--prompt",
            "labeled run",
            "--history",
            "--history-dir",
            &ds,
            "--history-label",
            "graph=cli",
            "--history-label",
            "cli=kept",
            "--bypass",
            "--compact",
        ],
        &[
            ("MOCK_STDOUT", HISTORY_CODEX_TELEMETRY),
            ("ONEHARNESS_HISTORY_LABELS", "graph=environment,env=kept"),
        ],
        &fixture.user_config(),
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let path = json_stdout(&out)["history_file"]
        .as_str()
        .unwrap()
        .to_string();
    let record = first_history_run(Path::new(&path));
    // History requests the telemetry trace even though the user selected compact
    // report output, so ordinary new writes always use the current contract.
    assert_eq!(record["schema_version"], "1.1");
    assert!(record["started_at"].is_string());
    assert_eq!(record["labels"]["graph"], "cli");
    for key in ["user", "project", "env", "cli"] {
        assert_eq!(record["labels"][key], "kept", "label {key}");
    }

    let invalid = run(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "invalid",
            "--history-label",
            "bad/key=value",
        ],
        &[],
    );
    assert_eq!(invalid.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("invalid history label"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn history_canonicalizes_relative_cwd_for_project_lookup() {
    let dir = hist_dir("canonical-cwd");
    let ds = dir.display().to_string();
    let root = hist_dir("canonical-project");
    let project = root.join("project");
    let child = project.join("child");
    std::fs::create_dir_all(&child).unwrap();
    let relative = child.join("..");
    let out = run(
        &[
            "run",
            "--cwd",
            &relative.display().to_string(),
            "--harness",
            "codex",
            "--bin",
            &bin_override("codex"),
            "--prompt",
            "canonical",
            "--history",
            "--history-dir",
            &ds,
            "--bypass",
            "--compact",
        ],
        &[("MOCK_STDOUT", HISTORY_CODEX_TELEMETRY)],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let listed = json_stdout(&run(
        &[
            "history",
            "list",
            "--project",
            &project.display().to_string(),
            "--history-dir",
            &ds,
            "--compact",
        ],
        &[],
    ));
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(
        listed[0]["project"],
        std::fs::canonicalize(&project)
            .unwrap()
            .display()
            .to_string()
    );
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn history_watch_variant_filters_nonmatching_event_envelopes() {
    use std::io::BufReader;

    let dir = hist_dir("watch-variant-events");
    let ds = dir.display().to_string();
    let bin = mock_bin().display().to_string().replace('\\', "\\\\");
    let fx = ConfigFixture::new(
        "watch-filter-events",
        &format!(
            "[harness.codex.variant.work]\nbin = \"{bin}\"\n\
             [harness.codex.variant.personal]\nbin = \"{bin}\"\n"
        ),
        "",
    );
    let trace = concat!(
        "{\"type\":\"turn.started\"}\n",
        "{\"type\":\"item.started\",\"item\":{\"id\":\"call-1\",\"type\":\"command_execution\",\"command\":\"echo hi\",\"status\":\"in_progress\"}}\n",
        "{\"type\":\"item.completed\",\"item\":{\"id\":\"call-1\",\"type\":\"command_execution\",\"command\":\"echo hi\",\"aggregated_output\":\"hi\",\"exit_code\":0,\"status\":\"completed\"}}\n",
        "{\"type\":\"turn.completed\"}\n",
    );
    let record = |variant: &str, prompt: &str| {
        let output = run_with_config(
            &[
                "run",
                "--harness",
                &format!("codex:{variant}"),
                "--prompt",
                prompt,
                "--cwd",
                &fx.cwd(),
                "--stream",
                "--history",
                "--history-dir",
                &ds,
            ],
            &[("MOCK_STDOUT", trace)],
            &fx.user_config(),
        );
        assert!(output.status.success());
        let terminal = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .last()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .unwrap();
        let path = terminal["report"]["history_file"].as_str().unwrap();
        std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .find(|line| line["type"] == "run")
            .unwrap()["history_id"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let after = record("work", "seed");
    record("personal", "must be filtered");
    record("work", "must be emitted");

    let mut watcher = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        .args([
            "history",
            "watch",
            "--project",
            &fx.cwd(),
            "--events",
            "--variant",
            "work",
            "--after",
            &after,
            "--history-dir",
            &ds,
            "--format",
            "jsonl",
        ])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    let mut reader = BufReader::new(watcher.stdout.take().unwrap());
    reader.read_line(&mut line).unwrap();
    let envelope: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(envelope["type"], "event");
    assert_eq!(envelope["line"]["variant"], "work");
    assert_eq!(envelope["line"]["harness_id"], "codex:work");
    drop(reader);
    record("work", "close watcher");
    assert!(watcher.wait().unwrap().success());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn history_watch_filters_and_resumes_as_jsonl() {
    let dir = hist_dir("watch-cli");
    let ds = dir.display().to_string();
    let bin = mock_bin().display().to_string().replace('\\', "\\\\");
    let fx = ConfigFixture::new(
        "watch-variant",
        &format!(
            "[harness.codex.variant.work]\nbin = \"{bin}\"\n\
             [harness.codex.variant.personal]\nbin = \"{bin}\"\n"
        ),
        "",
    );
    let mut ids = Vec::new();
    for (name, graph, variant) in [
        ("first", "release", "work"),
        ("second", "release", "personal"),
        ("third", "release", "work"),
    ] {
        let out = run_with_config(
            &[
                "run",
                "--harness",
                &format!("codex:{variant}"),
                "--prompt",
                name,
                "--cwd",
                &fx.cwd(),
                "--history",
                "--history-dir",
                &ds,
                "--history-name",
                name,
                "--history-label",
                &format!("graph={graph}"),
                "--bypass",
                "--compact",
            ],
            &[("MOCK_STDOUT", HISTORY_CODEX_TELEMETRY)],
            &fx.user_config(),
        );
        let path = json_stdout(&out)["history_file"]
            .as_str()
            .unwrap()
            .to_string();
        let record = first_history_run(Path::new(&path));
        ids.push(record["history_id"].as_str().unwrap().to_string());
    }

    let mut child = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        .args([
            "history",
            "watch",
            "--all-projects",
            "--history-dir",
            &ds,
            "--after",
            &ids[0],
            "--label",
            "graph=release",
            "--variant",
            "work",
            "--format",
            "jsonl",
        ])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    let mut reader = std::io::BufReader::new(child.stdout.take().unwrap());
    reader.read_line(&mut line).unwrap();
    // Close the consumer pipe, then append one more matching record. The watch
    // process observes it through the index and exits cleanly on broken pipe,
    // proving the follow path while also letting coverage data flush (killing a
    // watcher would discard that process's profile).
    drop(reader);
    let trigger = run_with_config(
        &[
            "run",
            "--harness",
            "codex:work",
            "--prompt",
            "fourth",
            "--cwd",
            &fx.cwd(),
            "--history",
            "--history-dir",
            &ds,
            "--history-name",
            "fourth",
            "--history-label",
            "graph=release",
            "--bypass",
            "--compact",
        ],
        &[("MOCK_STDOUT", HISTORY_CODEX_TELEMETRY)],
        &fx.user_config(),
    );
    assert!(trigger.status.success());
    let status = child.wait().unwrap();
    assert!(status.success(), "watch exit: {status:?}");

    let envelope: Value = serde_json::from_str(&line).unwrap();
    let typed: HistoryStreamEnvelope = serde_json::from_str(&line).unwrap();
    assert_eq!(envelope["type"], "record");
    assert_eq!(envelope["record"]["history_id"], ids[2]);
    assert_eq!(envelope["record"]["prompt"], "third");
    match typed {
        HistoryStreamEnvelope::Record { record } => {
            assert_eq!(record.history_id.to_string(), ids[2]);
            assert_eq!(record.prompt, "third");
        }
        HistoryStreamEnvelope::Event { .. } => panic!("record-only watch emitted an event"),
    }
    let mut future = envelope;
    future["future_output_field"] = Value::Bool(true);
    assert!(serde_json::from_value::<HistoryStreamEnvelope>(future).is_ok());

    let missing = run(
        &[
            "history",
            "watch",
            "--all-projects",
            "--history-dir",
            &ds,
            "--after",
            "00000000-0000-7000-8000-000000000000",
        ],
        &[],
    );
    assert_eq!(missing.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("was not found"));

    let invalid = run(
        &[
            "history",
            "watch",
            "--all-projects",
            "--history-dir",
            &ds,
            "--after",
            "not-a-cursor",
        ],
        &[],
    );
    assert_eq!(invalid.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("invalid history cursor"));

    let exact_missing = run(
        &[
            "history",
            "show",
            "00000000-0000-7000-8000-000000000000",
            "--all-projects",
            "--history-dir",
            &ds,
        ],
        &[],
    );
    assert_eq!(exact_missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&exact_missing.stderr).contains("was not found"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn history_watch_scopes_to_explicit_and_current_project() {
    let dir = hist_dir("watch-project");
    let ds = dir.display().to_string();
    let pa = std::env::temp_dir().join(format!("oh-watch-a-{}", std::process::id()));
    let pb = std::env::temp_dir().join(format!("oh-watch-b-{}", std::process::id()));
    std::fs::create_dir_all(&pa).unwrap();
    std::fs::create_dir_all(&pb).unwrap();

    let record = |project: &Path, prompt: &str| {
        let out = run(
            &[
                "run",
                "--cwd",
                &project.display().to_string(),
                "--harness",
                "codex",
                "--bin",
                &bin_override("codex"),
                "--prompt",
                prompt,
                "--history",
                "--history-dir",
                &ds,
                "--bypass",
                "--compact",
            ],
            &[("MOCK_STDOUT", HISTORY_CODEX_TELEMETRY)],
        );
        assert!(out.status.success(), "{out:?}");
        let path = json_stdout(&out)["history_file"]
            .as_str()
            .unwrap()
            .to_string();
        let value: Value = serde_json::from_str(
            std::fs::read_to_string(path)
                .unwrap()
                .lines()
                .next()
                .unwrap(),
        )
        .unwrap();
        value["history_id"].as_str().unwrap().to_string()
    };

    let cursor = record(&pa, "cursor");
    let _ = record(&pb, "other-project");
    let expected = record(&pa, "same-project");

    for explicit_project in [true, false] {
        let mut command = Command::new(oneharness_bin());
        command.env("ONEHARNESS_NO_CONFIG", "1").args([
            "history",
            "watch",
            "--history-dir",
            &ds,
            "--after",
            &cursor,
            "--format",
            "jsonl",
        ]);
        if explicit_project {
            command.args(["--project", &pa.display().to_string()]);
        } else {
            command.current_dir(&pa);
        }
        let mut child = command.stdout(Stdio::piped()).spawn().unwrap();
        let mut line = String::new();
        let mut reader = std::io::BufReader::new(child.stdout.take().unwrap());
        reader.read_line(&mut line).unwrap();
        let envelope: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(envelope["record"]["history_id"], expected);
        assert_eq!(envelope["record"]["prompt"], "same-project");

        drop(reader);
        let _ = record(
            &pa,
            if explicit_project {
                "trigger-explicit"
            } else {
                "trigger-current"
            },
        );
        let status = child.wait().unwrap();
        assert!(status.success(), "watch exit: {status:?}");
    }

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&pa);
    let _ = std::fs::remove_dir_all(&pb);
}

#[test]
fn concurrent_processes_append_complete_history_index_lines() {
    let mock_profile = mock_profile_redirect();
    let dir = hist_dir("concurrent-process-index");
    let ds = dir.display().to_string();
    let mut children = Vec::new();
    for index in 0..8 {
        children.push(
            Command::new(oneharness_bin())
                .env("ONEHARNESS_NO_CONFIG", "1")
                .env("MOCK_STDOUT", HISTORY_CODEX_TELEMETRY)
                .args([
                    "run",
                    "--harness",
                    "codex",
                    "--bin",
                    &bin_override("codex"),
                    "--prompt",
                    &format!("process-{index}"),
                    "--history",
                    "--history-dir",
                    &ds,
                    "--history-name",
                    &format!("process-{index}"),
                    "--bypass",
                    "--compact",
                    "--env",
                    mock_profile.as_str(),
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
    }
    for child in children {
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let lines: Vec<Value> = std::fs::read_to_string(dir.join(".index.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 8);
    let ids: std::collections::BTreeSet<&str> = lines
        .iter()
        .map(|line| line["record"]["history_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 8);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn history_records_a_failed_run_and_shows_by_id() {
    let dir = hist_dir("failed");
    let ds = dir.display().to_string();
    // A nonzero run is still recorded, with its status and classified failure.
    let out = run(
        &[
            "run",
            "--harness",
            "codex",
            "--bin",
            &bin_override("codex"),
            "--prompt",
            "boom",
            "--history",
            "--history-dir",
            &ds,
            "--bypass",
            "--compact",
        ],
        &[
            ("MOCK_EXIT", "1"),
            (
                "MOCK_STDERR",
                "Error: 401 Unauthorized — please authenticate",
            ),
            ("MOCK_STDOUT", HISTORY_CODEX_TELEMETRY),
        ],
    );
    let hf = json_stdout(&out)["history_file"]
        .as_str()
        .unwrap()
        .to_string();
    let id = std::path::Path::new(&hf)
        .file_stem()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let rec: Value = serde_json::from_str(
        std::fs::read_to_string(&hf)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(rec["status"], "nonzero");
    assert_eq!(rec["failure_kind"], "auth");
    // `show` resolves by the exact session id (not just name).
    let show = json_stdout(&run(
        &[
            "history",
            "show",
            &id,
            "--all-projects",
            "--history-dir",
            &ds,
            "--compact",
        ],
        &[],
    ));
    assert_eq!(show[0]["status"], "nonzero");
    // A record's UUID is a second, exact lookup surface and returns only that
    // record rather than resolving the containing session.
    let history_id = rec["history_id"].as_str().unwrap();
    let exact = json_stdout(&run(
        &[
            "history",
            "show",
            history_id,
            "--history-dir",
            &ds,
            "--compact",
        ],
        &[],
    ));
    assert_eq!(exact.as_array().unwrap().len(), 1);
    assert_eq!(exact[0]["history_id"], history_id);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn history_list_scopes_by_project() {
    let dir = hist_dir("scope");
    let ds = dir.display().to_string();
    let pa = std::env::temp_dir().join(format!("oh-projA-{}", std::process::id()));
    let pb = std::env::temp_dir().join(format!("oh-projB-{}", std::process::id()));
    std::fs::create_dir_all(&pa).unwrap();
    std::fs::create_dir_all(&pb).unwrap();
    for p in [&pa, &pb] {
        run(
            &[
                "run",
                "--cwd",
                &p.display().to_string(),
                "--harness",
                "codex",
                "--bin",
                &bin_override("codex"),
                "--prompt",
                "x",
                "--history",
                "--history-dir",
                &ds,
                "--bypass",
                "--compact",
            ],
            &[("MOCK_STDOUT", HISTORY_CODEX_TELEMETRY)],
        );
    }
    // --all-projects sees both; --project scopes to one.
    assert_eq!(
        json_stdout(&run(
            &[
                "history",
                "list",
                "--all-projects",
                "--history-dir",
                &ds,
                "--compact"
            ],
            &[],
        ))
        .as_array()
        .unwrap()
        .len(),
        2
    );
    let just_a = json_stdout(&run(
        &[
            "history",
            "list",
            "--project",
            &pa.display().to_string(),
            "--history-dir",
            &ds,
            "--compact",
        ],
        &[],
    ));
    assert_eq!(just_a.as_array().unwrap().len(), 1);
    assert_eq!(
        just_a[0]["project"],
        std::fs::canonicalize(&pa).unwrap().display().to_string()
    );
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&pa);
    let _ = std::fs::remove_dir_all(&pb);
}

// --- large prompts / system (issue #1115): off-argv delivery ----------------

/// A prompt/system value bigger than the 64 KiB off-argv threshold, carrying a
/// unique marker so the test can prove exactly which text reached the harness.
fn big_with_marker(marker: &str) -> String {
    format!("{marker} {}", "x".repeat(70 * 1024))
}

/// A temp file (removed on drop) so a big prompt/system reaches oneharness via
/// `--prompt-file`/`--system-file` — NOT on its own argv. Windows caps the whole
/// command line at ~32 KiB, so a >64 KiB `--prompt`/`--system` value would fail to
/// spawn oneharness itself (the caller→oneharness hop #1108 addressed), masking
/// the harness-spawn delivery these tests actually exercise.
struct BigFile(PathBuf);

impl BigFile {
    fn new(tag: &str, contents: &str) -> Self {
        let p = std::env::temp_dir().join(format!(
            "oneharness-bigtest-{tag}-{}.txt",
            std::process::id()
        ));
        std::fs::write(&p, contents).unwrap();
        BigFile(p)
    }
    fn path(&self) -> String {
        self.0.display().to_string()
    }
}

impl Drop for BigFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn large_prompt_rides_stdin_not_the_argv() {
    // A > 64 KiB prompt on claude-code must be delivered on the child's stdin
    // (`-p --input-format text`, positional dropped), never inlined — so it can't
    // trip the OS argv ceiling (E2BIG, issue #1115). The mock echoes stdin, so the
    // marker surfacing in the captured stdout proves the prompt actually arrived
    // there, and its absence from `command` proves it left the argv.
    let marker = "OHBIGPROMPT-marker-777";
    // Delivered to oneharness via a file (not argv) so the caller→oneharness spawn
    // clears Windows' command-line limit; the harness-spawn stdin path is the SUT.
    let pf = BigFile::new("prompt-stdin", &big_with_marker(marker));
    let pfp = pf.path();
    let out = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt-file",
            &pfp,
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[("MOCK_ECHO_STDIN", "1")],
    );
    assert!(out.status.success(), "{out:?}");
    let v = json_stdout(&out);
    assert_eq!(v["results"][0]["status"], "ok");
    // The prompt arrived on stdin (mock echoed it back).
    let stdout = v["results"][0]["stdout"].as_str().unwrap();
    assert!(stdout.contains(marker), "prompt did not reach stdin");
    // The command switched to stdin mode and carries no giant argument.
    let cmd = command_of(&v, 0);
    assert!(
        cmd.windows(2).any(|w| w == ["--input-format", "text"]),
        "{cmd:?}"
    );
    assert!(
        !cmd.iter().any(|a| a.contains(marker)),
        "the prompt must not be on the argv: {cmd:?}"
    );
}

#[test]
fn large_system_rides_a_temp_file_that_is_cleaned_up() {
    // A > 64 KiB `--system` on claude-code must be materialized to a temp file and
    // delivered via `--append-system-prompt-file`, off the argv. The mock cats the
    // file named after that flag, so the marker in the captured stdout proves the
    // file held the system text; the temp file must be gone once the run returns.
    let marker = "OHBIGSYS-marker-555";
    // Big system delivered to oneharness via a file (caller hop); oneharness then
    // re-materializes it to its OWN temp file for `--append-system-prompt-file`
    // (the file asserted-cleaned below is that one, not this input file).
    let sf = BigFile::new("sys-file", &big_with_marker(marker));
    let sfp = sf.path();
    let out = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--system-file",
            &sfp,
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[("MOCK_CAT_ARG_AFTER", "--append-system-prompt-file")],
    );
    assert!(out.status.success(), "{out:?}");
    let v = json_stdout(&out);
    let stdout = v["results"][0]["stdout"].as_str().unwrap();
    assert!(
        stdout.contains(marker),
        "system file did not carry the text"
    );
    let cmd = command_of(&v, 0);
    // The file flag replaced the inline one, and the inline value is absent.
    let pos = cmd
        .iter()
        .position(|a| a == "--append-system-prompt-file")
        .expect("file flag present");
    assert!(
        !cmd.iter().any(|a| a == "--append-system-prompt"),
        "inline flag must be gone: {cmd:?}"
    );
    assert!(
        !cmd.iter().any(|a| a.contains(marker)),
        "system text must not be on the argv: {cmd:?}"
    );
    // The temp file is removed on drop once the run completes.
    let temp_path = &cmd[pos + 1];
    assert!(
        !std::path::Path::new(temp_path).exists(),
        "temp system file `{temp_path}` should be cleaned up after the run"
    );
}

#[test]
fn large_prompt_folds_system_into_stdin_for_a_prepend_harness() {
    // On a harness with no system flag (codex), a large prompt rides stdin via the
    // `-` sentinel, and the `--system` text is prepended into that same stdin
    // payload (mirroring the inline `prompt_with_system`). Both markers must reach
    // the harness's stdin, and neither may appear on the argv.
    let pmark = "OHCODEXPROMPT-111";
    let smark = "OHCODEXSYS-222";
    // Big prompt via a file (caller hop); the small system stays an inline flag.
    let pf = BigFile::new("codex-prompt", &big_with_marker(pmark));
    let pfp = pf.path();
    let out = run(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt-file",
            &pfp,
            "--system",
            smark,
            "--bin",
            &bin_override("codex"),
            "--compact",
        ],
        &[("MOCK_ECHO_STDIN", "1")],
    );
    assert!(out.status.success(), "{out:?}");
    let v = json_stdout(&out);
    let stdout = v["results"][0]["stdout"].as_str().unwrap();
    assert!(stdout.contains(pmark), "prompt missing from stdin");
    assert!(stdout.contains(smark), "system missing from stdin");
    // The system is prepended, then the prompt (prompt_with_system order).
    assert!(
        stdout.find(smark).unwrap() < stdout.find(pmark).unwrap(),
        "system should precede the prompt on stdin"
    );
    let cmd = command_of(&v, 0);
    assert!(
        cmd.iter().any(|a| a == "-"),
        "codex stdin sentinel: {cmd:?}"
    );
    assert!(
        !cmd.iter().any(|a| a.contains(pmark) || a.contains(smark)),
        "neither prompt nor system may be on the argv: {cmd:?}"
    );
}

// Non-Windows only: goose keeps a large system on its inline `--system` argv, and
// there is no size that is both over the 64 KiB threshold (to fire the branch) AND
// under Windows' ~32 KiB command-line limit (to spawn the harness), so the inline
// delivery this asserts can only be exercised where the argv ceiling is higher.
#[cfg(not(windows))]
#[test]
fn large_system_on_goose_warns_no_off_argv_route() {
    // Goose's `--system` is inline TEXT with no file/stdin route, so a large
    // system prompt cannot leave the argv — oneharness must warn loudly (not fail
    // silently) and keep it inline. Kept under 128 KiB so spawning the mock itself
    // doesn't E2BIG on Linux, but over the 64 KiB threshold so the branch fires.
    let marker = "OHGOOSESYS-333";
    // Delivered to oneharness via a file (caller hop); goose still inlines it.
    let sf = BigFile::new("goose-sys", &format!("{marker} {}", "y".repeat(80 * 1024)));
    let sfp = sf.path();
    let out = run(
        &[
            "run",
            "--harness",
            "goose",
            "--prompt",
            "hi",
            "--system-file",
            &sfp,
            "--bin",
            &bin_override("goose"),
            "--compact",
        ],
        &[],
    );
    assert!(out.status.success(), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--system prompt for harness `goose`")
            && stderr.contains("cannot be delivered off the argv"),
        "expected a large-system warning, got: {stderr}"
    );
    // It stays on goose's inline `--system` flag.
    let v = json_stdout(&out);
    let cmd = command_of(&v, 0);
    let pos = cmd
        .iter()
        .position(|a| a == "--system")
        .expect("inline flag");
    assert!(
        cmd[pos + 1].contains(marker),
        "system stays inline: {cmd:?}"
    );
}

#[test]
fn small_prompt_keeps_the_inline_argv() {
    // The common case is unperturbed: a prompt under the threshold stays a
    // positional argv argument (no stdin, no --input-format), so `--print-command`
    // and every existing assertion keep their byte-identical shape.
    let out = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "just a small prompt",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[("MOCK_ECHO_STDIN", "1")],
    );
    assert!(out.status.success(), "{out:?}");
    let v = json_stdout(&out);
    let cmd = command_of(&v, 0);
    assert!(
        cmd.iter().any(|a| a == "just a small prompt"),
        "small prompt stays inline: {cmd:?}"
    );
    assert!(
        !cmd.iter().any(|a| a == "--input-format"),
        "no stdin mode for a small prompt: {cmd:?}"
    );
    // Nothing was piped, so the mock's echo produced empty stdout.
    assert_eq!(v["results"][0]["stdout"].as_str().unwrap(), "");
}

// (The large-prompt warning path — a harness with no off-argv route — is now
// covered by a unit test in src/commands/run.rs against a synthetic spec, since
// every real harness is wired for off-argv delivery. See
// `plan_large_input_warns_and_stays_inline_when_unwired`.)

// ---------------------------------------------------------------------------
// Fallback run mode (`--run-mode fallback`): drive the selected harnesses in
// priority order, stopping at the first that actually runs, and falling through
// only the ones that cannot run at all (not installed / unspawnable / auth /
// quota). A real task failure or a timeout does NOT fall through.
// ---------------------------------------------------------------------------

/// A `--bin ID=PATH` override pointing at a path that does not exist, so the
/// harness resolves as unavailable (Status::Skipped) — the "not installed" case.
fn missing_bin(id: &str) -> String {
    let path = std::env::temp_dir().join(format!("oneharness-no-such-bin-{}", std::process::id()));
    format!("{id}={}", path.display())
}

/// A `--bin` override that `which` resolves — so the candidate is *available*
/// and gets as far as a spawn attempt — but that the OS then refuses to execute,
/// which is what separates `spawn-error` from `not-installed`. Unix: an
/// executable file naming an interpreter that does not exist. Windows: a
/// zero-byte `.exe`, which `CreateProcess` rejects as a bad image format.
fn unspawnable_bin(id: &str) -> String {
    let dir = std::env::temp_dir().join(format!("oneharness-unspawnable-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("the staging directory");
    #[cfg(windows)]
    let path = {
        let path = dir.join(format!("{id}.exe"));
        std::fs::write(&path, b"").expect("stage the unspawnable program");
        path
    };
    #[cfg(not(windows))]
    let path = {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(id);
        std::fs::write(&path, "#!/oneharness/no-such-interpreter\n")
            .expect("stage the unspawnable program");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("mark it executable so `which` resolves it");
        path
    };
    format!("{id}={}", path.display())
}

#[test]
fn multiple_models_run_the_harness_by_model_cross_product() {
    // Repeated --model fans out over the model axis: in parallel mode every
    // selected harness runs once per model, so `results` holds the (harness ×
    // model) cross-product, harness-major then model-minor. --print-command pins
    // the argv (each unit carries its own --model) without spawning.
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code,codex",
            "--prompt",
            "hi",
            "--model",
            "alpha",
            "--model",
            "beta",
            "--print-command",
            "--compact",
        ],
        &[],
    );
    assert!(output.status.success());
    let v = json_stdout(&output);
    // The fan-out list is echoed; the top-level model is the first of the list.
    assert_eq!(v["models"][0], "alpha");
    assert_eq!(v["models"][1], "beta");
    assert_eq!(v["model"], "alpha");
    // Four (harness, model) units in cross-product order.
    let results = v["results"].as_array().unwrap();
    assert_eq!(results.len(), 4);
    let pairs: Vec<(&str, &str)> = results
        .iter()
        .map(|r| (r["harness"].as_str().unwrap(), r["model"].as_str().unwrap()))
        .collect();
    assert_eq!(
        pairs,
        vec![
            ("claude-code", "alpha"),
            ("claude-code", "beta"),
            ("codex", "alpha"),
            ("codex", "beta"),
        ]
    );
    // Each unit's argv carries its own model.
    assert!(command_of(&v, 0)
        .windows(2)
        .any(|w| w == ["--model", "alpha"]));
    assert!(command_of(&v, 1)
        .windows(2)
        .any(|w| w == ["--model", "beta"]));
    assert!(command_of(&v, 3)
        .windows(2)
        .any(|w| w == ["--model", "beta"]));
}

#[test]
fn multiple_models_execute_each_pair_in_parallel() {
    // The cross-product actually runs (via the mock): two models on one harness
    // yield two ok results, each tagged with the model it ran.
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--model",
            "alpha",
            "--model",
            "beta",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[],
    );
    assert!(
        output.status.success(),
        "exit {:?}: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let v = json_stdout(&output);
    assert!(v["fallback"].is_null());
    assert!(v["batch"].is_null());
    let results = v["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);
    for r in results {
        assert_eq!(r["harness"], "claude-code");
        assert_eq!(r["status"], "ok");
    }
    assert_eq!(results[0]["model"], "alpha");
    assert_eq!(results[1]["model"], "beta");
}

#[test]
fn explicit_base_harnesses_preserve_cli_order() {
    let output = run(
        &[
            "run",
            "--harness",
            "cursor",
            "--harness",
            "claude-code",
            "--harness",
            "cursor",
            "--prompt",
            "ordered",
            "--print-command",
            "--compact",
        ],
        &[],
    );
    assert!(output.status.success());
    let value = json_stdout(&output);
    let harnesses: Vec<_> = value["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|result| result["harness"].as_str().unwrap())
        .collect();
    assert_eq!(harnesses, ["cursor", "claude-code"]);
}

#[test]
fn a_single_model_is_not_a_fan_out() {
    // One --model behaves exactly as before: no `models` list on the report, and
    // the single result still records its model.
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--model",
            "solo",
            "--print-command",
            "--compact",
        ],
        &[],
    );
    let v = json_stdout(&output);
    assert!(v["models"].is_null(), "a single model is not a fan-out");
    assert_eq!(v["model"], "solo");
    let results = v["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["model"], "solo");
}

#[test]
fn config_models_list_drives_a_fan_out() {
    // The `models` config key fans out just like repeated --model; a bare CLI
    // (no --model) inherits it.
    let fx = ConfigFixture::new(
        "models-cfg",
        "harnesses = [\"claude-code\"]\nmodels = [\"opus\", \"sonnet\"]\n",
        "",
    );
    let output = run_with_config(
        &[
            "run",
            "--prompt",
            "hi",
            "--cwd",
            &fx.cwd(),
            "--print-command",
            "--compact",
        ],
        &[],
        &fx.user_config(),
    );
    let v = json_stdout(&output);
    assert_eq!(v["models"][0], "opus");
    assert_eq!(v["models"][1], "sonnet");
    let results = v["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["model"], "opus");
    assert_eq!(results[1]["model"], "sonnet");
    // A CLI --model overrides the config list entirely.
    let output = run_with_config(
        &[
            "run",
            "--prompt",
            "hi",
            "--cwd",
            &fx.cwd(),
            "--model",
            "cli-only",
            "--print-command",
            "--compact",
        ],
        &[],
        &fx.user_config(),
    );
    let v = json_stdout(&output);
    assert!(v["models"].is_null());
    assert_eq!(v["results"].as_array().unwrap()[0]["model"], "cli-only");
}

#[test]
fn models_via_env_override_fans_out() {
    // ONEHARNESS_MODELS is the env spelling of the fan-out list.
    let fx = ConfigFixture::new("models-env", "harnesses = [\"claude-code\"]\n", "");
    let output = run_with_config(
        &[
            "run",
            "--prompt",
            "hi",
            "--cwd",
            &fx.cwd(),
            "--print-command",
            "--compact",
        ],
        &[("ONEHARNESS_MODELS", "a, b")],
        &fx.user_config(),
    );
    let v = json_stdout(&output);
    assert_eq!(v["models"][0], "a");
    assert_eq!(v["models"][1], "b");
    assert_eq!(v["results"].as_array().unwrap().len(), 2);
}

#[test]
fn fallback_falls_through_a_bad_model_to_the_next_model() {
    // A model list in fallback mode is a priority chain across models: a per-model
    // rejection falls through to the next, which runs. Both widened reasons are
    // covered — a missing model (`model_not_found`) and an over-limit one
    // (`rate_limit`). A single-model fallback would STOP at either.
    for (fail_stderr, kind, reason) in [
        (
            "error: model not found: bad-model",
            "model_not_found",
            "model-not-found",
        ),
        (
            "Error 429: rate limit exceeded, too many requests",
            "rate_limit",
            "rate-limit",
        ),
    ] {
        let output = run(
            &[
                "run",
                "--run-mode",
                "fallback",
                "--harness",
                "claude-code",
                "--prompt",
                "hi",
                "--model",
                "bad-model",
                "--model",
                "good-model",
                "--bin",
                &bin_override("claude-code"),
                "--compact",
            ],
            &[
                ("MOCK_FAIL_IF_MODEL", "bad-model"),
                ("MOCK_FAIL_STDERR", fail_stderr),
            ],
        );
        assert!(
            output.status.success(),
            "{kind}: exit {:?}",
            output.status.code()
        );
        let v = json_stdout(&output);
        assert_eq!(v["fallback"]["ran"], "claude-code", "{kind}");
        let fell = v["fallback"]["fell_through"].as_array().unwrap();
        assert_eq!(fell.len(), 1, "{kind}");
        assert_eq!(fell[0]["harness"], "claude-code", "{kind}");
        assert_eq!(fell[0]["reason"], reason, "{kind}");
        // Both attempts are recorded, in priority order: the bad model, then good.
        let results = v["results"].as_array().unwrap();
        assert_eq!(results.len(), 2, "{kind}");
        assert_eq!(results[0]["model"], "bad-model", "{kind}");
        assert_eq!(results[0]["status"], "nonzero", "{kind}");
        assert_eq!(results[0]["failure_kind"], kind, "{kind}");
        assert_eq!(results[1]["model"], "good-model", "{kind}");
        assert_eq!(results[1]["status"], "ok", "{kind}");
    }
}

#[test]
fn fallback_does_not_try_the_next_model_after_the_first_did_work() {
    // The exact counterpart of `fallback_falls_through_a_bad_model_to_the_next_model`:
    // the same two per-model rejections, on a model that was billed for a real
    // turn before the rejection landed. Work evidence is consulted first, so each
    // one STOPS the chain here instead of paying for the same work again on the
    // next model. Asserted on both deliveries: the model axis streams under
    // fallback, and the two must agree on the model they settle on.
    for (fail_stderr, kind) in [
        ("error: model not found: opus", "model_not_found"),
        (
            "Error 429: rate limit exceeded, too many requests",
            "rate_limit",
        ),
    ] {
        let mock = mock_bin().display().to_string();
        let project = format!(
            r#"
            harnesses = ["claude-code"]
            run_mode = "fallback"
            models = ["opus", "sonnet"]

            [harness.claude-code]
            bin = '{mock}'
            env = {{ MOCK_EXIT = "1", MOCK_STDOUT = "{{\"type\":\"result\",\"result\":\"partial\",\"usage\":{{\"input_tokens\":812,\"output_tokens\":96}}}}", MOCK_STDERR = "{fail_stderr}" }}
            "#
        );
        let fx = ConfigFixture::new(&format!("fallback-model-work-{kind}"), &project, "");
        let buffered = run_with_config(
            &["run", "--prompt", "hi", "--cwd", &fx.cwd(), "--compact"],
            &[],
            &fx.user_config(),
        );
        let streamed = run_with_config(
            &["run", "--prompt", "hi", "--cwd", &fx.cwd(), "--stream"],
            &[],
            &fx.user_config(),
        );
        let envelopes = stream_envelopes(&streamed);
        let terminal = envelopes.last().expect("a terminal report line");
        assert_eq!(terminal["type"], "result", "{kind}");
        assert_eq!(buffered.status.code(), Some(1), "{kind}");
        assert_eq!(
            buffered.status.code(),
            streamed.status.code(),
            "{kind}: exit codes disagreed"
        );
        let buffered_report = json_stdout(&buffered);
        assert_eq!(
            fallback_selection(&buffered_report),
            fallback_selection(&terminal["report"]),
            "{kind}: streamed and buffered fallback disagreed"
        );
        for (path, v) in [
            ("buffered", &buffered_report),
            ("streamed", &terminal["report"]),
        ] {
            assert_eq!(v["fallback"]["ran"], "claude-code", "{kind}/{path}");
            assert_eq!(
                v["fallback"]["fell_through"].as_array().unwrap().len(),
                0,
                "{kind}/{path}"
            );
            let results = v["results"].as_array().unwrap();
            assert_eq!(
                results.len(),
                1,
                "{kind}/{path}: the second model must never be spawned"
            );
            assert_eq!(results[0]["model"], "opus", "{kind}/{path}");
            // The rejection is still classified honestly; it just did not fall through.
            assert_eq!(results[0]["failure_kind"], kind, "{kind}/{path}");
            assert_eq!(results[0]["usage"]["input_tokens"], 812, "{kind}/{path}");
        }
    }
}

#[test]
fn fallback_tries_every_model_of_a_harness_before_the_next_harness() {
    // The chain is harness-major, model-minor: with the `opus` model doomed on
    // every harness, claude-code's SECOND model (`sonnet`) is tried before codex
    // is reached — so the run stops at (claude-code, sonnet) and codex is never
    // spawned. This is the direct proof of the fan-out ordering in fallback.
    let output = run(
        &[
            "run",
            "--run-mode",
            "fallback",
            "--harness",
            "claude-code,codex",
            "--prompt",
            "hi",
            "--model",
            "opus",
            "--model",
            "sonnet",
            "--bin",
            &bin_override("claude-code"),
            "--bin",
            &bin_override("codex"),
            "--compact",
        ],
        &[("MOCK_FAIL_IF_MODEL", "opus")],
    );
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let v = json_stdout(&output);
    assert_eq!(v["fallback"]["ran"], "claude-code");
    let fell = v["fallback"]["fell_through"].as_array().unwrap();
    // Only (claude-code, opus) fell through; (claude-code, sonnet) then ran.
    assert_eq!(fell.len(), 1);
    assert_eq!(fell[0]["harness"], "claude-code");
    assert_eq!(fell[0]["reason"], "model-not-found");
    let results = v["results"].as_array().unwrap();
    assert_eq!(results.len(), 2, "codex must never be attempted");
    assert_eq!(
        (
            results[0]["harness"].as_str().unwrap(),
            results[0]["model"].as_str().unwrap(),
            results[0]["status"].as_str().unwrap(),
        ),
        ("claude-code", "opus", "nonzero")
    );
    assert_eq!(
        (
            results[1]["harness"].as_str().unwrap(),
            results[1]["model"].as_str().unwrap(),
            results[1]["status"].as_str().unwrap(),
        ),
        ("claude-code", "sonnet", "ok")
    );
    assert!(
        results.iter().all(|r| r["harness"] != "codex"),
        "codex is beyond the harness that ran and must be absent"
    );
}

#[test]
fn fallback_reports_no_run_when_every_model_of_every_harness_fails() {
    // Every (harness, model) pair rejects with a per-model failure, so the whole
    // chain falls through: `ran` is null, exit is 1, and the four attempts are
    // recorded in harness-major/model-minor order.
    let output = run(
        &[
            "run",
            "--run-mode",
            "fallback",
            "--harness",
            "claude-code,codex",
            "--prompt",
            "hi",
            "--model",
            "opus",
            "--model",
            "sonnet",
            "--bin",
            &bin_override("claude-code"),
            "--bin",
            &bin_override("codex"),
            "--compact",
        ],
        // Every run fails with a model_not_found stderr regardless of the model.
        &[
            ("MOCK_EXIT", "1"),
            ("MOCK_STDERR", "error: model not found"),
        ],
    );
    assert_eq!(output.status.code(), Some(1), "no candidate ran → exit 1");
    let v = json_stdout(&output);
    assert!(v["fallback"]["ran"].is_null());
    let fell = v["fallback"]["fell_through"].as_array().unwrap();
    assert_eq!(fell.len(), 4);
    let order: Vec<(&str, &str)> = v["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["harness"].as_str().unwrap(), r["model"].as_str().unwrap()))
        .collect();
    assert_eq!(
        order,
        vec![
            ("claude-code", "opus"),
            ("claude-code", "sonnet"),
            ("codex", "opus"),
            ("codex", "sonnet"),
        ]
    );
}

#[test]
fn all_selects_every_harness_by_model_cross_product() {
    // `--all` composes with the model fan-out: every harness runs once per model,
    // harness-major then model-minor. Declared variants remain opt-in and never
    // join this base-only selection. Pinned with --print-command (no spawning).
    let fx = ConfigFixture::new(
        "all-excludes-variants",
        "[harness.claude-code.variant.work]\nmodel = \"variant-only\"\n",
        "",
    );
    let output = run_with_config(
        &[
            "run",
            "--all",
            "--prompt",
            "hi",
            "--cwd",
            &fx.cwd(),
            "--model",
            "a",
            "--model",
            "b",
            "--print-command",
            "--compact",
        ],
        &[],
        &fx.user_config(),
    );
    assert!(output.status.success());
    let v = json_stdout(&output);
    let results = v["results"].as_array().unwrap();
    // Two models across all eight harnesses.
    assert_eq!(results.len(), ALL_IDS.len() * 2);
    assert_eq!(v["models"][0], "a");
    assert_eq!(v["models"][1], "b");
    // Each harness's two models are adjacent (model-minor within a harness).
    for chunk in results.chunks(2) {
        assert_eq!(chunk[0]["harness"], chunk[1]["harness"]);
        assert!(chunk[0]["variant"].is_null());
        assert_eq!(chunk[0]["harness_id"], chunk[0]["harness"]);
        assert_eq!(chunk[0]["model"], "a");
        assert_eq!(chunk[1]["model"], "b");
    }
}

#[test]
fn multiple_models_apply_the_schema_to_every_pair() {
    // A model fan-out composes with --schema: each (harness, model) pair is
    // validated independently, so both results conform.
    let schema = temp_file("models-schema", PERSON_SCHEMA);
    let output = run(
        &[
            "run",
            "--harness",
            "crush",
            "--prompt",
            "describe ada",
            "--model",
            "m1",
            "--model",
            "m2",
            "--schema",
            &schema,
            "--bin",
            &bin_override("crush"),
            "--compact",
        ],
        &[("MOCK_STDOUT", r#"{"name":"Ada","age":36}"#)],
    );
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let v = json_stdout(&output);
    let results = v["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);
    for (i, model) in ["m1", "m2"].iter().enumerate() {
        assert_eq!(results[i]["model"], *model);
        assert_eq!(results[i]["schema_valid"], true, "pair {model}");
        assert_eq!(results[i]["structured"]["name"], "Ada");
    }
}

#[test]
fn multiple_models_history_records_each_pair_model() {
    // Each history record carries the model its result ran with, so a fan-out
    // writes one record per model with the right model on each.
    let hist = session_store_dir("models-hist");
    let cwd = session_store_dir("models-hist-cwd");
    let output = run(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "hi",
            "--model",
            "opus",
            "--model",
            "sonnet",
            "--history",
            "--history-dir",
            &hist.display().to_string(),
            "--cwd",
            &cwd.display().to_string(),
            "--bin",
            &bin_override("codex"),
            "--compact",
        ],
        &[("MOCK_STDOUT", HISTORY_CODEX_TELEMETRY)],
    );
    assert!(
        output.status.success(),
        "exit {:?}: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let v = json_stdout(&output);
    let hist_file = v["history_file"].as_str().expect("history recorded");
    let text = std::fs::read_to_string(hist_file).unwrap();
    let models: Vec<String> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            serde_json::from_str::<Value>(l).unwrap()["model"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(models, vec!["opus".to_string(), "sonnet".to_string()]);
    let _ = std::fs::remove_dir_all(&hist);
    let _ = std::fs::remove_dir_all(&cwd);
}

#[test]
fn multiple_models_output_dir_disambiguates_the_same_harness() {
    // One harness fanned over two models writes two same-harness results, so the
    // output-dir stems are indexed (neither overwrites the other).
    let dir = std::env::temp_dir().join(format!("oneharness-modelsout-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--model",
            "opus",
            "--model",
            "sonnet",
            "--output-dir",
            &dir.display().to_string(),
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[("MOCK_STDOUT", HISTORY_CODEX_TELEMETRY)],
    );
    assert!(output.status.success());
    assert!(dir.join("claude-code-0.stdout").exists());
    assert!(dir.join("claude-code-1.stdout").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_multi_model_run_refuses_single_unit_shapes() {
    // A model fan-out multiplies the run into several units, so every single-unit
    // shape is a loud usage error before anything spawns. `--stream` is refused
    // for the concurrency, not the count, so it is refused only in this default
    // `parallel` mode — under fallback the pairs are a priority chain that
    // streams (`stream_under_fallback_publishes_only_the_model_that_runs`).
    let base = [
        "run",
        "--harness",
        "claude-code",
        "--model",
        "a",
        "--model",
        "b",
        "--compact",
    ];
    let cases: &[(&[&str], &str)] = &[
        (&["--resume", "sess"], "--resume/--fork"),
        (&["--resume", "sess", "--fork"], "--resume/--fork"),
        (&["--session", "sess"], "--session"),
        (&["--stream"], "--stream"),
        // A batch (two prompts) plus a model fan-out is refused too.
        (&["--prompt", "one", "--prompt", "two"], "batch run"),
    ];
    for (extra, needle) in cases {
        let mut args: Vec<&str> = base.to_vec();
        // The batch case supplies its own prompts; others need one.
        if !extra.contains(&"--prompt") {
            args.extend(["--prompt", "hi"]);
        }
        args.extend(extra.iter().copied());
        let output = run(&args, &[]);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{needle}: expected a usage error"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("multi-model") && stderr.contains(needle),
            "{needle}: stderr was {stderr}"
        );
    }
}

#[test]
fn fallback_falls_through_a_not_installed_harness() {
    // claude-code is "not installed"; the run falls through to codex, which runs.
    let output = run(
        &[
            "run",
            "--run-mode",
            "fallback",
            "--harness",
            "claude-code,codex",
            "--prompt",
            "hi",
            "--bin",
            &missing_bin("claude-code"),
            "--bin",
            &bin_override("codex"),
            "--compact",
        ],
        &[],
    );
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let v = json_stdout(&output);
    // The fallback block names the harness that ran and the ones fallen through.
    assert_eq!(v["fallback"]["ran"], "codex");
    assert_eq!(v["fallback"]["fell_through"][0]["harness"], "claude-code");
    assert_eq!(v["fallback"]["fell_through"][0]["reason"], "not-installed");
    assert!(v["batch"].is_null());
    // results carry every *attempted* harness in priority order: the skipped
    // one, then the one that ran. Candidates after it are never in the list.
    let results = v["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["harness"], "claude-code");
    assert_eq!(results[0]["status"], "skipped");
    assert_eq!(results[1]["harness"], "codex");
    assert_eq!(results[1]["status"], "ok");
}

#[test]
fn fallback_stops_at_a_real_task_failure_and_does_not_fall_through() {
    // The first harness actually RUNS and exits non-zero (a plain task failure,
    // not a setup problem). Fallback must stop there — the second harness is
    // never spawned — so a long real run can never trigger a fallback.
    let output = run(
        &[
            "run",
            "--run-mode",
            "fallback",
            "--harness",
            "claude-code,codex",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("claude-code"),
            "--bin",
            &bin_override("codex"),
            "--compact",
        ],
        &[("MOCK_EXIT", "1"), ("MOCK_STDERR", "boom: the task failed")],
    );
    assert_eq!(output.status.code(), Some(1), "a real failure exits 1");
    let v = json_stdout(&output);
    assert_eq!(v["fallback"]["ran"], "claude-code");
    assert_eq!(v["fallback"]["fell_through"].as_array().unwrap().len(), 0);
    // Only the harness that ran is present; codex was never attempted.
    let results = v["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["harness"], "claude-code");
    assert_eq!(results[0]["status"], "nonzero");
}

#[test]
fn fallback_stops_at_a_classified_rate_limit_and_does_not_fall_through() {
    // The core guarantee: a non-zero exit that is *classified* (here rate_limit,
    // a transient hiccup of a WORKING, authenticated harness) is still a real
    // run — fallback must STOP, not fall through, so a working harness's 429 is
    // never masked behind the next candidate. Same for an unknown model.
    for (needle, kind) in [
        (
            "Error 429: rate limit exceeded, too many requests",
            "rate_limit",
        ),
        (
            "model not found: no such model 'gpt-nope'",
            "model_not_found",
        ),
    ] {
        let output = run(
            &[
                "run",
                "--run-mode",
                "fallback",
                "--harness",
                "claude-code,codex",
                "--prompt",
                "hi",
                "--bin",
                &bin_override("claude-code"),
                "--bin",
                &bin_override("codex"),
                "--compact",
            ],
            &[("MOCK_EXIT", "1"), ("MOCK_STDERR", needle)],
        );
        assert_eq!(output.status.code(), Some(1), "{kind}: should stop, exit 1");
        let v = json_stdout(&output);
        // Stopped at the first harness — it ran (badly), so it is the answer.
        assert_eq!(v["fallback"]["ran"], "claude-code", "{kind}");
        assert_eq!(
            v["fallback"]["fell_through"].as_array().unwrap().len(),
            0,
            "{kind}"
        );
        let results = v["results"].as_array().unwrap();
        assert_eq!(results.len(), 1, "{kind}: codex must never be attempted");
        assert_eq!(results[0]["failure_kind"], kind);
    }
}

#[test]
fn fallback_falls_through_quota_across_a_multi_harness_chain() {
    // Two setup failures in a row before a working harness: not-installed, then a
    // quota/no-credit rejection (a provisioning problem, like auth), then a run.
    // Proves quota falls through AND that a chain of >1 fall-through works, with
    // per-harness behavior scripted via config env.
    let mock = mock_bin().display().to_string();
    let project = format!(
        r#"
        harnesses = ["claude-code", "codex", "opencode"]
        run_mode = "fallback"

        [harness.codex]
        bin = '{mock}'
        env = {{ MOCK_EXIT = "1", MOCK_STDERR = "Error: insufficient_quota — your credit balance is too low" }}

        [harness.opencode]
        bin = '{mock}'
        "#
    );
    let fx = ConfigFixture::new("fallback-quota", &project, "");
    let output = run_with_config(
        &[
            "run",
            "--prompt",
            "hi",
            "--cwd",
            &fx.cwd(),
            // claude-code is not installed (points at a missing path).
            "--bin",
            &missing_bin("claude-code"),
            "--compact",
        ],
        &[],
        &fx.user_config(),
    );
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let v = json_stdout(&output);
    assert_eq!(v["fallback"]["ran"], "opencode");
    let fell = v["fallback"]["fell_through"].as_array().unwrap();
    assert_eq!(fell.len(), 2);
    assert_eq!(fell[0]["harness"], "claude-code");
    assert_eq!(fell[0]["reason"], "not-installed");
    assert_eq!(fell[1]["harness"], "codex");
    assert_eq!(fell[1]["reason"], "quota");
    let results = v["results"].as_array().unwrap();
    assert_eq!(results.len(), 3);
    assert_eq!(results[1]["failure_kind"], "quota");
    assert_eq!(results[2]["status"], "ok");
}

#[test]
fn fallback_via_env_override_and_cli_beats_config() {
    let mock = mock_bin().display().to_string();

    // ONEHARNESS_RUN_MODE drives fallback with no config/CLI flag: the missing
    // first harness falls through to the mock second. The presence of a
    // `fallback` block (parallel emits none) proves the env override took.
    let fx = ConfigFixture::new("fallback-env", "", "");
    let output = run_with_config(
        &[
            "run",
            "--harness",
            "claude-code,codex",
            "--prompt",
            "hi",
            "--cwd",
            &fx.cwd(),
            "--bin",
            &missing_bin("claude-code"),
            "--bin",
            &bin_override("codex"),
            "--compact",
        ],
        &[("ONEHARNESS_RUN_MODE", "fallback")],
        &fx.user_config(),
    );
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let v = json_stdout(&output);
    assert_eq!(
        v["fallback"]["ran"], "codex",
        "env override should select fallback"
    );

    // CLI `--run-mode parallel` overrides config `run_mode = "fallback"`: both
    // harnesses run and there is no fallback block.
    let project = format!(
        "run_mode = \"fallback\"\nharnesses = [\"claude-code\", \"codex\"]\n\
         [harness.claude-code]\nbin = '{mock}'\n[harness.codex]\nbin = '{mock}'\n"
    );
    let fx = ConfigFixture::new("fallback-cli-wins", &project, "");
    let output = run_with_config(
        &[
            "run",
            "--run-mode",
            "parallel",
            "--prompt",
            "hi",
            "--cwd",
            &fx.cwd(),
            "--compact",
        ],
        &[],
        &fx.user_config(),
    );
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let v = json_stdout(&output);
    assert!(
        v["fallback"].is_null(),
        "CLI parallel must beat config fallback"
    );
    assert_eq!(v["results"].as_array().unwrap().len(), 2);
}

#[test]
fn fallback_stops_at_a_timeout_and_does_not_fall_through() {
    // A slow real run that times out is a genuine run, not a setup failure:
    // fallback stops there rather than masking it behind the next harness.
    let output = run(
        &[
            "run",
            "--run-mode",
            "fallback",
            "--harness",
            "claude-code,codex",
            "--prompt",
            "hi",
            "--timeout",
            "1",
            "--bin",
            &bin_override("claude-code"),
            "--bin",
            &bin_override("codex"),
            "--compact",
        ],
        &[("MOCK_SLEEP_MS", "4000")],
    );
    assert_eq!(output.status.code(), Some(1));
    let v = json_stdout(&output);
    assert_eq!(v["fallback"]["ran"], "claude-code");
    let results = v["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["status"], "timeout");
}

#[test]
fn fallback_falls_through_an_auth_failure_to_a_working_harness() {
    // The first harness spawns but is rejected before doing any work (an auth
    // failure classified from its output) — a setup problem, so fallback tries
    // the next. Per-harness behavior is scripted via config `[harness.<id>.env]`
    // (global env would hit both). This also proves config drives run_mode.
    let mock = mock_bin().display().to_string();
    let project = format!(
        r#"
        harnesses = ["claude-code", "codex"]
        run_mode = "fallback"

        [harness.claude-code]
        bin = '{mock}'
        env = {{ MOCK_EXIT = "1", MOCK_STDERR = "Error: unauthorized (invalid api key)" }}

        [harness.codex]
        bin = '{mock}'
        "#
    );
    let fx = ConfigFixture::new("fallback-auth", &project, "");
    let output = run_with_config(
        &["run", "--prompt", "hi", "--cwd", &fx.cwd(), "--compact"],
        &[],
        &fx.user_config(),
    );
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let v = json_stdout(&output);
    assert_eq!(v["fallback"]["ran"], "codex");
    assert_eq!(v["fallback"]["fell_through"][0]["harness"], "claude-code");
    assert_eq!(v["fallback"]["fell_through"][0]["reason"], "auth");
    let results = v["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["status"], "nonzero");
    assert_eq!(results[0]["failure_kind"], "auth");
    assert_eq!(results[1]["harness"], "codex");
    assert_eq!(results[1]["status"], "ok");
}

#[test]
fn fallback_falls_through_a_clean_exit_provider_auth_error() {
    let mock = mock_bin().display().to_string();
    let provider_error = r#"{"type":"result","subtype":"success","is_error":true,"api_error_status":401,"result":"Invalid API key · Fix external API key"}"#;
    let project = format!(
        r#"
        harnesses = ["claude-code", "codex"]
        run_mode = "fallback"

        [harness.claude-code]
        bin = '{mock}'
        env = {{ MOCK_STDOUT = '{provider_error}' }}

        [harness.codex]
        bin = '{mock}'
        "#
    );
    let fx = ConfigFixture::new("fallback-clean-provider-auth", &project, "");
    let output = run_with_config(
        &["run", "--prompt", "hi", "--cwd", &fx.cwd(), "--compact"],
        &[],
        &fx.user_config(),
    );
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let value = json_stdout(&output);
    assert_eq!(value["fallback"]["ran"], "codex");
    assert_eq!(value["fallback"]["fell_through"][0]["reason"], "auth");
    assert_eq!(value["results"][0]["status"], "ok");
    assert_eq!(value["results"][0]["failure_kind"], "auth");
    assert_eq!(value["results"][1]["status"], "ok");
}

#[test]
fn fallback_falls_through_a_clean_exit_provider_quota_error() {
    let mock = mock_bin().display().to_string();
    let provider_error = r#"{"type":"result","subtype":"success","is_error":true,"api_error_status":400,"result":"insufficient_quota: credit balance exhausted"}"#;
    let project = format!(
        r#"
        harnesses = ["claude-code", "codex"]
        run_mode = "fallback"

        [harness.claude-code]
        bin = '{mock}'
        env = {{ MOCK_STDOUT = '{provider_error}' }}

        [harness.codex]
        bin = '{mock}'
        "#
    );
    let fx = ConfigFixture::new("fallback-clean-provider-quota", &project, "");
    let output = run_with_config(
        &["run", "--prompt", "hi", "--cwd", &fx.cwd(), "--compact"],
        &[],
        &fx.user_config(),
    );
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let value = json_stdout(&output);
    assert_eq!(value["fallback"]["ran"], "codex");
    assert_eq!(value["fallback"]["fell_through"][0]["reason"], "quota");
    assert_eq!(value["results"][0]["status"], "ok");
    assert_eq!(value["results"][0]["failure_kind"], "quota");
    assert_eq!(value["results"][1]["status"], "ok");
}

#[test]
fn fallback_falls_through_zero_work_claude_subscription_limit_captures() {
    let mock = mock_bin().display().to_string();
    let captures = [
        (
            "session-json",
            "MOCK_STDOUT",
            include_str!("fixtures/claude-session-limit.json"),
            "1",
        ),
        (
            "session-text",
            "MOCK_STDERR",
            include_str!("fixtures/claude-session-limit.txt"),
            "1",
        ),
        (
            "session-text-stdout",
            "MOCK_STDOUT",
            include_str!("fixtures/claude-session-limit.txt"),
            "1",
        ),
        // The same rejection with a different qualifier, printed as bare text.
        // There is no record to read structurally here — the line is all the
        // harness gives — so this is the surface the phrase match exists for,
        // and the surface the enumerated list was one wording short on.
        (
            "weekly-text",
            "MOCK_STDERR",
            include_str!("fixtures/claude-weekly-limit.txt"),
            "1",
        ),
        (
            "weekly-text-stdout",
            "MOCK_STDOUT",
            include_str!("fixtures/claude-weekly-limit.txt"),
            "1",
        ),
    ];

    for (tag, stream, capture, exit) in captures {
        let capture = serde_json::to_string(capture.trim()).unwrap();
        let project = format!(
            r#"
            harnesses = ["claude-code", "codex"]
            run_mode = "fallback"

            [harness.claude-code]
            bin = '{mock}'
            env = {{ MOCK_EXIT = "{exit}", {stream} = {capture} }}

            [harness.codex]
            bin = '{mock}'
            "#
        );
        let fx = ConfigFixture::new(&format!("fallback-claude-limit-{tag}"), &project, "");
        let output = run_with_config(
            &["run", "--prompt", "hi", "--cwd", &fx.cwd(), "--compact"],
            &[],
            &fx.user_config(),
        );
        assert!(
            output.status.success(),
            "{tag}: exit {:?}, stderr {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
        let value = json_stdout(&output);
        assert_eq!(value["fallback"]["ran"], "codex", "{tag}");
        assert_eq!(
            value["fallback"]["fell_through"][0]["reason"], "quota",
            "{tag}"
        );
        let expected_status = if exit == "0" { "ok" } else { "nonzero" };
        assert_eq!(value["results"][0]["status"], expected_status, "{tag}");
        assert_eq!(value["results"][0]["failure_kind"], "quota", "{tag}");
        assert_eq!(value["results"][1]["status"], "ok", "{tag}");
    }
}

/// The captured record from issue #1211: a Claude session limit that did no work
/// at all (zero tokens, empty `modelUsage`, sub-second) and reports itself
/// through `terminal_reason: "api_error"` + `api_error_status: 429` while
/// `subtype` still reads `"success"` and `is_error` is absent entirely.
///
/// Two things made this dead-end a run-killer with two authenticated codex
/// identities sitting idle: the embedded `429` was read by the generic
/// vocabulary as the deliberately non-fall-through `rate_limit`, and the
/// `is_error`-only envelope gate never let the record reach the Claude-specific
/// reading at all.
#[test]
fn fallback_falls_through_a_claude_session_limit_reported_as_an_api_error() {
    let mock = mock_bin().display().to_string();
    let capture =
        serde_json::to_string(include_str!("fixtures/claude-session-limit-api-error.json").trim())
            .unwrap();
    let alternate =
        r#"{"type":"item.completed","item":{"type":"agent_message","text":"served-by-codex"}}"#;
    let project = format!(
        r#"
        harnesses = ["claude-code", "codex"]
        run_mode = "fallback"

        [harness.claude-code]
        bin = '{mock}'
        env = {{ MOCK_EXIT = "1", MOCK_STDOUT = {capture} }}

        [harness.codex]
        bin = '{mock}'
        env = {{ MOCK_EXIT = "0", MOCK_STDOUT = '{alternate}' }}
        "#
    );
    let fx = ConfigFixture::new("fallback-claude-session-limit-api-error", &project, "");
    let output = run_with_config(
        &["run", "--prompt", "hi", "--cwd", &fx.cwd(), "--compact"],
        &[],
        &fx.user_config(),
    );
    assert!(
        output.status.success(),
        "exit {:?}, stderr {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let value = json_stdout(&output);
    assert_eq!(value["fallback"]["ran"], "codex");
    assert_eq!(
        value["fallback"]["fell_through"][0]["harness"],
        "claude-code"
    );
    assert_eq!(value["fallback"]["fell_through"][0]["reason"], "quota");
    assert_eq!(value["results"][0]["status"], "nonzero");
    assert_eq!(
        value["results"][0]["failure_kind"], "quota",
        "the embedded 429 must not out-rank the session-limit message"
    );
    assert_eq!(value["results"][0]["failure_kind_source"], "stdout");
    assert_eq!(value["results"][1]["status"], "ok");
    assert_eq!(value["results"][1]["text"], "served-by-codex");
}

/// The captured weekly-limit record, driven through a real chain: the same
/// zero-work shape as the session-limit capture with one word changed
/// (`weekly`), which was enough to miss every phrase in the list and kill the
/// dispatch while two authenticated candidates at 4% and 53% of their windows
/// sat unused.
///
/// The chain here is the one that failed — several Claude identities ahead of
/// codex — so the assertion is not just "something else ran" but that the
/// rejection propagates *past every exhausted candidate* to the healthy one.
#[test]
fn fallback_falls_through_a_claude_weekly_limit_reported_as_an_api_error() {
    let mock = mock_bin().display().to_string();
    let capture =
        serde_json::to_string(include_str!("fixtures/claude-weekly-limit-api-error.json").trim())
            .unwrap();
    let alternate =
        r#"{"type":"item.completed","item":{"type":"agent_message","text":"served-by-codex"}}"#;
    let project = format!(
        r#"
        harnesses = ["claude-code:alternate", "claude-code:alternate2", "codex"]
        run_mode = "fallback"

        [harness.claude-code.variant.alternate]
        bin = '{mock}'
        env = {{ MOCK_EXIT = "1", MOCK_STDOUT = {capture} }}

        [harness.claude-code.variant.alternate2]
        bin = '{mock}'
        env = {{ MOCK_EXIT = "1", MOCK_STDOUT = {capture} }}

        [harness.codex]
        bin = '{mock}'
        env = {{ MOCK_EXIT = "0", MOCK_STDOUT = '{alternate}' }}
        "#
    );
    let fx = ConfigFixture::new("fallback-claude-weekly-limit-api-error", &project, "");
    let output = run_with_config(
        &["run", "--prompt", "hi", "--cwd", &fx.cwd(), "--compact"],
        &[],
        &fx.user_config(),
    );
    assert!(
        output.status.success(),
        "exit {:?}, stderr {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let value = json_stdout(&output);
    assert_eq!(value["fallback"]["ran"], "codex");
    let fell_through = value["fallback"]["fell_through"].as_array().unwrap();
    assert_eq!(
        fell_through.len(),
        2,
        "both exhausted identities are skipped"
    );
    for entry in fell_through {
        assert_eq!(entry["reason"], "quota");
    }
    for index in [0, 1] {
        assert_eq!(value["results"][index]["status"], "nonzero");
        assert_eq!(
            value["results"][index]["failure_kind"], "quota",
            "a zero-work 429 is a quota rejection whatever word precedes `limit`"
        );
        assert_eq!(value["results"][index]["failure_kind_source"], "stdout");
    }
    assert_eq!(value["results"][2]["status"], "ok");
    assert_eq!(value["results"][2]["text"], "served-by-codex");
}

/// The structural rule end to end, on prose no phrase list anticipates — which
/// is the whole point of having it. A candidate that reports a `429` and no
/// accounting is routed around whatever it says and whichever harness said it:
/// the wording that stranded the chain twice is exactly what stops being
/// load-bearing here, and the reading is dialect-agnostic because
/// `api_error_status` states what the *provider* did.
///
/// Each case also exits **zero**, which isolates the record reading: the
/// non-zero text classifier never runs, so the fall-through can only come from
/// the harness's own declaration.
#[test]
fn fallback_falls_through_a_zero_work_429_with_no_recognized_limit_wording() {
    let mock = mock_bin().display().to_string();
    let served =
        r#"{"type":"item.completed","item":{"type":"agent_message","text":"served-by-next"}}"#;
    // (tag, the harness whose dialect is exercised, the rejection's prose)
    let cases = [
        (
            "claude-rephrased",
            "claude-code",
            "Your plan's usage cap for this period has been used up",
        ),
        ("claude-bare-429", "claude-code", "Rate limit exceeded"),
        ("generic-dialect", "opencode", "Too many requests"),
    ];

    for (tag, first, prose) in cases {
        let rejection = serde_json::to_string(
            &serde_json::json!({
                "type": "result",
                "subtype": "success",
                "api_error_status": 429,
                "num_turns": 1,
                "usage": {"input_tokens": 0, "output_tokens": 0},
                "modelUsage": {},
                "result": prose,
            })
            .to_string(),
        )
        .unwrap();
        let project = format!(
            r#"
            harnesses = ["{first}", "codex"]
            run_mode = "fallback"

            [harness.{first}]
            bin = '{mock}'
            env = {{ MOCK_EXIT = "0", MOCK_STDOUT = {rejection} }}

            [harness.codex]
            bin = '{mock}'
            env = {{ MOCK_EXIT = "0", MOCK_STDOUT = '{served}' }}
            "#
        );
        let fx = ConfigFixture::new(&format!("fallback-unworded-429-{tag}"), &project, "");
        let output = run_with_config(
            &["run", "--prompt", "hi", "--cwd", &fx.cwd(), "--compact"],
            &[],
            &fx.user_config(),
        );
        assert!(
            output.status.success(),
            "{tag}: exit {:?}, stderr {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
        let value = json_stdout(&output);
        assert_eq!(value["fallback"]["ran"], "codex", "{tag}");
        assert_eq!(
            value["fallback"]["fell_through"][0]["harness"], first,
            "{tag}"
        );
        assert_eq!(
            value["fallback"]["fell_through"][0]["reason"], "quota",
            "{tag}: a zero-work 429 is routed around regardless of its wording"
        );
        assert_eq!(value["results"][0]["failure_kind"], "quota", "{tag}");
        assert_eq!(value["results"][1]["text"], "served-by-next", "{tag}");
    }
}

/// The same unrecognized rejection on the **non-zero exit** path, which is the
/// shape a real dispatch dies on: the harness gives up and exits 1. It is the
/// case the structural rule exists for, and the one where the wrong reading is
/// most expensive — the serialized record carries `429`, so the generic
/// vocabulary reads it as the transient, deliberately non-fall-through
/// `rate_limit` and the chain stops with authenticated candidates untried.
/// Reading the record's own declaration first is what routes around it.
#[test]
fn fallback_falls_through_a_zero_work_429_with_unrecognized_wording_on_a_nonzero_exit() {
    let mock = mock_bin().display().to_string();
    let served =
        r#"{"type":"item.completed","item":{"type":"agent_message","text":"served-by-codex"}}"#;
    let rejection = serde_json::to_string(
        &serde_json::json!({
            "type": "result",
            "subtype": "error_during_execution",
            "api_error_status": 429,
            "num_turns": 1,
            "usage": {"input_tokens": 0, "output_tokens": 0},
            "modelUsage": {},
            "result": "Your plan's usage cap for this period has been used up",
        })
        .to_string(),
    )
    .unwrap();
    let project = format!(
        r#"
        harnesses = ["claude-code", "codex"]
        run_mode = "fallback"

        [harness.claude-code]
        bin = '{mock}'
        env = {{ MOCK_EXIT = "1", MOCK_STDOUT = {rejection} }}

        [harness.codex]
        bin = '{mock}'
        env = {{ MOCK_STDOUT = '{served}' }}
        "#
    );
    let fx = ConfigFixture::new("fallback-unworded-429-nonzero", &project, "");
    let output = run_with_config(
        &["run", "--prompt", "hi", "--cwd", &fx.cwd(), "--compact"],
        &[],
        &fx.user_config(),
    );
    assert!(
        output.status.success(),
        "exit {:?}, stderr {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let value = json_stdout(&output);
    assert_eq!(value["fallback"]["ran"], "codex");
    assert_eq!(
        value["fallback"]["fell_through"][0]["harness"],
        "claude-code"
    );
    assert_eq!(value["fallback"]["fell_through"][0]["reason"], "quota");
    // The rejection is still reported honestly as the non-zero run it was.
    assert_eq!(value["results"][0]["status"], "nonzero");
    assert_eq!(value["results"][0]["failure_kind"], "quota");
    assert_eq!(value["results"][1]["text"], "served-by-codex");
}

/// The other edge of the same record: a weekly limit that landed **after** the
/// harness spent real tokens is a run, so the chain stops there rather than
/// handing the task to the next candidate and paying for it twice. The only
/// difference from the capture above is the accounting — same status, same
/// wording, same envelope — which is what makes work, not prose, the
/// discriminator.
#[test]
fn fallback_stops_at_a_weekly_limit_429_that_landed_after_real_work() {
    let mock = mock_bin().display().to_string();
    let worked = serde_json::to_string(
        r#"{"type":"result","subtype":"success","is_error":true,"api_error_status":429,"num_turns":10,"usage":{"input_tokens":17,"cache_read_input_tokens":58446,"output_tokens":800},"modelUsage":{"claude-opus-4-6":{"inputTokens":17}},"result":"You've hit your weekly limit · resets Aug 6, 7am (America/Mexico_City)"}"#,
    )
    .unwrap();
    let project = format!(
        r#"
        harnesses = ["claude-code", "codex"]
        run_mode = "fallback"

        [harness.claude-code]
        bin = '{mock}'
        env = {{ MOCK_EXIT = "1", MOCK_STDOUT = {worked} }}

        [harness.codex]
        bin = '{mock}'
        "#
    );
    let fx = ConfigFixture::new("fallback-weekly-limit-after-work", &project, "");
    let output = run_with_config(
        &["run", "--prompt", "hi", "--cwd", &fx.cwd(), "--compact"],
        &[],
        &fx.user_config(),
    );
    let value = json_stdout(&output);
    assert_eq!(value["fallback"]["ran"], "claude-code");
    assert!(
        value["fallback"]["fell_through"]
            .as_array()
            .unwrap()
            .is_empty(),
        "a harness that did work must never be fallen through"
    );
    let results = value["results"].as_array().unwrap();
    assert_eq!(results.len(), 1, "codex must never be attempted");
    assert_eq!(results[0]["status"], "nonzero");
    assert_eq!(
        results[0]["failure_kind"], "rate_limit",
        "with the work evidence present the 429 keeps its transient reading"
    );
    assert_eq!(results[0]["usage"]["output_tokens"], 800);
}

/// Each of the record's failure declarations is sufficient **on its own** —
/// `terminal_reason: "api_error"` with no status, and a numeric
/// `api_error_status` with no `terminal_reason`, neither carrying `is_error`.
/// The predicate is dialect agnostic, so a non-Claude harness reaches the same
/// classification through the generic vocabulary. The Claude-specific *phrasing*
/// stays scoped to the Claude dialect: the same words from another harness are
/// not exhaustion.
///
/// Every case exits **zero** on purpose. A clean exit is what isolates the
/// envelope predicate: the non-zero text classifier never runs, so the record is
/// classified only if the harness's own declaration is read off the record.
#[test]
fn fallback_reads_an_api_error_envelope_across_declarations_and_dialects() {
    let mock = mock_bin().display().to_string();
    let served =
        r#"{"type":"item.completed","item":{"type":"agent_message","text":"served-by-next"}}"#;
    const LIMIT: &str = "You've hit your session limit · resets 1pm (America/Mexico_City)";
    const CREDIT: &str = "insufficient_quota: credit balance exhausted";
    // (tag, harness whose dialect is exercised, the record's sole failure
    //  declaration, result text, expected outcome)
    let cases = [
        (
            "claude-terminal-reason",
            "claude-code",
            serde_json::json!({"terminal_reason": "api_error"}),
            LIMIT,
            Some("quota"),
        ),
        // `api_error_status` alone, with the 429 the limit message has to
        // out-rank: neither `is_error` nor `terminal_reason` is present.
        (
            "claude-status-only",
            "claude-code",
            serde_json::json!({"api_error_status": 429}),
            LIMIT,
            Some("quota"),
        ),
        (
            "generic-terminal-reason",
            "opencode",
            serde_json::json!({"terminal_reason": "api_error"}),
            CREDIT,
            Some("quota"),
        ),
        (
            "generic-status-only",
            "opencode",
            serde_json::json!({"api_error_status": 402}),
            CREDIT,
            Some("quota"),
        ),
        // The scoping guarantee, at the integration level: OpenCode is the
        // Generic dialect, so Claude's wording is not a quota rejection — the
        // chain stops at a harness that ran rather than burning the next one.
        (
            "generic-ignores-claude-phrasing",
            "opencode",
            serde_json::json!({"terminal_reason": "api_error"}),
            LIMIT,
            None,
        ),
    ];

    for (tag, first, declaration, result_text, expected) in cases {
        let mut record = serde_json::json!({
            "type": "result",
            "subtype": "success",
            "result": result_text,
        });
        for (key, value) in declaration.as_object().unwrap() {
            record[key] = value.clone();
        }
        let record = serde_json::to_string(&record).unwrap();
        let record = serde_json::to_string(&record).unwrap();
        let project = format!(
            r#"
            harnesses = ["{first}", "codex"]
            run_mode = "fallback"

            [harness.{first}]
            bin = '{mock}'
            env = {{ MOCK_EXIT = "0", MOCK_STDOUT = {record} }}

            [harness.codex]
            bin = '{mock}'
            env = {{ MOCK_EXIT = "0", MOCK_STDOUT = '{served}' }}
            "#
        );
        let fx = ConfigFixture::new(&format!("fallback-api-error-envelope-{tag}"), &project, "");
        let output = run_with_config(
            &["run", "--prompt", "hi", "--cwd", &fx.cwd(), "--compact"],
            &[],
            &fx.user_config(),
        );
        assert!(
            output.status.success(),
            "{tag}: exit {:?}, stderr {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
        let value = json_stdout(&output);
        assert_eq!(value["results"][0]["status"], "ok", "{tag}");
        match expected {
            Some(kind) => {
                assert_eq!(value["fallback"]["ran"], "codex", "{tag}");
                assert_eq!(
                    value["fallback"]["fell_through"][0]["reason"], kind,
                    "{tag}"
                );
                assert_eq!(value["results"][0]["failure_kind"], kind, "{tag}");
                assert_eq!(value["results"][1]["text"], "served-by-next", "{tag}");
            }
            None => {
                assert_eq!(value["fallback"]["ran"], first, "{tag}");
                assert!(
                    value["fallback"]["fell_through"]
                        .as_array()
                        .unwrap()
                        .is_empty(),
                    "{tag}"
                );
                assert_eq!(value["results"].as_array().unwrap().len(), 1, "{tag}");
                assert!(value["results"][0]["failure_kind"].is_null(), "{tag}");
            }
        }
    }
}

/// The discriminator, driven through the CLI on the **exact** shape the fix
/// accepts: a record carrying the session-limit signature, the `api_error`
/// envelope, and real spent tokens. Same words as the fall-through capture, same
/// declarations — the only difference is that this harness got somewhere, and
/// that alone has to keep the chain from handing the task to the next candidate
/// and paying for the work twice.
///
/// The 429 case is the sharper one: with the limit signal disqualified, the
/// generic vocabulary reads the embedded status as the transient `rate_limit`,
/// which is a stop. Without a 429 there is nothing left to read and the run stays
/// unclassified — also a stop. Both exit codes, since neither is load-bearing.
#[test]
fn fallback_stops_at_a_session_limit_that_landed_after_real_work() {
    let mock = mock_bin().display().to_string();
    let spent = r#""duration_ms":92610,"usage":{"input_tokens":17,"cache_creation_input_tokens":19624,"cache_read_input_tokens":58446,"output_tokens":800},"modelUsage":{"claude-opus-4-6":{"inputTokens":17}}"#;
    let limit = "You've hit your session limit · resets 1pm (America/Mexico_City)";
    let cases = [
        // (tag, the record's failure declaration, expected failure_kind)
        (
            "mid-run-429",
            r#""terminal_reason":"api_error","api_error_status":429"#,
            Some("rate_limit"),
        ),
        (
            "mid-run-no-status",
            r#""terminal_reason":"api_error""#,
            None,
        ),
        // The real weekly-limit capture, whose expectation this change reverses:
        // ten turns and 78k prompt tokens in, `is_error` set, exit 0. It used to
        // fall through; a harness that did that much work has run.
        ("weekly-capture", r#""is_error":true"#, None),
    ];

    for (tag, declaration, expected_kind) in cases {
        // Escaped as a TOML basic string: the limit message has an apostrophe,
        // which a literal '…' value cannot carry.
        let worked = serde_json::to_string(&format!(
            r#"{{"type":"result","subtype":"success",{declaration},{spent},"result":"{limit}"}}"#
        ))
        .unwrap();
        for exit in ["0", "1"] {
            let project = format!(
                r#"
                harnesses = ["claude-code", "codex"]
                run_mode = "fallback"

                [harness.claude-code]
                bin = '{mock}'
                env = {{ MOCK_EXIT = "{exit}", MOCK_STDOUT = {worked} }}

                [harness.codex]
                bin = '{mock}'
                "#
            );
            let fx = ConfigFixture::new(
                &format!("fallback-limit-after-work-{tag}-{exit}"),
                &project,
                "",
            );
            let output = run_with_config(
                &["run", "--prompt", "hi", "--cwd", &fx.cwd(), "--compact"],
                &[],
                &fx.user_config(),
            );
            let value = json_stdout(&output);
            let at = format!("{tag}/exit {exit}");
            assert_eq!(value["fallback"]["ran"], "claude-code", "{at}");
            assert!(
                value["fallback"]["fell_through"]
                    .as_array()
                    .unwrap()
                    .is_empty(),
                "{at}: a harness that did work must never be fallen through"
            );
            let results = value["results"].as_array().unwrap();
            assert_eq!(results.len(), 1, "{at}: codex must never be attempted");
            let expected_status = if exit == "0" { "ok" } else { "nonzero" };
            assert_eq!(results[0]["status"], expected_status, "{at}");
            match expected_kind {
                Some(kind) => assert_eq!(results[0]["failure_kind"], kind, "{at}"),
                None => assert!(results[0]["failure_kind"].is_null(), "{at}"),
            }
            // The work it did is still reported, which is what makes it a real run.
            assert_eq!(results[0]["usage"]["output_tokens"], 800, "{at}");
        }
    }
}

/// The same guard for an API error carrying no limit wording at all — the
/// widened envelope must not turn a plain mid-run server error into a
/// fall-through either.
#[test]
fn fallback_stops_at_an_api_error_after_real_work_and_does_not_fall_through() {
    let mock = mock_bin().display().to_string();
    let worked = r#"{"type":"result","subtype":"success","terminal_reason":"api_error","api_error_status":500,"duration_ms":92610,"usage":{"input_tokens":17,"output_tokens":800},"modelUsage":{"claude-opus-4-6":{"inputTokens":17}},"result":"API Error: Internal server error"}"#;
    let project = format!(
        r#"
        harnesses = ["claude-code", "codex"]
        run_mode = "fallback"

        [harness.claude-code]
        bin = '{mock}'
        env = {{ MOCK_EXIT = "1", MOCK_STDOUT = '{worked}' }}

        [harness.codex]
        bin = '{mock}'
        "#
    );
    let fx = ConfigFixture::new("fallback-api-error-after-work", &project, "");
    let output = run_with_config(
        &["run", "--prompt", "hi", "--cwd", &fx.cwd(), "--compact"],
        &[],
        &fx.user_config(),
    );
    assert_eq!(output.status.code(), Some(1));
    let value = json_stdout(&output);
    assert_eq!(value["fallback"]["ran"], "claude-code");
    assert!(value["fallback"]["fell_through"]
        .as_array()
        .unwrap()
        .is_empty());
    let results = value["results"].as_array().unwrap();
    assert_eq!(results.len(), 1, "codex must never be attempted");
    assert_eq!(results[0]["status"], "nonzero");
    assert!(results[0]["failure_kind"].is_null());
    assert_eq!(results[0]["usage"]["output_tokens"], 800);
}

/// A timeout stays a real run even when the bytes captured before the deadline
/// carry the exhaustion signature: a slow harness must never be re-classified as
/// one that was rejected, or a long task would silently restart on the next
/// candidate. The mock streams the limit record, then hangs past the deadline.
#[test]
fn fallback_stops_at_a_timeout_carrying_a_session_limit_record() {
    let mock = mock_bin().display().to_string();
    let streamed = format!(
        "{}\n{{\"type\":\"result\",\"result\":\"never delivered\"}}",
        include_str!("fixtures/claude-session-limit-api-error.json").trim()
    );
    let streamed = serde_json::to_string(&streamed).unwrap();
    let project = format!(
        r#"
        harnesses = ["claude-code", "codex"]
        run_mode = "fallback"
        timeout = 1

        [harness.claude-code]
        bin = '{mock}'
        env = {{ MOCK_STDOUT = {streamed}, MOCK_STREAM_DELAY_MS = "6000" }}

        [harness.codex]
        bin = '{mock}'
        "#
    );
    let fx = ConfigFixture::new("fallback-timeout-with-limit-record", &project, "");
    let output = run_with_config(
        &["run", "--prompt", "hi", "--cwd", &fx.cwd(), "--compact"],
        &[],
        &fx.user_config(),
    );
    assert_eq!(output.status.code(), Some(1));
    let value = json_stdout(&output);
    assert_eq!(value["fallback"]["ran"], "claude-code");
    assert!(value["fallback"]["fell_through"]
        .as_array()
        .unwrap()
        .is_empty());
    let results = value["results"].as_array().unwrap();
    assert_eq!(results.len(), 1, "codex must never be attempted");
    assert_eq!(results[0]["status"], "timeout");
    assert!(
        results[0]["failure_kind"].is_null(),
        "a timeout is never a rejection, whatever it managed to print"
    );
}

/// The `codex` → `codex:alternate` chain the second-account setup exists for: an
/// exhausted account must hand the task to the alternate identity. Codex reports
/// the limit as a `turn.failed` event on stdout after the turn started, so the
/// exit code it pairs that with is not load-bearing — both are exercised.
#[test]
fn fallback_falls_through_a_codex_usage_limit_to_the_alternate_account() {
    let mock = mock_bin().display().to_string();
    let capture =
        serde_json::to_string(include_str!("fixtures/codex-usage-limit.jsonl").trim()).unwrap();
    let alternate =
        r#"{"type":"item.completed","item":{"type":"agent_message","text":"served-by-alternate"}}"#;

    for exit in ["0", "1"] {
        let project = format!(
            r#"
            harnesses = ["codex", "codex:alternate"]
            run_mode = "fallback"

            [harness.codex]
            bin = '{mock}'
            env = {{ MOCK_EXIT = "{exit}", MOCK_STDOUT = {capture}, MOCK_STDERR = "Reading additional input from stdin...\n" }}

            [harness.codex.variant.alternate]
            bin = '{mock}'

            [harness.codex.variant.alternate.env]
            MOCK_EXIT = "0"
            MOCK_STDOUT = '{alternate}'
            "#
        );
        let fx = ConfigFixture::new(&format!("fallback-codex-usage-limit-{exit}"), &project, "");
        let output = run_with_config(
            &["run", "--prompt", "hi", "--cwd", &fx.cwd(), "--compact"],
            &[],
            &fx.user_config(),
        );
        assert!(
            output.status.success(),
            "exit {exit}: status {:?}, stderr {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
        let value = json_stdout(&output);
        assert_eq!(value["fallback"]["ran"], "codex:alternate", "exit {exit}");
        assert_eq!(
            value["fallback"]["fell_through"][0]["harness"], "codex",
            "exit {exit}"
        );
        assert_eq!(
            value["fallback"]["fell_through"][0]["reason"], "quota",
            "exit {exit}"
        );
        let expected_status = if exit == "0" { "ok" } else { "nonzero" };
        assert_eq!(
            value["results"][0]["status"], expected_status,
            "exit {exit}"
        );
        assert_eq!(value["results"][0]["failure_kind"], "quota", "exit {exit}");
        assert_eq!(
            value["results"][0]["failure_kind_source"], "stdout",
            "exit {exit}"
        );
        assert_eq!(
            value["results"][1]["harness_id"], "codex:alternate",
            "exit {exit}"
        );
        assert_eq!(value["results"][1]["status"], "ok", "exit {exit}");
        assert_eq!(value["results"][1]["text"], "served-by-alternate");
    }
}

#[test]
fn fallback_stops_at_a_codex_turn_failure_and_does_not_fall_through() {
    // The regression guard for the fall-through above: an ordinary `turn.failed`
    // is a real task failure, so the chain must stop rather than silently re-run
    // the task on the alternate account.
    let mock = mock_bin().display().to_string();
    let capture =
        serde_json::to_string(include_str!("fixtures/codex-turn-failed.jsonl").trim()).unwrap();
    let project = format!(
        r#"
        harnesses = ["codex", "codex:alternate"]
        run_mode = "fallback"

        [harness.codex]
        bin = '{mock}'
        env = {{ MOCK_EXIT = "1", MOCK_STDOUT = {capture} }}

        [harness.codex.variant.alternate]
        bin = '{mock}'
        "#
    );
    let fx = ConfigFixture::new("fallback-codex-turn-failed", &project, "");
    let output = run_with_config(
        &["run", "--prompt", "hi", "--cwd", &fx.cwd(), "--compact"],
        &[],
        &fx.user_config(),
    );
    assert_eq!(output.status.code(), Some(1));
    let value = json_stdout(&output);
    assert_eq!(value["fallback"]["ran"], "codex");
    assert!(value["fallback"]["fell_through"]
        .as_array()
        .unwrap()
        .is_empty());
    let results = value["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["status"], "nonzero");
    assert!(results[0]["failure_kind"].is_null());
}

#[test]
fn fallback_stops_at_a_codex_timeout_and_does_not_fall_through() {
    // A slow Codex run that timed out is a genuine run, not a rejected one: the
    // alternate account must not be tried, even though the same chain falls
    // through a usage limit.
    let mock = mock_bin().display().to_string();
    let project = format!(
        r#"
        harnesses = ["codex", "codex:alternate"]
        run_mode = "fallback"
        timeout = 1

        [harness.codex]
        bin = '{mock}'
        env = {{ MOCK_SLEEP_MS = "4000" }}

        [harness.codex.variant.alternate]
        bin = '{mock}'
        "#
    );
    let fx = ConfigFixture::new("fallback-codex-timeout", &project, "");
    let output = run_with_config(
        &["run", "--prompt", "hi", "--cwd", &fx.cwd(), "--compact"],
        &[],
        &fx.user_config(),
    );
    assert_eq!(output.status.code(), Some(1));
    let value = json_stdout(&output);
    assert_eq!(value["fallback"]["ran"], "codex");
    assert!(value["fallback"]["fell_through"]
        .as_array()
        .unwrap()
        .is_empty());
    let results = value["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["status"], "timeout");
}

#[test]
fn fallback_with_no_runnable_harness_is_a_failure() {
    // Every candidate is a startup failure (none installed): nothing ran, so the
    // run is a hard failure (exit 1) and `ran` is null.
    let output = run(
        &[
            "run",
            "--run-mode",
            "fallback",
            "--harness",
            "claude-code,codex",
            "--prompt",
            "hi",
            "--bin",
            &missing_bin("claude-code"),
            "--bin",
            &missing_bin("codex"),
            "--compact",
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(1));
    let v = json_stdout(&output);
    assert!(v["fallback"]["ran"].is_null());
    assert_eq!(v["fallback"]["fell_through"].as_array().unwrap().len(), 2);
    let results = v["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r["status"] == "skipped"));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no selected harness could be run"),
        "{stderr}"
    );
}

#[test]
fn fallback_runs_harnesses_in_the_caller_priority_order() {
    // The priority chain follows the --harness order, NOT the registry order
    // select_specs returns (registry puts claude-code before cursor). Proven via
    // --print-command, where every candidate is reported (nothing executes and
    // the fallback block stays null on a dry run).
    let output = run(
        &[
            "run",
            "--run-mode",
            "fallback",
            "--harness",
            "cursor,claude-code",
            "--prompt",
            "hi",
            "--print-command",
            "--compact",
        ],
        &[],
    );
    assert!(output.status.success());
    let v = json_stdout(&output);
    assert!(v["fallback"].is_null(), "dry run emits no fallback block");
    assert_eq!(v["dry_run"], true);
    let results = v["results"].as_array().unwrap();
    assert_eq!(results[0]["harness"], "cursor");
    assert_eq!(results[1]["harness"], "claude-code");
}

/// Read a streaming run's NDJSON stdout as envelopes, failing loudly with the
/// raw text (and stderr) when a line is not one.
fn stream_envelopes(output: &Output) -> Vec<Value> {
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line).unwrap_or_else(|err| {
                panic!(
                    "stream line was not JSON: {err}\n--- stdout ---\n{text}\n--- stderr ---\n{}",
                    String::from_utf8_lossy(&output.stderr)
                )
            })
        })
        .collect()
}

#[test]
fn stream_under_fallback_publishes_only_the_candidate_that_runs() {
    // `--stream` under `--run-mode fallback`: the chain still falls through a
    // candidate that could not run at all, and the consumer sees NOTHING from it.
    // The fallen-through candidate here is the real zero-work Claude session-limit
    // rejection (issue #1211), the shape a fallback chain exists to route around.
    let mock = mock_bin().display().to_string();
    let rejection =
        serde_json::to_string(include_str!("fixtures/claude-session-limit-api-error.json").trim())
            .unwrap();
    let transcript = serde_json::to_string(&format!(
        "{}\n{}\n{}\n",
        r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"echo hi"}}]}}"#,
        r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"hi"}]}}"#,
        r#"{"type":"result","subtype":"success","result":"done"}"#,
    ))
    .unwrap();
    let project = format!(
        r#"
        harnesses = ["claude-code", "qwen"]
        run_mode = "fallback"

        [harness.claude-code]
        bin = '{mock}'
        env = {{ MOCK_EXIT = "1", MOCK_STDOUT = {rejection} }}

        [harness.qwen]
        bin = '{mock}'
        env = {{ MOCK_STDOUT = {transcript} }}
        "#
    );
    let fx = ConfigFixture::new("stream-fallback-chain", &project, "");
    let history = hist_dir("stream-fallback-chain");
    let output = run_with_config(
        &[
            "run",
            "--prompt",
            "hi",
            "--cwd",
            &fx.cwd(),
            "--stream",
            "--history",
            "--history-dir",
            &history.display().to_string(),
        ],
        &[],
        &fx.user_config(),
    );
    assert!(
        output.status.success(),
        "exit {:?}: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let envelopes = stream_envelopes(&output);
    // Two event lines (both the winner's) and the terminal report — the
    // fell-through candidate contributed no line a consumer could act on.
    assert_eq!(envelopes.len(), 3, "{envelopes:#?}");
    assert_eq!(envelopes[0]["type"], "event");
    assert_eq!(envelopes[0]["event"]["kind"], "tool_call");
    assert_eq!(envelopes[0]["event"]["name"], "Bash");
    assert_eq!(envelopes[0]["event"]["index"], 0);
    assert_eq!(envelopes[1]["event"]["kind"], "tool_result");
    assert_eq!(envelopes[1]["event"]["index"], 1);
    assert_eq!(envelopes[2]["type"], "result");

    let report = &envelopes[2]["report"];
    assert_eq!(report["fallback"]["ran"], "qwen");
    let fell = report["fallback"]["fell_through"].as_array().unwrap();
    assert_eq!(fell.len(), 1);
    assert_eq!(fell[0]["harness"], "claude-code");
    assert_eq!(fell[0]["reason"], "quota");
    let results = report["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["failure_kind"], "quota");
    // The loser's transcript is still reported — it was withheld from the live
    // stream, not discarded.
    assert!(results[0]["stdout"]
        .as_str()
        .unwrap()
        .contains("session limit"));
    assert!(results[0]["events"].is_null());
    assert_eq!(results[1]["status"], "ok");
    assert_eq!(results[1]["events"].as_array().unwrap().len(), 2);

    // The winner's events were persisted live, under the same run as its
    // terminal record — not re-written when the chain closed.
    let history_file = report["history_file"].as_str().unwrap();
    let lines: Vec<Value> = std::fs::read_to_string(history_file)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let runs: Vec<&Value> = lines.iter().filter(|l| l["type"] != "event").collect();
    assert_eq!(runs.len(), 2, "both attempts recorded: {lines:#?}");
    let events: Vec<&Value> = lines.iter().filter(|l| l["type"] == "event").collect();
    assert_eq!(events.len(), 2);
    assert!(events.iter().all(|e| e["harness"] == "qwen"));
    let _ = std::fs::remove_dir_all(&history);
}

#[test]
fn stream_under_fallback_publishes_only_the_model_that_runs() {
    // The model axis of the same chain: with a model list, `--run-mode fallback`
    // tries the (harness, model) pairs in priority order, so `--stream` serves it
    // exactly as it serves a harness chain (in `parallel` the fan-out is several
    // concurrent results and stays refused — see
    // `a_multi_model_run_refuses_single_unit_shapes`). `opus` is rejected before
    // doing any work and publishes NOTHING a consumer could act on; `sonnet` then
    // runs and its events reach the consumer live, ahead of the terminal report.
    let transcript = format!(
        "{}\n{}\n{}\n",
        r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"echo hi"}}]}}"#,
        r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"hi"}]}}"#,
        r#"{"type":"result","subtype":"success","result":"done"}"#,
    );
    let history = hist_dir("stream-fallback-models");
    let output = run(
        &[
            "run",
            "--run-mode",
            "fallback",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--model",
            "opus",
            "--model",
            "sonnet",
            "--stream",
            "--bin",
            &bin_override("claude-code"),
            "--history",
            "--history-dir",
            &history.display().to_string(),
        ],
        &[
            ("MOCK_FAIL_IF_MODEL", "opus"),
            ("MOCK_FAIL_STDERR", "error: model not found: opus"),
            ("MOCK_STDOUT", &transcript),
        ],
    );
    assert!(
        output.status.success(),
        "exit {:?}: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let envelopes = stream_envelopes(&output);
    // Two event lines (both `sonnet`'s) and the terminal report.
    assert_eq!(envelopes.len(), 3, "{envelopes:#?}");
    assert_eq!(envelopes[0]["type"], "event");
    assert_eq!(envelopes[0]["event"]["kind"], "tool_call");
    assert_eq!(envelopes[0]["event"]["name"], "Bash");
    assert_eq!(envelopes[1]["event"]["kind"], "tool_result");
    assert_eq!(envelopes[2]["type"], "result");

    let report = &envelopes[2]["report"];
    assert_eq!(report["fallback"]["ran"], "claude-code");
    let fell = report["fallback"]["fell_through"].as_array().unwrap();
    assert_eq!(fell.len(), 1);
    assert_eq!(fell[0]["reason"], "model-not-found");
    let results = report["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["model"], "opus");
    assert_eq!(results[0]["failure_kind"], "model_not_found");
    assert!(
        results[0]["events"].is_null(),
        "the rejected model published nothing"
    );
    assert_eq!(results[1]["model"], "sonnet");
    assert_eq!(results[1]["status"], "ok");
    assert_eq!(results[1]["events"].as_array().unwrap().len(), 2);

    // History attributes the streamed events per plan entry, not per selected
    // harness — a model fan-out repeats one harness, so there are more entries
    // than there are selected ids.
    let lines: Vec<Value> = std::fs::read_to_string(report["history_file"].as_str().unwrap())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let runs: Vec<&Value> = lines.iter().filter(|l| l["type"] != "event").collect();
    assert_eq!(runs.len(), 2, "both models recorded: {lines:#?}");
    let events: Vec<&Value> = lines.iter().filter(|l| l["type"] == "event").collect();
    assert_eq!(events.len(), 2);
    assert!(events.iter().all(|e| e["harness"] == "claude-code"));
    let _ = std::fs::remove_dir_all(&history);
}

/// The fallback block reduced to a value two runs of the same chain can be
/// compared on.
fn fallback_selection(report: &Value) -> (Value, Vec<(String, String)>, usize) {
    let fell = report["fallback"]["fell_through"]
        .as_array()
        .expect("a fallback block")
        .iter()
        .map(|f| {
            (
                f["harness"].as_str().unwrap().to_string(),
                f["reason"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    (
        report["fallback"]["ran"].clone(),
        fell,
        report["results"].as_array().unwrap().len(),
    )
}

#[test]
fn streamed_and_buffered_fallback_select_the_same_candidate() {
    // The parity contract: `--stream` changes how a run is *delivered*, never
    // which candidate a fallback chain picks. Both paths take the verdict from
    // `fallback::startup_failure_reason` over the same normalized result, so each
    // scenario below is run twice — buffered and streamed — and must agree on the
    // harness that ran, the fallen-through candidates and their reasons, how many
    // candidates were attempted, and the exit code.
    let mock = mock_bin().display().to_string();
    let rejection =
        serde_json::to_string(include_str!("fixtures/claude-session-limit-api-error.json").trim())
            .unwrap();
    // A transcript that did real work: a tool call, then a result billing tokens.
    let worked = serde_json::to_string(&format!(
        "{}\n{}\n",
        r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"echo hi"}}]}}"#,
        r#"{"type":"result","subtype":"success","result":"partial","usage":{"input_tokens":812,"output_tokens":96}}"#,
    ))
    .unwrap();
    // Accounting-only records that differ in exactly ONE decisive field, so each
    // pins its own arm of the evidence: a prompt-cache read, a prompt-cache
    // write, a dollar cost with no token block at all — and the all-zero control
    // they are each measured against.
    let accounting = |usage: &str| {
        serde_json::to_string(&format!(
            r#"{{"type":"result","subtype":"success","result":"partial",{usage}}}"#
        ))
        .unwrap()
    };
    let zeros = r#""usage":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}"#;
    let unbilled = accounting(zeros);
    let cache_read = accounting(
        r#""usage":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":1240,"cache_creation_input_tokens":0}"#,
    );
    let cache_write = accounting(
        r#""usage":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":889}"#,
    );
    let cost_only = accounting(r#""total_cost_usd":0.0042"#);

    struct Case {
        tag: &'static str,
        /// Config env for the FIRST candidate (claude-code); the second (qwen) is
        /// always a clean mock run.
        first_env: String,
        /// `--bin` overrides and any extra run args the case needs.
        extra: Vec<String>,
        expected_ran: &'static str,
        expected_fell: Vec<(&'static str, &'static str)>,
        expected_exit: i32,
        /// The first candidate's `failure_kind`, asserted where the case turns on
        /// it: a work-evidence case only proves its point if the record really
        /// carries the classification that would otherwise fall through.
        expected_kind: Option<&'static str>,
    }

    let cases = vec![
        Case {
            // The real zero-work Claude 429 (issue #1211): rejected before doing
            // anything, so the chain moves on.
            tag: "zero-work-quota",
            first_env: format!(r#"{{ MOCK_EXIT = "1", MOCK_STDOUT = {rejection} }}"#),
            extra: vec![],
            expected_ran: "qwen",
            expected_fell: vec![("claude-code", "quota")],
            expected_exit: 0,
            expected_kind: Some("quota"),
        },
        Case {
            // The same `quota` classification as the case above, but this
            // candidate did the work first. Work evidence is consulted before
            // every fall-through reason, so the identical rejection that moves
            // the chain on above stops it here — the pair is what shows the
            // verdict turns on the work, not on the classification.
            tag: "worked-then-quota",
            first_env: format!(
                r#"{{ MOCK_EXIT = "1", MOCK_STDOUT = {worked}, MOCK_STDERR = "Error: insufficient_quota — your credit balance is too low" }}"#
            ),
            extra: vec![],
            expected_ran: "claude-code",
            expected_fell: vec![],
            expected_exit: 1,
            expected_kind: Some("quota"),
        },
        Case {
            // The same, on the other provisioning reason.
            tag: "worked-then-rejected",
            first_env: format!(
                r#"{{ MOCK_EXIT = "1", MOCK_STDOUT = {worked}, MOCK_STDERR = "Error: 401 unauthorized" }}"#
            ),
            extra: vec![],
            expected_ran: "claude-code",
            expected_fell: vec![],
            expected_exit: 1,
            expected_kind: Some("auth"),
        },
        Case {
            // Billed tokens with NO tool call: usage accounting alone is enough.
            tag: "billed-then-rejected",
            first_env: String::from(
                r#"{ MOCK_EXIT = "1", MOCK_STDOUT = "{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"partial\",\"usage\":{\"input_tokens\":812,\"output_tokens\":96}}", MOCK_STDERR = "Error: 401 unauthorized" }"#,
            ),
            extra: vec![],
            expected_ran: "claude-code",
            expected_fell: vec![],
            expected_exit: 1,
            expected_kind: Some("auth"),
        },
        Case {
            // The control for the four accounting cases below: the same 401, the
            // same terminal record shape, but every count zero. Absent spending is
            // not work, so this one — and only this one — falls through.
            tag: "zero-work-auth",
            first_env: format!(
                r#"{{ MOCK_EXIT = "1", MOCK_STDOUT = {unbilled}, MOCK_STDERR = "Error: 401 unauthorized" }}"#
            ),
            extra: vec![],
            expected_ran: "qwen",
            expected_fell: vec![("claude-code", "auth")],
            expected_exit: 0,
            expected_kind: Some("auth"),
        },
        Case {
            // Prompt-cache reads are spending like any other token: a run served
            // entirely from the cache still did the work.
            tag: "cache-read-then-rejected",
            first_env: format!(
                r#"{{ MOCK_EXIT = "1", MOCK_STDOUT = {cache_read}, MOCK_STDERR = "Error: 401 unauthorized" }}"#
            ),
            extra: vec![],
            expected_ran: "claude-code",
            expected_fell: vec![],
            expected_exit: 1,
            expected_kind: Some("auth"),
        },
        Case {
            // The other prompt-cache count: writing the prefix is billed work
            // even before a single ordinary token is charged.
            tag: "cache-write-then-rejected",
            first_env: format!(
                r#"{{ MOCK_EXIT = "1", MOCK_STDOUT = {cache_write}, MOCK_STDERR = "Error: 401 unauthorized" }}"#
            ),
            extra: vec![],
            expected_ran: "claude-code",
            expected_fell: vec![],
            expected_exit: 1,
            expected_kind: Some("auth"),
        },
        Case {
            // A harness that reports only a dollar cost and no token block at all.
            tag: "cost-only-then-rejected",
            first_env: format!(
                r#"{{ MOCK_EXIT = "1", MOCK_STDOUT = {cost_only}, MOCK_STDERR = "Error: 401 unauthorized" }}"#
            ),
            extra: vec![],
            expected_ran: "claude-code",
            expected_fell: vec![],
            expected_exit: 1,
            expected_kind: Some("auth"),
        },
        Case {
            // Never spawned at all.
            tag: "not-installed",
            first_env: String::from("{ }"),
            extra: vec!["--bin".into(), missing_bin("claude-code")],
            expected_ran: "qwen",
            expected_fell: vec![("claude-code", "not-installed")],
            expected_exit: 0,
            expected_kind: None,
        },
        Case {
            // Resolved, so the chain tried to launch it, but the OS refused —
            // the other "cannot run at all" arm, and the one where the candidate
            // has no signals at all to reason about.
            tag: "spawn-error",
            first_env: String::from("{ }"),
            extra: vec!["--bin".into(), unspawnable_bin("claude-code")],
            expected_ran: "qwen",
            expected_fell: vec![("claude-code", "spawn-error")],
            expected_exit: 0,
            expected_kind: None,
        },
        Case {
            // A plain non-zero task failure is a real run: never a fall-through.
            tag: "real-failure",
            first_env: String::from(
                r#"{ MOCK_EXIT = "1", MOCK_STDERR = "the task did not work" }"#,
            ),
            extra: vec![],
            expected_ran: "claude-code",
            expected_fell: vec![],
            expected_exit: 1,
            expected_kind: None,
        },
        Case {
            // A slow real run that timed out is a run, not a setup problem.
            tag: "timeout",
            first_env: String::from(r#"{ MOCK_SLEEP_MS = "4000" }"#),
            extra: vec!["--timeout".into(), "1".into()],
            expected_ran: "claude-code",
            expected_fell: vec![],
            expected_exit: 1,
            expected_kind: None,
        },
    ];

    for case in cases {
        let project = format!(
            r#"
            harnesses = ["claude-code", "qwen"]
            run_mode = "fallback"

            [harness.claude-code]
            bin = '{mock}'
            env = {}

            [harness.qwen]
            bin = '{mock}'
            "#,
            case.first_env
        );
        let fx = ConfigFixture::new(&format!("fallback-parity-{}", case.tag), &project, "");
        let mut args: Vec<String> = ["run", "--prompt", "hi", "--cwd", &fx.cwd()]
            .iter()
            .map(|s| s.to_string())
            .collect();
        args.extend(case.extra.iter().cloned());

        let mut buffered_args: Vec<&str> = args.iter().map(String::as_str).collect();
        buffered_args.push("--compact");
        let buffered = run_with_config(&buffered_args, &[], &fx.user_config());
        let buffered_report = json_stdout(&buffered);

        let mut streamed_args: Vec<&str> = args.iter().map(String::as_str).collect();
        streamed_args.push("--stream");
        let streamed = run_with_config(&streamed_args, &[], &fx.user_config());
        let envelopes = stream_envelopes(&streamed);
        let terminal = envelopes.last().expect("a terminal report line");
        assert_eq!(terminal["type"], "result", "{}", case.tag);
        let streamed_report = &terminal["report"];

        // Parity: same selection, same attempt count, same exit code.
        assert_eq!(
            fallback_selection(&buffered_report),
            fallback_selection(streamed_report),
            "{}: streamed and buffered fallback disagreed",
            case.tag
        );
        assert_eq!(
            buffered.status.code(),
            streamed.status.code(),
            "{}: exit codes disagreed",
            case.tag
        );

        // ...and that shared selection is the expected one.
        let (ran, fell, attempted) = fallback_selection(&buffered_report);
        assert_eq!(ran, case.expected_ran, "{}", case.tag);
        let expected_fell: Vec<(String, String)> = case
            .expected_fell
            .iter()
            .map(|(h, r)| (h.to_string(), r.to_string()))
            .collect();
        assert_eq!(fell, expected_fell, "{}", case.tag);
        assert_eq!(attempted, expected_fell.len() + 1, "{}", case.tag);
        assert_eq!(
            buffered.status.code(),
            Some(case.expected_exit),
            "{}: {}",
            case.tag,
            String::from_utf8_lossy(&buffered.stderr)
        );
        if let Some(kind) = case.expected_kind {
            for (path, report) in [
                ("buffered", &buffered_report),
                ("streamed", streamed_report),
            ] {
                assert_eq!(
                    report["results"][0]["failure_kind"], kind,
                    "{}: {path} misclassified the first candidate",
                    case.tag
                );
            }
        }

        // No fallen-through candidate published an event a consumer could act on:
        // every published line belongs to the harness that ran.
        let published = envelopes.iter().filter(|e| e["type"] == "event").count();
        let winner = streamed_report["results"]
            .as_array()
            .unwrap()
            .last()
            .expect("the harness that ran is the last result");
        assert_eq!(winner["harness"], case.expected_ran, "{}", case.tag);
        let winner_events = winner["events"].as_array().map_or(0, Vec::len);
        assert_eq!(
            published, winner_events,
            "{}: published events must all be the winner's",
            case.tag
        );
    }
}

/// Drive one `claude-code` → `qwen` fallback chain both ways over the same
/// config — buffered and streamed — and hand back the two reports plus the exit
/// code, after asserting the deliveries agreed on the selection and the exit.
/// The first candidate's `env` is the whole variable; the second is always a
/// clean mock run it must never reach.
fn fallback_reports_both_ways(tag: &str, first_env: &str) -> (Value, Value, Option<i32>) {
    let mock = mock_bin().display().to_string();
    let project = format!(
        r#"
        harnesses = ["claude-code", "qwen"]
        run_mode = "fallback"

        [harness.claude-code]
        bin = '{mock}'
        env = {first_env}

        [harness.qwen]
        bin = '{mock}'
        "#
    );
    let fx = ConfigFixture::new(tag, &project, "");
    let buffered = run_with_config(
        &["run", "--prompt", "hi", "--cwd", &fx.cwd(), "--compact"],
        &[],
        &fx.user_config(),
    );
    let buffered_report = json_stdout(&buffered);
    let streamed = run_with_config(
        &["run", "--prompt", "hi", "--cwd", &fx.cwd(), "--stream"],
        &[],
        &fx.user_config(),
    );
    let terminal = stream_envelopes(&streamed)
        .pop()
        .expect("a terminal report line");
    assert_eq!(terminal["type"], "result", "{tag}");
    let streamed_report = terminal["report"].clone();
    assert_eq!(
        fallback_selection(&buffered_report),
        fallback_selection(&streamed_report),
        "{tag}: streamed and buffered fallback disagreed"
    );
    assert_eq!(
        buffered.status.code(),
        streamed.status.code(),
        "{tag}: exit codes disagreed: {}",
        String::from_utf8_lossy(&streamed.stderr)
    );
    (buffered_report, streamed_report, buffered.status.code())
}

/// The tool-call witness on its own. `RunWork` reads two independent witnesses,
/// and every other end-to-end candidate that stops after a tool call also bills
/// tokens — so its spending explains the stop just as well. This record is the
/// `zero-work-auth` control of the parity table above with a tool call added:
/// the same `401`, the same all-zero accounting, so nothing it spent can be why
/// the chain stopped.
#[test]
fn fallback_stops_at_a_tool_call_that_billed_nothing() {
    let transcript = serde_json::to_string(&format!(
        "{}\n{}\n{}\n",
        r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"echo hi"}}]}}"#,
        r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"hi"}]}}"#,
        r#"{"type":"result","subtype":"success","result":"partial","usage":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}"#,
    ))
    .unwrap();
    let (buffered, streamed, exit) = fallback_reports_both_ways(
        "fallback-tool-call-without-billing",
        &format!(
            r#"{{ MOCK_EXIT = "1", MOCK_STDOUT = {transcript}, MOCK_STDERR = "Error: 401 unauthorized" }}"#
        ),
    );
    assert_eq!(exit, Some(1));
    for (path, report) in [("buffered", &buffered), ("streamed", &streamed)] {
        assert_eq!(report["fallback"]["ran"], "claude-code", "{path}");
        assert!(
            report["fallback"]["fell_through"]
                .as_array()
                .unwrap()
                .is_empty(),
            "{path}: a candidate that used a tool must never be fallen through"
        );
        let results = report["results"].as_array().unwrap();
        assert_eq!(results.len(), 1, "{path}: qwen must never be attempted");
        let first = &results[0];
        // The rejection that moves the chain on when the tool call is absent.
        assert_eq!(first["failure_kind"], "auth", "{path}");
        // ...and the accounting that cannot be what held it here.
        for count in [
            "input_tokens",
            "output_tokens",
            "cache_read_tokens",
            "cache_write_tokens",
        ] {
            assert_eq!(first["usage"][count], 0, "{path}: {count}");
        }
        assert!(first["usage"]["cost_usd"].is_null(), "{path}");
        assert_eq!(first["events"].as_array().unwrap().len(), 2, "{path}");
    }
}

/// The work-evidence short circuit on a **successful** record. A provider
/// rejection a harness reports while still exiting zero falls through when it
/// did no work — `fallback_falls_through_a_clean_exit_provider_quota_error` is
/// that shape exactly — so work has to reverse the verdict on the same
/// `Status::Ok`. Only a zero exit shows it: every other end-to-end work case
/// pairs the classification with a non-zero exit, where the status is already
/// the one the fall-through reasons are written against.
#[test]
fn fallback_stops_at_a_clean_exit_rejection_that_landed_after_work() {
    let transcript = serde_json::to_string(&format!(
        "{}\n{}\n",
        r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"echo hi"}}]}}"#,
        r#"{"type":"result","subtype":"success","is_error":true,"api_error_status":400,"result":"insufficient_quota: credit balance exhausted","usage":{"input_tokens":812,"output_tokens":96}}"#,
    ))
    .unwrap();
    let (buffered, streamed, exit) = fallback_reports_both_ways(
        "fallback-clean-exit-quota-after-work",
        &format!(r#"{{ MOCK_STDOUT = {transcript} }}"#),
    );
    // The declared rejection is still a failed run to report — what makes this
    // the `Status::Ok` arm is the harness's own zero exit, asserted below.
    assert_eq!(exit, Some(1));
    for (path, report) in [("buffered", &buffered), ("streamed", &streamed)] {
        assert_eq!(report["fallback"]["ran"], "claude-code", "{path}");
        assert!(
            report["fallback"]["fell_through"]
                .as_array()
                .unwrap()
                .is_empty(),
            "{path}: a candidate that did work must never be fallen through"
        );
        let results = report["results"].as_array().unwrap();
        assert_eq!(results.len(), 1, "{path}: qwen must never be attempted");
        let first = &results[0];
        // The exit that makes this the `Status::Ok` arm of the short circuit...
        assert_eq!(first["status"], "ok", "{path}");
        assert_eq!(first["exit_code"], 0, "{path}");
        // ...still carrying the classification that would otherwise fall through.
        assert_eq!(first["failure_kind"], "quota", "{path}");
        assert_eq!(first["usage"]["output_tokens"], 96, "{path}");
    }
}

#[test]
fn stream_under_fallback_publishes_nothing_when_no_candidate_can_run() {
    // Every candidate fails to start: the stream carries only the terminal report
    // (no events at all) and the run fails, exactly as the buffered path does.
    let output = run(
        &[
            "run",
            "--run-mode",
            "fallback",
            "--harness",
            "claude-code,codex",
            "--prompt",
            "hi",
            "--stream",
            "--bin",
            &missing_bin("claude-code"),
            "--bin",
            &missing_bin("codex"),
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(1));
    let envelopes = stream_envelopes(&output);
    assert_eq!(envelopes.len(), 1, "{envelopes:#?}");
    assert_eq!(envelopes[0]["type"], "result");
    let report = &envelopes[0]["report"];
    assert!(report["fallback"]["ran"].is_null());
    let fell = report["fallback"]["fell_through"].as_array().unwrap();
    assert_eq!(fell.len(), 2);
    assert!(fell.iter().all(|f| f["reason"] == "not-installed"));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no selected harness could be run"),
        "{stderr}"
    );
}

#[test]
fn fallback_refuses_incompatible_run_shapes() {
    // Fallback is single-outcome, so a batch and the low-level `--resume`
    // continuation are loud usage errors (exit 2), each naming why. (The
    // higher-level `--session` handle is instead *allowed* — it binds to the
    // anchor; see `session_in_fallback_mode_anchors_to_the_first_session_capable_harness`
    // — and so is `--stream`, which publishes only the candidate that runs; see
    // `stream_under_fallback_publishes_only_the_candidate_that_runs`.)
    let cases: &[(&[&str], &str)] = &[
        (&["--prompt", "a", "--prompt", "b"], "batch"),
        (&["--prompt", "a", "--resume", "sid"], "--resume"),
    ];
    for (extra, needle) in cases {
        let mut args = vec!["run", "--run-mode", "fallback", "--harness", "claude-code"];
        args.extend_from_slice(extra);
        let bin = bin_override("claude-code");
        args.extend_from_slice(&["--bin", &bin, "--compact"]);
        let output = run(&args, &[]);
        assert_eq!(
            output.status.code(),
            Some(2),
            "expected usage error for {extra:?}"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("fallback"), "{extra:?} -> {stderr}");
        assert!(stderr.contains(needle), "{extra:?} -> {stderr}");
    }
}

#[test]
fn fallback_validates_features_against_every_listed_harness() {
    // The command must be valid for the WHOLE candidate set: crush has no `plan`
    // mode, so `--mode plan` is a usage error even though claude-code (the first,
    // and the one that would run) supports it and crush is never reached.
    let output = run(
        &[
            "run",
            "--run-mode",
            "fallback",
            "--harness",
            "claude-code,crush",
            "--prompt",
            "hi",
            "--mode",
            "plan",
            "--print-command",
            "--compact",
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("does not support"), "{stderr}");
    assert!(stderr.contains("crush"), "{stderr}");
}

#[test]
fn fallback_with_all_uses_registry_order() {
    // Under --all (no explicit list) the priority chain is registry order — the
    // first-listed harness in `oneharness list`. Proven via --print-command.
    let output = run(
        &[
            "run",
            "--run-mode",
            "fallback",
            "--all",
            "--prompt",
            "hi",
            "--print-command",
            "--compact",
        ],
        &[],
    );
    assert!(output.status.success());
    let v = json_stdout(&output);
    let results = v["results"].as_array().unwrap();
    assert_eq!(results.len(), ALL_IDS.len());
    let ids: Vec<&str> = results
        .iter()
        .map(|r| r["harness"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ALL_IDS, "fallback --all follows registry order");
}

#[test]
fn fallback_applies_a_schema_to_the_harness_that_runs() {
    // Structured output composes with fallback: the not-installed harness is
    // skipped, and the one that runs is validated against the schema (exercising
    // the schema arm of the single-harness fallback driver).
    let schema = temp_file("fallback-schema", PERSON_SCHEMA);
    let output = run(
        &[
            "run",
            "--run-mode",
            "fallback",
            "--harness",
            "claude-code,codex",
            "--prompt",
            "describe ada",
            "--schema",
            &schema,
            "--bin",
            &missing_bin("claude-code"),
            "--bin",
            &bin_override("codex"),
            "--compact",
        ],
        &[(
            "MOCK_STDOUT",
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"{\"name\":\"Ada\",\"age\":36}"}}"#,
        )],
    );
    assert!(output.status.success(), "exit {:?}", output.status.code());
    let v = json_stdout(&output);
    assert_eq!(v["fallback"]["ran"], "codex");
    let results = v["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[1]["schema_valid"], true);
    assert_eq!(results[1]["structured"]["name"], "Ada");
}

/// One `control_response` line carrying the observed `get_usage` payload shape:
/// two live plan windows, a null one, and the flat `limits[]` array.
fn claude_usage_response() -> String {
    serde_json::json!({
        "type": "control_response",
        "response": {
            "subtype": "success",
            "request_id": "oneharness-usage-1",
            "response": {
                "session": {"total_cost_usd": 0, "model_usage": {}},
                "subscription_type": "max",
                "rate_limits_available": true,
                "rate_limits": {
                    "five_hour": {
                        "utilization": 42,
                        "resets_at": "2026-07-29T18:30:00.123456+00:00"
                    },
                    "seven_day": {
                        "utilization": 61,
                        "resets_at": "2026-08-02T09:00:00.000000-04:00"
                    },
                    "seven_day_opus": null,
                    "limits": [
                        {"kind": "session", "percent": 42, "is_active": false},
                        {"kind": "weekly_all", "percent": 61, "is_active": true}
                    ]
                }
            }
        }
    })
    .to_string()
}

/// codex's `account/rateLimits/read` reply, keyed by the JSON-RPC id the probe
/// sent.
fn codex_usage_response() -> String {
    serde_json::json!({
        "id": 2,
        "result": {
            "rateLimitsByLimitId": {
                "codex": {
                    "limitId": "codex",
                    "limitName": null,
                    "primary": {
                        "usedPercent": 31,
                        "windowDurationMins": 10080,
                        "resetsAt": 1_785_000_000
                    },
                    "secondary": null,
                    "planType": "pro"
                }
            }
        }
    })
    .to_string()
}

fn usage_identity(report: &Value, harness: &str) -> Value {
    report["identities"]
        .as_array()
        .expect("identities")
        .iter()
        .find(|identity| identity["harness"] == harness)
        .unwrap_or_else(|| panic!("no identity for {harness} in {report}"))
        .clone()
}

#[test]
fn usage_reports_headroom_for_a_probed_subscription_identity() {
    // The claude probe drives one `get_usage` control request over stream-json
    // and normalizes the reply — the whole feature, end to end, through the real
    // binary. The mock answers only after reading a request line, so a probe that
    // stopped sending one would never see this payload.
    let output = run(
        &[
            "usage",
            "--harness",
            "claude-code",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[
            ("MOCK_REPLY_AFTER_LINES", "1"),
            ("MOCK_STDOUT", &claude_usage_response()),
            ("CLAUDE_CONFIG_DIR", "/home/u/.claude"),
        ],
    );

    assert!(output.status.success(), "exit {:?}", output.status.code());
    let report = json_stdout(&output);
    assert_eq!(report["schema_version"], "0.1");
    let claude = usage_identity(&report, "claude-code");
    assert_eq!(claude["auth_mode"], "subscription");
    assert_eq!(claude["plan"], "max");
    assert_eq!(
        claude["selector"],
        serde_json::json!({
            "kind": "env_path",
            "env": "CLAUDE_CONFIG_DIR",
            "path": "/home/u/.claude"
        }),
        "the identity names the directory that selected it, never a credential"
    );

    let windows = claude["availability"]["windows"]
        .as_array()
        .expect("windows");
    assert_eq!(claude["availability"]["state"], "available");
    let five_hour = windows
        .iter()
        .find(|w| w["id"] == "five_hour")
        .expect("five_hour");
    assert_eq!(five_hour["usage"]["used_percent"], 42.0);
    assert_eq!(five_hour["window_seconds"], 18000);
    assert_eq!(five_hour["resets_at"], "2026-07-29T18:30:00Z");
    assert!(
        !windows.iter().any(|w| w["id"] == "seven_day_opus"),
        "a null window means not-applicable, never 0% used"
    );
    let seven_day = windows
        .iter()
        .find(|w| w["id"] == "seven_day")
        .expect("seven_day");
    assert_eq!(
        seven_day["resets_at"], "2026-08-02T13:00:00Z",
        "a -04:00 reset is normalized to absolute UTC"
    );
    assert_eq!(seven_day["is_binding"], true);
}

/// The bytes a probe actually wrote to a harness's stdin, recorded by the mock.
///
/// The zero-turn property is a claim about exactly these bytes, and no assertion
/// on the argv or on "the mock answered" can reach it: `claude -p --input-format
/// stream-json` takes its user message on **stdin**, so a probe could grow one
/// without changing a single flag and every argv assertion would stay green while
/// the probe started spending the quota it exists to measure.
struct ProbeStdin {
    path: PathBuf,
}

impl ProbeStdin {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "oneharness-usage-stdin-{label}-{}.jsonl",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        Self { path }
    }

    fn env(&self) -> String {
        self.path.display().to_string()
    }

    /// Everything the probe wrote, verbatim. Absent means the probe never spoke
    /// at all, which is a failure rather than an empty exchange.
    fn text(&self) -> String {
        std::fs::read_to_string(&self.path).expect("the mock recorded the probe's stdin")
    }

    /// The requests the probe wrote, in the order it wrote them.
    fn requests(&self) -> Vec<Value> {
        self.text()
            .lines()
            .map(|line| {
                serde_json::from_str(line)
                    .unwrap_or_else(|error| panic!("`{line}` is not a JSON request: {error}"))
            })
            .collect()
    }
}

impl Drop for ProbeStdin {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Every spelling a user message would arrive under in Claude's stream-json input
/// mode. A probe that carried one would complete a turn, which is the silent
/// regression this test exists to catch: quota burn with no failing check.
const CLAUDE_USER_TURN_MARKERS: &[&str] = &["\"user\"", "\"message\"", "\"content\"", "\"prompt\""];

#[test]
fn the_claude_usage_probe_sends_one_get_usage_request_and_no_user_message() {
    // The zero-turn property is the whole reason this command is usable as a
    // pre-flight check. It lives in the argv *and* in the bytes on stdin, so both
    // are recorded here and both are asserted: the argv proves no prompt flag and
    // an empty tool set, the stdin proves the one thing the argv cannot.
    let argv_file =
        std::env::temp_dir().join(format!("oneharness-usage-argv-{}", std::process::id()));
    let _ = std::fs::remove_file(&argv_file);
    let stdin = ProbeStdin::new("claude");

    let output = run(
        &[
            "usage",
            "--harness",
            "claude-code",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[
            ("MOCK_REPLY_AFTER_LINES", "1"),
            ("MOCK_STDOUT", &claude_usage_response()),
            ("MOCK_ARGV_FILE", argv_file.to_str().unwrap()),
            ("MOCK_REQUEST_FILE", &stdin.env()),
        ],
    );
    assert!(output.status.success());
    assert_eq!(
        usage_identity(&json_stdout(&output), "claude-code")["availability"]["state"],
        "available",
        "the exchange asserted below is the one that really answered"
    );

    let argv: Vec<String> = std::fs::read_to_string(&argv_file)
        .expect("the mock recorded its argv")
        .lines()
        .map(str::to_string)
        .collect();
    let _ = std::fs::remove_file(&argv_file);

    assert!(
        argv.contains(&"-p".to_string()) && argv.contains(&"--tools".to_string()),
        "the probe runs headless with an empty tool set: {argv:?}"
    );
    assert_eq!(
        argv.iter()
            .filter(|a| a.as_str() == "--input-format")
            .count(),
        1
    );
    assert!(
        argv.iter().all(|a| !a.contains("prompt")),
        "no prompt flag may appear — a user message would cost a turn: {argv:?}"
    );
    // Everything after the flags is a flag value; no bare positional prompt.
    assert!(
        !argv
            .iter()
            .any(|a| a.contains("hello") || a.contains("usage?")),
        "no prompt text: {argv:?}"
    );

    let requests = stdin.requests();
    assert_eq!(
        requests.len(),
        1,
        "exactly one line may reach this harness's stdin: {requests:?}"
    );
    let request = &requests[0];
    assert_eq!(
        request["type"], "control_request",
        "the only thing written is a control request: {request}"
    );
    assert_eq!(request["request"]["subtype"], "get_usage");
    assert_eq!(
        request["request"].as_object().map(serde_json::Map::len),
        Some(1),
        "the control request carries its subtype and nothing else: {request}"
    );
    assert_eq!(
        request["request_id"], "oneharness-usage-1",
        "the id is what makes the matching control response unmistakable: {request}"
    );
    assert_eq!(
        request.as_object().map(serde_json::Map::len),
        Some(3),
        "type, request_id, request — a fourth field is something new to justify: {request}"
    );

    let text = stdin.text();
    for marker in CLAUDE_USER_TURN_MARKERS {
        assert!(
            !text.contains(marker),
            "stdin carried {marker}: a user message is what makes `claude -p` take \
             a model turn, and this probe must take none.\nstdin was: {text}"
        );
    }
}

#[test]
fn the_codex_usage_probe_sends_exactly_the_zero_turn_handshake_in_order() {
    // codex's headroom read is three JSON-RPC lines, and the *bodies* are the
    // property: an exchange that skipped `initialized`, reordered the handshake,
    // or replaced the read with a request that starts a turn would still be
    // answered by any mock that replies after N lines. So the requests are read
    // back and compared whole.
    let stdin = ProbeStdin::new("codex");

    let output = run(
        &[
            "usage",
            "--harness",
            "codex",
            "--bin",
            &bin_override("codex"),
            "--compact",
        ],
        &[
            ("MOCK_REPLY_AFTER_LINES", "3"),
            ("MOCK_STDOUT", &codex_usage_response()),
            ("MOCK_REQUEST_FILE", &stdin.env()),
        ],
    );
    assert!(output.status.success(), "exit {:?}", output.status.code());
    assert_eq!(
        usage_identity(&json_stdout(&output), "codex")["availability"]["state"],
        "available",
        "the exchange asserted below is the one that really answered"
    );

    // Whole bodies, in order. Comparing the exchange field for field is what
    // makes "no turn is spent" a real assertion: a request that carried user
    // content, an extra field, a fourth line, or a changed calling convention all
    // fail here, rather than only the handful of fields someone thought to check.
    // Each detail below is load-bearing:
    //   - `initialized` is a notification, so it has no `id`; giving it one would
    //     make it a request the server is expected to answer.
    //   - `account/rateLimits/read` takes `params: null`, where its sibling
    //     `account/read` instead requires `{}`.
    //   - the read's `id` is what its reply is matched on, so nothing else can be
    //     mistaken for the answer.
    let expected = vec![
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "clientInfo": {
                    "name": "oneharness",
                    "title": null,
                    "version": core_version(),
                },
                "capabilities": null,
            },
        }),
        serde_json::json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}),
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "account/rateLimits/read",
            "params": null,
        }),
    ];

    assert_eq!(
        stdin.requests(),
        expected,
        "the whole zero-turn handshake, exactly, in this order:\n{}",
        stdin.text()
    );
}

/// The `oneharness-core` version the handshake announces as its client version,
/// read from that crate's manifest.
///
/// Not a literal, because release-plz bumps it every release and this test would
/// then fail on the release commit. Not `CARGO_PKG_VERSION` either: that is the
/// *binary* crate's version here, and the two crates version independently.
fn core_version() -> String {
    include_str!("../crates/oneharness-core/Cargo.toml")
        .lines()
        // Only the `[package]` version sits at column 0; a dependency's is
        // either inline in a table or indented under one.
        .find_map(|line| line.strip_prefix("version = "))
        .map(|value| value.trim().trim_matches('"').to_string())
        .expect("oneharness-core's manifest states a version")
}

/// Stage the mock harness on `PATH` under `name`, exactly the way an installer
/// puts a harness there: an extensionless executable on Unix, and on Windows a
/// `.cmd` shim beside the real program — the npm shape, which `CreateProcess`
/// cannot run from a bare name because it only ever appends `.exe`.
///
/// Returns the `PATH` value with `dir` first.
fn stage_harness_on_path(dir: &std::path::Path, name: &str) -> std::ffi::OsString {
    std::fs::create_dir_all(dir).expect("the staging directory");
    #[cfg(windows)]
    {
        let program = dir.join("mock-harness.exe");
        std::fs::copy(mock_bin(), &program).expect("stage the mock program");
        std::fs::write(
            dir.join(format!("{name}.cmd")),
            "@\"%~dp0mock-harness.exe\" %*\r\n",
        )
        .expect("stage the shim");
    }
    #[cfg(not(windows))]
    {
        std::fs::copy(mock_bin(), dir.join(name)).expect("stage the mock program");
    }
    let ambient = std::env::var_os("PATH").unwrap_or_default();
    std::env::join_paths(std::iter::once(dir.to_path_buf()).chain(std::env::split_paths(&ambient)))
        .expect("a PATH with the staged directory first")
}

#[test]
fn usage_probes_a_harness_installed_under_a_bare_name_on_path() {
    // A harness reaches the probe as the bare name the registry declares
    // (`codex`), not as a path — so the probe has to resolve that name the same
    // way `run` does. It did not: `run` resolves through `which` (PATHEXT-aware)
    // precisely because `CreateProcess` only appends `.exe` and never finds the
    // `codex.cmd` npm installs, and the probe spawned the bare name directly. On
    // Windows that reported `probe_failed: program not found` for a harness every
    // other verb drove fine — headroom that was readable the whole time, filed as
    // unknown. The staged install below is that exact shape.
    let dir = std::env::temp_dir().join(format!(
        "oneharness-usage-path-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let path = stage_harness_on_path(&dir, "oneharness-staged-codex");

    let output = run(
        &[
            "usage",
            "--harness",
            "codex",
            "--bin",
            "codex=oneharness-staged-codex",
            "--compact",
        ],
        &[
            ("PATH", path.to_str().expect("a UTF-8 PATH")),
            ("MOCK_REPLY_AFTER_LINES", "3"),
            ("MOCK_STDOUT", &codex_usage_response()),
        ],
    );
    let _ = std::fs::remove_dir_all(&dir);

    assert!(output.status.success(), "exit {:?}", output.status.code());
    let codex = usage_identity(&json_stdout(&output), "codex");
    assert_eq!(
        codex["availability"]["state"],
        "available",
        "the probe must spawn a PATH-installed harness, not report it missing: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        codex["availability"]["windows"][0]["usage"]["used_percent"], 31.0,
        "and read the headroom back off it"
    );
}

#[test]
fn usage_runs_each_probe_in_the_requested_working_directory() {
    // `--cwd` decides which directory a probe's child starts in, which is what
    // makes a project-relative credential store (a repo-local `CLAUDE_CONFIG_DIR`)
    // resolve to the right identity. A flag that were silently dropped would probe
    // the wrong account and report its headroom as this project's.
    //
    // The mock reads a *relative* path here, so it can only answer at all from
    // inside the requested directory — which is the observation.
    let dir = std::env::temp_dir().join(format!("oneharness-usage-cwd-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the probe's working directory");
    std::fs::write(
        dir.join("payload.jsonl"),
        format!("{}\n", claude_usage_response()),
    )
    .expect("a payload only reachable from that directory");

    let args = |cwd: Option<&str>| -> Vec<String> {
        let mut args = vec![
            "usage".to_string(),
            "--harness".to_string(),
            "claude-code".to_string(),
            "--bin".to_string(),
            bin_override("claude-code"),
            "--compact".to_string(),
        ];
        if let Some(cwd) = cwd {
            args.push("--cwd".to_string());
            args.push(cwd.to_string());
        }
        args
    };
    fn borrowed(args: &[String]) -> Vec<&str> {
        args.iter().map(String::as_str).collect()
    }
    let env = [("MOCK_CAT_FILE", "payload.jsonl")];

    let in_dir = args(Some(dir.to_str().unwrap()));
    let inside = run(&borrowed(&in_dir), &env);
    assert!(inside.status.success(), "exit {:?}", inside.status.code());
    let claude = usage_identity(&json_stdout(&inside), "claude-code");
    assert_eq!(
        claude["availability"]["state"], "available",
        "the probe answered from `payload.jsonl`, so it ran in the requested \
         directory: {claude}"
    );
    assert_eq!(claude["plan"], "max");

    // Without the flag the child starts wherever the command did, where that
    // relative path resolves to nothing — so the answer above was not a
    // coincidence of some absolute path.
    let elsewhere = args(None);
    let outside = run(&borrowed(&elsewhere), &env);
    assert!(outside.status.success());
    let unreached = usage_identity(&json_stdout(&outside), "claude-code");
    assert_eq!(
        unreached["availability"]["state"], "unknown",
        "the same relative payload must be unreachable from anywhere else: {unreached}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn usage_reports_codex_headroom_from_its_app_server_exchange() {
    // The codex probe writes three JSON-RPC lines (initialize, initialized, the
    // read) before an answer arrives; the mock only replies after the third, so
    // a probe that skipped the handshake would time out instead of passing.
    let output = run(
        &[
            "usage",
            "--harness",
            "codex",
            "--bin",
            &bin_override("codex"),
            "--compact",
        ],
        &[
            ("MOCK_REPLY_AFTER_LINES", "3"),
            ("MOCK_STDOUT", &codex_usage_response()),
            ("CODEX_HOME", "/home/u/.codex"),
        ],
    );

    assert!(output.status.success(), "exit {:?}", output.status.code());
    let codex = usage_identity(&json_stdout(&output), "codex");
    assert_eq!(codex["plan"], "pro");
    let window = &codex["availability"]["windows"][0];
    assert_eq!(window["id"], "codex/primary");
    assert_eq!(window["usage"]["used_percent"], 31.0);
    assert_eq!(
        window["window_seconds_source"], "reported",
        "codex states its window length rather than having it inferred"
    );
}

#[test]
fn usage_waits_for_an_answer_a_harness_only_sends_while_its_stdin_is_open() {
    // `codex app-server` answers `initialize` synchronously but reads rate limits
    // asynchronously, and it shuts down on stdin EOF — so a probe that wrote its
    // three requests and closed the pipe was reported as "exited without an
    // answer" on an account whose 45%-used weekly window was readable the whole
    // time. The mock reproduces exactly that race: it answers only after a delay,
    // and exits unanswered if EOF arrives first. The delay is what the fix has to
    // wait through; the EOF-shutdown is what makes the old close fail here.
    let output = run(
        &[
            "usage",
            "--harness",
            "codex",
            "--bin",
            &bin_override("codex"),
            "--compact",
        ],
        &[
            ("MOCK_REPLY_AFTER_LINES", "3"),
            ("MOCK_REPLY_DELAY_MS", "750"),
            ("MOCK_STDOUT", &codex_usage_response()),
        ],
    );

    assert!(output.status.success(), "exit {:?}", output.status.code());
    let codex = usage_identity(&json_stdout(&output), "codex");
    assert_eq!(
        codex["availability"]["state"], "available",
        "the headroom arrived late, which is not the same as never: {codex}"
    );
    assert_eq!(codex["plan"], "pro");
    assert_eq!(
        codex["availability"]["windows"][0]["usage"]["used_percent"],
        31.0
    );
}

#[test]
fn usage_gives_up_on_a_harness_that_never_answers_without_waiting_for_its_exit() {
    // Holding stdin open for a late answer must not become a probe that hangs on
    // a harness which has none: the deadline still ends the wait, the child's
    // tree is still torn down, and the reading is still honest data rather than a
    // fabricated 0%. The mock is asked for an answer ten times past the timeout.
    let started = std::time::Instant::now();
    let output = run(
        &[
            "usage",
            "--harness",
            "codex",
            "--bin",
            &bin_override("codex"),
            "--timeout",
            "1",
            "--compact",
        ],
        &[
            ("MOCK_REPLY_AFTER_LINES", "3"),
            ("MOCK_REPLY_DELAY_MS", "10000"),
            ("MOCK_STDOUT", &codex_usage_response()),
        ],
    );
    let elapsed = started.elapsed();

    assert!(
        output.status.success(),
        "a probe that never answers is data, not an exit code: {:?}",
        output.status.code()
    );
    let codex = usage_identity(&json_stdout(&output), "codex");
    assert_eq!(codex["availability"]["state"], "unknown");
    let message = codex["availability"]["reason"]["message"]
        .as_str()
        .expect("a message");
    assert!(message.contains("did not answer"), "{message}");
    assert!(
        elapsed < std::time::Duration::from_secs(9),
        "the probe returned on its own deadline rather than the harness's: {elapsed:?}"
    );
}

#[test]
fn mock_harness_refuses_a_reply_delay_it_could_never_wait_out() {
    // The scripted delay becomes an `Instant` deadline, so a value near u64::MAX
    // is both a wait no run outlasts and a sum that can leave the platform
    // clock's range, where `Instant + Duration` panics. Either way it is a typo,
    // and it reaches a shipped subcommand through the environment: the range
    // belongs where the value is read, in a message that names it.
    let output = run(
        &["mock-harness"],
        &[
            ("MOCK_REPLY_AFTER_LINES", "1"),
            ("MOCK_REPLY_DELAY_MS", "18446744073709551615"),
        ],
    );

    assert_eq!(
        output.status.code(),
        Some(2),
        "an out-of-range delay is a usage error, not a crash: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("MOCK_REPLY_DELAY_MS") && stderr.contains("from 0 to 600000"),
        "the refusal has to name the accepted range: {stderr}"
    );
    assert!(
        stderr.contains("18446744073709551615"),
        "the refusal has to name the value it rejected: {stderr}"
    );
}

#[test]
fn usage_contains_a_panicking_probe_to_its_own_identity() {
    // A report is the deliverable, so one misbehaving harness must cost exactly
    // its own reading — losing seven identities because the eighth crashed is
    // the defect the per-probe join fixed. Nothing a harness can send produces
    // a panicking probe (a bad payload, a timeout and a missing binary are all
    // ordinary data), so the test build injects a real one with
    // MOCK_PANIC_PROBE and drives the shipped verb end to end.
    let output = run(
        &[
            "usage",
            "--harness",
            "claude-code",
            "--harness",
            "codex",
            "--bin",
            &bin_override("claude-code"),
            "--bin",
            &bin_override("codex"),
            "--compact",
        ],
        &[
            ("MOCK_PANIC_PROBE", "claude-code"),
            ("MOCK_REPLY_AFTER_LINES", "3"),
            ("MOCK_STDOUT", &codex_usage_response()),
            ("CODEX_HOME", "/home/u/.codex"),
        ],
    );

    assert!(
        output.status.success(),
        "a crashed probe is data, not a failed command: exit {:?}",
        output.status.code()
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("panicked"),
        "the probe must genuinely panic, not report a crash it did not have:\n{stderr}"
    );

    let report = json_stdout(&output);
    let claude = usage_identity(&report, "claude-code");
    assert_eq!(claude["availability"]["state"], "unknown");
    assert_eq!(claude["availability"]["reason"]["kind"], "probe_failed");
    assert!(
        claude["availability"].get("windows").is_none(),
        "nothing was learned, so no percentage may appear: {claude}"
    );

    let codex = usage_identity(&report, "codex");
    assert_eq!(
        codex["availability"]["state"], "available",
        "the surviving identity keeps its reading: {codex}"
    );
    assert_eq!(codex["plan"], "pro");
    assert_eq!(
        codex["availability"]["windows"][0]["usage"]["used_percent"], 31.0,
        "the survivor's figure must be its own, not a placeholder: {codex}"
    );
}

#[test]
fn usage_reports_a_worker_thread_creation_failure() {
    let output = run(
        &["usage", "--harness", "codex", "--compact"],
        &[("MOCK_FAIL_PROBE_THREAD", "codex")],
    );

    assert!(
        output.status.success(),
        "worker resource failure is report data: exit {:?}",
        output.status.code()
    );
    let codex = usage_identity(&json_stdout(&output), "codex");
    assert_eq!(codex["availability"]["state"], "unknown");
    assert_eq!(codex["availability"]["reason"]["kind"], "probe_failed");
    assert!(
        codex["availability"]["reason"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("could not start probe worker")),
        "{codex}"
    );
}

#[test]
fn usage_reports_an_api_key_identity_as_unavailable_never_as_zero_used() {
    let api_key_response = serde_json::json!({
        "type": "control_response",
        "response": {
            "subtype": "success",
            "request_id": "oneharness-usage-1",
            "response": {
                "subscription_type": null,
                "rate_limits_available": false,
                "rate_limits": null
            }
        }
    })
    .to_string();

    let output = run(
        &[
            "usage",
            "--harness",
            "claude-code",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[
            ("MOCK_REPLY_AFTER_LINES", "1"),
            ("MOCK_STDOUT", &api_key_response),
        ],
    );

    assert!(output.status.success());
    let claude = usage_identity(&json_stdout(&output), "claude-code");
    assert_eq!(claude["auth_mode"], "api_key");
    assert_eq!(claude["availability"]["state"], "unavailable");
    assert_eq!(claude["availability"]["reason"], "api_key_auth");
    assert!(
        claude["availability"]["windows"].is_null(),
        "an unavailable identity carries no window a renderer could draw as a bar"
    );
}

#[test]
fn usage_degrades_to_unknown_when_the_claude_payload_changes_shape() {
    // Claude's usage surface is experimental and publishes no schema to diff, so
    // the guard is the only thing between a renamed field and a confident
    // "no headroom" for every user at once.
    let drifted = serde_json::json!({
        "type": "control_response",
        "response": {
            "subtype": "success",
            "request_id": "oneharness-usage-1",
            "response": {
                "subscription_type": "max",
                "plan_limits_available": true,
                "plan_limits": {"five_hour": {"pct": 42}}
            }
        }
    })
    .to_string();

    let output = run(
        &[
            "usage",
            "--harness",
            "claude-code",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[("MOCK_REPLY_AFTER_LINES", "1"), ("MOCK_STDOUT", &drifted)],
    );

    assert!(output.status.success());
    let claude = usage_identity(&json_stdout(&output), "claude-code");
    assert_eq!(claude["availability"]["state"], "unknown");
    assert_eq!(claude["availability"]["reason"]["kind"], "probe_failed");
    let message = claude["availability"]["reason"]["message"]
        .as_str()
        .expect("a message");
    assert!(
        message.contains("rate_limits_available"),
        "the message must name what moved: {message}"
    );
}

#[test]
fn usage_reports_a_malformed_payload_and_a_timeout_as_data_not_a_crash() {
    let garbled = run(
        &[
            "usage",
            "--harness",
            "claude-code",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[
            ("MOCK_REPLY_AFTER_LINES", "1"),
            ("MOCK_STDOUT", "not json at all"),
        ],
    );
    assert!(
        garbled.status.success(),
        "a bad payload is data, not an exit code"
    );
    let claude = usage_identity(&json_stdout(&garbled), "claude-code");
    assert_eq!(claude["availability"]["state"], "unknown");
    assert_eq!(claude["availability"]["reason"]["kind"], "probe_failed");

    // A harness that never answers: the probe must give up on its own deadline
    // rather than hang the command.
    let timed_out = run(
        &[
            "usage",
            "--harness",
            "codex",
            "--bin",
            &bin_override("codex"),
            "--timeout",
            "1",
            "--compact",
        ],
        &[("MOCK_SLEEP_MS", "30000")],
    );
    assert!(
        timed_out.status.success(),
        "a timeout is data, not an exit code"
    );
    let codex = usage_identity(&json_stdout(&timed_out), "codex");
    assert_eq!(codex["availability"]["state"], "unknown");
    let message = codex["availability"]["reason"]["message"]
        .as_str()
        .expect("a message");
    assert!(message.contains("did not answer"), "{message}");
}

#[test]
fn usage_reports_a_missing_binary_as_data_rather_than_failing() {
    let output = run(
        &[
            "usage",
            "--harness",
            "codex",
            "--bin",
            &missing_bin("codex"),
            "--compact",
        ],
        &[],
    );

    assert!(output.status.success(), "exit {:?}", output.status.code());
    let codex = usage_identity(&json_stdout(&output), "codex");
    assert_eq!(codex["availability"]["state"], "unknown");
    assert_eq!(codex["availability"]["reason"]["kind"], "binary_missing");
}

#[test]
fn usage_covers_every_harness_with_the_five_headroomless_ones_saying_so() {
    // The premise of oneharness is that one command works across every harness.
    // A `usage` that silently covered three of eight would undermine it, so all
    // eight appear — and each of the five that cannot report headroom says which
    // kind of cannot it is, rather than being omitted or rendered as 0%.
    let cursor_about = serde_json::json!({
        "cliVersion": "2026.07.23-e383d2b",
        "subscriptionTier": "Team"
    })
    .to_string();

    let output = run(
        &[
            "usage",
            "--all",
            "--bin",
            &missing_bin("claude-code"),
            "--bin",
            &missing_bin("codex"),
            "--bin",
            &bin_override("cursor"),
            "--compact",
        ],
        &[("MOCK_STDOUT", &cursor_about)],
    );

    assert!(output.status.success(), "exit {:?}", output.status.code());
    let report = json_stdout(&output);
    let harnesses: Vec<&str> = report["identities"]
        .as_array()
        .expect("identities")
        .iter()
        .map(|identity| identity["harness"].as_str().expect("an id"))
        .collect();
    assert_eq!(harnesses, ALL_IDS, "every harness is accounted for");

    for (id, reason) in [
        ("opencode", "no_plan_quota"),
        ("goose", "no_plan_quota"),
        ("qwen", "no_headroom_reader"),
        ("crush", "no_headroom_reader"),
        // Cursor is the fifth: it *does* answer, with a plan tier and an
        // affirmative "no non-interactive reader" — not a percentage.
        ("cursor", "no_headroom_reader"),
    ] {
        let identity = usage_identity(&report, id);
        assert_eq!(
            identity["availability"]["state"], "unavailable",
            "{id} affirmatively has no headroom to report"
        );
        assert_eq!(identity["availability"]["reason"], reason, "{id}");
        assert!(
            identity["availability"]["windows"].is_null(),
            "{id} must expose no window a renderer could draw as a bar"
        );
    }
    assert_eq!(usage_identity(&report, "cursor")["plan"], "Team");

    // No identity anywhere may carry a zero percentage it did not measure.
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        !text.contains("\"used_percent\":0"),
        "an absent figure must never be published as 0% used: {text}"
    );
}

#[test]
fn usage_without_any_selection_sweeps_every_harness_like_all() {
    // The shape a pre-flight check is actually typed as: bare `oneharness usage`,
    // no `--all` and no `--harness`. The default has to mean the whole sweep, or
    // a caller reads "I have headroom" off a subset they never chose. The
    // probing harnesses are pointed at missing binaries so this stays hermetic.
    let output = run(
        &[
            "usage",
            "--bin",
            &missing_bin("claude-code"),
            "--bin",
            &missing_bin("codex"),
            "--bin",
            &missing_bin("cursor"),
            "--compact",
        ],
        &[],
    );

    assert!(output.status.success(), "exit {:?}", output.status.code());
    let report = json_stdout(&output);
    let harnesses: Vec<&str> = report["identities"]
        .as_array()
        .expect("identities")
        .iter()
        .map(|identity| identity["harness"].as_str().expect("an id"))
        .collect();
    assert_eq!(
        harnesses, ALL_IDS,
        "the bare default covers every harness, exactly as `--all` does"
    );
}

#[test]
fn usage_selection_narrows_with_exclude() {
    // `usage` defaults to every harness, so `--exclude` is the only way to drop
    // one from a sweep — a distinct path from naming harnesses explicitly.
    let output = run(
        &[
            "usage",
            "--exclude",
            "claude-code,codex,cursor",
            "--compact",
        ],
        &[],
    );

    assert!(output.status.success(), "exit {:?}", output.status.code());
    let harnesses: Vec<String> = json_stdout(&output)["identities"]
        .as_array()
        .expect("identities")
        .iter()
        .map(|identity| identity["harness"].as_str().expect("an id").to_string())
        .collect();
    assert_eq!(
        harnesses,
        vec!["opencode", "goose", "qwen", "crush", "copilot"],
        "the excluded harnesses are dropped and the rest keep registry order"
    );
}

#[test]
fn usage_refuses_an_exclude_it_could_not_apply() {
    // `--exclude` drops harnesses from the all-harness sweep. Against an explicit
    // `--harness` there is no sweep to narrow, so honouring the flag's name would
    // mean either silently ignoring it — the caller believing an identity was
    // dropped when it was probed — or a second, subtractive spelling of a
    // selection the caller already wrote out. It is a usage error instead.
    let output = run(
        &[
            "usage",
            "--harness",
            "goose,crush",
            "--exclude",
            "crush",
            "--compact",
        ],
        &[],
    );

    assert_eq!(
        output.status.code(),
        Some(2),
        "stdout {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--exclude") && stderr.contains("--harness"),
        "the refusal must name both flags: {stderr}"
    );
    assert!(
        output.stdout.is_empty(),
        "no report may be emitted for a selection that was refused"
    );
}

#[test]
fn usage_refuses_all_together_with_an_explicit_harness() {
    // `usage` already sweeps every harness when none is named, so `--all` beside
    // `--harness` states two selections at once. Honouring either silently would
    // report headroom for a fleet the caller did not ask about, or drop identities
    // they did — and the report is an attribution contract, so it is refused.
    let output = run(&["usage", "--all", "--harness", "goose", "--compact"], &[]);

    assert_eq!(
        output.status.code(),
        Some(2),
        "stdout {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--all") && stderr.contains("--harness"),
        "the refusal must name both flags: {stderr}"
    );
    assert!(
        output.stdout.is_empty(),
        "no report may be emitted for a selection that was refused"
    );
}

#[test]
fn usage_refuses_a_config_path_together_with_no_config() {
    // One flag names the only file to read and the other says to read none, so a
    // winner picked either way leaves the caller unable to tell which layering
    // produced the identities they are looking at.
    let fx = ConfigFixture::new("usage-config-conflict", "", "");
    let config = fx.user_config();
    let output = run(
        &[
            "usage",
            "--config",
            &config.display().to_string(),
            "--no-config",
            "--compact",
        ],
        &[],
    );

    assert_eq!(
        output.status.code(),
        Some(2),
        "stdout {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--config") && stderr.contains("--no-config"),
        "the refusal must name both flags: {stderr}"
    );
    assert!(
        output.stdout.is_empty(),
        "no report may be emitted for a configuration that was refused"
    );
}

#[test]
fn usage_keeps_each_reading_with_the_identity_that_produced_it() {
    // Every plan is built by pairing the selected ids with the resolved specs
    // positionally, so anything that could shorten or reorder one list without
    // the other would silently file one harness's headroom under another's
    // name. A mixed selection — two headroomless harnesses around a variant that
    // reports — is where that shift would show: the report must read the way it
    // was asked for.
    let fixture = ConfigFixture::new(
        "usage-attribution",
        &format!(
            "[harness.claude-code]\nbin = {bin:?}\n\
             [harness.claude-code.variant.work]\nenv = {{ CLAUDE_CONFIG_DIR = \"/home/u/.claude-work\" }}\n",
            bin = mock_bin().display().to_string()
        ),
        "",
    );

    let output = run_with_config(
        &[
            "usage",
            "--harness",
            "goose,claude-code:work,crush",
            "--cwd",
            &fixture.cwd(),
            "--compact",
        ],
        &[
            ("MOCK_REPLY_AFTER_LINES", "1"),
            ("MOCK_STDOUT", &claude_usage_response()),
        ],
        &fixture.user_config(),
    );

    assert!(output.status.success(), "exit {:?}", output.status.code());
    let report = json_stdout(&output);
    let identities = report["identities"].as_array().expect("identities");
    let named: Vec<(&str, Option<&str>)> = identities
        .iter()
        .map(|identity| {
            (
                identity["harness"].as_str().expect("an id"),
                identity["variant"].as_str(),
            )
        })
        .collect();
    assert_eq!(
        named,
        vec![
            ("goose", None),
            ("claude-code", Some("work")),
            ("crush", None)
        ],
        "an explicit selection is reported in the order it was named: {report}"
    );

    // The variant's own reading, not a neighbour's: goose and crush have no
    // headroom to report at all, so a shifted pairing would show up here.
    let claude = usage_identity(&report, "claude-code");
    assert_eq!(claude["selector"]["path"], "/home/u/.claude-work");
    assert_eq!(claude["availability"]["state"], "available");
    for headroomless in ["goose", "crush"] {
        assert_eq!(
            usage_identity(&report, headroomless)["availability"]["state"],
            "unavailable",
            "{headroomless} has no reading to have been given one"
        );
    }
}

#[test]
fn usage_finds_its_answer_among_the_lines_a_real_harness_interleaves() {
    // Neither probe's answer arrives alone: Claude writes an init line first and
    // may carry another control response, and codex replies to `initialize`
    // before the rate-limit read. Both are matched by their own request id, so a
    // decoy carrying a *different* id must be walked past rather than parsed —
    // answering from the wrong message is how a probe reports someone else's
    // numbers, or none.
    let claude_stream = [
        r#"{"type":"system","subtype":"init","apiKeySource":"none"}"#.to_string(),
        serde_json::json!({
            "type": "control_response",
            "response": {
                "subtype": "success",
                "request_id": "someone-elses-request",
                "response": {"subscription_type": "pro", "rate_limits_available": false}
            }
        })
        .to_string(),
        claude_usage_response(),
    ]
    .join("\n");

    let output = run(
        &[
            "usage",
            "--harness",
            "claude-code",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[
            ("MOCK_REPLY_AFTER_LINES", "1"),
            ("MOCK_STDOUT", &claude_stream),
            ("MOCK_PRESERVE_STDOUT", "1"),
        ],
    );

    assert!(output.status.success(), "exit {:?}", output.status.code());
    let claude = usage_identity(&json_stdout(&output), "claude-code");
    assert_eq!(
        claude["plan"], "max",
        "the decoy response reports `pro` and no rate limits: {claude}"
    );
    assert_eq!(claude["availability"]["state"], "available");
    assert_eq!(
        claude["availability"]["windows"][0]["usage"]["used_percent"],
        42.0
    );

    let codex_stream = [
        r#"{"jsonrpc":"2.0","id":1,"result":{"userAgent":"codex-cli/0.145.0"}}"#.to_string(),
        r#"{"jsonrpc":"2.0","method":"sessionConfigured","params":{}}"#.to_string(),
        codex_usage_response(),
    ]
    .join("\n");

    let output = run(
        &[
            "usage",
            "--harness",
            "codex",
            "--bin",
            &bin_override("codex"),
            "--compact",
        ],
        &[
            ("MOCK_REPLY_AFTER_LINES", "3"),
            ("MOCK_STDOUT", &codex_stream),
            ("MOCK_PRESERVE_STDOUT", "1"),
        ],
    );

    assert!(output.status.success(), "exit {:?}", output.status.code());
    let codex = usage_identity(&json_stdout(&output), "codex");
    assert_eq!(
        codex["plan"], "pro",
        "the `initialize` reply carries no rate limits to answer from: {codex}"
    );
    assert_eq!(
        codex["availability"]["windows"][0]["usage"]["used_percent"],
        31.0
    );
}

#[test]
fn usage_attributes_two_identities_of_one_harness_separately() {
    // Two subscriptions of the same harness, selected by the same variant
    // machinery `run` uses. Each entry must carry its own credential directory
    // and its own reading, or per-identity attribution is nominal only.
    let fixture = ConfigFixture::new(
        "usage-variants",
        &format!(
            "[harness.claude-code]\nbin = {bin:?}\n\
             [harness.claude-code.variant.work]\nenv = {{ CLAUDE_CONFIG_DIR = \"/home/u/.claude-work\" }}\n\
             [harness.claude-code.variant.personal]\nenv = {{ CLAUDE_CONFIG_DIR = \"/home/u/.claude-personal\" }}\n",
            bin = mock_bin().display().to_string()
        ),
        "",
    );

    let output = run_with_config(
        &[
            "usage",
            "--harness",
            "claude-code:work,claude-code:personal",
            "--cwd",
            &fixture.cwd(),
            "--compact",
        ],
        &[
            ("MOCK_REPLY_AFTER_LINES", "1"),
            ("MOCK_STDOUT", &claude_usage_response()),
        ],
        &fixture.user_config(),
    );

    assert!(output.status.success(), "exit {:?}", output.status.code());
    let identities = json_stdout(&output)["identities"].clone();
    let identities = identities.as_array().expect("identities");
    assert_eq!(identities.len(), 2);
    assert_eq!(identities[0]["variant"], "work");
    assert_eq!(identities[1]["variant"], "personal");
    assert_eq!(
        identities[0]["selector"]["path"], "/home/u/.claude-work",
        "each identity names the credential directory that selected it"
    );
    assert_eq!(
        identities[1]["selector"]["path"],
        "/home/u/.claude-personal"
    );
    assert_ne!(
        identities[0]["selector"], identities[1]["selector"],
        "two subscriptions must not collapse into one entry"
    );
}

#[test]
fn usage_text_view_is_human_readable_and_prints_no_invented_percentage() {
    let output = run(
        &[
            "usage",
            "--harness",
            "claude-code,goose",
            "--bin",
            &bin_override("claude-code"),
            "--format",
            "text",
        ],
        &[
            ("MOCK_REPLY_AFTER_LINES", "1"),
            ("MOCK_STDOUT", &claude_usage_response()),
        ],
    );

    assert!(output.status.success(), "exit {:?}", output.status.code());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("claude-code"), "{text}");
    assert!(text.contains("plan max"), "{text}");
    assert!(text.contains("five_hour: 42% used"), "{text}");
    assert!(text.contains("← binding"), "{text}");
    assert!(
        text.contains("goose") && text.contains("no first-party plan quota"),
        "a harness with no quota says so in prose too: {text}"
    );
    assert!(
        !text.contains("0% used"),
        "goose has no percentage to print: {text}"
    );
    assert!(
        serde_json::from_slice::<Value>(&output.stdout).is_err(),
        "the text view is prose, not the JSON contract"
    );
}

#[test]
fn usage_text_view_neutralizes_terminal_escapes_a_harness_wrote_to_its_stderr() {
    // A failed probe quotes the harness's own diagnostic, and the text view is
    // the surface someone reads to decide whether to start work. So a harness
    // that writes ANSI escapes, a carriage return, or a bell to stderr must not
    // be able to clear the screen, recolour, or overwrite the report it lands in
    // — while still saying what went wrong.
    let output = run(
        &[
            "usage",
            "--harness",
            "claude-code",
            "--bin",
            &bin_override("claude-code"),
            "--format",
            "text",
        ],
        &[
            ("MOCK_REPLY_AFTER_LINES", "1"),
            ("MOCK_STDOUT", "not a control response"),
            (
                "MOCK_STDERR",
                "\u{1b}[2J\u{1b}[1;31mclaude-code: credit balance too low\u{7}\r0% used\u{1b}[0m",
            ),
        ],
    );

    assert!(output.status.success(), "exit {:?}", output.status.code());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("credit balance too low"),
        "the reader must still learn what the harness said: {text:?}"
    );
    let surviving: Vec<char> = text
        .chars()
        .filter(|c| c.is_control() && *c != '\n')
        .collect();
    assert!(
        surviving.is_empty(),
        "no control byte from the harness may reach the rendered report: \
         {surviving:?} in {text:?}"
    );
}

#[test]
fn usage_text_view_neutralizes_terminal_escapes_a_harness_reported_in_its_payload() {
    // The sibling case: codex reports its failure inside a JSON-RPC `error`
    // rather than on stderr, and a JSON string can carry an escaped ESC that
    // decodes to a real one. That message reaches the same text view, so the
    // same rule holds — bounded and flattened where it is first read out of the
    // payload, not at the render site.
    let output = run(
        &[
            "usage",
            "--harness",
            "codex",
            "--bin",
            &bin_override("codex"),
            "--format",
            "text",
        ],
        &[
            ("MOCK_REPLY_AFTER_LINES", "3"),
            (
                "MOCK_STDOUT",
                "{\"id\":2,\"error\":{\"code\":-32603,\
                 \"message\":\"\\u001b[2Jcodex: rate limit backend unreachable\\u0007\\r100% free\"}}",
            ),
            ("CODEX_HOME", "/home/u/.codex"),
        ],
    );

    assert!(output.status.success(), "exit {:?}", output.status.code());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("rate limit backend unreachable"),
        "the reader must still learn what codex reported: {text:?}"
    );
    let surviving: Vec<char> = text
        .chars()
        .filter(|c| c.is_control() && *c != '\n')
        .collect();
    assert!(
        surviving.is_empty(),
        "no control byte from the payload may reach the rendered report: \
         {surviving:?} in {text:?}"
    );
}

#[test]
fn usage_text_view_neutralizes_terminal_escapes_in_every_display_string_it_prints() {
    // A *successful* read prints external strings too — the plan name, a window
    // id, a scoped model name, and the identity path read from the environment.
    // Each reaches the reader by the same route a failure diagnostic does, so
    // each is neutralized where it is first read rather than at the renderer.
    let payload = serde_json::json!({
        "type": "control_response",
        "response": {
            "subtype": "success",
            "request_id": "oneharness-usage-1",
            "response": {
                "session": {"total_cost_usd": 0, "model_usage": {}},
                "subscription_type": "max\u{1b}[31m",
                "rate_limits_available": true,
                "rate_limits": {
                    "five_hour\u{7}": {"utilization": 42},
                    "limits": [
                        {
                            "kind": "weekly_scoped",
                            "percent": 10,
                            "scope": {"model": {"display_name": "Opus\r4.8"}}
                        }
                    ]
                }
            }
        }
    })
    .to_string();

    let output = run(
        &[
            "usage",
            "--harness",
            "claude-code",
            "--bin",
            &bin_override("claude-code"),
            "--format",
            "text",
        ],
        &[
            ("MOCK_REPLY_AFTER_LINES", "1"),
            ("MOCK_STDOUT", &payload),
            ("CLAUDE_CONFIG_DIR", "/home/u/\u{1b}[2J.claude"),
        ],
    );

    assert!(output.status.success(), "exit {:?}", output.status.code());
    let text = String::from_utf8_lossy(&output.stdout);
    let surviving: Vec<char> = text
        .chars()
        .filter(|c| c.is_control() && *c != '\n')
        .collect();
    assert!(
        surviving.is_empty(),
        "no control byte from a payload or the environment may reach the \
         rendered report: {surviving:?} in {text:?}"
    );
    for readable in ["max", "five_hour", "Opus 4.8", ".claude"] {
        assert!(
            text.contains(readable),
            "{readable:?} must survive sanitization: {text:?}"
        );
    }
}

#[test]
fn usage_text_view_neutralizes_terminal_escapes_in_a_missing_binary_name() {
    // The one display string that comes from the *caller* rather than a payload:
    // an absent `--bin` is echoed back in the "not installed" line, so a name
    // out of a config file carries the same hazard a harness's own output does.
    let output = run(
        &[
            "usage",
            "--harness",
            "claude-code",
            "--bin",
            "claude-code=/nonexistent/cl\u{1b}[2Jaude\u{7}",
            "--format",
            "text",
        ],
        &[],
    );

    assert!(output.status.success(), "exit {:?}", output.status.code());
    let text = String::from_utf8_lossy(&output.stdout);
    let surviving: Vec<char> = text
        .chars()
        .filter(|c| c.is_control() && *c != '\n')
        .collect();
    assert!(
        surviving.is_empty(),
        "no control byte from a binary override may reach the rendered report: \
         {surviving:?} in {text:?}"
    );
    assert!(
        text.contains("is not installed"),
        "the readable reason must survive: {text:?}"
    );
}

/// The README documents the per-probe default in prose, which is where a reader
/// learns it — clap renders the same number from
/// [`oneharness::cli::USAGE_DEFAULT_TIMEOUT_SECS`], so tie the two together
/// rather than letting a changed default leave the docs confidently wrong.
#[test]
fn documented_usage_timeout_default_tracks_the_flag_constant() {
    let documented = format!(
        "(per probe, default {})",
        oneharness::cli::USAGE_DEFAULT_TIMEOUT_SECS
    );
    assert!(
        include_str!("../README.md").contains(&documented),
        "README.md must state the per-probe timeout as `{documented}`"
    );

    let help = run(&["usage", "--help"], &[]);
    let text = String::from_utf8_lossy(&help.stdout);
    let rendered = format!("[default: {}]", oneharness::cli::USAGE_DEFAULT_TIMEOUT_SECS);
    assert!(
        text.contains(&rendered),
        "`--help` must render the same default: {text}"
    );
}

#[test]
fn usage_rejects_an_unknown_harness_and_an_undeclared_variant() {
    let unknown = run(&["usage", "--harness", "nope", "--compact"], &[]);
    assert_eq!(unknown.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("unknown harness"));

    let fixture = ConfigFixture::new("usage-bad-variant", "", "");
    let undeclared = run_with_config(
        &[
            "usage",
            "--harness",
            "claude-code:ghost",
            "--cwd",
            &fixture.cwd(),
        ],
        &[],
        &fixture.user_config(),
    );
    assert_eq!(
        undeclared.status.code(),
        Some(2),
        "an identity that was never declared is a usage error, not a silent fallback"
    );
    assert!(String::from_utf8_lossy(&undeclared.stderr).contains("unknown harness variant"));
}

/// A one-shot local HTTP server for the Copilot probe: serves `status` and
/// `body` to the first request, then stops. Returns its `http://127.0.0.1:port`
/// base and the join handle, so a test can point the probe at it with no
/// network and no credential.
fn one_shot_http_server(
    status: u16,
    body: &'static str,
) -> (String, std::thread::JoinHandle<String>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a local port");
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("one request");
        // Read just the request head; the probe sends no body.
        let mut request = Vec::new();
        let mut byte = [0u8; 1];
        while !request.ends_with(b"\r\n\r\n") {
            match std::io::Read::read(&mut stream, &mut byte) {
                Ok(0) | Err(_) => break,
                Ok(_) => request.push(byte[0]),
            }
        }
        let response = format!(
            "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = std::io::Write::write_all(&mut stream, response.as_bytes());
        let _ = std::io::Write::flush(&mut stream);
        String::from_utf8_lossy(&request).into_owned()
    });
    (base, handle)
}

/// Run `usage` with every GitHub token variable cleared, so a developer's real
/// token can never reach a Copilot assertion.
fn run_copilot_usage(args: &[&str], envs: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(oneharness_bin());
    cmd.env("ONEHARNESS_NO_CONFIG", "1");
    for var in ["COPILOT_GITHUB_TOKEN", "GH_TOKEN", "GITHUB_TOKEN"] {
        cmd.env_remove(var);
    }
    cmd.args(args);
    for (key, value) in envs {
        cmd.env(key, value);
    }
    cmd.output().expect("failed to run oneharness")
}

fn curl_available() -> bool {
    which_curl().is_some()
}

fn which_curl() -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join(format!("curl{}", std::env::consts::EXE_SUFFIX)))
            .find(|candidate| candidate.is_file())
    })
}

const COPILOT_BODY: &str = r#"{"copilot_plan":"individual","token_based_billing":true,
 "quota_reset_date_utc":"2026-08-01T00:00:00.000Z",
 "quota_snapshots":{
   "chat":{"unlimited":true,"percent_remaining":100.0,"has_quota":true,"entitlement":0,
           "remaining":0,"credits_used":0,"overage_permitted":false},
   "premium_interactions":{"unlimited":false,"percent_remaining":0.0,"has_quota":false,
           "entitlement":1500,"credits_used":13518,"remaining":-12019,
           "overage_permitted":false}}}"#;

#[test]
fn usage_reads_copilot_headroom_out_of_band_from_a_bearer_token() {
    if !curl_available() {
        eprintln!("skipping: curl is not installed (the Copilot probe's HTTP client)");
        return;
    }
    let (base, server) = one_shot_http_server(200, COPILOT_BODY);

    let output = run_copilot_usage(
        &[
            "usage",
            "--harness",
            "copilot",
            "--bin",
            // The probe is out of band: it must answer with no Copilot CLI at all.
            &missing_bin("copilot"),
            "--compact",
        ],
        &[
            ("ONEHARNESS_COPILOT_API_BASE", &base),
            ("GH_TOKEN", "ghs_hermetic_token"),
        ],
    );

    assert!(output.status.success(), "exit {:?}", output.status.code());
    let copilot = usage_identity(&json_stdout(&output), "copilot");
    assert_eq!(copilot["plan"], "individual");
    assert_eq!(
        copilot["selector"],
        serde_json::json!({"kind": "env_secret", "env": "GH_TOKEN"}),
        "the identity names the variable, never the token"
    );
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("ghs_hermetic_token"),
        "a credential must never reach the report"
    );

    let windows = copilot["availability"]["windows"]
        .as_array()
        .expect("windows");
    let chat = windows.iter().find(|w| w["id"] == "chat").expect("chat");
    assert_eq!(
        chat["usage"]["kind"], "unlimited",
        "an unlimited quota reports no counters to draw as a full bar"
    );
    let premium = windows
        .iter()
        .find(|w| w["id"] == "premium_interactions")
        .expect("premium_interactions");
    assert_eq!(
        premium["usage"]["used_percent"], 100.0,
        "percent_remaining 0.0 is 100% used, not 0%"
    );
    assert_eq!(premium["usage"]["counters"]["remaining"], -12019);

    let request = server.join().expect("the server thread");
    assert!(
        request.starts_with("GET /copilot_internal/user "),
        "the probe issues one authenticated GET: {request}"
    );
    assert!(
        request.contains("Authorization: Bearer ghs_hermetic_token"),
        "the token rides the header, not the URL: {request}"
    );
}

#[test]
fn usage_reports_a_rejected_copilot_token_as_not_logged_in() {
    if !curl_available() {
        eprintln!("skipping: curl is not installed (the Copilot probe's HTTP client)");
        return;
    }
    let (base, server) = one_shot_http_server(401, r#"{"message":"Bad credentials"}"#);

    let output = run_copilot_usage(
        &["usage", "--harness", "copilot", "--compact"],
        &[
            ("ONEHARNESS_COPILOT_API_BASE", &base),
            ("GH_TOKEN", "ghs_expired"),
        ],
    );

    assert!(
        output.status.success(),
        "an unauthenticated harness is data"
    );
    let copilot = usage_identity(&json_stdout(&output), "copilot");
    assert_eq!(copilot["availability"]["state"], "unavailable");
    assert_eq!(copilot["availability"]["reason"], "not_logged_in");
    let _ = server.join();
}

/// The Copilot probe borrows `curl` rather than carrying a TLS stack, so a
/// machine without it has a token and no way to use it. That must read as
/// "nothing was learned", with the missing program named — not as an absence of
/// headroom, and not as a crash that costs the other seven identities their
/// readings.
///
/// Unix-only: on Windows `CreateProcess` searches the system directory, where
/// `curl.exe` ships, so emptying `PATH` cannot hide it there.
#[cfg(unix)]
#[test]
fn usage_reports_an_absent_curl_as_a_probe_failure_naming_it() {
    let empty = std::env::temp_dir().join(format!("oneharness-no-curl-{}", std::process::id()));
    std::fs::create_dir_all(&empty).expect("an empty PATH directory");

    let output = run_copilot_usage(
        &["usage", "--harness", "copilot", "--compact"],
        &[
            ("PATH", empty.to_str().unwrap()),
            ("GH_TOKEN", "ghs_canary"),
            // Unreachable on purpose: if `curl` were somehow resolved anyway,
            // this test must still not reach the real GitHub API.
            ("ONEHARNESS_COPILOT_API_BASE", "http://127.0.0.1:1"),
        ],
    );
    let _ = std::fs::remove_dir_all(&empty);

    assert!(
        output.status.success(),
        "a missing HTTP client is data, not an exit code: {:?}",
        output.status.code()
    );
    let copilot = usage_identity(&json_stdout(&output), "copilot");
    assert_eq!(copilot["availability"]["state"], "unknown");
    assert_eq!(copilot["availability"]["reason"]["kind"], "probe_failed");
    let message = copilot["availability"]["reason"]["message"]
        .as_str()
        .expect("a message");
    assert!(
        message.contains("curl"),
        "the message must name the program that is missing: {message}"
    );
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("ghs_canary"),
        "a failed probe must not spill the token it would have used"
    );
}

#[test]
fn usage_says_which_variable_to_set_when_no_copilot_token_exists() {
    // With no token there is nothing to authenticate with — and Copilot's own
    // stored login lives in the OS keyring, which oneharness cannot read. That
    // is an unknown with an actionable message, not a claim about headroom.
    let output = run_copilot_usage(&["usage", "--harness", "copilot", "--compact"], &[]);

    assert!(output.status.success(), "exit {:?}", output.status.code());
    let copilot = usage_identity(&json_stdout(&output), "copilot");
    assert_eq!(copilot["availability"]["state"], "unknown");
    assert_eq!(copilot["selector"]["kind"], "ambient");
    let message = copilot["availability"]["reason"]["message"]
        .as_str()
        .expect("a message");
    for var in ["COPILOT_GITHUB_TOKEN", "GH_TOKEN", "GITHUB_TOKEN"] {
        assert!(message.contains(var), "{message}");
    }
}

#[test]
fn usage_reports_cursors_plan_tier_and_no_headroom_reader() {
    let about = serde_json::json!({
        "cliVersion": "2026.07.23-e383d2b",
        "model": "Auto",
        "subscriptionTier": "Team",
        "userEmail": "someone@example.com"
    })
    .to_string();

    let output = run(
        &[
            "usage",
            "--harness",
            "cursor",
            "--bin",
            &bin_override("cursor"),
            "--compact",
        ],
        &[("MOCK_STDOUT", &about)],
    );

    assert!(output.status.success(), "exit {:?}", output.status.code());
    let cursor = usage_identity(&json_stdout(&output), "cursor");
    assert_eq!(
        cursor["plan"], "Team",
        "the tier is a display name, kept verbatim"
    );
    assert_eq!(cursor["availability"]["state"], "unavailable");
    assert_eq!(
        cursor["availability"]["reason"], "no_headroom_reader",
        "Cursor's dollar pools reach only its interactive TUI"
    );
}

#[test]
fn usage_reports_cursor_without_a_login_rather_than_authenticating() {
    // A null tier means no stored token pair. The probe must report that and
    // stop — resolving it would mean the API-key exchange, which writes to the
    // shared credential store and has been observed clobbering a real login.
    let about = serde_json::json!({
        "cliVersion": "2026.07.23-e383d2b",
        "subscriptionTier": null,
        "userEmail": null
    })
    .to_string();

    let output = run(
        &[
            "usage",
            "--harness",
            "cursor",
            "--bin",
            &bin_override("cursor"),
            "--compact",
        ],
        &[
            ("MOCK_STDOUT", &about),
            ("CURSOR_API_KEY", "key_do_not_use"),
        ],
    );

    assert!(output.status.success());
    let cursor = usage_identity(&json_stdout(&output), "cursor");
    assert_eq!(cursor["availability"]["reason"], "not_logged_in");
    assert!(cursor["plan"].is_null());
}

#[test]
fn the_cursor_probe_masks_the_api_key_from_its_child() {
    // The API-key path is a *login*: it exchanges the key for tokens and
    // persists them to the shared store. Masking the variable is what makes it
    // impossible for this probe to authenticate, now or after a CLI release that
    // starts honoring the key on `about`. The mock echoes what it inherited.
    let output = run(
        &[
            "usage",
            "--harness",
            "cursor",
            "--bin",
            &bin_override("cursor"),
            "--compact",
        ],
        &[
            ("MOCK_ECHO_ENV", "CURSOR_API_KEY"),
            ("CURSOR_API_KEY", "key_that_would_trigger_a_login"),
        ],
    );

    assert!(output.status.success());
    let cursor = usage_identity(&json_stdout(&output), "cursor");
    let echoed = cursor["availability"]["reason"]["message"]
        .as_str()
        .expect("the child's own output is quoted back");
    assert!(
        echoed.contains("CURSOR_API_KEY="),
        "the mock echoed what it inherited: {echoed}"
    );
    assert!(
        !echoed.contains("key_that_would_trigger_a_login"),
        "the child must inherit no API key: {echoed}"
    );
}

/// How each registry tier must be spelled in the README matrix's `usage` column
/// and under the usage reference's support-tier table. Exhaustive on purpose: a
/// new probe or tier cannot be added without deciding how it reads to a human.
fn documented_usage_tier(support: UsageSupport) -> (&'static str, &'static str, String) {
    let (readme, heading) = match support {
        UsageSupport::Probed(UsageProbe::ClaudeGetUsage) => {
            ("`headroom` (`get_usage`)", "**Headroom**")
        }
        UsageSupport::Probed(UsageProbe::CodexAppServer) => {
            ("`headroom` (app-server)", "**Headroom**")
        }
        UsageSupport::Probed(UsageProbe::CopilotUserEndpoint) => {
            ("`headroom` (GitHub API)", "**Headroom**")
        }
        UsageSupport::Probed(UsageProbe::CursorAbout) => ("plan tier only", "**Plan tier only**"),
        UsageSupport::NoPlanQuota => ("no plan quota", "**No plan quota**"),
        UsageSupport::NoHeadroomReader => ("no reader", "**No reader**"),
    };
    // The reference also quotes the enum value itself, which is the spelling
    // most likely to rot: a variant rename leaves prose that still reads as
    // sensible while naming a type that no longer exists.
    let spelling = match support {
        UsageSupport::Probed(probe) => format!("`Probed({probe:?})`"),
        UsageSupport::NoPlanQuota => "`NoPlanQuota`".to_string(),
        UsageSupport::NoHeadroomReader => "`NoHeadroomReader`".to_string(),
    };
    (readme, heading, spelling)
}

/// The row for `id` in a markdown table whose first cell is `` `id` ``.
fn markdown_row<'a>(doc: &'a str, id: &str) -> Option<&'a str> {
    doc.lines()
        .find(|line| line.starts_with(&format!("| `{id}` |")))
}

#[test]
fn the_documented_usage_tiers_match_the_registry() {
    // `HarnessSpec.usage` is the source; the README matrix and the usage
    // reference restate it for readers. Restating a registry value is exactly
    // how a doc goes quietly stale — someone flips a tier after an upstream
    // release and the tables keep promising the old answer. This fails instead.
    let readme = std::fs::read_to_string("README.md").expect("README.md");
    let reference =
        std::fs::read_to_string("docs/harness-usage.md").expect("docs/harness-usage.md");

    for spec in oneharness_core::domain::harness::all() {
        let (readme_cell, tier_heading, spelling) = documented_usage_tier(spec.usage);
        let id = spec.id;

        let row = markdown_row(&readme, id)
            .unwrap_or_else(|| panic!("README.md has no harness row for `{id}`"));
        assert!(
            row.ends_with(&format!("| {readme_cell} |")),
            "README.md's `usage` column for `{id}` should end with `| {readme_cell} |`, got:\n{row}"
        );

        let tier_row = reference
            .lines()
            .find(|line| {
                line.starts_with("| ")
                    && line.contains(tier_heading)
                    && line.contains(&format!("`{id}`"))
            })
            .unwrap_or_else(|| {
                panic!("docs/harness-usage.md lists no {tier_heading} row for `{id}`")
            });
        assert!(
            tier_row.contains(&spelling),
            "docs/harness-usage.md's {tier_heading} row for `{id}` should quote {spelling}, got:\n{tier_row}"
        );
    }
}

#[test]
fn usage_honors_an_explicit_config_file_and_ignores_it_under_no_config() {
    // `--config` pins the whole layered configuration to one file, which is how
    // a usage identity gets its variant. `--no-config` must then ignore the very
    // same file, so a probe cannot be reshaped by config a caller opted out of.
    let fixture = ConfigFixture::new(
        "usage-explicit-config",
        "",
        &format!(
            "[harness.claude-code]\nbin = {bin:?}\n\
             [harness.claude-code.variant.work]\nenv = {{ CLAUDE_CONFIG_DIR = \"/home/u/.claude-work\" }}\n",
            bin = mock_bin().display().to_string()
        ),
    );
    let config = fixture.user_config();
    let config = config.to_str().expect("a utf-8 path");

    let mut cmd = Command::new(oneharness_bin());
    for var in ENV_OVERRIDE_VARS {
        cmd.env_remove(var);
    }
    let output = cmd
        .args([
            "usage",
            "--harness",
            "claude-code:work",
            "--config",
            config,
            "--compact",
        ])
        .env("MOCK_REPLY_AFTER_LINES", "1")
        .env("MOCK_STDOUT", claude_usage_response())
        .output()
        .expect("failed to run oneharness");

    assert!(output.status.success(), "exit {:?}", output.status.code());
    let claude = usage_identity(&json_stdout(&output), "claude-code");
    assert_eq!(claude["variant"], "work");
    assert_eq!(
        claude["selector"]["path"], "/home/u/.claude-work",
        "the identity came from the config file named on the CLI"
    );

    // `--no-config` ignores that same file: with configuration off, the variant
    // was never declared, so the identity is a loud usage error rather than a
    // probe silently pointed at the base harness's ambient credentials.
    let mut cmd = Command::new(oneharness_bin());
    for var in ENV_OVERRIDE_VARS {
        cmd.env_remove(var);
    }
    let ignored = cmd
        .args(["usage", "--harness", "claude-code:work", "--no-config"])
        .env("ONEHARNESS_CONFIG", config)
        .output()
        .expect("failed to run oneharness");

    assert_eq!(ignored.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&ignored.stderr).contains("unknown harness variant"),
        "stderr: {}",
        String::from_utf8_lossy(&ignored.stderr)
    );
}

#[test]
fn usage_refuses_an_out_of_range_timeout_rather_than_panicking() {
    // A probe deadline is `Instant + Duration`, which *panics* on overflow — so
    // an absurd timeout has to be refused at the boundary. "Never panic on a
    // harness's behavior" is worth just as little if the CLI panics on its own
    // input.
    for value in ["18446744073709551615", "0", "3601"] {
        let output = run(
            &[
                "usage",
                "--harness",
                "cursor",
                "--bin",
                &bin_override("cursor"),
                "--timeout",
                value,
            ],
            &[],
        );

        assert_eq!(
            output.status.code(),
            Some(2),
            "--timeout {value} must be a usage error"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stderr.contains("panicked"), "{stderr}");
        assert!(
            stderr.contains("not in") || stderr.contains("invalid value"),
            "{stderr}"
        );
    }

    // The largest accepted value still runs the probe normally.
    let ok = run(
        &[
            "usage",
            "--harness",
            "cursor",
            "--bin",
            &bin_override("cursor"),
            "--timeout",
            "3600",
            "--compact",
        ],
        &[("MOCK_STDOUT", r#"{"subscriptionTier":"Team"}"#)],
    );
    assert!(ok.status.success(), "exit {:?}", ok.status.code());
    assert_eq!(usage_identity(&json_stdout(&ok), "cursor")["plan"], "Team");
}

#[test]
fn usage_reports_copilot_server_and_shape_failures_as_unknown() {
    if !curl_available() {
        eprintln!("skipping: curl is not installed (the Copilot probe's HTTP client)");
        return;
    }

    // A server error, an HTML body where JSON was expected (a captive proxy),
    // and a 200 whose quota surface was renamed. The endpoint is undocumented
    // internal, so each must reach the consumer as "nothing learned" — a
    // confident "no headroom" from any of them would be a lie about headroom.
    for (status, body, expected) in [
        (503u16, "upstream unavailable", "503"),
        (200, "<html>captive portal</html>", "not a JSON object"),
        (
            200,
            r#"{"copilot_plan":"individual","quotas":{"chat":{"unlimited":true}}}"#,
            "quota_snapshots",
        ),
    ] {
        let (base, server) = one_shot_http_server(status, body);
        let output = run_copilot_usage(
            &["usage", "--harness", "copilot", "--compact"],
            &[
                ("ONEHARNESS_COPILOT_API_BASE", &base),
                ("GH_TOKEN", "ghs_hermetic_token"),
            ],
        );

        assert!(
            output.status.success(),
            "HTTP {status} is data, not an exit code"
        );
        let copilot = usage_identity(&json_stdout(&output), "copilot");
        assert_eq!(
            copilot["availability"]["state"], "unknown",
            "HTTP {status} with body {body}"
        );
        let message = copilot["availability"]["reason"]["message"]
            .as_str()
            .expect("a message");
        assert!(
            message.contains(expected),
            "expected {expected:?} in {message:?}"
        );
        assert!(
            copilot["availability"]["windows"].is_null(),
            "no percentage is reachable from a failed probe"
        );
        let _ = server.join();
    }
}

#[test]
fn usage_refuses_a_copilot_api_base_that_could_be_injected() {
    // The base URL and token are interpolated into curl's own config grammar, so
    // both are validated before the fetch. A rejected value must reach the
    // consumer as a named probe failure — never as a silently skipped identity.
    let output = run_copilot_usage(
        &["usage", "--harness", "copilot", "--compact"],
        &[
            (
                "ONEHARNESS_COPILOT_API_BASE",
                "https://api.github.com\"\nheader = \"X: y",
            ),
            ("GH_TOKEN", "ghs_hermetic_token"),
        ],
    );

    assert!(output.status.success(), "exit {:?}", output.status.code());
    let copilot = usage_identity(&json_stdout(&output), "copilot");
    assert_eq!(copilot["availability"]["state"], "unknown");
    let message = copilot["availability"]["reason"]["message"]
        .as_str()
        .expect("a message");
    assert!(
        message.contains("ONEHARNESS_COPILOT_API_BASE"),
        "the message must name the variable to fix: {message}"
    );
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("ghs_hermetic_token"),
        "a credential must never reach the report"
    );
}

#[test]
fn usage_refuses_a_plaintext_copilot_api_base_that_would_expose_the_token() {
    // The probe sends the GitHub token as a bearer header, so a plaintext base
    // would put a live credential on the wire for anything between here and the
    // host. A misconfigured (or attacker-suggested) base is refused before the
    // fetch and reported as a named probe failure — never a quiet plaintext GET.
    //
    // No curl gate: the refusal happens before the HTTP client is spawned.
    for base in [
        "http://api.github.com",
        "http://ghe.internal.example:8080",
        // Loopback is the only plaintext exception, and userinfo does not make a
        // remote host into one.
        "http://127.0.0.1@evil.example",
    ] {
        let output = run_copilot_usage(
            &["usage", "--harness", "copilot", "--compact"],
            &[
                ("ONEHARNESS_COPILOT_API_BASE", base),
                ("GH_TOKEN", "ghs_hermetic_token"),
            ],
        );

        assert!(output.status.success(), "exit {:?}", output.status.code());
        let copilot = usage_identity(&json_stdout(&output), "copilot");
        assert_eq!(
            copilot["availability"]["state"], "unknown",
            "`{base}` must not be probed"
        );
        let message = copilot["availability"]["reason"]["message"]
            .as_str()
            .expect("a message");
        assert!(
            message.contains("ONEHARNESS_COPILOT_API_BASE") && message.contains("HTTPS"),
            "the message must name the variable and what it requires: {message}"
        );
        assert!(
            !String::from_utf8_lossy(&output.stdout).contains("ghs_hermetic_token"),
            "a credential must never reach the report"
        );
    }
}

#[test]
fn usage_refuses_a_copilot_token_carrying_config_syntax_without_leaking_it() {
    // The token is interpolated into curl's own config grammar, so one carrying
    // a quote or a newline is refused before any fetch. The refusal is only half
    // the property: the value is a live credential, so *nothing* of it may reach
    // the report a caller prints or the diagnostics a CI job archives — which is
    // why this drives the shipped binary rather than the branch in isolation.
    //
    // No curl gate: validation happens before the HTTP client is spawned, so this
    // path is reachable with no client installed at all.
    const CANARY: &str = "ghs_canary7Zq4Xr9Lm2";
    let injectable = format!("{CANARY}\"\nheader = \"X-Injected: y");

    for format in ["json", "text"] {
        let output = run_copilot_usage(
            &[
                "usage",
                "--harness",
                "copilot",
                "--format",
                format,
                "--compact",
            ],
            &[("GH_TOKEN", &injectable)],
        );

        assert!(
            output.status.success(),
            "a refused credential is data, not an exit code: {:?}",
            output.status.code()
        );
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

        if format == "json" {
            let copilot = usage_identity(&json_stdout(&output), "copilot");
            assert_eq!(
                copilot["availability"]["state"], "unknown",
                "a token that cannot be forwarded teaches nothing about headroom: {copilot}"
            );
            assert_eq!(copilot["availability"]["reason"]["kind"], "probe_failed");
            assert_eq!(
                copilot["selector"],
                serde_json::json!({"kind": "env_secret", "env": "GH_TOKEN"}),
                "the identity names the variable, never the token"
            );
            let message = copilot["availability"]["reason"]["message"]
                .as_str()
                .expect("a message");
            assert!(
                message.contains("GitHub token"),
                "the message must say what to fix: {message}"
            );
        }

        // Every run of the credential, not just the whole string: a partial echo
        // (a truncated log line, a quoted prefix) leaks a token just as well.
        for length in 4..=CANARY.len() {
            for fragment in CANARY
                .as_bytes()
                .windows(length)
                .map(|window| String::from_utf8_lossy(window).into_owned())
            {
                assert!(
                    !stdout.contains(&fragment),
                    "`--format {format}` leaked `{fragment}` into the report:\n{stdout}"
                );
                assert!(
                    !stderr.contains(&fragment),
                    "`--format {format}` leaked `{fragment}` into its diagnostics:\n{stderr}"
                );
            }
        }
    }
}

#[test]
fn usage_reports_a_failed_stdout_write_instead_of_panicking() {
    // A command whose output *is* its deliverable must not die mid-sentence:
    // a reader closing the pipe (`oneharness usage | head -1`) is an ordinary
    // event, and `print!`/`println!` panic on it.
    //
    // The read end is closed immediately after spawn, while the harness is held
    // for MOCK_SLEEP_MS before the report is rendered — so the close precedes
    // the first write by a wide margin rather than racing it.
    //
    // `run` is here because both output paths now share one writer: `usage
    // --format text` is the text half, and `print_json` — which every
    // JSON-emitting command uses — is the half `run` and `usage --format json`
    // exercise.
    let cases: [&[&str]; 3] = [
        &["usage", "--format", "text"],
        &["usage", "--format", "json"],
        &["run", "--prompt", "hi", "--compact"],
    ];
    for case in cases {
        let label = case.join(" ");
        let mut child = Command::new(oneharness_bin())
            .env("ONEHARNESS_NO_CONFIG", "1")
            .env("MOCK_SLEEP_MS", "400")
            .env("MOCK_STDOUT", "{}")
            .args(case)
            .args(["--harness", "cursor", "--bin", &bin_override("cursor")])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to spawn oneharness");
        drop(child.stdout.take().expect("stdout was piped"));

        let output = child.wait_with_output().expect("failed to wait");
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert!(
            !stderr.contains("panicked"),
            "`{label}` panicked on a closed stdout:\n{stderr}"
        );
        assert_eq!(
            output.status.code(),
            Some(2),
            "`{label}` must report the write failure as an error, stderr:\n{stderr}"
        );
        assert!(
            stderr.contains("could not write to stdout"),
            "`{label}` must name what failed, stderr:\n{stderr}"
        );
    }
}

#[cfg(unix)]
#[test]
fn interrupt_reports_a_failed_stdout_write_instead_of_panicking() {
    // The answer frame IS this command's deliverable, and its reader is a
    // supervisor that may pipe it into `head` or die between sending the
    // interrupt and reading the reply. `println!` panics on that; the shared
    // writer reports it, like every other JSON-emitting command.
    //
    // No run is addressed, so this answers `not_running` — which is the point:
    // the write path is reached whatever the verdict, and no live turn is
    // needed to exercise it.
    let store = control_store_dir("brokenpipe");
    let mut child = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        .args([
            "interrupt",
            "--session",
            "gone",
            "--session-dir",
            &store.display().to_string(),
            "--compact",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn oneharness interrupt");
    drop(child.stdout.take().expect("stdout was piped"));

    let output = child.wait_with_output().expect("failed to wait");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked"),
        "interrupt panicked on a closed stdout:\n{stderr}"
    );
    let _ = std::fs::remove_dir_all(&store);
}

// --- out-of-band turn control (`run --control` + `oneharness interrupt`) ------

/// A private, per-test session store (which is also where the run's control
/// socket lives, at `<dir>/control/<name>.sock`).
fn control_store_dir(tag: &str) -> PathBuf {
    let name = format!("oh-control-{tag}-{}-{}", std::process::id(), tag.len());
    let dir = control_store_root(tag, &name).join(&name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The root a control store goes under.
///
/// `/tmp` rather than `std::env::temp_dir()`, because on unix this path becomes
/// a socket *address* and not just a place to put files: `sockaddr_un.sun_path`
/// holds 104 bytes on macOS against Linux's 108, and macOS's per-user `$TMPDIR`
/// is a `/var/folders/…` path the bind canonicalizes to `/private/var/folders/…`
/// before binding — over half the budget spent on the root alone, so an address
/// under it overruns `sun_path` on macOS while fitting comfortably on Linux.
#[cfg(unix)]
fn control_store_root(tag: &str, name: &str) -> PathBuf {
    // Budget the part a caller controls against the TIGHTEST platform, so a
    // too-long tag fails on Linux instead of only in macOS CI: `/tmp`
    // canonicalizes to `/private/tmp` there, leaving 90 of the 103 usable bytes
    // for `<name>/control/<session>.sock`. The session name is the caller's, so
    // it is charged at the tag's length or 16 — whichever is larger — which
    // every control test is comfortably inside.
    const BUDGET: usize = 103 - "/private/tmp/".len();
    let session = tag.len().max(16);
    let longest = name.len() + "/control/".len() + session + ".sock".len();
    assert!(
        longest <= BUDGET,
        "control store `{name}` leaves no room for a `sun_path`-sized socket \
         address ({longest} > {BUDGET}) — shorten the tag"
    );
    // Canonical, because a bound socket path IS canonical: `bind` resolves the
    // directory before binding — the address is handed to a separate process,
    // and a symlinked one resolves differently depending on where that process
    // runs — so the run reports `/private/tmp/…` on macOS whatever was passed
    // in. A store rooted at the uncanonicalized `/tmp` still binds and still
    // interrupts (the symlink resolves either way), but its path no longer
    // matches the one the report echoes. Resolving here is also what makes the
    // budget above the real one rather than an estimate.
    std::fs::canonicalize("/tmp").expect("/tmp must exist to root a control store")
}

/// No socket is ever bound under this root — `--control` is refused outright
/// where there are no unix sockets — so the ordinary temp dir is right and
/// there is no address length to budget for.
#[cfg(not(unix))]
fn control_store_root(_tag: &str, _name: &str) -> PathBuf {
    std::env::temp_dir()
}

/// Poll until `condition` holds or the deadline passes. Control is inherently a
/// race between two processes, so the tests wait on observable state (a socket
/// appearing, a prompt frame reaching the harness) rather than on a sleep.
///
/// Unix-gated with the control tests that poll: control needs a unix socket, so
/// there is nothing to wait on where there is none.
#[cfg(unix)]
fn wait_until(label: &str, mut condition: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while std::time::Instant::now() < deadline {
        if condition() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    panic!("timed out waiting for {label}");
}

#[cfg(unix)]
#[test]
fn control_interrupt_aborts_a_live_turn_from_a_separate_process() {
    let mock_profile = mock_profile_redirect();
    // The whole contract in one exchange: a `run --control --session` process
    // opens an addressable socket, a SEPARATE `oneharness interrupt` process
    // resolves it and aborts the in-flight turn, and the run finishes normally
    // (session intact) with the interrupt recorded in its report.
    let store = control_store_dir("interrupt");
    let store_arg = store.display().to_string();
    let cwd = control_store_dir("interrupt-cwd");
    let cwd_arg = cwd.display().to_string();
    let turn_log = store.join("turn.log");
    let turn_log_arg = turn_log.display().to_string();

    let child = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        .env("MOCK_TURN_LOG", &turn_log_arg)
        .env("MOCK_TURN_HOLD", "1")
        .env(
            "MOCK_STDOUT",
            r#"{"type":"system","subtype":"init","session_id":"sess-ctl"}"#,
        )
        .args([
            "run",
            "--harness",
            "claude-code",
            "--control",
            "--session",
            "watched",
            "--session-dir",
            &store_arg,
            "--cwd",
            &cwd_arg,
            "--prompt",
            "keep working",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
            "--env",
            mock_profile.as_str(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn the controlled run");

    let socket = store.join("control").join("watched.sock");
    wait_until("the control socket to appear", || socket.exists());
    // 0600: the socket is a lever over a running agent.
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&socket).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "control socket must be owner-only");
    }
    // The turn is genuinely in flight once the harness has the prompt frame.
    wait_until("the turn to start", || {
        std::fs::read_to_string(&turn_log)
            .map(|log| log.contains("keep working"))
            .unwrap_or(false)
    });

    let interrupt = run(
        &[
            "interrupt",
            "--session",
            "watched",
            "--session-dir",
            &store_arg,
            "--cwd",
            &cwd_arg,
            "--compact",
        ],
        &[],
    );
    assert!(interrupt.status.success(), "{interrupt:?}");
    let frame = json_stdout(&interrupt);
    assert_eq!(frame["v"], 1);
    assert_eq!(frame["ok"], true);
    assert_eq!(frame["mechanism"], "claude-control-request");

    let output = child.wait_with_output().expect("run did not finish");
    assert!(output.status.success(), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).expect("run report was not JSON");
    assert_eq!(report["schema_version"], "0.6");
    assert_eq!(report["control"]["mechanism"], "claude-control-request");
    assert_eq!(report["control"]["socket"], socket.display().to_string());
    let interrupts = report["control"]["interrupts"].as_array().unwrap();
    assert_eq!(interrupts.len(), 1);
    assert_eq!(interrupts[0]["verb"], "interrupt");
    assert_eq!(interrupts[0]["outcome"], "served");
    assert!(!interrupts[0]["at"].as_str().unwrap().is_empty());

    // The harness really received the control frame — asserted at the harness,
    // not from oneharness's own bookkeeping.
    let log = std::fs::read_to_string(&turn_log).unwrap();
    assert!(log.contains("control_request"), "turn log:\n{log}");
    assert!(log.contains("INTERRUPTED"), "turn log:\n{log}");

    // The socket is gone with the run, and the session survived the interrupt.
    assert!(
        !socket.exists(),
        "socket must be removed when the run exits"
    );
    assert_eq!(report["results"][0]["status"], "ok");
    assert_eq!(report["session"]["name"], "watched");
    assert_eq!(report["session"]["token"], "sess-ctl");

    let _ = std::fs::remove_dir_all(&store);
    let _ = std::fs::remove_dir_all(&cwd);
}

#[cfg(unix)]
#[test]
fn control_run_pins_the_message_stream_argv_and_leaves_the_prompt_off_it() {
    let mock_profile = mock_profile_redirect();
    // The control channel IS the run's stdin, so the prompt cannot ride the
    // argv: `--input-format stream-json` replaces the positional.
    let store = control_store_dir("argv");
    let store_arg = store.display().to_string();
    let cwd = control_store_dir("argv-cwd");
    let cwd_arg = cwd.display().to_string();
    let turn_log = store.join("turn.log");
    let argv_file = store.join("argv.txt");

    let output = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        .env("MOCK_TURN_LOG", turn_log.display().to_string())
        .env("MOCK_ARGV_FILE", argv_file.display().to_string())
        .args([
            "run",
            "--harness",
            "claude-code",
            "--control",
            "--session",
            "argvcheck",
            "--session-dir",
            &store_arg,
            "--cwd",
            &cwd_arg,
            "--prompt",
            "secret-prompt-text",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
            "--env",
            mock_profile.as_str(),
        ])
        .output()
        .expect("failed to run oneharness");
    assert!(output.status.success(), "{output:?}");

    let argv = std::fs::read_to_string(&argv_file).unwrap();
    let args: Vec<&str> = argv.lines().collect();
    assert!(
        args.windows(2)
            .any(|w| w == ["--input-format", "stream-json"]),
        "argv:\n{argv}"
    );
    assert!(
        args.windows(2)
            .any(|w| w == ["--output-format", "stream-json"]),
        "argv:\n{argv}"
    );
    assert!(
        !args.contains(&"secret-prompt-text"),
        "the prompt must ride the control stream, not the argv:\n{argv}"
    );
    // It reached the harness over stdin instead.
    let log = std::fs::read_to_string(&turn_log).unwrap();
    assert!(log.contains("secret-prompt-text"), "turn log:\n{log}");

    let _ = std::fs::remove_dir_all(&store);
    let _ = std::fs::remove_dir_all(&cwd);
}

#[cfg(unix)]
#[test]
fn a_controlled_run_reports_the_resolved_socket_address_not_the_one_passed_in() {
    // The socket path is an ADDRESS a separate `oneharness interrupt` process
    // resolves, so `bind` canonicalizes it and the report must echo what was
    // actually bound — a supervisor that stores the reported path and hands it
    // to another process needs the resolved one.
    //
    // A symlinked store is the shape macOS puts every control test in for free:
    // `/tmp` is a symlink to `/private/tmp` there, so a run told `--session-dir
    // /tmp/…` binds and reports `/private/tmp/…`. Reproducing it with an
    // explicit symlink is what lets Linux prove the contract macOS exercises.
    let mock_profile = mock_profile_redirect();
    let store = control_store_dir("symlink");
    let real = store.join("real");
    std::fs::create_dir_all(&real).unwrap();
    let link = store.join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let output = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        .args([
            "run",
            "--harness",
            "claude-code",
            "--control",
            "--session",
            "resolved",
            "--session-dir",
            &link.display().to_string(),
            "--cwd",
            &store.display().to_string(),
            "--prompt",
            "hi",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
            "--env",
            mock_profile.as_str(),
        ])
        .output()
        .expect("failed to run oneharness");
    assert!(output.status.success(), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).expect("run report was not JSON");
    assert_eq!(
        report["control"]["socket"],
        real.join("control")
            .join("resolved.sock")
            .display()
            .to_string(),
        "the report must name the bound address, not the symlink it was given"
    );

    let _ = std::fs::remove_dir_all(&store);
}

#[cfg(not(unix))]
#[test]
fn control_on_a_platform_without_unix_sockets_is_a_usage_error() {
    // There is no socket to open on Windows, so `--control` must say so before
    // spawning rather than running a turn nobody can address.
    let store = control_store_dir("platform");
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--control",
            "--session",
            "win",
            "--session-dir",
            &store.display().to_string(),
            "--prompt",
            "hi",
            "--bin",
            &bin_override("claude-code"),
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--control needs a unix domain socket"),
        "stderr:\n{stderr}"
    );
    let _ = std::fs::remove_dir_all(&store);
}

#[test]
fn control_without_a_session_is_a_usage_error_naming_the_reason() {
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--control",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("claude-code"),
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--control requires --session"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn control_on_a_harness_without_a_mechanism_is_a_usage_error() {
    let store = control_store_dir("unsupported");
    let store_arg = store.display().to_string();
    // cursor-agent was probed and has no headless control surface at all.
    let output = run(
        &[
            "run",
            "--harness",
            "cursor",
            "--control",
            "--session",
            "nope",
            "--session-dir",
            &store_arg,
            "--prompt",
            "hi",
            "--bin",
            &bin_override("cursor"),
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("has no out-of-band turn control"),
        "stderr:\n{stderr}"
    );
    let _ = std::fs::remove_dir_all(&store);
}

#[test]
fn control_refuses_edit_mode_on_a_driven_turn_rather_than_over_granting() {
    // A driven turn negotiates approvals on the wire, so oneharness answers each
    // permission request itself. `edit` means auto-approved edits with shell
    // still gated, and no protocol here carries a sourced way to tell those
    // apart — so answering at all would either grant shell authority the mode
    // denies or silently downgrade `edit`. Refuse before spawning instead.
    // opencode declares `edit` AND a control mechanism, so it is exactly the
    // combination that would otherwise reach the blanket grant.
    let store = control_store_dir("edit-mode");
    let store_arg = store.display().to_string();
    let output = run(
        &[
            "run",
            "--harness",
            "opencode",
            "--control",
            "--session",
            "edits",
            "--session-dir",
            &store_arg,
            "--prompt",
            "hi",
            "--mode",
            "edit",
            "--bin",
            &bin_override("opencode"),
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("negotiates approvals on the wire")
            && stderr.contains("--mode edit")
            && stderr.contains("--mode bypass"),
        "the refusal must name the mode and an expressible alternative; stderr:\n{stderr}"
    );
    // Refused *before* anything spawned, which is what makes it a usage error
    // rather than a run that quietly acted with the wrong authority.
    assert!(
        !store.join("control").exists(),
        "a refused control run must open no socket"
    );
    let _ = std::fs::remove_dir_all(&store);
}

#[test]
fn control_still_accepts_edit_mode_where_the_argv_carries_it() {
    // Claude Code's control frame rides its ordinary `-p` run, so `edit` is
    // delivered by the same argv a plain dispatch uses (`acceptEdits`) and the
    // refusal above must not reach it. `--print-command` proves the mapping
    // survives without spawning.
    let store = control_store_dir("edit-mode-claude");
    let store_arg = store.display().to_string();
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--control",
            "--session",
            "edits",
            "--session-dir",
            &store_arg,
            "--prompt",
            "hi",
            "--mode",
            "edit",
            "--print-command",
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("acceptEdits"),
        "claude-code must still deliver `edit` on its argv; stdout:\n{stdout}"
    );
    let _ = std::fs::remove_dir_all(&store);
}

#[test]
fn control_needs_exactly_one_harness() {
    let store = control_store_dir("multi");
    let store_arg = store.display().to_string();
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--harness",
            "codex",
            "--control",
            "--session",
            "many",
            "--session-dir",
            &store_arg,
            "--prompt",
            "hi",
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("needs exactly one harness"),
        "stderr:\n{stderr}"
    );
    let _ = std::fs::remove_dir_all(&store);
}

#[test]
fn control_rejects_an_output_format_the_mechanism_cannot_use() {
    let store = control_store_dir("format");
    let store_arg = store.display().to_string();
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--control",
            "--session",
            "fmt",
            "--session-dir",
            &store_arg,
            "--output-format",
            "json",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("claude-code"),
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("needs output format `stream-json` for --control"),
        "stderr:\n{stderr}"
    );
    let _ = std::fs::remove_dir_all(&store);
}

#[test]
fn interrupt_against_a_run_that_is_not_running_reports_not_running() {
    let store = control_store_dir("notrunning");
    let store_arg = store.display().to_string();
    let output = run(
        &[
            "interrupt",
            "--session",
            "ghost",
            "--session-dir",
            &store_arg,
            "--compact",
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let frame = json_stdout(&output);
    assert_eq!(frame["ok"], false);
    assert_eq!(frame["reason"], "not_running");
    let _ = std::fs::remove_dir_all(&store);
}

#[cfg(unix)]
#[test]
fn interrupt_between_turns_reports_no_active_turn() {
    // A run that is alive and listening but has no turn in flight must answer
    // `no_active_turn` — distinct from `not_running`, because a supervisor
    // retries one and gives up on the other. Driven through the real
    // `oneharness interrupt` command against a bound socket, so the CLI's own
    // resolution and exit code are what is asserted.
    use oneharness_core::domain::control::socket_path;
    let store = control_store_dir("noturn");
    let listener = oneharness_core::io::control::bind(
        &socket_path(&store, "idle"),
        oneharness_core::domain::control::ControlShape::ClaudeControlRequest,
        None,
    )
    .unwrap();

    let output = run(
        &[
            "interrupt",
            "--session",
            "idle",
            "--session-dir",
            &store.display().to_string(),
            "--compact",
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let frame = json_stdout(&output);
    assert_eq!(frame["ok"], false);
    assert_eq!(frame["reason"], "no_active_turn");
    // Refused requests are recorded too, so a supervisor's failed attempt is
    // visible in the run's own report rather than only in its own logs.
    let events = listener.handle_ref().events();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].reason(),
        Some(oneharness_core::domain::control::ControlReason::NoActiveTurn)
    );

    let _ = std::fs::remove_dir_all(&store);
}

#[test]
fn interrupt_on_a_harness_without_a_mechanism_reports_unsupported() {
    // The store binds the session to a harness with no control mechanism, so
    // the refusal is knowable without a live run — the one refusal a supervisor
    // can learn before spending a dispatch.
    let store = control_store_dir("unsupported-interrupt");
    let cwd = control_store_dir("unsupported-interrupt-cwd");
    let record = serde_json::json!({
        "schema_version": "0.1",
        "name": "bound",
        "project": cwd.display().to_string(),
        "harness": "cursor",
        "token": "th-1",
        "created": "2026-08-08T00:00:00Z",
        "updated": "2026-08-08T00:00:00Z",
    });
    let path = oneharness_core::io::session::session_path(&store, &cwd, "bound");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, serde_json::to_string(&record).unwrap()).unwrap();

    let output = run(
        &[
            "interrupt",
            "--session",
            "bound",
            "--session-dir",
            &store.display().to_string(),
            "--cwd",
            &cwd.display().to_string(),
            "--compact",
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let frame = json_stdout(&output);
    assert_eq!(frame["ok"], false);
    assert_eq!(frame["reason"], "unsupported");
    assert!(frame["error"].as_str().unwrap().contains("cursor"));

    let _ = std::fs::remove_dir_all(&store);
    let _ = std::fs::remove_dir_all(&cwd);
}

#[test]
fn a_run_without_control_opens_no_socket_and_keeps_its_argv() {
    let mock_profile = mock_profile_redirect();
    // The non-breaking guarantee: absent `--control`, nothing changes — no
    // socket directory, and the prompt still rides the argv as a positional.
    let store = control_store_dir("absent");
    let store_arg = store.display().to_string();
    let cwd = control_store_dir("absent-cwd");
    let argv_file = store.join("argv.txt");

    let output = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        .env("MOCK_ARGV_FILE", argv_file.display().to_string())
        .env(
            "MOCK_STDOUT",
            r#"{"type":"result","result":"hi","session_id":"sess-plain"}"#,
        )
        .args([
            "run",
            "--harness",
            "claude-code",
            "--session",
            "plain",
            "--session-dir",
            &store_arg,
            "--cwd",
            &cwd.display().to_string(),
            "--prompt",
            "ordinary-prompt",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
            "--env",
            mock_profile.as_str(),
        ])
        .output()
        .expect("failed to run oneharness");
    assert!(output.status.success(), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(report["control"].is_null(), "{report}");
    assert!(
        !store.join("control").exists(),
        "no control directory should be created"
    );
    let argv = std::fs::read_to_string(&argv_file).unwrap();
    assert!(
        argv.lines().any(|l| l == "ordinary-prompt"),
        "the prompt must still ride the argv:\n{argv}"
    );
    assert!(
        !argv.lines().any(|l| l == "stream-json"),
        "the ordinary argv must be unchanged:\n{argv}"
    );

    let _ = std::fs::remove_dir_all(&store);
    let _ = std::fs::remove_dir_all(&cwd);
}

#[cfg(unix)]
#[test]
fn a_host_signal_cancels_a_controlled_run_and_takes_its_socket_with_it() {
    let mock_profile = mock_profile_redirect();
    // A controlled run is the one shape that holds the harness's stdin open past
    // the prompt, so a harness waiting for its next frame never reaches EOF on its
    // own — exactly the run a supervisor's SIGTERM must still be able to end. The
    // signal has to tear the tree down through the ordinary cancel path, emit the
    // report, and take the 0600 socket with it; a lever left addressable after the
    // run is gone is worse than none.
    let store = control_store_dir("signal");
    let store_arg = store.display().to_string();
    let cwd = control_store_dir("signal-cwd");
    let cwd_arg = cwd.display().to_string();
    let turn_log = store.join("turn.log");

    let child = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        .env("MOCK_TURN_LOG", turn_log.display().to_string())
        // The turn never ends by itself, so only the cancellation can end it.
        .env("MOCK_TURN_HOLD", "1")
        .env(
            "MOCK_STDOUT",
            r#"{"type":"system","subtype":"init","session_id":"sess-signal"}"#,
        )
        .args([
            "run",
            "--harness",
            "claude-code",
            "--control",
            "--session",
            "signalled",
            "--session-dir",
            &store_arg,
            "--cwd",
            &cwd_arg,
            "--prompt",
            "keep working",
            "--bin",
            &bin_override("claude-code"),
            // Far beyond the teardown measured below, so a run that only ended at
            // its own deadline could never pass.
            "--timeout",
            "60",
            "--compact",
            "--env",
            mock_profile.as_str(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn the controlled run");

    let socket = store.join("control").join("signalled.sock");
    wait_until("the control socket to appear", || socket.exists());
    // Signal a genuinely in-flight turn, not a spawn race.
    wait_until("the turn to start", || {
        std::fs::read_to_string(&turn_log)
            .map(|log| log.contains("keep working"))
            .unwrap_or(false)
    });

    let signalled = std::time::Instant::now();
    let sent = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .expect("failed to signal oneharness");
    assert!(sent.success(), "kill -TERM did not succeed");

    let output = child.wait_with_output().expect("failed to reap oneharness");
    assert!(
        signalled.elapsed() < std::time::Duration::from_secs(15),
        "the signalled controlled run did not tear down promptly: {:?}",
        signalled.elapsed()
    );
    assert_eq!(
        output.status.code(),
        Some(1),
        "oneharness did not exit through its own reporting path: {:?}",
        output.status
    );

    let report = json_stdout(&output);
    assert_eq!(report["results"][0]["status"], "cancelled");
    // The control block still describes the run that was cut short, rather than
    // vanishing with it.
    assert_eq!(report["control"]["mechanism"], "claude-control-request");
    assert!(
        report["control"]["interrupts"]
            .as_array()
            .unwrap()
            .is_empty(),
        "{report}"
    );
    assert!(
        !socket.exists(),
        "the socket must be removed when a cancelled run exits"
    );

    let _ = std::fs::remove_dir_all(&store);
    let _ = std::fs::remove_dir_all(&cwd);
}

#[cfg(unix)]
#[test]
fn control_works_alongside_streaming_so_a_supervisor_can_watch_and_interrupt() {
    let mock_profile = mock_profile_redirect();
    // The supervisor use case: read normalized events as they arrive and cut the
    // turn short on what you see. Streaming and control share one open stdin, so
    // this pins that they compose.
    let store = control_store_dir("stream");
    let store_arg = store.display().to_string();
    let cwd = control_store_dir("stream-cwd");
    let turn_log = store.join("turn.log");

    let child = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        .env("MOCK_TURN_LOG", turn_log.display().to_string())
        .env("MOCK_TURN_HOLD", "1")
        .env(
            "MOCK_STDOUT",
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"step"}}]},"session_id":"sess-stream"}"#,
        )
        .args([
            "run",
            "--harness",
            "claude-code",
            "--control",
            "--stream",
            "--session",
            "streamed",
            "--session-dir",
            &store_arg,
            "--cwd",
            &cwd.display().to_string(),
            "--prompt",
            "keep working",
            "--bin",
            &bin_override("claude-code"),
            "--env",
            mock_profile.as_str(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn the streamed controlled run");

    let socket = store.join("control").join("streamed.sock");
    wait_until("the control socket to appear", || socket.exists());
    wait_until("the turn to start", || {
        std::fs::read_to_string(&turn_log)
            .map(|log| log.contains("keep working"))
            .unwrap_or(false)
    });

    let interrupt = run(
        &[
            "interrupt",
            "--session",
            "streamed",
            "--session-dir",
            &store_arg,
            "--cwd",
            &cwd.display().to_string(),
            "--compact",
        ],
        &[],
    );
    assert!(interrupt.status.success(), "{interrupt:?}");

    let output = child.wait_with_output().expect("run did not finish");
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8_lossy(&output.stdout);
    // The incremental event envelope arrived before the closing report.
    let mut envelopes = text
        .lines()
        .filter_map(|line| serde_json::from_str::<RunStreamEnvelope>(line).ok());
    assert!(
        matches!(envelopes.next(), Some(RunStreamEnvelope::Event { .. })),
        "expected a streamed event first:\n{text}"
    );
    let report = match envelopes.next_back() {
        Some(RunStreamEnvelope::Result { report }) => report,
        other => panic!("expected a closing result envelope, got {other:?}\n{text}"),
    };
    let control = report.control.expect("streamed run should report control");
    assert_eq!(
        control.mechanism,
        oneharness_core::domain::control::ControlShape::ClaudeControlRequest
    );
    assert_eq!(control.interrupts.len(), 1);
    assert!(control.interrupts[0].is_served());

    let _ = std::fs::remove_dir_all(&store);
    let _ = std::fs::remove_dir_all(&cwd);
}

#[test]
fn control_with_a_schema_is_a_usage_error() {
    // The structured-output loop re-prompts, which is a second turn; the control
    // channel owns the one open stdin. Refused up front rather than silently
    // running with retries disabled.
    let store = control_store_dir("schema");
    let schema = store.join("schema.json");
    std::fs::write(&schema, r#"{"type":"object"}"#).unwrap();
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--control",
            "--session",
            "sch",
            "--session-dir",
            &store.display().to_string(),
            "--schema",
            &schema.display().to_string(),
            "--prompt",
            "hi",
            "--bin",
            &bin_override("claude-code"),
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--control cannot be combined with --schema"),
        "stderr:\n{stderr}"
    );
    let _ = std::fs::remove_dir_all(&store);
}

/// Every row of the README's control support matrix: the harness id and the
/// `control` cell (a mechanism id, or the em dash meaning "none").
fn readme_control_matrix() -> Vec<(String, String)> {
    let readme = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("README.md"))
        .expect("README.md is readable");
    let table = readme
        .split("#### Control support matrix")
        .nth(1)
        .expect("README has a control support matrix");
    table
        .lines()
        .skip_while(|line| !line.starts_with("| Harness "))
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .map(|line| {
            let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
            (
                cells[0].to_lowercase().replace(' ', "-"),
                cells[1].trim_matches('`').to_string(),
            )
        })
        .collect()
}

#[test]
fn the_readme_control_matrix_matches_the_registry() {
    // The matrix is the capability's public face, and a stale row is exactly the
    // decay this feature exists to prevent (a supervisor reading support that
    // was removed). Pin it to `oneharness list`, the registry's own report.
    let listed = json_stdout(&run(&["list"], &[]));
    let mut declared: Vec<(String, String)> = listed["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| {
            (
                h["id"].as_str().unwrap().to_string(),
                h["control"].as_str().unwrap_or("—").to_string(),
            )
        })
        .collect();
    declared.sort();

    let mut documented = readme_control_matrix();
    // The matrix names harnesses by display-ish name; map them to registry ids.
    for row in &mut documented {
        row.0 = match row.0.as_str() {
            "claude-code" => "claude-code",
            "opencode" => "opencode",
            "codex" => "codex",
            "crush" => "crush",
            "goose" => "goose",
            "copilot" => "copilot",
            "cursor" => "cursor",
            "qwen" => "qwen",
            other => panic!("unknown harness `{other}` in the README control matrix"),
        }
        .to_string();
    }
    documented.sort();

    assert_eq!(
        documented, declared,
        "the README control matrix has drifted from the registry (`oneharness list`)"
    );
}

#[test]
fn the_readme_documents_every_control_refusal_reason() {
    // The three reasons are a wire contract a supervisor branches on; a reason
    // added to the enum but missing from the docs is an undocumented branch.
    let readme =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("README.md")).unwrap();
    let section = readme
        .split("### Turn control")
        .nth(1)
        .expect("README documents turn control");
    for reason in [
        oneharness_core::domain::control::ControlReason::Unsupported,
        oneharness_core::domain::control::ControlReason::NoActiveTurn,
        oneharness_core::domain::control::ControlReason::NotRunning,
    ] {
        assert!(
            section.contains(reason.as_str()),
            "README does not document the `{}` refusal reason",
            reason.as_str()
        );
    }
}

#[test]
fn control_under_print_command_shows_the_control_argv_and_opens_nothing() {
    // A dry run must still show what a control run WOULD spawn (the message
    // stream replaces the positional prompt), while opening no socket — the
    // rule `--print-command` follows everywhere else: nothing executes, so
    // nothing is created.
    let store = control_store_dir("dryrun");
    let store_arg = store.display().to_string();
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--control",
            "--session",
            "dry",
            "--session-dir",
            &store_arg,
            "--prompt",
            "dry-run-prompt",
            "--bin",
            &bin_override("claude-code"),
            "--print-command",
            "--compact",
        ],
        &[],
    );
    assert!(output.status.success(), "{output:?}");
    let report = json_stdout(&output);
    assert_eq!(report["dry_run"], true);
    assert!(report["control"].is_null(), "a dry run opens no socket");
    let command: Vec<String> = serde_json::from_value(report["results"][0]["command"].clone())
        .expect("the planned command");
    assert!(
        command
            .windows(2)
            .any(|w| w == ["--input-format", "stream-json"]),
        "command: {command:?}"
    );
    assert!(
        !command.contains(&"dry-run-prompt".to_string()),
        "the prompt rides the control stream, not the argv: {command:?}"
    );
    assert!(
        !store.join("control").exists(),
        "a dry run must create no control directory"
    );
    let _ = std::fs::remove_dir_all(&store);
}

#[cfg(unix)]
#[test]
fn a_second_controlled_run_cannot_steal_a_live_sessions_socket() {
    let mock_profile = mock_profile_redirect();
    // The socket is a lever over a running agent, so a second run of the same
    // session name must not displace the first — the supervisor's `interrupt`
    // would otherwise silently address the wrong dispatch. A bind failure is a
    // loud usage error before the second run spawns anything.
    let store = control_store_dir("conflict");
    let store_arg = store.display().to_string();
    let cwd = control_store_dir("conflict-cwd");
    let cwd_arg = cwd.display().to_string();
    let turn_log = store.join("turn.log");

    let child = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        .env("MOCK_TURN_LOG", turn_log.display().to_string())
        .env("MOCK_TURN_HOLD", "1")
        .args([
            "run",
            "--harness",
            "claude-code",
            "--control",
            "--session",
            "taken",
            "--session-dir",
            &store_arg,
            "--cwd",
            &cwd_arg,
            "--prompt",
            "keep working",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
            "--env",
            mock_profile.as_str(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn the first controlled run");

    wait_until("the first run's socket", || {
        store.join("control").join("taken.sock").exists()
    });
    wait_until("the first turn to start", || {
        std::fs::read_to_string(&turn_log)
            .map(|log| log.contains("keep working"))
            .unwrap_or(false)
    });

    let second = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--control",
            "--session",
            "taken",
            "--session-dir",
            &store_arg,
            "--cwd",
            &cwd_arg,
            "--prompt",
            "me too",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[],
    );
    assert_eq!(second.status.code(), Some(2), "{second:?}");
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(
        stderr.contains("could not open the control socket")
            && stderr.contains("already listening"),
        "stderr:\n{stderr}"
    );

    // The first run is untouched and still interruptible.
    let interrupt = run(
        &[
            "interrupt",
            "--session",
            "taken",
            "--session-dir",
            &store_arg,
            "--cwd",
            &cwd_arg,
            "--compact",
        ],
        &[],
    );
    assert!(interrupt.status.success(), "{interrupt:?}");
    let output = child.wait_with_output().expect("first run did not finish");
    assert!(output.status.success(), "{output:?}");

    let _ = std::fs::remove_dir_all(&store);
    let _ = std::fs::remove_dir_all(&cwd);
}

#[test]
fn control_with_more_than_one_prompt_is_a_usage_error() {
    // A batch fans one harness over N prompts, so there is no single live turn
    // for a supervisor to address.
    let store = control_store_dir("batch");
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--control",
            "--session",
            "batched",
            "--session-dir",
            &store.display().to_string(),
            "--prompt",
            "one",
            "--prompt",
            "two",
            "--bin",
            &bin_override("claude-code"),
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--control drives one live turn") && stderr.contains("batch run"),
        "stderr:\n{stderr}"
    );
    let _ = std::fs::remove_dir_all(&store);
}

#[test]
fn the_readme_documents_the_control_protocol_version_in_force() {
    // The frames in the README are what a supervisor copies; a `v` that drifted
    // from the constant would have them writing frames the run refuses.
    let readme =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("README.md")).unwrap();
    let section = readme
        .split("### Turn control")
        .nth(1)
        .expect("README documents turn control");
    let expected = format!(
        r#"{{"v":{},"verb":"interrupt"}}"#,
        oneharness_core::domain::control::PROTOCOL_VERSION
    );
    assert!(
        section.contains(&expected),
        "README does not show the current control request frame ({expected})"
    );
}

#[cfg(unix)]
#[test]
fn interrupt_defaults_to_the_platform_session_store_and_prints_readable_json() {
    // Without --session-dir the command must find the SAME default store a run
    // uses; binding the socket under that default and getting `no_active_turn`
    // (rather than `not_running`) is what proves it resolved there. Also covers
    // the default pretty output — the form a human reads at a terminal.
    use oneharness_core::domain::control::socket_path;
    let state = control_store_dir("default-store");
    let store = state.join("oneharness").join("sessions");
    let _listener = oneharness_core::io::control::bind(
        &socket_path(&store, "defaulted"),
        oneharness_core::domain::control::ControlShape::ClaudeControlRequest,
        None,
    )
    .unwrap();

    let output = run(
        &["interrupt", "--session", "defaulted"],
        &[("XDG_STATE_HOME", &state.display().to_string())],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.lines().count() > 1,
        "the default output is pretty-printed:\n{text}"
    );
    let frame: Value = serde_json::from_str(&text).expect("pretty output is still JSON");
    assert_eq!(frame["reason"], "no_active_turn");

    let _ = std::fs::remove_dir_all(&state);
}

#[cfg(unix)]
#[test]
fn interrupt_refuses_a_session_dir_that_is_not_utf8() {
    // Silently dropping it would resolve the DEFAULT store, so the command
    // would report `not_running` for a run that is very much running.
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let output = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        .args([
            OsStr::new("interrupt"),
            OsStr::new("--session"),
            OsStr::new("x"),
        ])
        .arg("--session-dir")
        .arg(OsStr::from_bytes(b"/tmp/oh-\xff-store"))
        .output()
        .expect("failed to run oneharness");
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("is not valid UTF-8"), "stderr:\n{stderr}");
}

#[cfg(unix)]
#[test]
fn a_run_refuses_a_session_dir_that_is_not_utf8() {
    // The other half of the pair: dropping it here would write the handle to
    // the DEFAULT store, and the `interrupt` that refuses the same path would
    // then be looking somewhere the run never wrote.
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let output = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        .args([
            OsStr::new("run"),
            OsStr::new("--harness"),
            OsStr::new("claude-code"),
            OsStr::new("--session"),
            OsStr::new("x"),
            OsStr::new("--prompt"),
            OsStr::new("hi"),
            OsStr::new("--print-command"),
        ])
        .arg("--session-dir")
        .arg(OsStr::from_bytes(b"/tmp/oh-\xff-store"))
        .output()
        .expect("failed to run oneharness");
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("is not valid UTF-8"), "stderr:\n{stderr}");
}

#[cfg(unix)]
#[test]
fn interrupt_without_a_resolvable_store_is_a_usage_error() {
    // No --session-dir and no platform state dir: there is no address to
    // resolve, which must be said rather than guessed at.
    let output = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        .env_remove("HOME")
        .env_remove("XDG_STATE_HOME")
        .args(["interrupt", "--session", "homeless"])
        .output()
        .expect("failed to run oneharness");
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no session store directory"),
        "stderr:\n{stderr}"
    );
}

#[cfg(unix)]
#[test]
fn an_acp_controlled_run_answers_permission_and_records_a_cancel_the_harness_calls_end_turn() {
    let mock_profile = mock_profile_redirect();
    // The two ACP behaviors that decide whether this works at all, driven
    // through the real CLI against a server that reproduces both:
    //   * the turn does not begin until the client answers
    //     `session/request_permission` — so an unanswered request would leave
    //     every downstream assertion vacuous; and
    //   * after a genuine cancel the harness reports `stopReason: "end_turn"`,
    //     so the interrupt has to be recorded from oneharness's own side.
    let store = control_store_dir("acp");
    let store_arg = store.display().to_string();
    let cwd = control_store_dir("acp-cwd");
    let cwd_arg = cwd.display().to_string();
    let acp_log = store.join("acp.log");

    let child = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        .env("MOCK_ACP_LOG", acp_log.display().to_string())
        .args([
            "run",
            "--harness",
            "copilot",
            "--control",
            "--session",
            "acp",
            "--session-dir",
            &store_arg,
            "--cwd",
            &cwd_arg,
            "--mode",
            "bypass",
            "--prompt",
            "keep working",
            "--bin",
            &bin_override("copilot"),
            "--compact",
            "--env",
            mock_profile.as_str(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn the ACP controlled run");

    wait_until("the control socket", || {
        store.join("control").join("acp.sock").exists()
    });
    // The server asked for permission and oneharness answered — without that
    // reply a real ACP harness never begins work.
    wait_until("the permission exchange", || {
        std::fs::read_to_string(&acp_log)
            .map(|log| log.contains("PERMISSION_ANSWERED"))
            .unwrap_or(false)
    });

    let interrupt = run(
        &[
            "interrupt",
            "--session",
            "acp",
            "--session-dir",
            &store_arg,
            "--cwd",
            &cwd_arg,
            "--compact",
        ],
        &[],
    );
    assert!(interrupt.status.success(), "{interrupt:?}");
    assert_eq!(json_stdout(&interrupt)["mechanism"], "acp-cancel");

    let output = child.wait_with_output().expect("run did not finish");
    assert!(output.status.success(), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).expect("a JSON report");

    let log = std::fs::read_to_string(&acp_log).unwrap();
    let cancel = log
        .lines()
        .find(|line| line.contains("session/cancel"))
        .expect("a cancel reached the server");
    // A cancel sent as a REQUEST gets `-32601 Method not found` from goose and
    // the work carries on, so the absence of an id is the contract.
    assert!(
        !cancel.contains("\"id\""),
        "cancel must be a notification: {cancel}"
    );
    assert!(cancel.contains("mock-acp-session"), "{cancel}");

    // The harness reported a normal stop reason; oneharness still records the
    // interrupt, because it records what IT did rather than what it was told.
    assert_eq!(report["control"]["mechanism"], "acp-cancel");
    assert_eq!(report["control"]["interrupts"][0]["outcome"], "served");
    assert_eq!(report["session"]["token"], "mock-acp-session");
    assert_eq!(report["results"][0]["session_id"], "mock-acp-session");
    assert_eq!(
        report["results"][0]["text_source"],
        "jsonrpc:acp-session-update"
    );

    let _ = std::fs::remove_dir_all(&store);
    let _ = std::fs::remove_dir_all(&cwd);
}

#[cfg(unix)]
#[test]
fn a_codex_controlled_run_drives_its_thread_and_interrupts_the_live_turn() {
    let mock_profile = mock_profile_redirect();
    // Codex's control is its own execution model: the turn runs over the
    // app-server's JSON-RPC protocol rather than `codex exec`, and the one
    // protocol fact that decides whether it works at all is that `turn/start`
    // answers immediately with an in-progress turn. A client reading that
    // response as the end of the turn ends every run in under half a second,
    // with nothing left to interrupt — so this drives the whole lifecycle
    // through the CLI and aborts it from a separate process.
    let store = control_store_dir("codex");
    let store_arg = store.display().to_string();
    let cwd = control_store_dir("codex-cwd");
    let cwd_arg = cwd.display().to_string();
    let log = store.join("app-server.log");

    let child = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        .env("MOCK_CODEX_APP_SERVER_LOG", log.display().to_string())
        .args([
            "run",
            "--harness",
            "codex",
            "--control",
            "--session",
            "codex",
            "--session-dir",
            &store_arg,
            "--cwd",
            &cwd_arg,
            "--mode",
            "bypass",
            "--prompt",
            "keep working",
            "--bin",
            &bin_override("codex"),
            "--compact",
            "--env",
            mock_profile.as_str(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn the codex controlled run");

    wait_until("the control socket", || {
        store.join("control").join("codex.sock").exists()
    });
    // The turn must be genuinely in flight before the interrupt, or the
    // assertions below would hold for a turn that had already ended.
    wait_until("the turn to start", || {
        std::fs::read_to_string(&log)
            .map(|text| text.contains("turn/start"))
            .unwrap_or(false)
    });

    let interrupt = run(
        &[
            "interrupt",
            "--session",
            "codex",
            "--session-dir",
            &store_arg,
            "--cwd",
            &cwd_arg,
            "--compact",
        ],
        &[],
    );
    assert!(interrupt.status.success(), "{interrupt:?}");
    assert_eq!(json_stdout(&interrupt)["mechanism"], "codex-app-server");

    let output = child.wait_with_output().expect("run did not finish");
    assert!(output.status.success(), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).expect("a JSON report");

    let served = std::fs::read_to_string(&log).unwrap();
    let abort = served
        .lines()
        .find(|line| line.contains("turn/interrupt"))
        .expect("an interrupt reached the app-server");
    // It addresses both coordinates: a thread alone names no turn to stop, and
    // the fixture refuses an interrupt that misses either.
    assert!(
        abort.contains("mock-codex-thread") && abort.contains("mock-codex-turn"),
        "the interrupt must name the thread and the turn: {abort}"
    );
    assert!(
        !served.contains("INTERRUPT_MISADDRESSED"),
        "the app-server rejected the interrupt's coordinates:\n{served}"
    );

    assert_eq!(report["control"]["mechanism"], "codex-app-server");
    assert_eq!(report["control"]["interrupts"][0]["outcome"], "served");
    // The turn's own signals come off the wire, not out of an output format.
    assert_eq!(report["session"]["token"], "mock-codex-thread");
    assert_eq!(report["results"][0]["session_id"], "mock-codex-thread");
    assert_eq!(report["results"][0]["text"], "still working");
    assert_eq!(
        report["results"][0]["text_source"],
        "jsonrpc:codex-app-server"
    );

    let _ = std::fs::remove_dir_all(&store);
    let _ = std::fs::remove_dir_all(&cwd);
}

#[cfg(unix)]
#[test]
fn a_restrictive_controlled_run_declines_the_permission_it_is_asked_for() {
    let mock_profile = mock_profile_redirect();
    // A driven turn negotiates its approvals on the wire, so the run's posture
    // reaches the harness only through what oneharness answers. Under a
    // restrictive mode that answer must decline — and it must still BE an
    // answer, because both ACP harnesses block forever on an unanswered
    // request, which would look like a slow harness rather than a denial.
    let store = control_store_dir("acp-deny");
    let store_arg = store.display().to_string();
    let cwd = control_store_dir("acp-deny-cwd");
    let cwd_arg = cwd.display().to_string();
    let acp_log = store.join("acp.log");

    let child = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        .env("MOCK_ACP_LOG", acp_log.display().to_string())
        .args([
            "run",
            "--harness",
            "copilot",
            "--control",
            "--session",
            "acp-deny",
            "--session-dir",
            &store_arg,
            "--cwd",
            &cwd_arg,
            "--mode",
            "default",
            "--prompt",
            "keep working",
            "--bin",
            &bin_override("copilot"),
            "--compact",
            "--env",
            mock_profile.as_str(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn the restrictive ACP controlled run");

    wait_until("the permission exchange", || {
        std::fs::read_to_string(&acp_log)
            .map(|log| log.contains("PERMISSION_ANSWERED"))
            .unwrap_or(false)
    });
    let answered = std::fs::read_to_string(&acp_log).unwrap();
    let reply = answered
        .lines()
        .find(|line| line.contains("outcome"))
        .expect("a permission answer reached the server");
    assert!(
        reply.contains("\"outcome\":\"cancelled\""),
        "a restrictive run must decline rather than select an option: {reply}"
    );
    assert!(
        !reply.contains("optionId"),
        "a declined permission selects nothing: {reply}"
    );

    // The turn is still open (the harness waits out a declined permission), so
    // end it the way a supervisor would rather than leaving the run hanging.
    let interrupt = run(
        &[
            "interrupt",
            "--session",
            "acp-deny",
            "--session-dir",
            &store_arg,
            "--cwd",
            &cwd_arg,
            "--compact",
        ],
        &[],
    );
    assert!(interrupt.status.success(), "{interrupt:?}");
    let output = child.wait_with_output().expect("run did not finish");
    assert!(output.status.success(), "{output:?}");

    let _ = std::fs::remove_dir_all(&store);
    let _ = std::fs::remove_dir_all(&cwd);
}

#[cfg(unix)]
#[test]
fn a_streamed_controlled_run_publishes_the_protocol_turns_own_signals() {
    let mock_profile = mock_profile_redirect();
    // Streaming and a turn-driving mechanism are two different sources of the
    // run's signals: the streamed envelope is assembled as the turn goes, while
    // the session id and text come off the protocol dialogue rather than the
    // harness's output format. A consumer reading the stream must still see the
    // dialogue's own answers — and the interrupt it asked for — on the wire.
    let store = control_store_dir("acp-stream");
    let store_arg = store.display().to_string();
    let cwd = control_store_dir("acp-stream-cwd");
    let cwd_arg = cwd.display().to_string();
    let acp_log = store.join("acp.log");

    let child = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        .env("MOCK_ACP_LOG", acp_log.display().to_string())
        .args([
            "run",
            "--harness",
            "copilot",
            "--control",
            "--stream",
            "--session",
            "acps",
            "--session-dir",
            &store_arg,
            "--cwd",
            &cwd_arg,
            "--mode",
            "bypass",
            "--prompt",
            "keep working",
            "--bin",
            &bin_override("copilot"),
            "--env",
            mock_profile.as_str(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn the streamed ACP controlled run");

    wait_until("the control socket", || {
        store.join("control").join("acps.sock").exists()
    });
    wait_until("the permission exchange", || {
        std::fs::read_to_string(&acp_log)
            .map(|log| log.contains("PERMISSION_ANSWERED"))
            .unwrap_or(false)
    });

    let interrupt = run(
        &[
            "interrupt",
            "--session",
            "acps",
            "--session-dir",
            &store_arg,
            "--cwd",
            &cwd_arg,
            "--compact",
        ],
        &[],
    );
    assert!(interrupt.status.success(), "{interrupt:?}");
    assert_eq!(json_stdout(&interrupt)["mechanism"], "acp-cancel");

    let output = child.wait_with_output().expect("run did not finish");
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    // Every published line is the typed envelope contract, and the terminal one
    // carries the report.
    let typed: Vec<RunStreamEnvelope> = lines
        .iter()
        .map(|l| serde_json::from_str(l).expect("each stream line matches the Rust contract"))
        .collect();
    assert!(
        matches!(typed.last(), Some(RunStreamEnvelope::Result { .. })),
        "the stream must end with a result envelope: {text}"
    );
    let last: Value = serde_json::from_str(lines.last().expect("a terminal line")).unwrap();
    let report = &last["report"];
    assert_eq!(report["control"]["mechanism"], "acp-cancel");
    assert_eq!(report["control"]["interrupts"][0]["outcome"], "served");
    // The signals a streamed run would otherwise read out of an output format
    // the ACP turn never produces.
    assert_eq!(report["session"]["token"], "mock-acp-session");
    assert_eq!(report["results"][0]["session_id"], "mock-acp-session");
    assert_eq!(
        report["results"][0]["text_source"],
        "jsonrpc:acp-session-update"
    );
    assert_eq!(
        report["results"][0]["text"],
        "Info: Operation cancelled by user"
    );

    let _ = std::fs::remove_dir_all(&store);
    let _ = std::fs::remove_dir_all(&cwd);
}

#[cfg(unix)]
#[test]
fn an_http_controlled_run_submits_the_turn_to_a_server_and_interrupts_it_there() {
    let mock_profile = mock_profile_redirect();
    // The third execution model, end to end through the real CLI: the harness
    // is never spawned as a run at all — oneharness leases its control server,
    // opens a session on it, answers what the server blocks on, and a SEPARATE
    // process aborts that same session.
    let store = control_store_dir("http");
    let store_arg = store.display().to_string();
    let cwd = control_store_dir("http-cwd");
    let cwd_arg = cwd.display().to_string();
    let log = store.join("server.log");
    let pool = store.join("pool");

    let child = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        .env("MOCK_HTTP_CONTROL_LOG", log.display().to_string())
        // A pool root inside the test's own temp tree, so it can never reuse or
        // disturb a real server on the developer's machine.
        .env("XDG_STATE_HOME", pool.display().to_string())
        .args([
            "run",
            "--harness",
            "opencode",
            "--control",
            "--session",
            "http",
            "--session-dir",
            &store_arg,
            "--cwd",
            &cwd_arg,
            "--mode",
            "bypass",
            "--prompt",
            "keep working",
            "--system",
            "preserve this controlled system prompt",
            "--bin",
            &bin_override("opencode"),
            "--compact",
            "--env",
            mock_profile.as_str(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn the HTTP controlled run");

    wait_until("the control socket", || {
        store.join("control").join("http.sock").exists()
    });
    // The server blocks on a permission decision; without an answer the turn
    // never does any work and every assertion below would be vacuous.
    wait_until("the permission exchange", || {
        std::fs::read_to_string(&log)
            .map(|text| text.contains("PERMISSION_ANSWERED"))
            .unwrap_or(false)
    });

    let interrupt = run(
        &[
            "interrupt",
            "--session",
            "http",
            "--session-dir",
            &store_arg,
            "--cwd",
            &cwd_arg,
            "--compact",
        ],
        &[],
    );
    assert!(interrupt.status.success(), "{interrupt:?}");
    assert_eq!(json_stdout(&interrupt)["mechanism"], "opencode-http");

    let output = child.wait_with_output().expect("run did not finish");
    assert!(output.status.success(), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).expect("a JSON report");

    let served = std::fs::read_to_string(&log).unwrap();
    // The interrupt went to the SESSION's own route on the server, not to any
    // process's stdin — there is no harness process here to write to.
    assert!(
        served
            .lines()
            .any(|line| line.starts_with("POST /api/session/ses_mock/interrupt")),
        "{served}"
    );
    assert!(
        served
            .lines()
            .any(|line| line.starts_with("POST /api/session/ses_mock/prompt")),
        "{served}"
    );
    let prompt_request = served
        .lines()
        .find(|line| line.starts_with("POST /api/session/ses_mock/prompt"))
        .expect("the prompt reached the control server");
    let system_at = prompt_request
        .find("preserve this controlled system prompt")
        .expect("the system prompt reached the server-driven turn");
    let prompt_at = prompt_request
        .find("keep working")
        .expect("the user prompt reached the server-driven turn");
    assert!(system_at < prompt_at, "{prompt_request}");

    assert_eq!(report["control"]["mechanism"], "opencode-http");
    assert_eq!(report["control"]["interrupts"][0]["outcome"], "served");
    // The turn's own signals: the server's session id and the text its stream
    // carried, plus the SERVER's launch argv as the command actually run.
    assert_eq!(report["session"]["token"], "ses_mock");
    assert_eq!(report["results"][0]["session_id"], "ses_mock");
    assert_eq!(report["results"][0]["text"], "stopped");
    assert_eq!(report["results"][0]["text_source"], "http:opencode-http");
    let command = report["results"][0]["command"].as_array().unwrap();
    assert_eq!(command[1], "serve", "{command:?}");
    assert_eq!(command[2], "--port", "{command:?}");

    // The pooled server outlives the dispatch by design and shuts itself down
    // once the turn it served is over.
    assert!(
        wait_for_pooled_server_to_exit(&pool),
        "the pool recorded the server it started"
    );

    let _ = std::fs::remove_dir_all(&store);
    let _ = std::fs::remove_dir_all(&cwd);
}

/// Drive one controlled opencode run through a pooled server under `pool`,
/// interrupt it from a separate process, and leave the pool quiet again.
///
/// `extra` carries the per-turn settings the caller wants to vary. The run is
/// only allowed to finish once its server is gone, so the next call starts from
/// the same state this one did — otherwise "did the key widen?" would be
/// answered by whichever run happened to still hold a live server.
#[cfg(unix)]
fn pooled_controlled_run(
    pool: &std::path::Path,
    log: &std::path::Path,
    session: &str,
    store: &std::path::Path,
    cwd: &std::path::Path,
    extra: &[&str],
) {
    let mock_profile = mock_profile_redirect();
    let store_arg = store.display().to_string();
    let cwd_arg = cwd.display().to_string();
    let bin = bin_override("opencode");
    let mut args = vec![
        "run",
        "--harness",
        "opencode",
        "--control",
        "--session",
        session,
        "--session-dir",
        &store_arg,
        "--cwd",
        &cwd_arg,
        "--bin",
        &bin,
        "--compact",
        "--env",
        mock_profile.as_str(),
    ];
    args.extend_from_slice(extra);

    let served_before = std::fs::read_to_string(log)
        .map(|text| text.matches("PERMISSION_ANSWERED").count())
        .unwrap_or(0);
    let child = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        .env("MOCK_HTTP_CONTROL_LOG", log.display().to_string())
        .env("XDG_STATE_HOME", pool.display().to_string())
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn the pooled controlled run");

    wait_until("the control socket", || {
        store
            .join("control")
            .join(format!("{session}.sock"))
            .exists()
    });
    // The turn only does work once the server's permission ask is answered, so
    // interrupting before that would abort a turn that never started.
    wait_until("the permission exchange", || {
        std::fs::read_to_string(log)
            .map(|text| text.matches("PERMISSION_ANSWERED").count() > served_before)
            .unwrap_or(false)
    });

    let interrupt = run(
        &[
            "interrupt",
            "--session",
            session,
            "--session-dir",
            &store_arg,
            "--cwd",
            &cwd_arg,
            "--compact",
        ],
        &[],
    );
    assert!(interrupt.status.success(), "{interrupt:?}");

    let output = child.wait_with_output().expect("run did not finish");
    assert!(output.status.success(), "{output:?}");
    assert!(
        wait_for_pooled_server_to_exit(pool),
        "the pool recorded the server it started"
    );
}

#[cfg(unix)]
#[test]
fn per_turn_settings_do_not_widen_the_pool_key() {
    // Two dispatches differing ONLY in what is negotiated per turn — working
    // directory, model, permission mode, prompt, system prompt, timeout — must
    // land on the same pool entry. The entry's directory name IS the key, so a
    // second one would mean every dispatch starts its own ~137MB server and the
    // pool buys nothing; the key is allowed to widen only on the harness id and
    // the `key_env` its `ServerSpec` declares.
    let pool = control_store_dir("pool-key-root");
    let store = control_store_dir("pool-key-store");
    let first_cwd = control_store_dir("pool-key-cwd-a");
    let second_cwd = control_store_dir("pool-key-cwd-b");
    let log = pool.join("server.log");

    pooled_controlled_run(
        &pool,
        &log,
        "key-one",
        &store,
        &first_cwd,
        &[
            "--prompt",
            "first turn",
            "--mode",
            "bypass",
            "--model",
            "one-model",
            "--system",
            "first system",
            "--timeout",
            "60",
        ],
    );
    pooled_controlled_run(
        &pool,
        &log,
        "key-two",
        &store,
        &second_cwd,
        &[
            "--prompt",
            "second turn",
            "--mode",
            "default",
            "--model",
            "another-model",
            "--system",
            "second system",
            "--timeout",
            "90",
        ],
    );

    let entries: Vec<String> = pool
        .join("oneharness")
        .join("servers")
        .read_dir()
        .expect("the pool root the dispatches used")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        entries.len(),
        1,
        "per-turn settings forked the pool: {entries:?}"
    );

    for dir in [&pool, &store, &first_cwd, &second_cwd] {
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(unix)]
#[test]
fn the_control_server_fixture_refuses_a_body_length_it_will_not_reserve_room_for() {
    // The fixture is a real HTTP server reading real sockets, so the number in
    // `Content-Length` is external input that sizes an allocation. Unbounded,
    // the peer decides how much memory it commits and then how long it blocks
    // waiting for a body that never comes — and a fixture that hangs reads as
    // the feature under test being broken.
    use std::io::{BufRead, BufReader, Write};
    let store = control_store_dir("mock-length");
    let log = store.join("server.log");
    let port = {
        let probe = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        probe.local_addr().unwrap().port()
    };
    // Reaped on the way out however this test ends: an assertion below fires
    // between the spawn and the teardown, and a fixture left running would
    // hold its port against the rest of the suite.
    struct Reaped(std::process::Child);
    impl Drop for Reaped {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let _server = Reaped(
        Command::new(mock_bin())
            .env("MOCK_HTTP_CONTROL_LOG", log.display().to_string())
            .args(["serve", "--port", &port.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to spawn the mock control server"),
    );

    let dial = || {
        for _ in 0..200 {
            if let Ok(stream) = std::net::TcpStream::connect(("127.0.0.1", port)) {
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                    .unwrap();
                return stream;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        panic!("the mock control server never accepted a connection");
    };
    let status_of = |mut stream: std::net::TcpStream, request: String| -> String {
        stream.write_all(request.as_bytes()).unwrap();
        stream.flush().unwrap();
        let mut line = String::new();
        BufReader::new(&stream)
            .read_line(&mut line)
            .expect("the mock control server never answered");
        line.trim().to_string()
    };

    // Four gigabytes declared, one byte sent.
    let refused = status_of(
        dial(),
        format!(
            "POST /api/session HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 4294967296\r\n\r\n{{"
        ),
    );
    assert!(
        refused.starts_with("HTTP/1.1 400"),
        "a length no body could satisfy was accepted: {refused}"
    );

    // A port that parses but names no address is refused before anything binds.
    // `0` asks the kernel to pick, and the caller goes on dialing the port that
    // is written down — so the fixture would be listening where nobody looks.
    let refused_port = Command::new(mock_bin())
        .env("MOCK_HTTP_CONTROL_LOG", log.display().to_string())
        .args(["serve", "--port", "0"])
        .output()
        .expect("failed to run the mock control server");
    assert!(!refused_port.status.success(), "{refused_port:?}");
    assert!(
        String::from_utf8_lossy(&refused_port.stderr).contains("is not a dialable port"),
        "{}",
        String::from_utf8_lossy(&refused_port.stderr)
    );

    // A header line with no ending in it: the same hazard one level up, where
    // the peer picks how much is accumulated before anything is validated.
    let long_header = status_of(
        dial(),
        format!(
            "POST /api/session HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Pad: {}",
            "p".repeat(32 * 1024)
        ),
    );
    assert!(
        long_header.starts_with("HTTP/1.1 400"),
        "an unterminated header line was accumulated: {long_header}"
    );

    // And a request line that never ends, which is the same again one level up.
    let long_line = status_of(
        dial(),
        format!("POST /api/session/{} HTTP/1.1\r\n", "x".repeat(32 * 1024)),
    );
    assert!(
        long_line.starts_with("HTTP/1.1 400"),
        "an unterminated request line was accumulated: {long_line}"
    );

    // And the server is still there: refusing one framing must not cost it the
    // next caller, or the bound would just be a different way of falling over.
    let served = status_of(
        dial(),
        format!(
            "POST /api/session HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 2\r\n\r\n{{}}"
        ),
    );
    assert!(served.starts_with("HTTP/1.1 200"), "{served}");

    let _ = std::fs::remove_dir_all(&store);
}

#[cfg(unix)]
#[test]
fn a_control_server_that_redirects_the_interrupt_is_not_reported_as_having_served_it() {
    let mock_profile = mock_profile_redirect();
    // The route answers `302 Found` and aborts nothing. This client follows no
    // redirect, so the turn is still running — and a supervisor told `served`
    // would walk away from a turn that never stopped, which is strictly worse
    // than being told the interrupt failed.
    let store = control_store_dir("http-redirect");
    let store_arg = store.display().to_string();
    let cwd = control_store_dir("http-redirect-cwd");
    let cwd_arg = cwd.display().to_string();
    let log = store.join("server.log");
    let pool = store.join("pool");

    let child = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
        .env("MOCK_HTTP_CONTROL_LOG", log.display().to_string())
        .env("MOCK_HTTP_CONTROL_FAULT", "redirect-interrupt")
        .env("XDG_STATE_HOME", pool.display().to_string())
        .args([
            "run",
            "--harness",
            "opencode",
            "--control",
            "--session",
            "redirect",
            "--session-dir",
            &store_arg,
            "--cwd",
            &cwd_arg,
            "--mode",
            "bypass",
            "--prompt",
            "keep working",
            "--bin",
            &bin_override("opencode"),
            "--timeout",
            "60",
            "--compact",
            "--env",
            mock_profile.as_str(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn the HTTP controlled run");

    wait_until("the control socket", || {
        store.join("control").join("redirect.sock").exists()
    });
    // Without the permission answer the turn never begins, and interrupting a
    // turn that never started would pass for the wrong reason.
    wait_until("the permission exchange", || {
        std::fs::read_to_string(&log)
            .map(|text| text.contains("PERMISSION_ANSWERED"))
            .unwrap_or(false)
    });

    let interrupt = run(
        &[
            "interrupt",
            "--session",
            "redirect",
            "--session-dir",
            &store_arg,
            "--cwd",
            &cwd_arg,
            "--compact",
        ],
        &[],
    );
    let answer = json_stdout(&interrupt);
    assert_eq!(answer["ok"], false, "{answer}");
    assert_eq!(interrupt.status.code(), Some(1), "{interrupt:?}");
    let error = answer["error"].as_str().expect("a reason");
    assert!(
        error.contains("302"),
        "the reason must carry the server's own answer: {error}"
    );

    let output = child.wait_with_output().expect("run did not finish");
    let report: Value = serde_json::from_slice(&output.stdout).expect("a JSON report");
    // Recorded as refused, and the turn's own end says the same: the server
    // never sent the aborted turn's `stopped` text, because it never aborted.
    assert_eq!(report["control"]["interrupts"][0]["outcome"], "refused");
    assert!(report["results"][0]["text"].is_null(), "{report}");

    let served = std::fs::read_to_string(&log).unwrap();
    assert!(
        served
            .lines()
            .any(|line| line.starts_with("POST /api/session/ses_mock/interrupt")),
        "the interrupt never reached the server:\n{served}"
    );

    wait_for_pooled_server_to_exit(&pool);
    let _ = std::fs::remove_dir_all(&store);
    let _ = std::fs::remove_dir_all(&cwd);
}

/// Wait for the server the pool under `pool` started to be gone, reporting
/// whether there was one to wait for.
///
/// This waits for the PROCESS, not merely for its port to close: a detached
/// process still finishing its exit is one whose coverage profile is still
/// being written, and a profile half-written while the coverage merge reads the
/// directory corrupts the whole run's data. A pool holding no record has
/// nothing to wait for — a server that died before it ever listened is already
/// reclaimed by the time the dispatch releases its lease.
#[cfg(unix)]
fn wait_for_pooled_server_to_exit(pool: &std::path::Path) -> bool {
    let Some(record) = pool
        .join("oneharness")
        .join("servers")
        .read_dir()
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("server.json"))
        .find(|path| path.exists())
    else {
        return false;
    };
    let pid: u32 = serde_json::from_str::<Value>(&std::fs::read_to_string(&record).unwrap())
        .unwrap()["pid"]
        .as_u64()
        .expect("a recorded pid") as u32;
    wait_until("the pooled server process to exit", || {
        !std::path::Path::new(&format!("/proc/{pid}")).exists()
            && std::process::Command::new("kill")
                .args(["-0", &pid.to_string()])
                .output()
                .map(|out| !out.status.success())
                .unwrap_or(true)
    });
    true
}

/// Drive a controlled opencode run against a control server broken the way
/// `fault` names, and hand back its output, its report, and the server's own
/// request log.
///
/// Most of these are a failure the run must report as DATA — a server that
/// never comes up or a session that cannot be opened is `spawn_error` with the
/// reason in `error`, exactly like a harness that could not be spawned — so the
/// turn never starts, the run is synchronous, and no interrupt is sent.
#[cfg(unix)]
fn http_control_run_with_fault(tag: &str, fault: &str, timeout: &str) -> (Output, Value, String) {
    let store = control_store_dir(tag);
    let store_arg = store.display().to_string();
    let cwd = control_store_dir(&format!("{tag}-cwd"));
    let cwd_arg = cwd.display().to_string();
    let pool = store.join("pool");
    let output = run(
        &[
            "run",
            "--harness",
            "opencode",
            "--control",
            "--session",
            tag,
            "--session-dir",
            &store_arg,
            "--cwd",
            &cwd_arg,
            "--mode",
            "bypass",
            "--prompt",
            "keep working",
            "--bin",
            &bin_override("opencode"),
            "--timeout",
            timeout,
            "--compact",
        ],
        &[
            (
                "MOCK_HTTP_CONTROL_LOG",
                store.join("server.log").display().to_string().as_str(),
            ),
            ("MOCK_HTTP_CONTROL_FAULT", fault),
            ("XDG_STATE_HOME", pool.display().to_string().as_str()),
        ],
    );
    let report = json_stdout(&output);
    wait_for_pooled_server_to_exit(&pool);
    let served = std::fs::read_to_string(store.join("server.log")).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&store);
    let _ = std::fs::remove_dir_all(&cwd);
    (output, report, served)
}

#[cfg(unix)]
#[test]
fn a_control_server_that_never_answers_is_reported_rather_than_waited_out() {
    // The pool started a process and it died before binding anything. The run
    // must say so within the budget its caller set — a bring-up that outlasts
    // `--timeout` is a hang as far as that caller is concerned.
    let started = std::time::Instant::now();
    let (output, report, _) = http_control_run_with_fault("http-unready", "never-ready", "5");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let result = &report["results"][0];
    assert_eq!(result["status"], "spawn-error", "{report}");
    let error = result["error"].as_str().expect("a reason");
    assert!(
        error.contains("did not answer within 5s"),
        "the reason must name the readiness wait: {error}"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(60),
        "the readiness wait outlasted the run's own timeout"
    );
    // Nothing ran, so nothing is claimed: no session token, no answer text.
    assert!(report["session"]["token"].is_null(), "{report}");
    assert!(result["text"].is_null(), "{report}");
}

#[cfg(unix)]
#[test]
fn a_control_server_that_refuses_the_session_reports_its_refusal() {
    // The server is up and answers, but will not open a session — so there is
    // no turn to interrupt and nothing to drive.
    let (output, report, _) = http_control_run_with_fault("http-refused", "refuse-session", "30");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let result = &report["results"][0];
    assert_eq!(result["status"], "spawn-error", "{report}");
    let error = result["error"].as_str().expect("a reason");
    assert!(
        error.contains("could not open the control session")
            && error.contains("503")
            && error.contains("no provider configured"),
        "the reason must carry the server's own refusal: {error}"
    );
    // No turn ran, so no signal is invented for one.
    assert!(result["session_id"].is_null(), "{report}");
    assert!(result["text"].is_null(), "{report}");
}

#[cfg(unix)]
#[test]
fn a_control_server_that_names_no_session_is_refused_rather_than_guessed_at() {
    // A 200 with no id in it: the answer parsed, but named nothing to address.
    // Addressing a guessed id would send the prompt — and later the interrupt —
    // at a route belonging to some other turn, or to none.
    let (output, report, _) = http_control_run_with_fault("http-noid", "no-session-id", "30");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let result = &report["results"][0];
    assert_eq!(result["status"], "spawn-error", "{report}");
    let error = result["error"].as_str().expect("a reason");
    assert!(
        error.contains("answered no usable id"),
        "the reason must name the unusable answer: {error}"
    );
    assert!(result["session_id"].is_null(), "{report}");
}

#[cfg(unix)]
#[test]
fn a_control_server_that_refuses_the_prompt_reports_its_refusal() {
    // Session creation succeeded, but no turn exists unless the prompt itself
    // was admitted. Drive the real HTTP boundary so a refusal cannot be lost
    // behind an event stream that will never announce a turn.
    let (output, report, served) =
        http_control_run_with_fault("http-prompt-refused", "refuse-prompt", "30");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let result = &report["results"][0];
    assert_eq!(result["status"], "nonzero", "{report}");
    let error = result["error"].as_str().expect("a reason");
    assert!(
        error.contains("refused the prompt")
            && error.contains("503")
            && error.contains("model unavailable"),
        "the reason must carry the prompt route's refusal: {error}"
    );
    assert!(
        served
            .lines()
            .any(|line| line.starts_with("POST /api/session/ses_mock/prompt")),
        "the prompt request never reached the server:\n{served}"
    );
    assert_eq!(result["session_id"], "ses_mock", "{report}");
    assert!(result["text"].is_null(), "{report}");
}

#[cfg(unix)]
#[test]
fn a_control_server_that_refuses_event_subscription_reports_it_at_the_cli() {
    let (output, report, _) =
        http_control_run_with_fault("http-events-refused", "refuse-events", "30");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let result = &report["results"][0];
    assert_eq!(result["status"], "nonzero", "{report}");
    assert_eq!(
        result["error"], "the control server refused the event subscription (503)",
        "{report}"
    );
}

#[cfg(unix)]
#[test]
fn an_unreadable_event_subscription_reports_its_framing_error_at_the_cli() {
    let (output, report, _) =
        http_control_run_with_fault("http-events-unreadable", "unreadable-events", "30");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let result = &report["results"][0];
    assert_eq!(result["status"], "nonzero", "{report}");
    let error = result["error"].as_str().expect("a framing reason");
    assert!(
        error.contains("event subscription cannot be read")
            && error.contains("two different Content-Length values"),
        "{error}"
    );
}

#[cfg(unix)]
#[test]
fn a_control_server_that_never_answers_the_prompt_cannot_outlast_the_run_budget() {
    // The prompt is sent on a worker so the event stream can be consumed at
    // the same time. Joining that worker must not turn a one-second run budget
    // into the HTTP client's otherwise fixed sixty-second wait.
    let started = std::time::Instant::now();
    let (output, report, served) =
        http_control_run_with_fault("http-prompt-hang", "hang-prompt", "1");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let result = &report["results"][0];
    assert_eq!(result["status"], "timeout", "{report}");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "a blocked prompt submission outlasted the run's own timeout"
    );
    assert!(
        served
            .lines()
            .any(|line| line.starts_with("POST /api/session/ses_mock/prompt")),
        "the blocked prompt request never reached the server:\n{served}"
    );
    // The worker is still blocked when the timeout is recorded, so no later
    // socket error is fabricated as though it had already been observed.
    assert!(result["error"].is_null(), "{report}");
}

#[cfg(unix)]
#[test]
fn a_control_server_that_stops_talking_mid_turn_is_not_reported_as_a_clean_finish() {
    // The stream ending is not the turn ending. A server that dies mid-flight
    // would otherwise hand a supervisor an `ok` for work that was cut short —
    // and unlike a timeout or a refusal, there is nothing else in the envelope
    // to notice it by.
    let (output, report, served) = http_control_run_with_fault("http-cutoff", "close-stream", "30");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let result = &report["results"][0];
    // The turn genuinely started, so this is a cut-off turn rather than one
    // that never began.
    assert!(
        served
            .lines()
            .any(|line| line.starts_with("POST /api/session/ses_mock/prompt")),
        "the turn never started:\n{served}"
    );
    assert_eq!(result["status"], "nonzero", "{report}");
    assert_eq!(
        result["error"], "the control server closed the event stream before the turn ended",
        "{report}"
    );
}

#[cfg(unix)]
#[test]
fn a_permission_ask_for_another_session_is_not_answered_by_a_controlled_run() {
    // The pooled server's event stream carries every dispatch's asks, so a
    // concurrent run's permission request lands on this turn's stream. Driven
    // through the CLI because the reply is a real request to a real route:
    // answering would spend this run's `--mode bypass` posture on a turn it
    // does not own.
    let (output, report, served) =
        http_control_run_with_fault("http-foreign", "foreign-permission", "30");
    assert!(output.status.success(), "{output:?}");
    let result = &report["results"][0];
    // The turn genuinely ran and the ask genuinely arrived — without both, the
    // assertion below would hold for the wrong reason.
    assert!(
        served
            .lines()
            .any(|line| line.starts_with("POST /api/session/ses_mock/prompt")),
        "the turn never started:\n{served}"
    );
    assert!(
        result["stdout"]
            .as_str()
            .unwrap_or_default()
            .contains("ses_intruder"),
        "the foreign ask never reached the run: {result}"
    );
    // Nothing answered it, and the run said why rather than skipping silently.
    assert!(
        !served.contains("PERMISSION_ANSWERED"),
        "a permission for another session was answered:\n{served}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ignored a permission request for another session (ses_intruder)"),
        "stderr:\n{stderr}"
    );
}

#[cfg(unix)]
#[test]
fn a_server_submitted_controlled_run_skips_a_harness_whose_binary_is_missing() {
    // "Never panic on a harness's behavior": a missing binary is `skipped` data
    // in the report. This path is the one place that could break the rule — an
    // unavailable harness never reaches the branch that assembles a prompt, so
    // the HTTP-control branch has no turn to submit and must decline to take
    // over rather than unwrap a prompt nobody built.
    let store = control_store_dir("http-missing");
    let output = run(
        &[
            "run",
            "--harness",
            "crush",
            "--control",
            "--session",
            "gone",
            "--session-dir",
            &store.display().to_string(),
            "--prompt",
            "keep working",
            "--bin",
            "crush=/no/such/oneharness-binary-xyz",
            "--compact",
        ],
        &[],
    );
    assert!(output.status.success(), "{output:?}");
    let report = json_stdout(&output);
    assert_eq!(report["results"][0]["status"], "skipped", "{report}");
    assert_eq!(report["results"][0]["available"], false, "{report}");
    // No turn ran, so no interrupt could have been served — and the socket the
    // run opened is gone with it.
    assert!(report["control"]["interrupts"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(!store.join("control").join("gone.sock").exists());

    let _ = std::fs::remove_dir_all(&store);
}

#[test]
fn a_dry_run_of_a_server_submitted_controlled_turn_shows_the_server_it_would_launch() {
    // `--print-command` answers "what would run". For these mechanisms that is
    // the harness's SERVER, never its CLI — printing `opencode run …` would name
    // a process this run never starts and whose interrupt was refuted.
    let store = control_store_dir("dry-http");
    let output = run(
        &[
            "run",
            "--harness",
            "opencode",
            "--control",
            "--session",
            "dry",
            "--session-dir",
            &store.display().to_string(),
            "--prompt",
            "hi",
            "--print-command",
            "--bin",
            &bin_override("opencode"),
            "--compact",
        ],
        &[],
    );
    assert!(output.status.success(), "{output:?}");
    let report = json_stdout(&output);
    let command: Vec<String> = report["results"][0]["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        command[1..],
        ["serve", "--port", "{address}"],
        "{command:?}"
    );
    assert_eq!(report["results"][0]["status"], "planned");
    // Nothing was launched and no socket opened: a dry run stays dry.
    assert!(!store.join("control").join("dry.sock").exists());

    // The same dry run on a stdin-borne mechanism still shows its CLI.
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--control",
            "--session",
            "dry2",
            "--session-dir",
            &store.display().to_string(),
            "--prompt",
            "hi",
            "--print-command",
            "--bin",
            &bin_override("claude-code"),
            "--compact",
        ],
        &[],
    );
    assert!(output.status.success(), "{output:?}");
    let command = json_stdout(&output)["results"][0]["command"].clone();
    let command = command.as_array().unwrap();
    assert!(
        command.iter().any(|arg| arg == "--input-format"),
        "{command:?}"
    );
    let _ = std::fs::remove_dir_all(&store);
}

#[test]
fn streaming_a_server_submitted_controlled_turn_is_a_usage_error() {
    // `--stream` publishes a spawned run's stdout line by line, and a turn
    // submitted to a control server has no such run. Refusing is what keeps the
    // combination from silently selecting the ordinary CLI run — whose
    // interrupt does not reach the turn, which is why this mechanism exists.
    let store = control_store_dir("stream-http");
    let output = run(
        &[
            "run",
            "--harness",
            "opencode",
            "--control",
            "--stream",
            "--session",
            "s",
            "--session-dir",
            &store.display().to_string(),
            "--prompt",
            "hi",
            "--bin",
            &bin_override("opencode"),
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("submits its controlled turn to a server"),
        "{stderr}"
    );
    assert!(stderr.contains("--stream"), "{stderr}");
    // A stdin-borne mechanism still streams: the refusal is about the execution
    // model, not about `--control`.
    let ok = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--control",
            "--stream",
            "--session",
            "s2",
            "--session-dir",
            &store.display().to_string(),
            "--prompt",
            "hi",
            "--print-command",
            "--bin",
            &bin_override("claude-code"),
        ],
        &[],
    );
    assert!(ok.status.success(), "{ok:?}");
    let _ = std::fs::remove_dir_all(&store);
}

#[test]
fn control_with_a_model_fan_out_is_a_usage_error() {
    // A fan-out multiplies the run into several (harness, model) units, and the
    // controlled path drives exactly one live turn — so there is no single turn
    // for a supervisor to address.
    let store = control_store_dir("fanout");
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--control",
            "--session",
            "fan",
            "--session-dir",
            &store.display().to_string(),
            "--model",
            "one",
            "--model",
            "two",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("claude-code"),
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("incompatible with --control"),
        "stderr:\n{stderr}"
    );
    let _ = std::fs::remove_dir_all(&store);
}
