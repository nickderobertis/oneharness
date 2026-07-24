//! End-to-end tests that drive the real `oneharness` binary the way a consumer
//! does, asserting on exit codes and the JSON contract. The subprocess path is
//! exercised hermetically through the `oneharness-mock-harness` fixture (a fake
//! harness wired in via `--bin`/env overrides), so these are deterministic,
//! network-free, and run identically on every platform.

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use oneharness_core::domain::history::{HistoryLine, HistoryStreamEnvelope};
use oneharness_core::domain::report::RunStreamEnvelope;
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
        assert_eq!(record["schema_version"], "1.0");
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
    assert_eq!(record["schema_version"], "1.0");
    for field in [
        "started_at",
        "model_ms",
        "tool_ms",
        "time_to_first_token_ms",
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
        let record = first_history_run(Path::new(report["history_file"].as_str().unwrap()));
        assert_eq!(record["schema_version"], "1.0", "{id}");
        assert!(record.get("model_ms").is_none(), "{id}");
        assert!(record.get("tool_ms").is_none(), "{id}");
        assert!(record.get("time_to_first_token_ms").is_none(), "{id}");
        if let Some(events) = record["events"].as_array() {
            for event in events.iter().filter(|event| event["kind"] == "tool_call") {
                assert!(event["started_at"].is_null(), "{id}");
                assert!(event["duration_ms"].is_null(), "{id}");
            }
        }
        let _ = std::fs::remove_dir_all(dir);
    }
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
        assert_eq!(record["schema_version"], "1.0", "{id}");
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
    assert_eq!(record["schema_version"], "1.0");

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
    assert_eq!(record["schema_version"], "1.0");
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
    assert_eq!(record["schema_version"], "1.0");
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
    use std::io::BufReader;

    let dir = hist_dir("interrupted-stream");
    let ds = dir.display().to_string();
    let lines: Vec<String> = (0..5)
        .map(|i| format!(r#"{{"type":"item.started","item":{{"id":"call-{i}","type":"command_execution","command":"step {i}","status":"in_progress"}}}}"#))
        .collect();
    let mut child = Command::new(oneharness_bin())
        .env("ONEHARNESS_NO_CONFIG", "1")
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
    assert_eq!(record["schema_version"], "1.0");
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
fn history_watch_filters_and_resumes_as_jsonl() {
    let dir = hist_dir("watch-cli");
    let ds = dir.display().to_string();
    let mut ids = Vec::new();
    for (name, graph) in [
        ("first", "release"),
        ("second", "release"),
        ("third", "other"),
    ] {
        let out = run(
            &[
                "run",
                "--harness",
                "codex",
                "--bin",
                &bin_override("codex"),
                "--prompt",
                name,
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
    let trigger = run(
        &[
            "run",
            "--harness",
            "codex",
            "--bin",
            &bin_override("codex"),
            "--prompt",
            "fourth",
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
    );
    assert!(trigger.status.success());
    let status = child.wait().unwrap();
    assert!(status.success(), "watch exit: {status:?}");

    let envelope: Value = serde_json::from_str(&line).unwrap();
    let typed: HistoryStreamEnvelope = serde_json::from_str(&line).unwrap();
    assert_eq!(envelope["type"], "record");
    assert_eq!(envelope["record"]["history_id"], ids[1]);
    assert_eq!(envelope["record"]["prompt"], "second");
    match typed {
        HistoryStreamEnvelope::Record { record } => {
            assert_eq!(record.history_id.to_string(), ids[1]);
            assert_eq!(record.prompt, "second");
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
    // harness-major then model-minor. Pinned with --print-command (no spawning).
    let output = run(
        &[
            "run",
            "--all",
            "--prompt",
            "hi",
            "--model",
            "a",
            "--model",
            "b",
            "--print-command",
            "--compact",
        ],
        &[],
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
    // shape is a loud usage error before anything spawns.
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

#[test]
fn fallback_refuses_incompatible_run_shapes() {
    // Fallback is single-outcome, so a batch, the low-level `--resume` continuation,
    // and `--stream` are loud usage errors (exit 2), each naming why. (The
    // higher-level `--session` handle is instead *allowed* — it binds to the
    // anchor; see `session_in_fallback_mode_anchors_to_the_first_session_capable_harness`.)
    let cases: &[(&[&str], &str)] = &[
        (&["--prompt", "a", "--prompt", "b"], "batch"),
        (&["--prompt", "a", "--resume", "sid"], "--resume"),
        (&["--prompt", "a", "--stream"], "--stream"),
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
