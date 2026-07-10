//! End-to-end tests that drive the real `oneharness` binary the way a consumer
//! does, asserting on exit codes and the JSON contract. The subprocess path is
//! exercised hermetically through the `oneharness-mock-harness` fixture (a fake
//! harness wired in via `--bin`/env overrides), so these are deterministic,
//! network-free, and run identically on every platform.

use std::path::PathBuf;
use std::process::{Command, Output};

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
    cmd.args(args);
    for (key, value) in envs {
        cmd.env(key, value);
    }
    cmd.output().expect("failed to run oneharness")
}

/// The `ONEHARNESS_*` env overrides recognized as a config layer. Cleared in
/// `run_with_config` so a developer's ambient value can never reshape a
/// config-layering assertion; a test that wants one passes it via `envs`.
const ENV_OVERRIDE_VARS: &[&str] = &[
    "ONEHARNESS_ALL",
    "ONEHARNESS_HARNESSES",
    "ONEHARNESS_EXCLUDE",
    "ONEHARNESS_MODEL",
    "ONEHARNESS_SYSTEM",
    "ONEHARNESS_BYPASS",
    "ONEHARNESS_TIMEOUT",
    "ONEHARNESS_OUTPUT_FORMAT",
    "ONEHARNESS_SCHEMA_FILE",
    "ONEHARNESS_SCHEMA_MAX_RETRIES",
    "ONEHARNESS_MAX_PARALLEL",
    "ONEHARNESS_REQUIRE_AVAILABLE",
    "ONEHARNESS_HISTORY",
    "ONEHARNESS_HISTORY_DIR",
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
    cmd.args(args);
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

#[test]
fn mock_fixture_is_built() {
    assert!(
        mock_bin().exists(),
        "mock harness not found at {}; run tests with `--features mock-harness` (e.g. `just test`)",
        mock_bin().display()
    );
}

#[test]
fn list_describes_every_harness() {
    let output = run(&["list"], &[]);
    assert!(output.status.success());
    let value = json_stdout(&output);
    assert_eq!(value["schema_version"], "0.1");
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
            &["exec", "--dangerously-bypass-approvals-and-sandbox", "hi"],
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
}

#[test]
fn stream_short_circuit_tears_down_the_child_when_the_consumer_closes() {
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
    assert_eq!(value["schema_version"], "0.1");
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

#[test]
fn history_records_a_run_and_reports_the_file() {
    let dir = hist_dir("record");
    let ds = dir.display().to_string();
    let output = run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "Fix the login bug",
            "--bin",
            &bin_override("claude-code"),
            "--history",
            "--history-dir",
            &ds,
            "--bypass",
            "--compact",
        ],
        &[("MOCK_STDOUT", r#"{"result":"done","session_id":"s1"}"#)],
    );
    assert!(output.status.success());
    let value = json_stdout(&output);
    // The report carries the absolute session-file path (the programmatic handle).
    let hf = value["history_file"].as_str().expect("history_file set");
    assert!(hf.ends_with(".jsonl"), "{hf}");
    // The file holds one normalized record with the prompt-derived name.
    let text = std::fs::read_to_string(hf).unwrap();
    let rec: Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    assert_eq!(rec["harness"], "claude-code");
    assert_eq!(rec["name"], "fix-the-login-bug");
    assert_eq!(rec["status"], "ok");
    assert_eq!(rec["session_id"], "s1");
    assert_eq!(rec["permission_mode"], "bypass");
    // Normalized only — no raw stdout/stderr leaks into history.
    assert!(rec.get("stdout").is_none());
    let _ = std::fs::remove_dir_all(&dir);
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
            "claude-code",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("claude-code"),
            "--history-dir",
            &ds,
            "--bypass",
            "--compact",
        ],
        &[("MOCK_STDOUT", r#"{"result":"hi"}"#)],
    ));
    assert!(v["history_file"].is_null());
    assert!(!dir.exists());
    // Explicit --no-history: still nothing (the override over config is proven in
    // `history_enabled_via_config_records_the_run`).
    let v = json_stdout(&run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("claude-code"),
            "--no-history",
            "--history-dir",
            &ds,
            "--bypass",
            "--compact",
        ],
        &[("MOCK_STDOUT", r#"{"result":"hi"}"#)],
    ));
    assert!(v["history_file"].is_null());
    assert!(!dir.exists());
    // --print-command executes nothing, so history is never written even with --history.
    let v = json_stdout(&run(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("claude-code"),
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
            "claude-code",
            "--prompt",
            "whatever",
            "--bin",
            &bin_override("claude-code"),
            "--history",
            "--history-dir",
            &ds,
            "--history-name",
            "My Release v2!",
            "--bypass",
            "--compact",
        ],
        &[("MOCK_STDOUT", r#"{"result":"hi"}"#)],
    ));
    let hf = v["history_file"].as_str().unwrap();
    // The label is slugified into the session id / filename.
    assert!(hf.contains("my-release-v2-"), "{hf}");
    let rec: Value =
        serde_json::from_str(std::fs::read_to_string(hf).unwrap().lines().next().unwrap()).unwrap();
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
            "claude-code",
            "--prompt",
            "whatever",
            "--bin",
            &bin_override("claude-code"),
            "--history",
            "--history-dir",
            &ds,
            "--history-name",
            "My Session",
            "--bypass",
            "--compact",
        ],
        &[("MOCK_STDOUT", r#"{"result":"hi"}"#)],
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
    assert_eq!(list[0]["harnesses"][0], "claude-code");

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
    assert_eq!(show[0]["harness"], "claude-code");

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
    assert!(String::from_utf8_lossy(&last_text.stdout).contains("[claude-code]"));

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
            "claude-code",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("claude-code"),
            "--bypass",
            "--compact",
        ],
        &[("MOCK_STDOUT", r#"{"result":"hi"}"#)],
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
            "claude-code",
            "--prompt",
            "hi",
            "--bin",
            &bin_override("claude-code"),
            "--no-history",
            "--bypass",
            "--compact",
        ],
        &[("MOCK_STDOUT", r#"{"result":"hi"}"#)],
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
            "claude-code,codex",
            "--bin",
            &bin_override("claude-code"),
            "--bin",
            &bin_override("codex"),
            "--prompt",
            "multi",
            "--history",
            "--history-dir",
            &ds,
            "--bypass",
            "--compact",
        ],
        &[("MOCK_STDOUT", r#"{"result":"x"}"#)],
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
        ["claude-code", "codex"],
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
        serde_json::json!(["claude-code", "codex"])
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
            "claude-code",
            "--bin",
            &bin_override("claude-code"),
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
        &[("MOCK_STDOUT", r#"{"result":"x"}"#)],
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
            "claude-code",
            "--bin",
            &bin_override("claude-code"),
            "--prompt",
            "stream test",
            "--stream",
            "--history",
            "--history-dir",
            &ds,
            "--bypass",
        ],
        &[("MOCK_STDOUT", r#"{"type":"result","result":"streamed"}"#)],
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
    assert_eq!(list[0]["harnesses"][0], "claude-code");
    let _ = std::fs::remove_dir_all(&dir);
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
            "claude-code",
            "--bin",
            &bin_override("claude-code"),
            "--prompt",
            "env test",
            "--bypass",
            "--compact",
        ],
        &[
            ("MOCK_STDOUT", r#"{"result":"x"}"#),
            ("ONEHARNESS_HISTORY", "1"),
            ("ONEHARNESS_HISTORY_DIR", &ds),
        ],
        &fixture.user_config(),
    );
    let hf = json_stdout(&out)["history_file"]
        .as_str()
        .map(str::to_string);
    let hf = hf.expect("env should enable history");
    assert!(
        hf.starts_with(&ds),
        "ONEHARNESS_HISTORY_DIR should place the store: {hf}"
    );
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
            "claude-code",
            "--bin",
            &bin_override("claude-code"),
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
            ("MOCK_STDOUT", ""),
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
                "claude-code",
                "--bin",
                &bin_override("claude-code"),
                "--prompt",
                "x",
                "--history",
                "--history-dir",
                &ds,
                "--bypass",
                "--compact",
            ],
            &[("MOCK_STDOUT", r#"{"result":"x"}"#)],
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
    assert_eq!(just_a[0]["project"], pa.display().to_string());
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&pa);
    let _ = std::fs::remove_dir_all(&pb);
}
