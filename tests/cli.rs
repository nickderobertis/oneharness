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

/// Run with config loading enabled but still hermetic: the user-level config is
/// pinned to `user_config` via ONEHARNESS_CONFIG (so the developer's real one is
/// never read), and project discovery is steered with `--cwd` by the caller.
fn run_with_config(args: &[&str], envs: &[(&str, &str)], user_config: &std::path::Path) -> Output {
    let mut cmd = Command::new(oneharness_bin());
    cmd.env("ONEHARNESS_CONFIG", user_config);
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
            "--compact",
        ],
        &[],
    );
    assert!(output.status.success());
    let value = json_stdout(&output);
    assert_eq!(value["dry_run"], true);

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
    assert!(command.contains("default"), "{command}");
    assert!(!command.contains("bypassPermissions"), "{command}");
    assert_eq!(value["bypass_permissions"], false);
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
        "cache_read_input_tokens":34}}"#;
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

#[test]
fn resume_on_unsupported_harness_is_a_usage_error() {
    let output = run(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "hi",
            "--resume",
            "sess-abc",
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("does not support --resume"), "{stderr}");
    assert!(stderr.contains("claude-code"), "{stderr}");
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
fn list_exposes_resume_capability() {
    let output = run(&["list", "--compact"], &[]);
    let value = json_stdout(&output);
    let harnesses = value["harnesses"].as_array().unwrap();
    let claude = harnesses.iter().find(|h| h["id"] == "claude-code").unwrap();
    let codex = harnesses.iter().find(|h| h["id"] == "codex").unwrap();
    assert_eq!(claude["supports_resume"], true);
    assert_eq!(codex["supports_resume"], false);
    let opencode = harnesses.iter().find(|h| h["id"] == "opencode").unwrap();
    let cursor = harnesses.iter().find(|h| h["id"] == "cursor").unwrap();
    assert_eq!(opencode["supports_resume"], true);
    assert_eq!(cursor["supports_resume"], true);
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
        "\"tokens\":{\"input\":671,\"output\":8}}}\n",
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
            .any(|w| w == ["--permission-mode", "default"]),
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
    assert_eq!(value["bypass"]["value"], true);
    assert_eq!(value["bypass"]["source"], "default");
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
