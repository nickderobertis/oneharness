//! End-to-end journeys for the settings that keep a long continued session
//! cheap: `auto_compact_tokens`, the lean tool set (`tool_set`, `load_skills`,
//! `load_mcp`), `images`, per-call `calls`, and `oneharness compact`.
//!
//! Each drives the real `oneharness` binary with the mock harness standing in
//! for the harness process, and asserts what that process received (its argv,
//! environment and stdin, recorded by the mock) or what the report says.
//!
//! Why the harness process is the mock, stated once for every journey's
//! `e2e_not_mocked` suppression below: the deterministic gate cannot run the
//! paid, authenticated CLIs, so only that process is replaced, at the existing
//! `--bin` seam. What holds each double to its real harness is the capture
//! `domain::lean`'s translation table cites, which the session double replays —
//! Claude Code: `tests/fixtures/claude-stream-json-turn.jsonl` and
//! `claude-compact-fork.jsonl`; codex 0.159.2: `codex-app-server-fork-compact.jsonl`
//! and `codex-rollout-token-counts.jsonl`; OpenCode: `opencode_run.jsonl`; the
//! rest: the documentation their table rows cite. The drift gate against the
//! real harnesses is `oh_lean_session_enforce` in `scripts/e2e-lib.sh`, run by
//! `just live-claude` and `just live-codex`: it drives these settings through
//! the real CLIs and fails when a release stops honouring a knob (the tools,
//! skills and MCP servers Claude Code reports loading, the image the model
//! sees, per-call usage, a compaction starting below `auto_compact_tokens`,
//! and `oneharness compact`). The other harnesses' rows are checked only
//! against the documentation they cite.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use oneharness_core::io::scratch::ScratchDir;
use serde_json::Value;

#[path = "../../../tests/profile-scratch/profile_scratch.rs"]
mod profile_scratch;
use profile_scratch::ProfileScratch;

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

// llmlint: ignore-block[e2e_not_mocked] These journeys must drive the real built `oneharness` against the repository's mock harness standing in for each harness's process: that is this change's stated acceptance bar, since the paid, authenticated CLIs cannot run in the deterministic gate. Only the harness subprocess is replaced, at the existing `--bin` seam, and it replays output captured from the real CLIs (tests/fixtures); the live suites hold the same paths to the real harnesses.
fn mock() -> String {
    workspace_bin("oneharness-mock-harness")
        .display()
        .to_string()
}

fn bin(id: &str) -> String {
    format!("{id}={}", mock())
}
// llmlint: ignore-end[e2e_not_mocked]

/// One invocation of the binary. `config` pins the user config (and lets
/// project discovery run); `None` is hermetic (`ONEHARNESS_NO_CONFIG=1`).
/// Every `run`/`compact` child gets the coverage-profile redirect, because a
/// cancelled or timed-out mock never flushes its profile.
fn oh(args: &[&str], envs: &[(&str, &str)], config: Option<&Path>) -> Output {
    let profiles = ProfileScratch::new();
    let mut cmd = Command::new(workspace_bin("oneharness"));
    // Every ambient `ONEHARNESS_*` override first, so the machine's own
    // settings never reshape an assertion; a journey that wants one passes it
    // in `envs`. Stripped before the config selection below, or an ambient
    // `ONEHARNESS_CONFIG`/`ONEHARNESS_NO_CONFIG` would remove the helper's own.
    for (var, _) in std::env::vars().filter(|(var, _)| var.starts_with("ONEHARNESS_")) {
        cmd.env_remove(var);
    }
    match config {
        Some(user) => {
            cmd.env("ONEHARNESS_CONFIG", user);
        }
        None => {
            cmd.env("ONEHARNESS_NO_CONFIG", "1");
        }
    }
    let mut argv: Vec<&str> = args.to_vec();
    if matches!(args.first(), Some(&"run") | Some(&"compact")) {
        argv.push("--env");
        argv.push(profiles.as_str());
    }
    if !argv.iter().any(|a| *a == "--format") && args.first() != Some(&"mock-harness") {
        argv.push("--format");
        argv.push("json");
    }
    cmd.args(&argv);
    for (key, value) in envs {
        cmd.env(key, value);
    }
    cmd.output().expect("run oneharness")
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|err| {
        panic!(
            "stdout was not JSON ({err}):\n{}\n--- stderr ---\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|a| {
            a.iter()
                .map(|s| s.as_str().unwrap_or_default().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// Whether `argv` holds `items` as one contiguous run.
fn has_seq(argv: &[String], items: &[&str]) -> bool {
    argv.windows(items.len())
        .any(|w| w.iter().zip(items).all(|(a, b)| a == b))
}

fn recorded(path: &Path) -> (Vec<String>, String) {
    let v: Value =
        serde_json::from_slice(&std::fs::read(path).expect("the mock recorded its input")).unwrap();
    (
        strings(&v["argv"]),
        v["stdin"].as_str().unwrap_or_default().to_string(),
    )
}

fn recorded_env(path: &Path) -> serde_json::Map<String, Value> {
    serde_json::from_slice::<Value>(&std::fs::read(path).expect("the mock recorded its env"))
        .unwrap()
        .as_object()
        .cloned()
        .unwrap()
}

fn diagnostics_for<'a>(result: &'a Value, setting: &str) -> Vec<&'a Value> {
    result["diagnostics"]
        .as_array()
        .map(|d| d.iter().filter(|x| x["setting"] == setting).collect())
        .unwrap_or_default()
}

fn list() -> Value {
    let out = oh(&["list"], &[], None);
    assert!(out.status.success(), "{out:?}");
    json(&out)
}

fn supports(list: &Value, id: &str, flag: &str) -> bool {
    list["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["id"] == id)
        .unwrap_or_else(|| panic!("{id} listed"))[flag]
        .as_bool()
        .unwrap_or_else(|| panic!("{id} {flag} is a boolean"))
}

/// A project dir with an `oneharness.toml` (and its `extends` parent).
struct Project {
    dir: ScratchDir,
}

impl Project {
    fn new(tag: &str, project: &str, parent: &str) -> Self {
        let dir = ScratchDir::new(&format!("lean-{tag}")).unwrap();
        std::fs::write(dir.join("oneharness.toml"), project).unwrap();
        std::fs::write(dir.join("parent.toml"), parent).unwrap();
        std::fs::write(dir.join("user.toml"), "").unwrap();
        Self { dir }
    }

    fn cwd(&self) -> String {
        self.dir.display().to_string()
    }

    fn user(&self) -> PathBuf {
        self.dir.join("user.toml")
    }
}

/// The argv `run --print-command` builds for `id` under the project's config.
fn printed(p: &Project, id: &str, flags: &[&str], envs: &[(&str, &str)]) -> Vec<String> {
    let cwd = p.cwd();
    let mut args = vec![
        "run",
        "--harness",
        id,
        "--prompt",
        "hi",
        "--cwd",
        &cwd,
        "--print-command",
    ];
    args.extend_from_slice(flags);
    let out = oh(&args, envs, Some(&p.user()));
    assert!(out.status.success(), "{out:?}");
    strings(&json(&out)["results"][0]["command"])
}

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its translation run; only the harness process (claude-code, codex) is the mock harness, held to its real CLI by the capture it cites and `live-claude`, `live-codex` (module doc).
#[test]
fn each_setting_layers_like_timeout_through_extends_env_and_flag() {
    // auto_compact_tokens, read off codex's own override.
    let compact = |argv: &[String]| {
        argv.iter()
            .find_map(|a| a.strip_prefix("model_auto_compact_token_limit="))
            .map(str::to_string)
    };
    let only_parent = Project::new(
        "layers-parent",
        "extends = \"parent.toml\"\n",
        "auto_compact_tokens = 2000\n",
    );
    assert_eq!(
        compact(&printed(&only_parent, "codex", &[], &[])).as_deref(),
        Some("2000")
    );
    let file = Project::new(
        "layers-file",
        "extends = \"parent.toml\"\nauto_compact_tokens = 1000\n",
        "auto_compact_tokens = 2000\n",
    );
    assert_eq!(
        compact(&printed(&file, "codex", &[], &[])).as_deref(),
        Some("1000")
    );
    let env = [("ONEHARNESS_AUTO_COMPACT_TOKENS", "3000")];
    assert_eq!(
        compact(&printed(&file, "codex", &[], &env)).as_deref(),
        Some("3000")
    );
    assert_eq!(
        compact(&printed(
            &file,
            "codex",
            &["--auto-compact-tokens", "4000"],
            &env
        ))
        .as_deref(),
        Some("4000")
    );
    let none = Project::new("layers-none", "", "");
    assert_eq!(
        compact(&printed(&none, "codex", &[], &[])),
        None,
        "absent changes nothing"
    );

    // tool_set, load_skills and load_mcp, read off claude-code's flags.
    let tools = |argv: &[String]| {
        let at = argv.iter().position(|a| a == "--tools")?;
        Some(
            argv[at + 1..]
                .iter()
                .take_while(|a| !a.starts_with('-'))
                .cloned()
                .collect::<Vec<_>>(),
        )
    };
    let lean = Project::new(
        "layers-lean",
        "extends = \"parent.toml\"\nload_mcp = false\n",
        "tool_set = [\"read\"]\nload_skills = false\nload_mcp = true\n",
    );
    let argv = printed(&lean, "claude-code", &[], &[]);
    assert_eq!(
        tools(&argv),
        Some(vec!["Read".to_string()]),
        "from the extends parent"
    );
    assert!(
        argv.iter().any(|a| a == "--disable-slash-commands"),
        "parent load_skills"
    );
    assert!(
        argv.iter().any(|a| a == "--strict-mcp-config"),
        "the file beats its parent"
    );
    let env = [
        ("ONEHARNESS_TOOL_SET", "shell,read"),
        ("ONEHARNESS_LOAD_SKILLS", "true"),
        ("ONEHARNESS_LOAD_MCP", "true"),
    ];
    let argv = printed(&lean, "claude-code", &[], &env);
    assert_eq!(
        tools(&argv),
        Some(vec!["Bash".to_string(), "Read".to_string()])
    );
    assert!(
        !argv.iter().any(|a| a == "--disable-slash-commands"),
        "env beats the files"
    );
    assert!(!argv.iter().any(|a| a == "--strict-mcp-config"));
    let argv = printed(
        &lean,
        "claude-code",
        &["--tool-set", "shell", "--no-skills", "--no-mcp"],
        &env,
    );
    assert_eq!(
        tools(&argv),
        Some(vec!["Bash".to_string()]),
        "the flag beats env"
    );
    assert!(argv.iter().any(|a| a == "--disable-slash-commands"));
    assert!(argv.iter().any(|a| a == "--strict-mcp-config"));
    let argv = printed(&none, "claude-code", &[], &[]);
    for flag in ["--tools", "--disable-slash-commands", "--strict-mcp-config"] {
        assert!(
            !argv.iter().any(|a| a == flag),
            "absent adds no {flag}: {argv:?}"
        );
    }

    // replace_system, read off which system flag claude-code receives.
    let replaced = |argv: &[String]| {
        let replacing = argv.iter().any(|a| a == "--system-prompt-file");
        let appending = argv.iter().any(|a| a == "--append-system-prompt-file");
        assert_ne!(replacing, appending, "exactly one system flag: {argv:?}");
        replacing
    };
    let parent = Project::new(
        "layers-replace-parent",
        "extends = \"parent.toml\"\nsystem = \"S\"\n",
        "replace_system = true\n",
    );
    assert!(
        replaced(&printed(&parent, "claude-code", &[], &[])),
        "from the extends parent"
    );
    let child = Project::new(
        "layers-replace-file",
        "extends = \"parent.toml\"\nsystem = \"S\"\nreplace_system = false\n",
        "replace_system = true\n",
    );
    assert!(
        !replaced(&printed(&child, "claude-code", &[], &[])),
        "the file beats its parent"
    );
    let on = [("ONEHARNESS_REPLACE_SYSTEM", "true")];
    assert!(
        replaced(&printed(&child, "claude-code", &[], &on)),
        "env beats the files"
    );
    let off = [("ONEHARNESS_REPLACE_SYSTEM", "false")];
    assert!(!replaced(&printed(&parent, "claude-code", &[], &off)));
    assert!(
        replaced(&printed(&child, "claude-code", &["--replace-system"], &off)),
        "the flag beats env"
    );
    let unset = Project::new("layers-replace-none", "system = \"S\"\n", "");
    assert!(
        !replaced(&printed(&unset, "claude-code", &[], &[])),
        "absent appends"
    );

    // The config report attributes each to its layer.
    let cwd = file.cwd();
    let out = oh(&["config", "--cwd", &cwd], &[], Some(&file.user()));
    let report = json(&out);
    assert_eq!(report["auto_compact_tokens"]["value"], 1000);
    let cwd = parent.cwd();
    let report = json(&oh(&["config", "--cwd", &cwd], &[], Some(&parent.user())));
    assert_eq!(report["replace_system"]["value"], true);
    assert!(report["replace_system"]["source"]
        .as_str()
        .unwrap()
        .ends_with("parent.toml"));
}
// llmlint: ignore-end[e2e_not_mocked]

#[test]
fn an_out_of_range_setting_is_refused_naming_it() {
    for (toml, named) in [
        ("auto_compact_tokens = 0\n", "auto_compact_tokens"),
        ("auto_compact_tokens = -5\n", "auto_compact_tokens"),
        ("auto_compact_tokens = \"lots\"\n", "auto_compact_tokens"),
        ("auto_compact_tokens = 1.5\n", "auto_compact_tokens"),
        ("tool_set = [\"shell\", \"edit\"]\n", "edit"),
        ("load_skills = \"no\"\n", "load_skills"),
        ("load_mcp = 0\n", "load_mcp"),
        ("replace_system = \"yes\"\n", "replace_system"),
    ] {
        let p = Project::new("refused", toml, "");
        let cwd = p.cwd();
        let out = oh(
            &[
                "run",
                "--harness",
                "codex",
                "--prompt",
                "hi",
                "--cwd",
                &cwd,
                "--print-command",
            ],
            &[],
            Some(&p.user()),
        );
        assert_eq!(out.status.code(), Some(2), "{toml}: {out:?}");
        assert!(stderr(&out).contains(named), "{toml}: {}", stderr(&out));
    }
    let none = Project::new("refused-env", "", "");
    let cwd = none.cwd();
    for (var, value) in [
        ("ONEHARNESS_AUTO_COMPACT_TOKENS", "0"),
        ("ONEHARNESS_AUTO_COMPACT_TOKENS", "-1"),
        ("ONEHARNESS_AUTO_COMPACT_TOKENS", "many"),
        ("ONEHARNESS_TOOL_SET", "shell,web"),
        ("ONEHARNESS_LOAD_SKILLS", "maybe"),
        ("ONEHARNESS_LOAD_MCP", "2"),
        ("ONEHARNESS_REPLACE_SYSTEM", "sometimes"),
    ] {
        let out = oh(
            &[
                "run",
                "--harness",
                "codex",
                "--prompt",
                "hi",
                "--cwd",
                &cwd,
                "--print-command",
            ],
            &[(var, value)],
            Some(&none.user()),
        );
        assert_eq!(out.status.code(), Some(2), "{var}={value}: {out:?}");
        assert!(stderr(&out).contains(var), "{var}: {}", stderr(&out));
    }
    for flag in [
        "--auto-compact-tokens=0",
        "--auto-compact-tokens=-3",
        "--auto-compact-tokens=ten",
        "--tool-set=shell,write",
    ] {
        let out = oh(
            &[
                "run",
                "--harness",
                "codex",
                "--prompt",
                "hi",
                "--print-command",
                flag,
            ],
            &[],
            None,
        );
        assert_eq!(out.status.code(), Some(2), "{flag}: {out:?}");
        let name = flag.split('=').next().unwrap();
        assert!(stderr(&out).contains(name), "{flag}: {}", stderr(&out));
    }
}

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its translation run; only the harness process (claude-code, codex, opencode, the other adapters) is the mock harness, held to its real CLI by the capture it cites and `live-claude`, `live-codex`, `live-opencode`, their `live-*` suites (module doc).
/// Every lean setting on, against each harness in turn: what its process
/// receives, and the diagnostic for each it cannot apply.
#[test]
fn each_harness_receives_its_own_translation_of_the_lean_settings() {
    let scratch = ScratchDir::new("lean-delivery").unwrap();
    let codex_home = scratch.join("codex-home");
    std::fs::create_dir_all(&codex_home).unwrap();
    std::fs::write(
        codex_home.join("config.toml"),
        "[mcp_servers.docs]\ncommand = \"docs-server\"\n",
    )
    .unwrap();
    let home_env = format!("CODEX_HOME={}", codex_home.display());
    let listed = list();
    for id in ALL_IDS {
        let input = scratch.join(format!("{id}.input.json"));
        let env_file = scratch.join(format!("{id}.env.json"));
        let input_env = format!("MOCK_INPUT_FILE={}", input.display());
        let env_env = format!("MOCK_ENV_FILE={}", env_file.display());
        let b = bin(id);
        let out = oh(
            &[
                "run",
                "--harness",
                id,
                "--prompt",
                "hi",
                "--bin",
                &b,
                "--mode",
                "bypass",
                "--auto-compact-tokens",
                "90000",
                "--tool-set",
                "shell,read",
                "--no-skills",
                "--no-mcp",
                "--env",
                &input_env,
                "--env",
                &env_env,
                "--env",
                &home_env,
            ],
            &[],
            None,
        );
        assert!(out.status.success(), "{id}: {out:?}");
        let report = json(&out);
        let result = &report["results"][0];
        let (argv, _) = recorded(&input);
        let env = recorded_env(&env_file);
        match *id {
            "claude-code" => {
                assert_eq!(
                    env["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "123000",
                    "N + 33000"
                );
                assert!(!env.contains_key("CLAUDE_AUTOCOMPACT_PCT_OVERRIDE"));
                assert!(has_seq(&argv, &["--tools", "Bash", "Read"]), "{argv:?}");
                assert_eq!(argv.iter().filter(|a| *a == "--tools").count(), 1);
                assert!(argv.iter().any(|a| a == "--disable-slash-commands"));
                assert!(argv.iter().any(|a| a == "--strict-mcp-config"));
                assert!(
                    !argv.iter().any(|a| a == "--mcp-config"),
                    "no server is loaded"
                );
            }
            "codex" => {
                for setting in [
                    "model_auto_compact_token_limit=90000",
                    "features.goals=false",
                    "features.view_image=false",
                    "features.sleep_tool=false",
                    "web_search=\"disabled\"",
                    "skills.include_instructions=false",
                    "mcp_servers.docs={command=\"true\",enabled=false}",
                    "features.apps=false",
                ] {
                    assert!(has_seq(&argv, &["-c", setting]), "{setting}: {argv:?}");
                }
                let partial = diagnostics_for(result, "tool_set");
                assert_eq!(partial.len(), 1, "{result}");
                let message = partial[0]["message"].as_str().unwrap();
                for tool in [
                    "apply_patch",
                    "request_user_input",
                    "collaboration",
                    "clock__curr_time",
                ] {
                    assert!(
                        message.contains(tool),
                        "names {tool} as still loaded: {message}"
                    );
                }
            }
            _ => {
                let joined = argv.join(" ");
                for knob in ["--tools", "slash", "mcp", "compact", "skills"] {
                    assert!(!joined.contains(knob), "{id} received {knob}: {argv:?}");
                }
                assert!(!env.keys().any(|k| k.contains("COMPACT")), "{id}");
            }
        }
        for (setting, flag) in [
            ("auto_compact_tokens", "supports_auto_compact"),
            ("tool_set", "supports_tool_set"),
            ("load_skills", "supports_skills_off"),
            ("load_mcp", "supports_mcp_off"),
        ] {
            let reported = diagnostics_for(result, setting);
            assert_eq!(
                reported.is_empty(),
                supports(&listed, id, flag),
                "{id} {setting}: `{flag}` and the run's diagnostics agree"
            );
            for d in reported {
                assert_eq!(d["harness"], *id);
                assert!(d["message"].as_str().unwrap().contains(setting));
            }
        }
    }
}
// llmlint: ignore-end[e2e_not_mocked]

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its translation run; only the harness process (claude-code, codex) is the mock harness, held to its real CLI by the capture it cites and `live-claude`, `live-codex` (module doc).
#[test]
fn each_setting_reaches_only_its_own_harness_and_nothing_is_added_when_absent() {
    let scratch = ScratchDir::new("lean-only-own").unwrap();
    let file = |id: &str, kind: &str| scratch.join(format!("{id}.{kind}.json"));
    let mut toml = String::new();
    for id in ["claude-code", "codex"] {
        toml.push_str(&format!(
            "[harness.{id}.env]\nMOCK_INPUT_FILE = {:?}\nMOCK_ENV_FILE = {:?}\n",
            file(id, "input").display().to_string(),
            file(id, "env").display().to_string()
        ));
    }
    std::fs::write(scratch.join("oneharness.toml"), &toml).unwrap();
    std::fs::write(scratch.join("user.toml"), "").unwrap();
    let cwd = scratch.display().to_string();
    let user = scratch.join("user.toml");
    let (b1, b2) = (bin("claude-code"), bin("codex"));
    let base = [
        "run",
        "--harness",
        "claude-code,codex",
        "--run-mode",
        "parallel",
        "--prompt",
        "hi",
        "--cwd",
        &cwd,
        "--bin",
        &b1,
        "--bin",
        &b2,
        "--mode",
        "bypass",
    ];
    let mut lean = base.to_vec();
    lean.extend([
        "--auto-compact-tokens",
        "90000",
        "--tool-set",
        "shell",
        "--no-skills",
    ]);
    let out = oh(&lean, &[], Some(&user));
    assert!(out.status.success(), "{out:?}");
    let claude_env = recorded_env(&file("claude-code", "env"));
    let codex_env = recorded_env(&file("codex", "env"));
    let (claude_argv, _) = recorded(&file("claude-code", "input"));
    let (codex_argv, _) = recorded(&file("codex", "input"));
    assert!(claude_env.contains_key("CLAUDE_CODE_AUTO_COMPACT_WINDOW"));
    assert!(
        !codex_env.contains_key("CLAUDE_CODE_AUTO_COMPACT_WINDOW"),
        "claude's knob is claude's"
    );
    assert!(!claude_argv
        .join(" ")
        .contains("model_auto_compact_token_limit"));
    assert!(!claude_argv
        .join(" ")
        .contains("skills.include_instructions"));
    assert!(!codex_argv
        .iter()
        .any(|a| a == "--disable-slash-commands" || a == "--tools"));

    let out = oh(&base, &[], Some(&user));
    assert!(out.status.success(), "{out:?}");
    let claude_env = recorded_env(&file("claude-code", "env"));
    let (claude_argv, _) = recorded(&file("claude-code", "input"));
    let (codex_argv, _) = recorded(&file("codex", "input"));
    assert!(!claude_env.contains_key("CLAUDE_CODE_AUTO_COMPACT_WINDOW"));
    assert!(!claude_env.contains_key("CLAUDE_AUTOCOMPACT_PCT_OVERRIDE"));
    for flag in ["--tools", "--disable-slash-commands", "--strict-mcp-config"] {
        assert!(!claude_argv.iter().any(|a| a == flag), "{flag}");
    }
    let joined = codex_argv.join(" ");
    for key in [
        "model_auto_compact_token_limit",
        "skills.include_instructions",
        "features.goals",
        "mcp_servers",
    ] {
        assert!(!joined.contains(key), "{key}: {codex_argv:?}");
    }
    for r in json(&out)["results"].as_array().unwrap() {
        for setting in ["auto_compact_tokens", "tool_set", "load_skills", "load_mcp"] {
            assert!(diagnostics_for(r, setting).is_empty(), "{r}");
        }
    }
}
// llmlint: ignore-end[e2e_not_mocked]

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its translation run; only the harness process (claude-code) is the mock harness, held to its real CLI by the capture it cites and `live-claude` (module doc).
#[test]
fn a_small_auto_compact_on_claude_rides_the_window_floor_and_a_percentage() {
    let scratch = ScratchDir::new("lean-pct").unwrap();
    let env_file = scratch.join("env.json");
    let env_env = format!("MOCK_ENV_FILE={}", env_file.display());
    let b = bin("claude-code");
    let out = oh(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--bin",
            &b,
            "--auto-compact-tokens",
            "40000",
            "--env",
            &env_env,
        ],
        &[],
        None,
    );
    assert!(out.status.success(), "{out:?}");
    let env = recorded_env(&env_file);
    assert_eq!(env["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "100000");
    let pct: f64 = env["CLAUDE_AUTOCOMPACT_PCT_OVERRIDE"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        (80_000.0 * pct / 100.0).floor() as u64,
        40_000,
        "{pct}% of the 80000 effective window"
    );
}
// llmlint: ignore-end[e2e_not_mocked]

#[test]
fn claude_read_only_keeps_its_floor_under_a_lean_tool_set() {
    let out = oh(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--mode",
            "read-only",
            "--tool-set",
            "shell,read",
            "--print-command",
        ],
        &[],
        None,
    );
    assert!(out.status.success(), "{out:?}");
    let argv = strings(&json(&out)["results"][0]["command"]);
    let at = argv
        .iter()
        .position(|a| a == "--tools")
        .expect("one --tools list");
    let tools: Vec<&String> = argv[at + 1..]
        .iter()
        .take_while(|a| !a.starts_with('-'))
        .collect();
    assert_eq!(tools, ["Read"], "{argv:?}");
    assert_eq!(argv.iter().filter(|a| *a == "--tools").count(), 1);
    assert!(
        !argv.iter().any(|a| a == "Bash"),
        "read-only never loads the shell: {argv:?}"
    );
}

fn png(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    // A 1x1 PNG.
    let bytes: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52, 0, 0,
        0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0, 0x90, 0x77, 0x53, 0xde, 0, 0, 0, 12, 0x49, 0x44, 0x41,
        0x54, 0x08, 0xd7, 0x63, 0xf8, 0xcf, 0xc0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xdd,
        0x8d, 0xb0, 0, 0, 0, 0, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    std::fs::write(&path, bytes).unwrap();
    path
}

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its translation run; only the harness process (codex) is the mock harness, held to its real CLI by the capture it cites and `live-codex` (module doc).
#[test]
fn a_missing_or_unreadable_image_is_refused_before_any_harness_starts() {
    let scratch = ScratchDir::new("lean-image-missing").unwrap();
    let input = scratch.join("input.json");
    let input_env = format!("MOCK_INPUT_FILE={}", input.display());
    let b = bin("codex");
    let missing = scratch.join("gone.png").display().to_string();
    let text = scratch.join("notes.png");
    std::fs::write(&text, "not an image").unwrap();
    let text = text.display().to_string();
    for image in [missing.as_str(), text.as_str()] {
        let out = oh(
            &[
                "run",
                "--harness",
                "codex",
                "--prompt",
                "hi",
                "--bin",
                &b,
                "--image",
                image,
                "--env",
                &input_env,
            ],
            &[],
            None,
        );
        assert_eq!(out.status.code(), Some(2), "{out:?}");
        assert!(stderr(&out).contains(image), "{}", stderr(&out));
        assert!(!input.exists(), "no harness started");
    }
}
// llmlint: ignore-end[e2e_not_mocked]

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its translation run; only the harness process (claude-code, codex, opencode, the other adapters) is the mock harness, held to its real CLI by the capture it cites and `live-claude`, `live-codex`, `live-opencode`, their `live-*` suites (module doc).
#[test]
fn images_reach_each_harness_as_its_own_image_input() {
    let scratch = ScratchDir::new("lean-images").unwrap();
    let one = png(&scratch, "one.png");
    let two = png(&scratch, "two.png");
    let (one_s, two_s) = (one.display().to_string(), two.display().to_string());
    let listed = list();
    for id in ALL_IDS {
        let input = scratch.join(format!("{id}.json"));
        let input_env = format!("MOCK_INPUT_FILE={}", input.display());
        let b = bin(id);
        let out = oh(
            &[
                "run",
                "--harness",
                id,
                "--prompt",
                "describe",
                "--bin",
                &b,
                "--mode",
                "bypass",
                "--image",
                &one_s,
                "--image",
                &two_s,
                "--env",
                &input_env,
            ],
            &[],
            None,
        );
        let report = json(&out);
        let result = &report["results"][0];
        if !supports(&listed, id, "supports_images") {
            assert_eq!(result["status"], "skipped", "{id}");
            assert_eq!(result["failure_kind"], "unsupported_input", "{id}");
            assert!(result["error"].as_str().unwrap().contains("images"), "{id}");
            assert!(!input.exists(), "{id} was not started");
            continue;
        }
        assert!(out.status.success(), "{id}: {out:?}");
        let (argv, stdin) = recorded(&input);
        match *id {
            "claude-code" => {
                assert!(
                    has_seq(&argv, &["--input-format", "stream-json"]),
                    "{argv:?}"
                );
                assert!(
                    has_seq(&argv, &["--output-format", "stream-json"]),
                    "{argv:?}"
                );
                assert!(
                    !argv.iter().any(|a| a == "describe"),
                    "the prompt rides the frame"
                );
                let frame: Value = serde_json::from_str(stdin.trim()).unwrap();
                let content = frame["message"]["content"].as_array().unwrap();
                assert_eq!(content.len(), 3);
                let bytes = std::fs::read(&one).unwrap();
                assert_eq!(content[0]["type"], "image");
                assert_eq!(content[0]["source"]["media_type"], "image/png");
                assert_eq!(
                    content[0]["source"]["data"].as_str().unwrap().len(),
                    bytes.len().div_ceil(3) * 4
                );
                assert_eq!(content[2]["text"], "describe");
            }
            "codex" => {
                assert!(argv.contains(&format!("--image={one_s}")), "{argv:?}");
                assert!(argv.contains(&format!("--image={two_s}")), "{argv:?}");
            }
            "opencode" => {
                assert!(
                    has_seq(&argv, &["-f", &one_s]) && has_seq(&argv, &["-f", &two_s]),
                    "{argv:?}"
                );
            }
            "copilot" => {
                assert!(
                    has_seq(&argv, &["--attachment", &one_s])
                        && has_seq(&argv, &["--attachment", &two_s]),
                    "{argv:?}"
                );
            }
            other => panic!("{other} is listed as taking images but has no assertion here"),
        }
    }
}
// llmlint: ignore-end[e2e_not_mocked]

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its translation run; only the harness process (codex, the other adapters) is the mock harness, held to its real CLI by the capture it cites and `live-codex`, their `live-*` suites (module doc).
#[test]
fn a_fallback_chain_moves_past_a_harness_without_image_input() {
    let scratch = ScratchDir::new("lean-image-chain").unwrap();
    let image = png(&scratch, "frame.png").display().to_string();
    let (g, c) = (bin("goose"), bin("codex"));
    let out = oh(
        &[
            "run",
            "--harness",
            "goose,codex",
            "--prompt",
            "describe",
            "--bin",
            &g,
            "--bin",
            &c,
            "--mode",
            "bypass",
            "--image",
            &image,
        ],
        &[],
        None,
    );
    assert!(out.status.success(), "{out:?}");
    let report = json(&out);
    assert_eq!(report["fallback"]["ran"], "codex");
    let fell = &report["fallback"]["fell_through"][0];
    assert_eq!(fell["harness"], "goose");
    assert_eq!(fell["reason"], "unsupported-input");
    assert_eq!(report["results"][0]["failure_kind"], "unsupported_input");
    assert_eq!(report["results"][1]["status"], "ok");
}
// llmlint: ignore-end[e2e_not_mocked]

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its translation run; only the harness process (claude-code, codex, opencode, the other adapters) is the mock harness, held to its real CLI by the capture it cites and `live-claude`, `live-codex`, `live-opencode`, their `live-*` suites (module doc).
#[test]
fn calls_list_one_usage_per_model_call_where_the_harness_records_them() {
    let scratch = ScratchDir::new("lean-calls").unwrap();
    let claude_stream = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/claude-stream-json-turn.jsonl"),
    )
    .unwrap();
    let opencode_capture = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/opencode_run.jsonl"),
    )
    .unwrap();
    let listed = list();
    for id in ALL_IDS {
        let b = bin(id);
        let home = scratch.join(format!("{id}-home"));
        let home_env = format!("CODEX_HOME={}", home.display());
        let mut args = vec![
            "run",
            "--harness",
            id,
            "--prompt",
            "hi",
            "--bin",
            &b,
            "--mode",
            "bypass",
            "--env",
            &home_env,
        ];
        let mut envs: Vec<(&str, &str)> = Vec::new();
        match *id {
            "claude-code" => {
                args.extend(["--output-format", "stream-json"]);
                envs.push(("MOCK_STDOUT", &claude_stream));
            }
            "codex" => {
                args.extend(["--env", "MOCK_CODEX_SESSIONS=1"]);
            }
            "opencode" => envs.push(("MOCK_STDOUT", &opencode_capture)),
            _ => {}
        }
        let out = oh(&args, &envs, None);
        assert!(out.status.success(), "{id}: {out:?}");
        let result = json(&out)["results"][0].clone();
        if !supports(&listed, id, "supports_call_usage") {
            assert!(
                result.get("calls").is_none_or(Value::is_null),
                "{id}: {result}"
            );
            assert_eq!(diagnostics_for(&result, "calls").len(), 1, "{id}: {result}");
            continue;
        }
        let calls = result["calls"]
            .as_array()
            .unwrap_or_else(|| panic!("{id}: {result}"));
        let inputs: Vec<u64> = calls
            .iter()
            .map(|c| c["input_tokens"].as_u64().unwrap())
            .collect();
        match *id {
            "claude-code" => {
                assert_eq!(inputs, vec![10, 8], "two calls, in order");
                assert_eq!(calls[0]["cache_write_tokens"], 34_280);
                assert_eq!(calls[1]["cache_read_tokens"], 47_969);
            }
            "codex" => {
                assert_eq!(
                    inputs,
                    vec![13_006, 13_101, 13_180],
                    "three calls, in order"
                );
                assert_eq!(calls[2]["output_tokens"], 5);
            }
            "opencode" => {
                // The real single-call capture: one step_finish, one call.
                assert_eq!(inputs, vec![3], "one call");
                assert_eq!(calls[0]["output_tokens"], 7);
                assert_eq!(calls[0]["cache_write_tokens"], 8_176);
                assert_eq!(calls[0]["cache_read_tokens"], 0);
                assert_eq!(
                    result["usage"]["input_tokens"], 3,
                    "the run's usage total is unchanged"
                );
            }
            other => panic!("{other} is listed as carrying calls but has no replay here"),
        }
        assert!(diagnostics_for(&result, "calls").is_empty());
    }

    // claude-code's single-document format carries none, and says so.
    let b = bin("claude-code");
    let out = oh(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--bin",
            &b,
        ],
        &[],
        None,
    );
    let result = json(&out)["results"][0].clone();
    assert!(result.get("calls").is_none());
    assert!(diagnostics_for(&result, "calls")[0]["message"]
        .as_str()
        .unwrap()
        .contains("stream-json"));

    // An opencode turn whose steps carry no token figures says so.
    let b = bin("opencode");
    let out = oh(
        &[
            "run",
            "--harness",
            "opencode",
            "--prompt",
            "hi",
            "--bin",
            &b,
        ],
        &[(
            "MOCK_STDOUT",
            r#"{"type":"step_finish","sessionID":"ses_1","part":{"type":"step-finish","cost":0}}"#,
        )],
        None,
    );
    let result = json(&out)["results"][0].clone();
    assert!(result.get("calls").is_none(), "{result}");
    assert!(diagnostics_for(&result, "calls")[0]["message"]
        .as_str()
        .unwrap()
        .contains("step_finish"));

    // A step reporting a negative cost keeps its token figures; its cost is
    // unavailable rather than reported.
    let out = oh(
        &[
            "run",
            "--harness",
            "opencode",
            "--prompt",
            "hi",
            "--bin",
            &b,
        ],
        &[(
            "MOCK_STDOUT",
            r#"{"type":"step_finish","sessionID":"ses_1","part":{"type":"step-finish","tokens":{"input":5,"output":2},"cost":-0.01}}"#,
        )],
        None,
    );
    let result = json(&out)["results"][0].clone();
    assert_eq!(result["calls"][0]["output_tokens"], 2, "{result}");
    assert!(result["calls"][0]["cost_usd"].is_null(), "{result}");

    // A turn of several steps lists one usage per step, in order. These events
    // are composed here, in the captured event's shape, to give a multi-call
    // turn: no real multi-call OpenCode capture is in the repository yet.
    let steps = [(11, 3, 0.01), (13, 5, 0.02), (17, 7, 0.03)]
        .iter()
        .map(|(input, output, cost)| {
            format!(
                r#"{{"type":"step_finish","sessionID":"ses_1","part":{{"type":"step-finish","tokens":{{"input":{input},"output":{output}}},"cost":{cost}}}}}"#
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let out = oh(
        &[
            "run",
            "--harness",
            "opencode",
            "--prompt",
            "hi",
            "--bin",
            &b,
        ],
        &[("MOCK_STDOUT", &steps)],
        None,
    );
    let result = json(&out)["results"][0].clone();
    let calls = result["calls"].as_array().expect("one usage per step");
    let read: Vec<_> = calls
        .iter()
        .map(|c| (c["input_tokens"].clone(), c["output_tokens"].clone()))
        .collect();
    assert_eq!(
        read,
        [(11, 3), (13, 5), (17, 7)]
            .iter()
            .map(|(i, o)| (serde_json::json!(i), serde_json::json!(o)))
            .collect::<Vec<_>>(),
        "{result}"
    );
}
// llmlint: ignore-end[e2e_not_mocked]

struct Sessions {
    dir: ScratchDir,
}

impl Sessions {
    fn new(tag: &str) -> Self {
        let dir = ScratchDir::new(&format!("lean-compact-{tag}")).unwrap();
        for sub in ["proj", "store", "home"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        Self { dir }
    }

    fn path(&self, sub: &str) -> String {
        self.dir.join(sub).display().to_string()
    }

    /// The env a session-keeping mock of `id` needs.
    fn env(&self, id: &str) -> Vec<String> {
        match id {
            "claude-code" => vec![
                format!("CLAUDE_CONFIG_DIR={}", self.path("home")),
                "MOCK_CLAUDE_SESSIONS=1".to_string(),
            ],
            _ => vec![
                format!("CODEX_HOME={}", self.path("home")),
                "MOCK_CODEX_SESSIONS=1".to_string(),
            ],
        }
    }

    fn with_env<'a>(&'a self, args: &mut Vec<&'a str>, env: &'a [String]) {
        for e in env {
            args.push("--env");
            args.push(e);
        }
    }

    fn turn(&self, id: &str, prompt: &str) -> Value {
        let (proj, store, b) = (self.path("proj"), self.path("store"), bin(id));
        let env = self.env(id);
        let mut args = vec![
            "run",
            "--harness",
            id,
            "--session",
            "watch",
            "--session-dir",
            &store,
            "--cwd",
            &proj,
            "--bin",
            &b,
            "--mode",
            "bypass",
            "--prompt",
            prompt,
        ];
        self.with_env(&mut args, &env);
        let out = oh(&args, &[], None);
        assert!(out.status.success(), "{out:?}");
        json(&out)
    }

    fn compact(&self, id: &str, extra: &[&str]) -> Output {
        let (proj, store, b) = (self.path("proj"), self.path("store"), bin(id));
        let env = self.env(id);
        let mut args = vec![
            "compact",
            "--session",
            "watch",
            "--session-dir",
            &store,
            "--cwd",
            &proj,
            "--bin",
            &b,
        ];
        args.extend_from_slice(extra);
        self.with_env(&mut args, &env);
        oh(&args, &[], None)
    }

    /// Every file the harness's own store holds, with its bytes.
    fn stored(&self) -> Vec<(PathBuf, Vec<u8>)> {
        let mut out = Vec::new();
        let mut stack = vec![self.dir.join("home")];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    out.push((path.clone(), std::fs::read(path).unwrap()));
                }
            }
        }
        out.sort();
        out
    }
}

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its translation run; only the harness process (claude-code, codex) is the mock harness, held to its real CLI by the capture it cites and `live-claude`, `live-codex` (module doc).
#[test]
fn compact_hands_the_next_turn_the_compacted_context_on_claude_and_codex() {
    for id in ["claude-code", "codex"] {
        let s = Sessions::new(id);
        s.turn(id, "alpha");
        let before = s.turn(id, "beta");
        let original = before["session"]["token"].as_str().unwrap().to_string();
        let stored = s.stored();

        let argv_file = s.dir.join("argv");
        let argv_env = format!("MOCK_ARGV_FILE={}", argv_file.display());
        let out = s.compact(id, &["--env", &argv_env]);
        assert!(out.status.success(), "{id}: {out:?}");
        let outcome = json(&out);
        assert_eq!(outcome["status"], "completed", "{id}: {outcome}");
        assert_eq!(outcome["harness"], id);
        assert!(outcome["usage"].is_object());
        let fork = outcome["token"].as_str().unwrap().to_string();
        assert_ne!(fork, original, "{id}: the name now continues the fork");
        let argv = std::fs::read_to_string(&argv_file).unwrap();
        let argv: Vec<String> = argv.lines().map(str::to_string).collect();
        match id {
            "claude-code" => {
                assert!(has_seq(&argv, &["-p", "/compact"]), "{argv:?}");
                assert!(
                    has_seq(&argv, &["--resume", &original, "--fork-session"]),
                    "{argv:?}"
                );
                assert_eq!(outcome["context_tokens_before"], 6_809);
                assert_eq!(outcome["context_tokens_after"], 7_405);
                // The compaction's own figures (`model_usage_since_last_result`),
                // not the zeroed top-level usage or the session's running cost.
                assert_eq!(
                    outcome["usage"],
                    serde_json::json!({
                        "input_tokens": 58_305, "output_tokens": 1_538,
                        "cache_read_tokens": 0, "cache_write_tokens": 13_711,
                        "cost_usd": 0.008_313_375,
                    })
                );
            }
            _ => assert_eq!(
                argv.first().map(String::as_str),
                Some("app-server"),
                "{argv:?}"
            ),
        }
        for (path, bytes) in &stored {
            assert_eq!(
                &std::fs::read(path).unwrap(),
                bytes,
                "{id}: {} untouched",
                path.display()
            );
        }

        let after = s.turn(id, "gamma");
        assert_eq!(after["session"]["token"], fork.as_str());
        if id == "codex" {
            // The fork carries the original's recorded calls; the turn after
            // compaction reports only its own.
            assert_eq!(after["results"][0]["calls"].as_array().unwrap().len(), 3);
        }
        assert_eq!(
            after["results"][0]["text"], "context: [summary of: alpha, beta, gamma]",
            "{id}: the compacted context, not the original"
        );
    }
}
// llmlint: ignore-end[e2e_not_mocked]

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its translation run; only the harness process (claude-code, codex, opencode) is the mock harness, held to its real CLI by the capture it cites and `live-claude`, `live-codex`, `live-opencode` (module doc).
#[test]
fn compact_refuses_an_unknown_or_rebound_session_and_reports_a_harness_without_compaction() {
    let s = Sessions::new("refusals");
    let out = s.compact("claude-code", &[]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(stderr(&out).contains("watch"), "{}", stderr(&out));

    s.turn("claude-code", "alpha");
    let out = s.compact("claude-code", &["--harness", "codex"]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(
        stderr(&out).contains("claude-code") && stderr(&out).contains("codex"),
        "{}",
        stderr(&out)
    );

    // A session-capable harness with no compaction: opencode.
    let o = Sessions::new("opencode");
    let (proj, store, b) = (o.path("proj"), o.path("store"), bin("opencode"));
    let out = oh(
        &[
            "run",
            "--harness",
            "opencode",
            "--session",
            "watch",
            "--session-dir",
            &store,
            "--cwd",
            &proj,
            "--bin",
            &b,
            "--mode",
            "bypass",
            "--prompt",
            "hi",
        ],
        &[(
            "MOCK_STDOUT",
            r#"{"type":"text","sessionID":"ses_mock","part":{"type":"text","text":"ok"}}"#,
        )],
        None,
    );
    assert!(out.status.success(), "{out:?}");
    let out = oh(
        &[
            "compact",
            "--session",
            "watch",
            "--session-dir",
            &store,
            "--cwd",
            &proj,
            "--bin",
            &b,
        ],
        &[],
        None,
    );
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let outcome = json(&out);
    assert_eq!(outcome["status"], "unsupported");
    assert!(outcome["error"].as_str().unwrap().contains("opencode"));
}
// llmlint: ignore-end[e2e_not_mocked]

#[test]
fn list_reports_every_new_capability_for_every_adapter() {
    let listed = list();
    for h in listed["harnesses"].as_array().unwrap() {
        for flag in [
            "supports_auto_compact",
            "supports_compact_now",
            "supports_tool_set",
            "supports_skills_off",
            "supports_mcp_off",
            "supports_images",
            "supports_call_usage",
        ] {
            assert!(h[flag].is_boolean(), "{} {flag}", h["id"]);
        }
    }
    let claude = |flag| supports(&listed, "claude-code", flag);
    assert!(
        claude("supports_auto_compact") && claude("supports_tool_set") && claude("supports_images")
    );
    assert!(
        !supports(&listed, "codex", "supports_tool_set"),
        "codex applies it in part"
    );
    assert!(supports(&listed, "codex", "supports_compact_now"));
    assert!(!supports(&listed, "goose", "supports_images"));
    assert!(
        supports(&listed, "opencode", "supports_call_usage"),
        "opencode's step_finish events carry per-call figures"
    );
}

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its translation run; only the harness process (claude-code, codex) is the mock harness, held to its real CLI by the capture it cites and `live-claude`, `live-codex` (module doc).
/// Every way a compaction can fail leaves the session as it was: the outcome
/// says `failed` and why, the name still continues the original, and the
/// harness's own stored files are byte for byte unchanged.
#[test]
fn a_failed_compaction_answers_failed_and_leaves_the_session_as_it_was() {
    for (id, fault, said) in [
        ("claude-code", "refuse", "Not enough messages to compact."),
        ("claude-code", "exit", "exited with code 1"),
        ("claude-code", "nofork", "without naming a forked session"),
        (
            "claude-code",
            "nosuccess",
            "exited with code 0 before the compaction completed",
        ),
        (
            "claude-code",
            "answered",
            "exited with code 0 before the compaction completed; it answered the turn instead of compacting: I don't have a /compact command.",
        ),
        ("codex", "refuse", "compaction unavailable"),
        ("codex", "fail-turn", "the compaction request failed"),
        ("codex", "exit", "before the compaction completed"),
        ("codex", "exit-after", "reported the compaction complete"),
        (
            "codex",
            "hang-after",
            "reported the compaction complete, but the compaction exceeded its timeout",
        ),
    ] {
        let s = Sessions::new(&format!("fail-{id}-{fault}"));
        s.turn(id, "alpha");
        let before = s.turn(id, "beta");
        let original = before["session"]["token"].as_str().unwrap().to_string();
        let stored = s.stored();
        let fault_env = format!("MOCK_COMPACT_FAIL={fault}");
        // A server that hangs after completing ends only at a deadline.
        let out = s.compact(id, &["--env", &fault_env, "--timeout", "2"]);
        assert_eq!(out.status.code(), Some(1), "{id} {fault}: {out:?}");
        let outcome = json(&out);
        assert_eq!(outcome["status"], "failed", "{id} {fault}: {outcome}");
        assert!(
            outcome["error"].as_str().unwrap().contains(said),
            "{id} {fault}: {outcome}"
        );
        assert_eq!(outcome["token"], original.as_str());
        for (path, bytes) in &stored {
            assert_eq!(
                &std::fs::read(path).unwrap(),
                bytes,
                "{id} {fault}: {}",
                path.display()
            );
        }
        let after = s.turn(id, "gamma");
        assert_eq!(
            after["results"][0]["text"], "context: [alpha, beta, gamma]",
            "{id} {fault}"
        );
    }
}
// llmlint: ignore-end[e2e_not_mocked]

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its translation run; only the harness process (claude-code) is the mock harness, held to its real CLI by the capture it cites and `live-claude` (module doc).
/// A claude that answers `/compact` as a prompt fails the compaction quoting
/// at most 300 characters of what the model said, marked where it was cut.
#[test]
fn a_compact_answered_as_a_prompt_quotes_a_clipped_answer() {
    let s = Sessions::new("answered-long");
    s.turn("claude-code", "alpha");
    let answer = format!("{}{}", "é".repeat(300), "TAIL");
    let answer_env = format!("MOCK_COMPACT_ANSWER={answer}");
    let out = s.compact(
        "claude-code",
        &["--env", "MOCK_COMPACT_FAIL=answered", "--env", &answer_env],
    );
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let outcome = json(&out);
    assert_eq!(outcome["status"], "failed", "{outcome}");
    let error = outcome["error"].as_str().unwrap();
    let quoted = format!(
        "it answered the turn instead of compacting: {}…",
        "é".repeat(300)
    );
    assert!(error.ends_with(&quoted), "{error}");
    assert!(!error.contains("TAIL"), "{error}");
}
// llmlint: ignore-end[e2e_not_mocked]

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its translation run; only the harness process (claude-code) is the mock harness, held to its real CLI by the capture it cites and `live-claude` (module doc).
#[test]
fn a_compaction_that_cannot_start_answers_failed() {
    let s = Sessions::new("cannot-start");
    s.turn("claude-code", "alpha");
    let (proj, store) = (s.path("proj"), s.path("store"));
    let missing = format!("claude-code={}", s.dir.join("no-such-claude").display());
    let out = oh(
        &[
            "compact",
            "--session",
            "watch",
            "--session-dir",
            &store,
            "--cwd",
            &proj,
            "--bin",
            &missing,
        ],
        &[],
        None,
    );
    let outcome = json(&out);
    assert_eq!(outcome["status"], "failed", "{outcome}");
    assert!(
        outcome["error"].as_str().unwrap().contains("not found"),
        "{outcome}"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Executable, so it resolves; its interpreter does not exist, so
        // starting it fails (`exec` answers ENOENT, which nothing retries).
        let junk = s.dir.join("junk-claude");
        std::fs::write(&junk, b"#!/nonexistent/interpreter\n").unwrap();
        std::fs::set_permissions(&junk, std::fs::Permissions::from_mode(0o755)).unwrap();
        let junk = format!("claude-code={}", junk.display());
        let out = oh(
            &[
                "compact",
                "--session",
                "watch",
                "--session-dir",
                &store,
                "--cwd",
                &proj,
                "--bin",
                &junk,
            ],
            &[],
            None,
        );
        let outcome = json(&out);
        assert_eq!(outcome["status"], "failed", "{outcome}");
        assert!(
            outcome["error"]
                .as_str()
                .unwrap()
                .contains("could not be started"),
            "{outcome}"
        );
    }
}

#[cfg(unix)]
#[test]
fn a_compaction_whose_fork_cannot_be_bound_keeps_the_original() {
    use std::os::unix::fs::PermissionsExt;
    let s = Sessions::new("unbindable");
    s.turn("claude-code", "alpha");
    let before = s.turn("claude-code", "beta");
    let original = before["session"]["token"].as_str().unwrap().to_string();
    let record = PathBuf::from(before["session"]["store_file"].as_str().unwrap());
    let dir = record.parent().unwrap().to_path_buf();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    let out = s.compact("claude-code", &[]);
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    let outcome = json(&out);
    assert_eq!(outcome["status"], "failed", "{outcome}");
    assert!(
        outcome["error"]
            .as_str()
            .unwrap()
            .contains("could not be bound"),
        "{outcome}"
    );
    assert_eq!(outcome["token"], original.as_str());
    let after = s.turn("claude-code", "gamma");
    assert_eq!(after["session"]["token"], original.as_str());
}
// llmlint: ignore-end[e2e_not_mocked]

#[cfg(unix)]
fn alive(pid: &str) -> bool {
    Command::new("kill")
        .args(["-0", pid])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(unix)]
fn await_file(path: &Path) -> String {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        if !text.is_empty() {
            return text;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{} never appeared",
            path.display()
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its translation run; only the harness process (codex) is the mock harness, held to its real CLI by the capture it cites and `live-codex` (module doc).
#[cfg(unix)]
#[test]
fn a_timed_out_compaction_ends_the_harness_and_the_next_turn_continues_uncompacted() {
    let s = Sessions::new("timeout");
    s.turn("codex", "alpha");
    s.turn("codex", "beta");
    let begun = s.dir.join("begun");
    let begun_env = format!("MOCK_COMPACT_BEGUN={}", begun.display());
    let out = s.compact("codex", &["--timeout", "2", "--env", &begun_env]);
    let outcome = json(&out);
    assert_eq!(outcome["status"], "failed", "{outcome}");
    assert!(
        outcome["error"].as_str().unwrap().contains("timeout"),
        "{outcome}"
    );
    let pid = std::fs::read_to_string(&begun).unwrap();
    assert!(!alive(pid.trim()), "the harness {pid} was ended");
    let after = s.turn("codex", "gamma");
    assert_eq!(after["results"][0]["text"], "context: [alpha, beta, gamma]");
}
// llmlint: ignore-end[e2e_not_mocked]

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its translation run; only the harness process (claude-code) is the mock harness, held to its real CLI by the capture it cites and `live-claude` (module doc).
#[cfg(unix)]
#[test]
fn sigterm_cancels_a_compaction_from_the_cli_and_leaves_the_session_as_it_was() {
    let s = Sessions::new("sigterm");
    s.turn("claude-code", "alpha");
    let before = s.turn("claude-code", "beta");
    let original = before["session"]["token"].as_str().unwrap().to_string();
    let stored = s.stored();
    let begun = s.dir.join("begun");
    let profiles = ProfileScratch::new();
    let (proj, store, b) = (s.path("proj"), s.path("store"), bin("claude-code"));
    let mut cmd = Command::new(workspace_bin("oneharness"));
    cmd.env("ONEHARNESS_NO_CONFIG", "1")
        .args([
            "compact",
            "--session",
            "watch",
            "--session-dir",
            &store,
            "--cwd",
            &proj,
            "--bin",
            &b,
            "--format",
            "json",
        ])
        .args(["--env", profiles.as_str()])
        .args(["--env", &format!("MOCK_COMPACT_BEGUN={}", begun.display())])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    for e in s.env("claude-code") {
        cmd.args(["--env", &e]);
    }
    let child = cmd.spawn().unwrap();
    let harness = await_file(&begun);
    // Signal only the process this test started.
    let term = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(term.success());
    let out = child.wait_with_output().unwrap();
    let outcome = json(&out);
    assert_eq!(outcome["status"], "cancelled", "{outcome}");
    // A compaction that did not complete exits 1, so a caller's shell sees it.
    assert_eq!(out.status.code(), Some(1), "{outcome}");
    assert_eq!(outcome["token"], original.as_str());
    assert!(!alive(harness.trim()), "the harness {harness} was ended");
    for (path, bytes) in &stored {
        assert_eq!(&std::fs::read(path).unwrap(), bytes, "{}", path.display());
    }
}

#[test]
fn an_image_run_refuses_the_shapes_an_image_cannot_ride() {
    let scratch = ScratchDir::new("lean-image-conflicts").unwrap();
    let image = png(&scratch, "frame.png").display().to_string();
    let schema = scratch.join("schema.json");
    std::fs::write(&schema, r#"{"type":"object"}"#).unwrap();
    let schema = schema.display().to_string();
    let store = scratch.join("store").display().to_string();
    let b = bin("claude-code");
    for (flags, named) in [
        (
            vec!["--control", "--session", "s", "--session-dir", &store],
            "--control",
        ),
        (vec!["--output-format", "json"], "--output-format"),
        // A schema does not lift the conflict: an explicit non-stream format
        // is still one an image frame cannot ride.
        (
            vec!["--output-format", "json", "--schema", &schema],
            "--output-format",
        ),
    ] {
        let mut args = vec![
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--bin",
            &b,
            "--image",
            &image,
        ];
        args.extend(flags.iter().copied());
        let out = oh(&args, &[], None);
        assert_eq!(out.status.code(), Some(2), "{named}: {out:?}");
        assert!(stderr(&out).contains(named), "{named}: {}", stderr(&out));
        assert!(
            stderr(&out).contains("--image"),
            "{named}: {}",
            stderr(&out)
        );
    }
}

#[test]
fn an_empty_tool_set_loads_no_built_in_tool_and_a_huge_threshold_is_clamped_and_said() {
    let out = oh(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--tool-set",
            "",
            "--print-command",
        ],
        &[],
        None,
    );
    let argv = strings(&json(&out)["results"][0]["command"]);
    assert!(has_seq(&argv, &["--tools", ""]), "{argv:?}");

    let scratch = ScratchDir::new("lean-clamp").unwrap();
    let env_file = scratch.join("env.json");
    let env_env = format!("MOCK_ENV_FILE={}", env_file.display());
    let b = bin("claude-code");
    let out = oh(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--bin",
            &b,
            "--auto-compact-tokens",
            "2000000",
            "--env",
            &env_env,
        ],
        &[],
        None,
    );
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        recorded_env(&env_file)["CLAUDE_CODE_AUTO_COMPACT_WINDOW"],
        "1000000"
    );
    let result = json(&out)["results"][0].clone();
    let said = diagnostics_for(&result, "auto_compact_tokens");
    assert!(
        said[0]["message"].as_str().unwrap().contains("967000"),
        "{result}"
    );
}
// llmlint: ignore-end[e2e_not_mocked]

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its translation run; only the harness process (claude-code, codex) is the mock harness, held to its real CLI by the capture it cites and `live-claude`, `live-codex` (module doc).
#[test]
fn per_call_usage_reports_why_it_is_absent_and_reads_only_this_runs_calls() {
    let scratch = ScratchDir::new("lean-calls-absent").unwrap();
    let (c, x) = (bin("claude-code"), bin("codex"));
    let reason = |out: &Output| {
        let result = json(out)["results"][0].clone();
        assert!(result.get("calls").is_none(), "{result}");
        diagnostics_for(&result, "calls")[0]["message"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let out = oh(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--bin",
            &c,
            "--output-format",
            "stream-json",
        ],
        &[(
            "MOCK_STDOUT",
            r#"{"type":"result","subtype":"success","result":"x"}"#,
        )],
        None,
    );
    assert!(reason(&out).contains("no assistant usage"));

    let home = scratch.join("home");
    std::fs::create_dir_all(home.join("sessions")).unwrap();
    let home_env = format!("CODEX_HOME={}", home.display());
    let codex = |stdout: &str| {
        oh(
            &[
                "run",
                "--harness",
                "codex",
                "--prompt",
                "hi",
                "--bin",
                &x,
                "--mode",
                "bypass",
                "--env",
                &home_env,
            ],
            &[("MOCK_STDOUT", stdout)],
            None,
        )
    };
    let turn = r#"{"type":"item.completed","item":{"id":"i","type":"agent_message","text":"ok"}}"#;
    assert!(reason(&codex(turn)).contains("named no thread"));
    let named = format!("{{\"type\":\"thread.started\",\"thread_id\":\"t-1\"}}\n{turn}");
    assert!(reason(&codex(&named)).contains("no rollout record"));
    std::fs::write(
        home.join("sessions")
            .join("rollout-2026-11-02T00-00-00-t-1.jsonl"),
        "{\"type\":\"session_meta\",\"payload\":{}}\n",
    )
    .unwrap();
    assert!(reason(&codex(&named)).contains("holds no token_count"));

    // A resumed thread reports only the calls this run appended.
    let s = Sessions::new("calls-resumed");
    s.turn("codex", "alpha");
    let second = s.turn("codex", "beta");
    assert_eq!(second["results"][0]["calls"].as_array().unwrap().len(), 3);
}

/// `compact` without `--model`/`--timeout` takes them from configuration, and
/// runs the harness under its identity's own environment (a variant's `env`
/// added, its `unset_env` removed), exactly as `run` would.
#[test]
fn compact_resolves_the_configured_model_timeout_and_identity_environment() {
    let s = Sessions::new("configured");
    let proj = s.dir.join("proj");
    std::fs::write(
        proj.join("oneharness.toml"),
        "timeout = 2\n\
         [harness.codex]\nmodel = \"gpt-configured\"\n\
         [harness.claude-code]\nmodel = \"claude-configured\"\n\
         [harness.claude-code.variant.work]\nunset_env = [\"OH_LEAN_SECRET\"]\n\
         [harness.claude-code.variant.work.env]\nOH_LEAN_VARIANT = \"from-variant\"\n",
    )
    .unwrap();
    let user = s.dir.join("user.toml");
    std::fs::write(&user, "").unwrap();
    let (proj_s, store) = (s.path("proj"), s.path("store"));

    // codex: the fork runs under the configured model, and the configured
    // timeout ends a compaction that never finishes.
    s.turn("codex", "alpha");
    let log = s.dir.join("requests.jsonl");
    let begun = s.dir.join("begun");
    let b = bin("codex");
    let mut args = vec![
        "compact".to_string(),
        "--session".into(),
        "watch".into(),
        "--session-dir".into(),
        store.clone(),
        "--cwd".into(),
        proj_s.clone(),
        "--bin".into(),
        b,
        "--env".into(),
        format!("MOCK_COMPACT_LOG={}", log.display()),
        "--env".into(),
        format!("MOCK_COMPACT_BEGUN={}", begun.display()),
    ];
    for e in s.env("codex") {
        args.extend(["--env".to_string(), e]);
    }
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = oh(&args, &[], Some(&user));
    let outcome = json(&out);
    assert_eq!(outcome["status"], "failed", "{outcome}");
    assert!(
        outcome["error"].as_str().unwrap().contains("timeout"),
        "{outcome}"
    );
    let fork: Value = std::fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .find(|r| r["method"] == "thread/fork")
        .expect("the fork was requested");
    assert_eq!(fork["params"]["model"], "gpt-configured");

    // claude-code: a variant identity's session compacts under that
    // identity's environment and the configured model.
    let c = Sessions::new("configured-variant");
    let cproj = c.dir.join("proj");
    std::fs::copy(proj.join("oneharness.toml"), cproj.join("oneharness.toml")).unwrap();
    std::fs::write(c.dir.join("user.toml"), "").unwrap();
    let cuser = c.dir.join("user.toml");
    let (cp, cs, cb) = (c.path("proj"), c.path("store"), bin("claude-code"));
    let env = c.env("claude-code");
    let mut create = vec![
        "run",
        "--harness",
        "claude-code:work",
        "--session",
        "watch",
        "--session-dir",
        &cs,
        "--cwd",
        &cp,
        "--bin",
        &cb,
        "--mode",
        "bypass",
        "--prompt",
        "alpha",
    ];
    for e in &env {
        create.extend(["--env", e]);
    }
    let created = oh(&create, &[], Some(&cuser));
    assert!(created.status.success(), "{created:?}");
    let argv_file = c.dir.join("argv");
    let env_file = c.dir.join("env.json");
    let (argv_env, env_env) = (
        format!("MOCK_ARGV_FILE={}", argv_file.display()),
        format!("MOCK_ENV_FILE={}", env_file.display()),
    );
    let mut compact = vec![
        "compact",
        "--session",
        "watch",
        "--session-dir",
        &cs,
        "--cwd",
        &cp,
        "--bin",
        &cb,
        "--env",
        &argv_env,
        "--env",
        &env_env,
    ];
    for e in &env {
        compact.extend(["--env", e]);
    }
    let out = oh(
        &compact,
        &[("OH_LEAN_SECRET", "must-not-reach")],
        Some(&cuser),
    );
    let outcome = json(&out);
    assert_eq!(outcome["status"], "completed", "{outcome}");
    assert_eq!(outcome["harness"], "claude-code:work");
    let argv: Vec<String> = std::fs::read_to_string(&argv_file)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect();
    assert!(
        has_seq(&argv, &["--model", "claude-configured"]),
        "{argv:?}"
    );
    let env = recorded_env(&env_file);
    assert_eq!(env["OH_LEAN_VARIANT"], "from-variant");
    assert!(
        !env.contains_key("OH_LEAN_SECRET"),
        "the variant's unset_env applies"
    );
}
// llmlint: ignore-end[e2e_not_mocked]

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its translation run; only the harness process (codex) is the mock harness, held to its real CLI by the capture it cites and `live-codex` (module doc).
/// `--no-mcp` on codex disables every server its configuration would load:
/// the home config's and each `.codex/config.toml` from the working
/// directory upward, each named once.
#[test]
fn no_mcp_disables_codex_servers_from_home_and_every_ancestor_project() {
    let scratch = ScratchDir::new("lean-mcp-ancestors").unwrap();
    let home = scratch.join("home");
    let project = scratch.join("project");
    let work = project.join("sub");
    for dir in [&home, &project.join(".codex"), &work.join(".codex")] {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(
        home.join("config.toml"),
        "[mcp_servers.docs]\ncommand = \"d\"\n",
    )
    .unwrap();
    std::fs::write(
        project.join(".codex").join("config.toml"),
        "[mcp_servers.docs]\ncommand = \"d\"\n[mcp_servers.\"team tools\"]\nurl = \"http://x\"\n",
    )
    .unwrap();
    std::fs::write(
        work.join(".codex").join("config.toml"),
        "[mcp_servers.local]\ncommand = \"l\"\n",
    )
    .unwrap();
    let input = scratch.join("input.json");
    let (home_env, input_env, cwd, b) = (
        format!("CODEX_HOME={}", home.display()),
        format!("MOCK_INPUT_FILE={}", input.display()),
        work.display().to_string(),
        bin("codex"),
    );
    let out = oh(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "hi",
            "--bin",
            &b,
            "--mode",
            "bypass",
            "--cwd",
            &cwd,
            "--no-mcp",
            "--env",
            &home_env,
            "--env",
            &input_env,
        ],
        &[],
        None,
    );
    assert!(out.status.success(), "{out:?}");
    let (argv, _) = recorded(&input);
    let stubs: Vec<&String> = argv
        .iter()
        .filter(|a| a.starts_with("mcp_servers."))
        .collect();
    for name in ["docs", "\"team tools\"", "local"] {
        let wanted = format!("mcp_servers.{name}={{command=\"true\",enabled=false}}");
        assert_eq!(
            stubs.iter().filter(|a| ***a == wanted).count(),
            1,
            "{name}: {stubs:?}"
        );
    }
    assert_eq!(stubs.len(), 3, "{stubs:?}");
}
// llmlint: ignore-end[e2e_not_mocked]

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its translation run; only the harness process (claude-code) is the mock harness, held to its real CLI by the capture it cites and `live-claude` (module doc).
/// A message emitted once per content block is one call, read from its last
/// emission, and calls keep the order their messages first appeared in.
#[test]
fn a_claude_message_repeated_per_block_is_one_call_read_from_its_last_emission() {
    let stream = [
        r#"{"type":"assistant","message":{"id":"m-1","usage":{"input_tokens":5,"output_tokens":1}}}"#,
        r#"{"type":"assistant","message":{"id":"m-2","usage":{"input_tokens":9,"output_tokens":4}}}"#,
        r#"{"type":"assistant","message":{"id":"m-1","usage":{"input_tokens":5,"output_tokens":7}}}"#,
        r#"{"type":"result","subtype":"success","result":"done"}"#,
    ]
    .join("\n");
    let b = bin("claude-code");
    let out = oh(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--bin",
            &b,
            "--output-format",
            "stream-json",
        ],
        &[("MOCK_STDOUT", &stream)],
        None,
    );
    let calls = json(&out)["results"][0]["calls"].clone();
    let pairs: Vec<(u64, u64)> = calls
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["input_tokens"].as_u64().unwrap(),
                c["output_tokens"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(pairs, vec![(5, 7), (9, 4)]);
}

/// Each accepted format reaches claude-code under its own media type.
#[test]
fn jpeg_gif_and_webp_images_reach_claude_under_their_own_media_types() {
    let scratch = ScratchDir::new("lean-image-formats").unwrap();
    let files = [
        ("a.jpg", b"\xff\xd8\xff\xe0jpeg".to_vec(), "image/jpeg"),
        ("b.gif", b"GIF87a....".to_vec(), "image/gif"),
        ("c.gif", b"GIF89a....".to_vec(), "image/gif"),
        (
            "d.webp",
            b"RIFF\x10\x00\x00\x00WEBPVP8 ".to_vec(),
            "image/webp",
        ),
    ];
    let mut args: Vec<String> = [
        "run",
        "--harness",
        "claude-code",
        "--prompt",
        "look",
        "--bin",
    ]
    .iter()
    .map(|a| a.to_string())
    .collect();
    args.push(bin("claude-code"));
    for (name, bytes, _) in &files {
        let path = scratch.join(name);
        std::fs::write(&path, bytes).unwrap();
        args.extend(["--image".to_string(), path.display().to_string()]);
    }
    let input = scratch.join("input.json");
    args.extend([
        "--env".to_string(),
        format!("MOCK_INPUT_FILE={}", input.display()),
    ]);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = oh(&args, &[], None);
    assert!(out.status.success(), "{out:?}");
    let (_, stdin) = recorded(&input);
    let frame: Value = serde_json::from_str(stdin.trim()).unwrap();
    let types: Vec<&str> = frame["message"]["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["source"]["media_type"].as_str())
        .collect();
    let expected: Vec<&str> = files.iter().map(|(_, _, t)| *t).collect();
    assert_eq!(types, expected);
}

/// Under read-only the shell is withheld from a lean tool set, and the run
/// says so.
#[test]
fn a_read_only_run_withholds_the_shell_and_says_so() {
    let scratch = ScratchDir::new("lean-read-only").unwrap();
    let input = scratch.join("input.json");
    let input_env = format!("MOCK_INPUT_FILE={}", input.display());
    let b = bin("claude-code");
    let out = oh(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "hi",
            "--bin",
            &b,
            "--mode",
            "read-only",
            "--tool-set",
            "shell,read",
            "--env",
            &input_env,
        ],
        &[],
        None,
    );
    assert!(out.status.success(), "{out:?}");
    let (argv, _) = recorded(&input);
    assert!(!argv.iter().any(|a| a == "Bash"), "{argv:?}");
    let result = json(&out)["results"][0].clone();
    let said = diagnostics_for(&result, "tool_set");
    assert!(
        said[0]["message"].as_str().unwrap().contains("withheld"),
        "{result}"
    );
}
// llmlint: ignore-end[e2e_not_mocked]

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its translation run; only the harness process (codex) is the mock harness, held to its real CLI by the capture it cites and `live-codex` (module doc).
/// codex's compaction is read only off the fork's own compaction turn: a
/// response without a result, a "fork" that is the original, and another
/// thread's or turn's notifications neither advance nor end it.
#[test]
fn a_codex_compaction_reads_only_the_forks_own_turn() {
    for (fault, said) in [
        ("noresult", "without a result"),
        ("samefork", "named no new thread"),
    ] {
        let s = Sessions::new(&format!("codex-{fault}"));
        s.turn("codex", "alpha");
        let stored = s.stored();
        let fault_env = format!("MOCK_COMPACT_FAIL={fault}");
        let outcome = json(&s.compact("codex", &["--env", &fault_env]));
        assert_eq!(outcome["status"], "failed", "{fault}: {outcome}");
        assert!(
            outcome["error"].as_str().unwrap().contains(said),
            "{fault}: {outcome}"
        );
        for (path, bytes) in &stored {
            assert_eq!(
                &std::fs::read(path).unwrap(),
                bytes,
                "{fault}: {}",
                path.display()
            );
        }
    }

    let s = Sessions::new("codex-noise");
    s.turn("codex", "alpha");
    let outcome = json(&s.compact(
        "codex",
        &[
            "--env",
            "MOCK_COMPACT_NOISE=1",
            "--env",
            "MOCK_COMPACT_USAGE=1",
        ],
    ));
    assert_eq!(outcome["status"], "completed", "{outcome}");
    assert_eq!(outcome["usage"]["input_tokens"], 60, "{outcome}");
    assert_eq!(outcome["usage"]["output_tokens"], 9, "{outcome}");
    let after = s.turn("codex", "beta");
    assert_eq!(
        after["results"][0]["text"],
        "context: [summary of: alpha, beta]"
    );
}
// llmlint: ignore-end[e2e_not_mocked]

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its translation run; only the harness process (claude-code) is the mock harness, held to its real CLI by the capture it cites and `live-claude` (module doc).
/// A run that ends non-zero, times out or is cancelled still reports the
/// calls it made before it ended.
#[test]
fn calls_are_reported_for_a_run_that_did_not_finish_cleanly() {
    let first = r#"{"type":"assistant","message":{"id":"m-1","usage":{"input_tokens":5,"output_tokens":2}}}"#;
    let stream =
        format!("{first}\n{{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"x\"}}");
    let b = bin("claude-code");
    let base = [
        "run",
        "--harness",
        "claude-code",
        "--prompt",
        "hi",
        "--bin",
        &b,
        "--output-format",
        "stream-json",
    ];
    let calls = |out: &Output, status: &str| {
        let result = json(out)["results"][0].clone();
        assert_eq!(result["status"], status, "{result}");
        result["calls"].as_array().map_or(0, Vec::len)
    };

    let out = oh(&base, &[("MOCK_STDOUT", &stream), ("MOCK_EXIT", "1")], None);
    assert_eq!(calls(&out, "nonzero"), 1);

    let mut timed = base.to_vec();
    timed.extend(["--timeout", "1"]);
    let out = oh(
        &timed,
        &[("MOCK_STDOUT", &stream), ("MOCK_STREAM_DELAY_MS", "5000")],
        None,
    );
    assert_eq!(calls(&out, "timeout"), 1);

    #[cfg(unix)]
    {
        let profiles = ProfileScratch::new();
        let child = Command::new(workspace_bin("oneharness"))
            .env("ONEHARNESS_NO_CONFIG", "1")
            .env("MOCK_STDOUT", &stream)
            .env("MOCK_STREAM_DELAY_MS", "30000")
            .args(base)
            .args(["--env", profiles.as_str(), "--format", "json"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1500));
        // Signal only the process this test started.
        assert!(Command::new("kill")
            .args(["-TERM", &child.id().to_string()])
            .status()
            .unwrap()
            .success());
        let out = child.wait_with_output().unwrap();
        assert_eq!(calls(&out, "cancelled"), 1);
    }
}
// llmlint: ignore-end[e2e_not_mocked]

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its translation run; only the harness process (codex) is the mock harness, held to its real CLI by the capture it cites and `live-codex` (module doc).
/// A re-asked attempt on a resumed codex thread lists every attempt's calls:
/// both turns this run appended to the one thread, each read once, and none
/// of the earlier turn the session already held.
#[test]
fn a_retried_codex_turn_reports_every_attempts_calls_once() {
    let s = Sessions::new("calls-retried");
    s.turn("codex", "alpha");
    let schema = s.dir.join("schema.json");
    std::fs::write(&schema, r#"{"type":"object","required":["never"]}"#).unwrap();
    let (proj, store, b, schema) = (
        s.path("proj"),
        s.path("store"),
        bin("codex"),
        schema.display().to_string(),
    );
    let env = s.env("codex");
    let mut args = vec![
        "run",
        "--harness",
        "codex",
        "--session",
        "watch",
        "--session-dir",
        &store,
        "--cwd",
        &proj,
        "--bin",
        &b,
        "--mode",
        "bypass",
        "--prompt",
        "beta",
        "--schema",
        &schema,
        "--schema-max-retries",
        "1",
    ];
    for e in &env {
        args.extend(["--env", e]);
    }
    let report = json(&oh(&args, &[], None));
    let result = &report["results"][0];
    assert_eq!(result["schema_attempts"], 2, "{result}");
    assert_eq!(result["calls"].as_array().unwrap().len(), 6, "{result}");
}
// llmlint: ignore-end[e2e_not_mocked]

#[test]
fn compact_refuses_an_empty_session_name() {
    let out = oh(&["compact", "--session", ""], &[], None);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(stderr(&out).contains("--session"), "{}", stderr(&out));
}

/// `list`'s text view carries each harness's new capabilities on its own row.
#[test]
fn the_list_text_view_states_each_lean_capability() {
    let out = oh(&["list", "--format", "text"], &[], None);
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8(out.stdout).unwrap();
    let row = |id: &str| {
        let at = text
            .lines()
            .position(|l| l.starts_with(&format!("{id} (")))
            .unwrap();
        text.lines()
            .skip(at + 1)
            .find(|l| l.trim_start().starts_with("auto_compact "))
            .unwrap_or_else(|| panic!("{id} has a lean capability row:\n{text}"))
            .trim()
            .to_string()
    };
    // The run-level allowlist is proven on Linux alone; elsewhere every
    // harness withholds it, and the row says so.
    let granted = if cfg!(target_os = "linux") {
        "yes"
    } else {
        "no"
    };
    assert_eq!(
        row("claude-code"),
        format!("auto_compact yes · compact_now yes · tool_set yes · skills_off yes · mcp_off yes · images yes · call_usage yes · run_allowed_tools {granted} · replace_system yes")
    );
    assert_eq!(
        row("codex"),
        format!("auto_compact yes · compact_now yes · tool_set no · skills_off yes · mcp_off yes · images yes · call_usage yes · run_allowed_tools {granted} · replace_system yes")
    );
    assert_eq!(
        row("goose"),
        "auto_compact no · compact_now no · tool_set no · skills_off no · mcp_off no · images no · call_usage no · run_allowed_tools no · replace_system no"
    );
}

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its translation run; only the harness process (claude-code) is the mock harness, held to its real CLI by the capture it cites and `live-claude` (module doc).
/// `compact`'s text view says how it ended, the context sizes the harness
/// reported, and, when it did not complete, why.
#[test]
fn the_compact_text_view_states_the_outcome_sizes_and_reason() {
    let s = Sessions::new("text-view");
    s.turn("claude-code", "alpha");
    let out = s.compact("claude-code", &["--format", "text"]);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "completed · session watch on claude-code · context 6809 → 7405 tokens\n"
    );
    let out = s.compact(
        "claude-code",
        &["--format", "text", "--env", "MOCK_COMPACT_FAIL=refuse"],
    );
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "failed · session watch on claude-code\n"
    );
    assert!(
        stderr(&out).contains("compact failed: Not enough messages to compact."),
        "{}",
        stderr(&out)
    );
}
// llmlint: ignore-end[e2e_not_mocked]

/// The config report attributes each lean setting to the layer that set it.
#[test]
fn the_config_report_attributes_each_lean_setting_to_its_layer() {
    let p = Project::new(
        "provenance",
        "extends = \"parent.toml\"\nload_mcp = false\nauto_compact_tokens = 5000\n",
        "tool_set = [\"read\", \"shell\"]\nload_skills = false\n",
    );
    let cwd = p.cwd();
    let report = json(&oh(&["config", "--cwd", &cwd], &[], Some(&p.user())));
    let source = |field: &str| {
        report[field]["source"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    };
    assert_eq!(report["auto_compact_tokens"]["value"], 5000);
    assert_eq!(
        report["tool_set"]["value"],
        serde_json::json!(["read", "shell"])
    );
    assert_eq!(report["load_skills"]["value"], false);
    assert_eq!(report["load_mcp"]["value"], false);
    assert!(source("auto_compact_tokens").ends_with("oneharness.toml"));
    assert!(source("load_mcp").ends_with("oneharness.toml"));
    assert!(source("tool_set").ends_with("parent.toml"));
    assert!(source("load_skills").ends_with("parent.toml"));

    let report = json(&oh(
        &["config", "--cwd", &cwd],
        &[
            ("ONEHARNESS_TOOL_SET", "shell"),
            ("ONEHARNESS_LOAD_MCP", "true"),
        ],
        Some(&p.user()),
    ));
    assert_eq!(report["tool_set"]["value"], serde_json::json!(["shell"]));
    assert_eq!(report["tool_set"]["source"], "environment");
    assert_eq!(report["load_mcp"]["value"], true);
    assert_eq!(report["load_mcp"]["source"], "environment");
}

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its translation run; only the harness process (claude-code) is the mock harness, held to its real CLI by the capture it cites and `live-claude` (module doc).
/// A prompt past the off-argv threshold and an image both ride claude-code's
/// one stdin frame: the large-prompt route does not displace the image's.
/// The prompt reaches oneharness through `--prompt-file`, as a caller with a
/// prompt this size must: on Windows it would overflow oneharness's own
/// command line (32,767 characters, os error 206) before the run began.
#[test]
fn a_large_prompt_and_an_image_ride_one_claude_frame() {
    let scratch = ScratchDir::new("lean-large-image").unwrap();
    let image = png(&scratch, "frame.png").display().to_string();
    let prompt = format!("look: {}", "x".repeat(70 * 1024));
    let prompt_file = scratch.join("prompt.txt");
    std::fs::write(&prompt_file, &prompt).unwrap();
    let prompt_file = prompt_file.display().to_string();
    let input = scratch.join("input.json");
    let input_env = format!("MOCK_INPUT_FILE={}", input.display());
    let b = bin("claude-code");
    let out = oh(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt-file",
            &prompt_file,
            "--bin",
            &b,
            "--image",
            &image,
            "--env",
            &input_env,
        ],
        &[],
        None,
    );
    assert!(out.status.success(), "{out:?}");
    let (argv, stdin) = recorded(&input);
    assert!(
        has_seq(&argv, &["--input-format", "stream-json"]),
        "{argv:?}"
    );
    assert!(
        !argv.iter().any(|a| a == "text" || a.len() > 1024),
        "{argv:?}"
    );
    let frame: Value = serde_json::from_str(stdin.trim()).unwrap();
    assert_eq!(frame["message"]["content"][0]["type"], "image");
    assert_eq!(frame["message"]["content"][1]["text"], prompt.as_str());
}

/// `--timeout 0` lifts a configured compaction deadline, as it does for run.
#[test]
fn an_explicit_timeout_of_zero_lifts_a_configured_compaction_deadline() {
    let s = Sessions::new("timeout-zero");
    std::fs::write(s.dir.join("proj").join("oneharness.toml"), "timeout = 1\n").unwrap();
    let user = s.dir.join("user.toml");
    std::fs::write(&user, "").unwrap();
    s.turn("claude-code", "alpha");
    let (proj, store, b) = (s.path("proj"), s.path("store"), bin("claude-code"));
    let env = s.env("claude-code");
    let compact = |extra: &[&str]| {
        let mut args = vec![
            "compact",
            "--session",
            "watch",
            "--session-dir",
            &store,
            "--cwd",
            &proj,
            "--bin",
            &b,
            "--env",
            "MOCK_COMPACT_DELAY_MS=2500",
        ];
        args.extend_from_slice(extra);
        for e in &env {
            args.extend(["--env", e.as_str()]);
        }
        json(&oh(&args, &[], Some(&user)))
    };
    let configured = compact(&[]);
    assert_eq!(configured["status"], "failed", "{configured}");
    assert!(
        configured["error"].as_str().unwrap().contains("timeout"),
        "{configured}"
    );
    let lifted = compact(&["--timeout", "0"]);
    assert_eq!(lifted["status"], "completed", "{lifted}");
}
// llmlint: ignore-end[e2e_not_mocked]

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its translation run; only the harness process (codex) is the mock harness, held to its real CLI by the capture it cites and `live-codex` (module doc).
/// codex reads files through its shell: `read` keeps it (and says so), and
/// only an empty set turns it off.
#[test]
fn a_codex_tool_set_keeps_the_shell_for_read_and_drops_it_only_when_empty() {
    let printed = |set: &str| {
        let out = oh(
            &[
                "run",
                "--harness",
                "codex",
                "--prompt",
                "hi",
                "--tool-set",
                set,
                "--print-command",
            ],
            &[],
            None,
        );
        strings(&json(&out)["results"][0]["command"])
    };
    assert!(has_seq(&printed(""), &["-c", "features.shell_tool=false"]));
    assert!(!printed("read")
        .iter()
        .any(|a| a == "features.shell_tool=false"));
    let b = bin("codex");
    let out = oh(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "hi",
            "--bin",
            &b,
            "--mode",
            "bypass",
            "--tool-set",
            "read",
        ],
        &[],
        None,
    );
    let result = json(&out)["results"][0].clone();
    let said = diagnostics_for(&result, "tool_set");
    assert!(
        said[0]["message"].as_str().unwrap().contains("reads files"),
        "{result}"
    );
}

/// A relative `--image` is the caller's path: resolved against the directory
/// oneharness was started in, not the `--cwd` the harness runs in.
#[test]
fn a_relative_image_is_resolved_where_the_caller_named_it() {
    let scratch = ScratchDir::new("lean-image-relative").unwrap();
    let caller = scratch.join("caller");
    let elsewhere = scratch.join("elsewhere");
    std::fs::create_dir_all(&caller).unwrap();
    std::fs::create_dir_all(&elsewhere).unwrap();
    let image = png(&caller, "frame.png");
    let input = scratch.join("input.json");
    let profiles = ProfileScratch::new();
    let out = Command::new(workspace_bin("oneharness"))
        .current_dir(&caller)
        .env("ONEHARNESS_NO_CONFIG", "1")
        .args([
            "run",
            "--harness",
            "codex",
            "--prompt",
            "hi",
            "--mode",
            "bypass",
            "--format",
            "json",
        ])
        .args(["--bin", &bin("codex"), "--image", "frame.png"])
        .args(["--cwd", &elsewhere.display().to_string()])
        .args(["--env", &format!("MOCK_INPUT_FILE={}", input.display())])
        .args(["--env", profiles.as_str()])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let (argv, _) = recorded(&input);
    let passed = argv
        .iter()
        .find_map(|a| a.strip_prefix("--image="))
        .expect("an image flag");
    assert_eq!(
        std::fs::canonicalize(passed).unwrap(),
        std::fs::canonicalize(&image).unwrap(),
        "the caller's file, not one under --cwd"
    );
}

/// A variant's own `CODEX_HOME` is where its rollouts are read from, and a
/// resumed turn there still reports only its own calls.
#[test]
fn per_call_usage_is_read_from_a_variants_own_codex_home() {
    let s = Sessions::new("variant-home");
    let variant_home = s.dir.join("work-home");
    std::fs::create_dir_all(&variant_home).unwrap();
    let proj = s.dir.join("proj");
    std::fs::write(
        proj.join("oneharness.toml"),
        format!(
            "[harness.codex.variant.work.env]\nCODEX_HOME = {:?}\nMOCK_CODEX_SESSIONS = \"1\"\n",
            variant_home.display().to_string()
        ),
    )
    .unwrap();
    let user = s.dir.join("user.toml");
    std::fs::write(&user, "").unwrap();
    let (proj_s, store, b) = (s.path("proj"), s.path("store"), bin("codex"));
    let turn = |prompt: &str| {
        let out = oh(
            &[
                "run",
                "--harness",
                "codex:work",
                "--session",
                "watch",
                "--session-dir",
                &store,
                "--cwd",
                &proj_s,
                "--bin",
                &b,
                "--mode",
                "bypass",
                "--prompt",
                prompt,
            ],
            &[],
            Some(&user),
        );
        assert!(out.status.success(), "{out:?}");
        json(&out)["results"][0].clone()
    };
    for prompt in ["alpha", "beta"] {
        let result = turn(prompt);
        assert_eq!(
            result["calls"].as_array().map_or(0, Vec::len),
            3,
            "{prompt}: {result}"
        );
    }
    assert!(
        variant_home.join("sessions").is_dir(),
        "the variant's home holds the rollout"
    );
    assert!(
        !s.dir.join("home").join("sessions").exists(),
        "the base home does not"
    );
}
// llmlint: ignore-end[e2e_not_mocked]

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its translation run; only the harness process (claude-code, opencode) is the mock harness, held to its real CLI by the capture it cites and `live-claude`, `live-opencode` (module doc).
/// A malformed `--env` or `--bin` is a usage error even when the answer would
/// otherwise be `unsupported` or a missing binary.
#[test]
fn compact_refuses_malformed_env_or_bin_before_any_answer() {
    let o = Sessions::new("malformed-unsupported");
    let (proj, store, b) = (o.path("proj"), o.path("store"), bin("opencode"));
    let created = oh(
        &[
            "run",
            "--harness",
            "opencode",
            "--session",
            "watch",
            "--session-dir",
            &store,
            "--cwd",
            &proj,
            "--bin",
            &b,
            "--mode",
            "bypass",
            "--prompt",
            "hi",
        ],
        &[(
            "MOCK_STDOUT",
            r#"{"type":"text","sessionID":"ses_1","part":{"type":"text","text":"ok"}}"#,
        )],
        None,
    );
    assert!(created.status.success(), "{created:?}");
    for bad in [["--env", "NO_EQUALS"], ["--bin", "no-equals"]] {
        let mut args = vec![
            "compact",
            "--session",
            "watch",
            "--session-dir",
            &store,
            "--cwd",
            &proj,
            "--bin",
            &b,
        ];
        args.extend_from_slice(&bad);
        let out = oh(&args, &[], None);
        assert_eq!(out.status.code(), Some(2), "unsupported + {bad:?}: {out:?}");
    }

    let c = Sessions::new("malformed-missing");
    c.turn("claude-code", "alpha");
    let (proj, store) = (c.path("proj"), c.path("store"));
    let missing = format!("claude-code={}", c.dir.join("absent").display());
    let out = oh(
        &[
            "compact",
            "--session",
            "watch",
            "--session-dir",
            &store,
            "--cwd",
            &proj,
            "--bin",
            &missing,
            "--env",
            "NO_EQUALS",
        ],
        &[],
        None,
    );
    assert_eq!(
        out.status.code(),
        Some(2),
        "missing binary + bad env: {out:?}"
    );
}
// llmlint: ignore-end[e2e_not_mocked]

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its translation run; only the harness process (codex) is the mock harness, held to its real CLI by the capture it cites and `live-codex` (module doc).
/// codex's home is wherever the launch's codex would look: `.codex` in the
/// user's home (`USERPROFILE` on Windows, `HOME` elsewhere) when no
/// `CODEX_HOME` reaches it, including when a variant removes the one
/// oneharness inherited. Per-call usage and MCP discovery both read it.
#[test]
fn codex_home_follows_home_and_a_variants_removed_codex_home() {
    let user_var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    let scratch = ScratchDir::new("lean-codex-home").unwrap();
    let declare = |home: &Path, server: &str| {
        std::fs::create_dir_all(home).unwrap();
        std::fs::write(
            home.join("config.toml"),
            format!("[mcp_servers.{server}]\ncommand = \"x\"\n"),
        )
        .unwrap();
    };
    let inherited = scratch.join("inherited");
    let user_home = scratch.join("user");
    declare(&inherited, "inherited_server");
    declare(&user_home.join(".codex"), "home_server");
    let project = scratch.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let input = scratch.join("input.json");
    let profiles = ProfileScratch::new();
    let run = |extra: &[&str], inherit_codex_home: bool| {
        let mut cmd = Command::new(workspace_bin("oneharness"));
        cmd.env("ONEHARNESS_CONFIG", scratch.join("user.toml"));
        for (var, _) in
            std::env::vars().filter(|(v, _)| v.starts_with("ONEHARNESS_") || v == "CODEX_HOME")
        {
            cmd.env_remove(var);
        }
        if inherit_codex_home {
            cmd.env("CODEX_HOME", &inherited);
        }
        cmd.args([
            "run", "--prompt", "hi", "--mode", "bypass", "--no-mcp", "--format", "json",
        ])
        .args([
            "--bin",
            &bin("codex"),
            "--cwd",
            &project.display().to_string(),
        ])
        .args([
            "--env",
            &format!("MOCK_INPUT_FILE={}", input.display()),
            "--env",
            profiles.as_str(),
        ])
        .args(extra);
        let out = cmd.output().unwrap();
        assert!(out.status.success(), "{out:?}");
        let (argv, _) = recorded(&input);
        (json(&out)["results"][0].clone(), argv.join(" "))
    };
    std::fs::write(scratch.join("user.toml"), "").unwrap();

    // No CODEX_HOME at all: the user's home's .codex, as codex itself falls back.
    let home_env = format!("{user_var}={}", user_home.display());
    let (result, argv) = run(
        &[
            "--harness",
            "codex",
            "--env",
            &home_env,
            "--env",
            "MOCK_CODEX_SESSIONS=1",
        ],
        false,
    );
    assert_eq!(
        result["calls"].as_array().map_or(0, Vec::len),
        3,
        "{result}"
    );
    assert!(argv.contains("mcp_servers.home_server="), "{argv}");

    // A variant that removes the inherited CODEX_HOME reads the user's home too.
    std::fs::write(
        scratch.join("user.toml"),
        format!(
            "[harness.codex.variant.home]\nunset_env = [\"CODEX_HOME\"]\n[harness.codex.variant.home.env]\n{user_var} = {:?}\nMOCK_CODEX_SESSIONS = \"1\"\n",
            user_home.display().to_string()
        ),
    )
    .unwrap();
    let (result, argv) = run(&["--harness", "codex:home"], true);
    assert_eq!(
        result["calls"].as_array().map_or(0, Vec::len),
        3,
        "{result}"
    );
    assert!(argv.contains("mcp_servers.home_server="), "{argv}");
    assert!(
        !argv.contains("inherited_server"),
        "the removed home is not read: {argv}"
    );
}

/// Per-call usage that cannot be read leaves the run as it was and says why:
/// when no codex home can be resolved, and when the rollout exists but
/// cannot be read.
#[cfg(unix)]
#[test]
fn an_unreadable_codex_record_is_a_diagnostic_not_a_failed_run() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = ScratchDir::new("lean-unreadable-rollout").unwrap();
    let profiles = ProfileScratch::new();
    let named = "{\"type\":\"thread.started\",\"thread_id\":\"t-locked\"}\n{\"type\":\"item.completed\",\"item\":{\"id\":\"i\",\"type\":\"agent_message\",\"text\":\"ok\"}}";
    let run = |env: &[(&str, &str)], remove_homes: bool, extra: &[&str]| {
        let mut cmd = Command::new(workspace_bin("oneharness"));
        cmd.env("ONEHARNESS_NO_CONFIG", "1");
        if remove_homes {
            cmd.env_remove("HOME").env_remove("CODEX_HOME");
        }
        cmd.args([
            "run",
            "--harness",
            "codex",
            "--prompt",
            "hi",
            "--mode",
            "bypass",
            "--format",
            "json",
        ])
        .args(["--bin", &bin("codex"), "--env", profiles.as_str()])
        .args(extra);
        for (key, value) in env {
            cmd.env(key, value);
        }
        let out = cmd.output().unwrap();
        assert!(out.status.success(), "{out:?}");
        let result = json(&out)["results"][0].clone();
        assert_eq!(result["status"], "ok", "{result}");
        assert!(result.get("calls").is_none(), "{result}");
        diagnostics_for(&result, "calls")[0]["message"]
            .as_str()
            .unwrap()
            .to_string()
    };

    assert!(run(&[("MOCK_STDOUT", named)], true, &[]).contains("no codex home"));

    let home = scratch.join("home");
    let day = home.join("sessions").join("2026").join("11").join("02");
    std::fs::create_dir_all(&day).unwrap();
    let rollout = day.join("rollout-2026-11-02T00-00-00-t-locked.jsonl");
    std::fs::write(&rollout, "{}\n").unwrap();
    std::fs::set_permissions(&rollout, std::fs::Permissions::from_mode(0o000)).unwrap();
    let home_env = format!("CODEX_HOME={}", home.display());
    let said = run(&[("MOCK_STDOUT", named)], false, &["--env", &home_env]);
    std::fs::set_permissions(&rollout, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(said.contains("t-locked"), "{said}");
    assert!(said.to_lowercase().contains("permission denied"), "{said}");
}
// llmlint: ignore-end[e2e_not_mocked]

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its translation run; only the harness process (claude-code) is the mock harness, held to its real CLI by the capture it cites and `live-claude` (module doc).
/// A compaction whose reported per-model counts cannot be summed reports
/// that field as unavailable rather than wrapping or failing.
#[test]
fn a_compaction_total_that_overflows_is_reported_unavailable() {
    let s = Sessions::new("overflow");
    s.turn("claude-code", "alpha");
    let outcome = json(&s.compact("claude-code", &["--env", "MOCK_COMPACT_OVERFLOW=1"]));
    assert_eq!(outcome["status"], "completed", "{outcome}");
    assert!(outcome["usage"]["input_tokens"].is_null(), "{outcome}");
    assert!(outcome["usage"]["cost_usd"].is_null(), "{outcome}");
    assert_eq!(outcome["usage"]["output_tokens"], 7, "{outcome}");
}

/// A compaction whose result reports a negative per-model cost reports its
/// cost as unavailable, while the counts beside it still sum.
#[test]
fn a_compaction_with_a_negative_cost_reports_its_cost_unavailable() {
    let s = Sessions::new("negative-cost");
    s.turn("claude-code", "alpha");
    let outcome = json(&s.compact("claude-code", &["--env", "MOCK_COMPACT_NEGATIVE=1"]));
    assert_eq!(outcome["status"], "completed", "{outcome}");
    assert!(outcome["usage"]["cost_usd"].is_null(), "{outcome}");
    assert_eq!(outcome["usage"]["input_tokens"], 3, "{outcome}");
    assert_eq!(outcome["usage"]["output_tokens"], 7, "{outcome}");
}

/// A launcher that exits while a descendant still holds its stdout does not
/// hold the compaction: it ends at the launcher's exit, with no deadline set.
#[cfg(unix)]
#[test]
fn a_compaction_ends_when_its_launcher_exits_even_if_a_descendant_holds_stdout() {
    let s = Sessions::new("orphan");
    s.turn("claude-code", "alpha");
    let started = std::time::Instant::now();
    let outcome = json(&s.compact("claude-code", &["--env", "MOCK_COMPACT_ORPHAN=1"]));
    assert_eq!(outcome["status"], "completed", "{outcome}");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(15),
        "held for {:?} by the descendant",
        started.elapsed()
    );
}

/// An explicit `--model` beats the configured one, and an out-of-range
/// `--timeout` is refused before anything spawns.
#[test]
fn compact_takes_an_explicit_model_over_config_and_refuses_an_unrepresentable_timeout() {
    let s = Sessions::new("explicit-model");
    std::fs::write(
        s.dir.join("proj").join("oneharness.toml"),
        "[harness.claude-code]\nmodel = \"claude-configured\"\n",
    )
    .unwrap();
    let user = s.dir.join("user.toml");
    std::fs::write(&user, "").unwrap();
    s.turn("claude-code", "alpha");
    let (proj, store, b) = (s.path("proj"), s.path("store"), bin("claude-code"));
    let argv_file = s.dir.join("argv");
    let argv_env = format!("MOCK_ARGV_FILE={}", argv_file.display());
    let env = s.env("claude-code");
    let compact = |extra: &[&str]| {
        let mut args = vec![
            "compact",
            "--session",
            "watch",
            "--session-dir",
            &store,
            "--cwd",
            &proj,
            "--bin",
            &b,
            "--env",
            &argv_env,
        ];
        args.extend_from_slice(extra);
        for e in &env {
            args.extend(["--env", e.as_str()]);
        }
        oh(&args, &[], Some(&user))
    };
    let out = compact(&["--model", "claude-explicit"]);
    assert_eq!(json(&out)["status"], "completed", "{out:?}");
    let argv: Vec<String> = std::fs::read_to_string(&argv_file)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect();
    assert!(has_seq(&argv, &["--model", "claude-explicit"]), "{argv:?}");
    assert!(!argv.iter().any(|a| a == "claude-configured"), "{argv:?}");

    std::fs::remove_file(&argv_file).unwrap();
    let out = compact(&["--timeout", "18446744073709551615"]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(
        stderr(&out).contains("18446744073709551615"),
        "{}",
        stderr(&out)
    );
    assert!(!argv_file.exists(), "nothing was spawned");
}
// llmlint: ignore-end[e2e_not_mocked]

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its translation run; only the harness process (codex) is the mock harness, held to its real CLI by the capture it cites and `live-codex` (module doc).
/// A fallback chain whose session resumes on a later candidate reports that
/// candidate's own calls, not the turns the thread already held.
#[test]
fn a_fallback_candidate_resuming_the_session_reports_only_its_own_calls() {
    let s = Sessions::new("fallback-calls");
    s.turn("codex", "alpha");
    let (proj, store) = (s.path("proj"), s.path("store"));
    let missing = format!("claude-code={}", s.dir.join("no-claude").display());
    let codex = bin("codex");
    let env = s.env("codex");
    let mut args = vec![
        "run",
        "--harness",
        "claude-code,codex",
        "--session",
        "watch",
        "--session-dir",
        &store,
        "--cwd",
        &proj,
        "--bin",
        &missing,
        "--bin",
        &codex,
        "--mode",
        "bypass",
        "--prompt",
        "beta",
    ];
    for e in &env {
        args.extend(["--env", e.as_str()]);
    }
    let report = json(&oh(&args, &[], None));
    assert_eq!(report["fallback"]["ran"], "codex", "{report}");
    assert_eq!(report["session"]["phase"], "continue", "{report}");
    let ran = report["results"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert_eq!(ran["text"], "context: [alpha, beta]");
    assert_eq!(ran["calls"].as_array().unwrap().len(), 3, "{ran}");
}
// llmlint: ignore-end[e2e_not_mocked]

/// A scratch cwd (no project config) beside a user config setting a value no
/// other layer sets, for the two selection journeys below.
fn selection_scratch(tag: &str) -> (ScratchDir, PathBuf, String) {
    let dir = ScratchDir::new(tag).unwrap();
    let user = dir.join("user.toml");
    std::fs::write(&user, "auto_compact_tokens = 7777\n").unwrap();
    let cwd = dir.display().to_string();
    (dir, user, cwd)
}

/// Re-run by [`ambient_config_selection_never_displaces_the_helpers_own`]
/// under ambient overrides: the helper's explicit config is the one read.
#[test]
fn an_explicit_config_is_the_one_the_cli_reads() {
    let (_dir, user, cwd) = selection_scratch("lean-explicit");
    let explicit = json(&oh(&["config", "--cwd", &cwd], &[], Some(&user)));
    assert_eq!(explicit["auto_compact_tokens"]["value"], 7777, "{explicit}");
    assert_eq!(
        explicit["config_files"].as_array().map(Vec::len),
        Some(1),
        "only the explicit user config is read: {explicit}"
    );
}

/// Re-run likewise: a hermetic invocation reads no config file at all.
#[test]
fn a_hermetic_invocation_reads_no_config() {
    let (_dir, _user, cwd) = selection_scratch("lean-hermetic");
    let hermetic = json(&oh(&["config", "--cwd", &cwd], &[], None));
    assert_eq!(
        hermetic["config_files"],
        serde_json::json!([]),
        "{hermetic}"
    );
    assert_eq!(
        hermetic["auto_compact_tokens"]["value"],
        Value::Null,
        "{hermetic}"
    );
}

/// Re-runs real journeys of this suite in a child test process whose ambient
/// environment carries `ONEHARNESS_CONFIG` and `ONEHARNESS_NO_CONFIG`, with
/// the platform config directory holding a distorting `config.toml`. If the
/// helper's own selection were removed with the ambient overrides, the CLI
/// would read that file and the journeys would see its value.
#[test]
fn ambient_config_selection_never_displaces_the_helpers_own() {
    let dir = ScratchDir::new("lean-ambient").unwrap();
    let ambient = dir.join("ambient.toml");
    std::fs::write(&ambient, "auto_compact_tokens = 5555\n").unwrap();
    let platform = dir.join("platform");
    std::fs::create_dir_all(platform.join("oneharness")).unwrap();
    std::fs::write(
        platform.join("oneharness").join("config.toml"),
        "auto_compact_tokens = 4444\ntool_set = [\"read\"]\n",
    )
    .unwrap();

    let journeys = [
        "an_explicit_config_is_the_one_the_cli_reads",
        "a_hermetic_invocation_reads_no_config",
        "each_setting_layers_like_timeout_through_extends_env_and_flag",
        "each_setting_reaches_only_its_own_harness_and_nothing_is_added_when_absent",
    ];
    let out = Command::new(std::env::current_exe().expect("test executable path"))
        .args(["--exact", "--test-threads", "1"])
        .args(journeys)
        .env("ONEHARNESS_CONFIG", &ambient)
        .env("ONEHARNESS_NO_CONFIG", "1")
        .env("XDG_CONFIG_HOME", &platform)
        .env("APPDATA", &platform)
        .output()
        .expect("re-run this suite's journeys");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "journeys failed under ambient ONEHARNESS_CONFIG/ONEHARNESS_NO_CONFIG:\n{stdout}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains(&format!("{} passed", journeys.len())),
        "every named journey ran: {stdout}"
    );
}

/// Every invocation the mock recorded under MOCK_INVOCATION_LOG, in order.
fn invocations(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// The contents the mock read from the file `argv` names after `flag` (or,
/// for a `key=` override, in its value).
fn delivered_file(invocation: &Value, path: &str) -> String {
    invocation["files"][path]
        .as_str()
        .unwrap_or_else(|| panic!("no file {path} was readable while it ran: {invocation}"))
        .to_string()
}

fn after<'a>(argv: &'a [String], flag: &str) -> Option<&'a str> {
    argv.windows(2)
        .find(|w| w[0] == flag)
        .map(|w| w[1].as_str())
}

const REPLACEMENT: &str = "You are the print supervisor. Answer in one line.";

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its translation run; only the harness process (each adapter) is the mock harness, held to its real CLI by the request captures the `replace_system` table rows cite (module doc).
/// `replace_system` reaches each harness as its translation row records, the
/// text only ever on the replacing knob: Claude Code's `--system-prompt-file`,
/// codex's `-c model_instructions_file=`, qwen's `QWEN_SYSTEM_MD`. A harness
/// whose row names no knob is refused and never started, never handed the
/// text to append; `oneharness list` says which is which.
#[test]
fn replace_system_reaches_each_harness_on_its_replacing_knob_or_refuses_it() {
    let scratch = ScratchDir::new("lean-replace-system").unwrap();
    let listed = list();
    for id in ALL_IDS {
        let log = scratch.join(format!("{id}.jsonl"));
        let log_env = format!("MOCK_INVOCATION_LOG={}", log.display());
        let b = bin(id);
        let out = oh(
            &[
                "run",
                "--harness",
                id,
                "--prompt",
                "status?",
                "--system",
                REPLACEMENT,
                "--replace-system",
                "--bin",
                &b,
                "--mode",
                "bypass",
                "--env",
                &log_env,
                "--env",
                "MOCK_RECORD_ENV=QWEN_SYSTEM_MD",
            ],
            &[],
            None,
        );
        let report = json(&out);
        let result = &report["results"][0];
        if !supports(&listed, id, "supports_replace_system") {
            assert_eq!(result["status"], "skipped", "{id}: {result}");
            assert_eq!(result["failure_kind"], "unsupported_input", "{id}");
            assert_eq!(result["failure_kind_source"], "request:replace_system");
            let error = result["error"].as_str().unwrap();
            assert!(
                error.contains(id) && error.contains("replace_system"),
                "{id}: {error}"
            );
            assert!(invocations(&log).is_empty(), "{id} was started");
            continue;
        }
        assert!(out.status.success(), "{id}: {out:?}");
        let calls = invocations(&log);
        assert_eq!(calls.len(), 1, "{id}");
        let argv = strings(&calls[0]["argv"]);
        let stdin = calls[0]["stdin"].as_str().unwrap();
        assert!(
            !stdin.contains(REPLACEMENT),
            "{id} folded the text: {stdin}"
        );
        assert!(
            !argv.iter().any(|a| a.contains("append-system-prompt")),
            "{id}: {argv:?}"
        );
        let path = match *id {
            "claude-code" => after(&argv, "--system-prompt-file")
                .unwrap_or_else(|| panic!("{argv:?}"))
                .to_string(),
            "codex" => {
                let value = argv
                    .windows(2)
                    .find_map(|w| {
                        (w[0] == "-c")
                            .then(|| w[1].strip_prefix("model_instructions_file="))
                            .flatten()
                    })
                    .unwrap_or_else(|| panic!("{argv:?}"));
                // A TOML basic string, which a JSON string parse reads alike.
                serde_json::from_str::<String>(value).expect("a quoted path")
            }
            "qwen" => calls[0]["env"]["QWEN_SYSTEM_MD"]
                .as_str()
                .unwrap_or_else(|| panic!("{}", calls[0]))
                .to_string(),
            other => panic!("{other} is listed as replacing but has no assertion here"),
        };
        assert_eq!(delivered_file(&calls[0], &path), REPLACEMENT, "{id}");
        assert!(
            !Path::new(&path).exists(),
            "{id}: the private file outlived the run"
        );
    }
}
// llmlint: ignore-end[e2e_not_mocked]

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its translation run; only the harness process (claude-code, codex) is the mock harness, held to its real CLI by the request captures the `replace_system` rows cite (module doc).
/// Absent or false, `replace_system` changes nothing: Claude Code still takes
/// its appending flag and codex still has the text folded into its prompt.
/// A fallback chain moves past a harness that cannot replace to one that can.
#[test]
fn without_replace_system_the_text_is_appended_and_a_chain_moves_past_a_harness_that_cannot_replace(
) {
    let scratch = ScratchDir::new("lean-replace-absent").unwrap();
    let no_env: &[(&str, &str)] = &[];
    for (label, envs) in [
        ("absent", no_env),
        (
            "false in the environment",
            &[("ONEHARNESS_REPLACE_SYSTEM", "false")],
        ),
    ] {
        for id in ["claude-code", "codex"] {
            let log = scratch.join(format!("{id}-{}.jsonl", envs.len()));
            let log_env = format!("MOCK_INVOCATION_LOG={}", log.display());
            let b = bin(id);
            let args = [
                "run",
                "--harness",
                id,
                "--prompt",
                "status?",
                "--system",
                REPLACEMENT,
                "--bin",
                &b,
                "--env",
                &log_env,
            ];
            // A real (empty) user config, so the environment layer is read.
            let config = scratch.join("empty.toml");
            std::fs::write(&config, "").unwrap();
            let out = oh(&args, envs, Some(&config));
            assert!(out.status.success(), "{label} {id}: {out:?}");
            let calls = invocations(&log);
            let argv = strings(&calls[0]["argv"]);
            let stdin = calls[0]["stdin"].as_str().unwrap();
            assert!(
                !argv
                    .iter()
                    .any(|a| a == "--system-prompt-file"
                        || a.starts_with("model_instructions_file=")),
                "{label} {id}: {argv:?}"
            );
            if id == "claude-code" {
                let path = after(&argv, "--append-system-prompt-file").unwrap();
                assert_eq!(delivered_file(&calls[0], path), REPLACEMENT, "{label}");
            } else {
                assert!(stdin.starts_with(REPLACEMENT), "{label}: folded {stdin:?}");
            }
        }
    }

    let log = scratch.join("chain.jsonl");
    let log_env = format!("MOCK_INVOCATION_LOG={}", log.display());
    let (goose, claude) = (bin("goose"), bin("claude-code"));
    let out = oh(
        &[
            "run",
            "--harness",
            "goose",
            "--harness",
            "claude-code",
            "--run-mode",
            "fallback",
            "--prompt",
            "status?",
            "--system",
            REPLACEMENT,
            "--replace-system",
            "--bin",
            &goose,
            "--bin",
            &claude,
            "--env",
            &log_env,
        ],
        &[],
        None,
    );
    assert!(out.status.success(), "{out:?}");
    let report = json(&out);
    assert_eq!(report["results"][0]["harness"], "goose");
    assert_eq!(report["results"][0]["failure_kind"], "unsupported_input");
    assert_eq!(report["fallback"]["ran"], "claude-code", "{report}");
    let calls = invocations(&log);
    assert_eq!(calls.len(), 1, "only claude-code was started");
    assert!(after(&strings(&calls[0]["argv"]), "--system-prompt-file").is_some());
}
// llmlint: ignore-end[e2e_not_mocked]

/// A `replace_system` run on `--control` is refused before anything starts
/// for a harness whose replacing knob is set at launch, naming both.
#[test]
fn replace_system_on_a_controlled_codex_turn_is_a_usage_error() {
    let scratch = ScratchDir::new("lean-replace-control").unwrap();
    let store = scratch.join("store").display().to_string();
    let b = bin("codex");
    let out = oh(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "hi",
            "--system",
            REPLACEMENT,
            "--replace-system",
            "--control",
            "--session",
            "s",
            "--session-dir",
            &store,
            "--bin",
            &b,
        ],
        &[],
        None,
    );
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let said = stderr(&out);
    assert!(
        said.contains("replace_system") && said.contains("--control") && said.contains("codex"),
        "{said}"
    );
}

/// A schema OpenAI strict mode accepts: every property required, no others.
const PERSON: &str = r#"{"type":"object","properties":{"name":{"type":"string"},"age":{"type":"integer"}},"required":["name","age"],"additionalProperties":false}"#;

/// The prompt-based path's instruction, which a native delivery never adds.
const PROMPT_SCHEMA_INSTRUCTION: &str = "You must respond with a single JSON value";

/// One Claude Code stream-json turn, in the shape of
/// `tests/fixtures/claude-stream-json-turn.jsonl` and the 2.1.329 image run
/// the `schema` row cites: one model call answering through its
/// `StructuredOutput` tool, then the `result` event carrying the value.
fn claude_structured_turn(message: &str, input: u64, output: u64, value: &str) -> String {
    [
        r#"{"type":"system","subtype":"init","session_id":"s-structured"}"#.to_string(),
        format!(
            r#"{{"type":"assistant","message":{{"id":"{message}","role":"assistant","content":[{{"type":"tool_use","id":"t-{message}","name":"StructuredOutput","input":{value}}}],"usage":{{"input_tokens":{input},"output_tokens":{output},"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}}}}"#
        ),
        format!(
            r#"{{"type":"result","subtype":"success","is_error":false,"result":"","session_id":"s-structured","usage":{{"input_tokens":{input},"output_tokens":{output}}},"structured_output":{value}}}"#
        ),
    ]
    .join("\n")
}

fn input_and_output_tokens(result: &Value) -> Vec<(u64, u64)> {
    result["calls"]
        .as_array()
        .unwrap_or_else(|| panic!("no calls: {result}"))
        .iter()
        .map(|c| {
            (
                c["input_tokens"].as_u64().unwrap(),
                c["output_tokens"].as_u64().unwrap(),
            )
        })
        .collect()
}

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its structured-output loop run; only the harness process (claude-code) is the mock harness, replaying the stream-json shape of the captured turn and the 2.1.329 image run the `schema` row cites (module doc).
/// An image beside a schema on Claude Code, with the output format unset and
/// with `stream-json`: the image rides the stream-json frame, the schema
/// rides `--json-schema`, and the validated value comes back from the
/// `result` event. An invalid first answer is re-asked, and `calls` lists
/// both attempts' model calls with the usage each reported.
#[test]
fn claude_takes_an_image_and_a_schema_together_and_re_asks_an_invalid_answer() {
    let scratch = ScratchDir::new("lean-claude-image-schema").unwrap();
    let image = png(&scratch, "frame.png").display().to_string();
    let schema = scratch.join("person.json");
    std::fs::write(&schema, PERSON).unwrap();
    let schema = schema.display().to_string();
    let invalid = claude_structured_turn("msg_one", 11, 3, r#"{"name":"Ada"}"#);
    let valid = claude_structured_turn("msg_two", 21, 4, r#"{"name":"Ada","age":36}"#);
    let b = bin("claude-code");
    for (label, format) in [("unset", None), ("stream-json", Some("stream-json"))] {
        let log = scratch.join(format!("{label}.jsonl"));
        let counter = scratch.join(format!("{label}.count"));
        let mut args = vec![
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "who is this?",
            "--image",
            &image,
            "--schema",
            &schema,
            "--bin",
            &b,
        ];
        args.extend(format.iter().flat_map(|f| ["--output-format", *f]));
        let out = oh(
            &args,
            &[
                ("MOCK_INVOCATION_LOG", log.to_str().unwrap()),
                ("MOCK_ATTEMPT_FILE", counter.to_str().unwrap()),
                ("MOCK_STDOUT_1", &invalid),
                ("MOCK_STDOUT_2", &valid),
            ],
            None,
        );
        assert!(out.status.success(), "{label}: {out:?}");
        let result = json(&out)["results"][0].clone();
        assert_eq!(result["structured"]["age"], 36, "{label}: {result}");
        assert_eq!(result["schema_valid"], true, "{label}");
        assert!(result["schema_error"].is_null(), "{label}");
        assert_eq!(result["schema_attempts"], 2, "{label}");
        assert_eq!(
            input_and_output_tokens(&result),
            vec![(11, 3), (21, 4)],
            "{label}: one usage per call, both attempts, in order"
        );
        let calls = invocations(&log);
        assert_eq!(calls.len(), 2, "{label}");
        for (attempt, call) in calls.iter().enumerate() {
            let argv = strings(&call["argv"]);
            assert!(
                has_seq(&argv, &["--input-format", "stream-json"]),
                "{argv:?}"
            );
            assert!(
                has_seq(&argv, &["--output-format", "stream-json"]),
                "{argv:?}"
            );
            let delivered: Value =
                serde_json::from_str(after(&argv, "--json-schema").expect("native")).unwrap();
            assert_eq!(delivered, serde_json::from_str::<Value>(PERSON).unwrap());
            let frame: Value =
                serde_json::from_str(call["stdin"].as_str().unwrap().trim()).unwrap();
            let content = frame["message"]["content"].as_array().unwrap();
            assert_eq!(content[0]["type"], "image", "{label} attempt {attempt}");
            let text = content[1]["text"].as_str().unwrap();
            assert!(text.starts_with("who is this?"), "{text}");
            assert!(!text.contains(PROMPT_SCHEMA_INSTRUCTION), "native: {text}");
        }
    }

    // Still invalid after `schema_max_retries` re-asks: a schema failure.
    let log = scratch.join("exhausted.jsonl");
    let out = oh(
        &[
            "run",
            "--harness",
            "claude-code",
            "--prompt",
            "who is this?",
            "--image",
            &image,
            "--schema",
            &schema,
            "--schema-max-retries",
            "1",
            "--bin",
            &b,
        ],
        &[
            ("MOCK_INVOCATION_LOG", log.to_str().unwrap()),
            ("MOCK_STDOUT", &invalid),
        ],
        None,
    );
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let result = json(&out)["results"][0].clone();
    assert_eq!(result["schema_valid"], false, "{result}");
    assert_eq!(result["schema_attempts"], 2);
    assert!(
        result["schema_error"].as_str().unwrap().contains("\"age\""),
        "names the violation: {result}"
    );
    assert_eq!(result["structured"]["name"], "Ada");
    assert_eq!(invocations(&log).len(), 2);
}
// llmlint: ignore-end[e2e_not_mocked]

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its structured-output loop run; only the harness process (codex) is the mock harness, a session double recording the captured rollout token counts (`tests/fixtures/codex-rollout-token-counts.jsonl`), held to its real CLI by the 0.159.2 runs the `schema` row cites (module doc).
/// A schema strict mode accepts reaches codex natively, with and without an
/// image: `--output-schema` names a file holding the schema, the image rides
/// `--image=`, and the prompt carries no schema instruction. An invalid first
/// answer is re-asked, and `calls` lists both turns' calls from the rollout.
#[test]
fn codex_takes_a_strict_schema_natively_with_and_without_an_image() {
    let scratch = ScratchDir::new("lean-codex-native-schema").unwrap();
    let image = png(&scratch, "frame.png").display().to_string();
    let schema = scratch.join("person.json");
    std::fs::write(&schema, PERSON).unwrap();
    let schema = schema.display().to_string();
    let b = bin("codex");
    let answers = r#"["{\"name\":\"Ada\"}","{\"name\":\"Ada\",\"age\":36}"]"#;
    for with_image in [false, true] {
        let tag = if with_image { "image" } else { "plain" };
        let log = scratch.join(format!("{tag}.jsonl"));
        let counter = scratch.join(format!("{tag}.count"));
        let home = scratch.join(format!("{tag}-home"));
        let home_env = format!("CODEX_HOME={}", home.display());
        let mut args = vec![
            "run",
            "--harness",
            "codex",
            "--prompt",
            "who is this?",
            "--schema",
            &schema,
            "--bin",
            &b,
            "--mode",
            "bypass",
            "--env",
            &home_env,
            "--env",
            "MOCK_CODEX_SESSIONS=1",
        ];
        if with_image {
            args.extend(["--image", &image]);
        }
        let out = oh(
            &args,
            &[
                ("MOCK_INVOCATION_LOG", log.to_str().unwrap()),
                ("MOCK_ATTEMPT_FILE", counter.to_str().unwrap()),
                ("MOCK_CODEX_ANSWERS", answers),
            ],
            None,
        );
        assert!(out.status.success(), "{tag}: {out:?}");
        let result = json(&out)["results"][0].clone();
        assert_eq!(result["status"], "ok", "{tag}: {result}");
        assert_eq!(result["structured"]["age"], 36, "{tag}: {result}");
        assert_eq!(result["schema_valid"], true, "{tag}");
        assert!(result["schema_error"].is_null(), "{tag}");
        assert_eq!(result["schema_attempts"], 2, "{tag}");
        let inputs: Vec<u64> = input_and_output_tokens(&result)
            .into_iter()
            .map(|(input, _)| input)
            .collect();
        assert_eq!(
            inputs,
            vec![13_006, 13_101, 13_180, 13_006, 13_101, 13_180],
            "{tag}: each turn's recorded calls, both attempts, in order"
        );
        let calls = invocations(&log);
        assert_eq!(calls.len(), 2, "{tag}");
        for (attempt, call) in calls.iter().enumerate() {
            let argv = strings(&call["argv"]);
            let path = after(&argv, "--output-schema").unwrap_or_else(|| panic!("{argv:?}"));
            let delivered: Value = serde_json::from_str(&delivered_file(call, path)).unwrap();
            assert_eq!(delivered, serde_json::from_str::<Value>(PERSON).unwrap());
            assert!(
                !Path::new(path).exists(),
                "the schema file outlived the run"
            );
            assert_eq!(
                argv.iter().any(|a| a.starts_with("--image=")),
                with_image,
                "{argv:?}"
            );
            let stdin = call["stdin"].as_str().unwrap();
            assert!(
                !stdin.contains(PROMPT_SCHEMA_INSTRUCTION),
                "{tag} {attempt}: {stdin}"
            );
        }
    }

    // Still invalid after `schema_max_retries` re-asks: a schema failure.
    let home = scratch.join("exhausted-home");
    let home_env = format!("CODEX_HOME={}", home.display());
    let counter = scratch.join("exhausted.count");
    let out = oh(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "who is this?",
            "--schema",
            &schema,
            "--schema-max-retries",
            "1",
            "--bin",
            &b,
            "--mode",
            "bypass",
            "--env",
            &home_env,
            "--env",
            "MOCK_CODEX_SESSIONS=1",
        ],
        &[
            ("MOCK_ATTEMPT_FILE", counter.to_str().unwrap()),
            ("MOCK_CODEX_ANSWERS", r#"["{\"name\":\"Ada\"}"]"#),
        ],
        None,
    );
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let result = json(&out)["results"][0].clone();
    assert_eq!(result["schema_valid"], false, "{result}");
    assert_eq!(result["schema_attempts"], 2);
    assert!(
        result["schema_error"].as_str().unwrap().contains("\"age\""),
        "names the violation: {result}"
    );
}
// llmlint: ignore-end[e2e_not_mocked]

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its structured-output loop run; only the harness process (codex) is the mock harness, a session double recording the captured rollout token counts (`tests/fixtures/codex-rollout-token-counts.jsonl`), held to its real CLI by the 0.159.2 runs the `schema` row cites (module doc).
/// A re-asked codex run whose earlier attempt's rollout record is gone by the
/// time the run reads it still lists the final attempt's calls, and says
/// nothing is missing: only the final attempt's absence is the run's to report.
#[test]
fn a_codex_re_ask_whose_earlier_rollout_is_gone_lists_the_final_attempts_calls() {
    let scratch = ScratchDir::new("lean-codex-gone-rollout").unwrap();
    let schema = scratch.join("person.json");
    std::fs::write(&schema, PERSON).unwrap();
    let schema = schema.display().to_string();
    let b = bin("codex");
    let home_env = format!("CODEX_HOME={}", scratch.join("home").display());
    let counter = scratch.join("attempts.count");
    let out = oh(
        &[
            "run",
            "--harness",
            "codex",
            "--prompt",
            "who is this?",
            "--schema",
            &schema,
            "--bin",
            &b,
            "--mode",
            "bypass",
            "--env",
            &home_env,
            "--env",
            "MOCK_CODEX_SESSIONS=1",
        ],
        &[
            ("MOCK_ATTEMPT_FILE", counter.to_str().unwrap()),
            (
                "MOCK_CODEX_ANSWERS",
                r#"["{\"name\":\"Ada\"}","{\"name\":\"Ada\",\"age\":36}"]"#,
            ),
            ("MOCK_CODEX_UNRECORDED_ATTEMPTS", "1"),
        ],
        None,
    );
    assert!(out.status.success(), "{out:?}");
    let result = json(&out)["results"][0].clone();
    assert_eq!(result["schema_attempts"], 2, "{result}");
    assert_eq!(result["structured"]["age"], 36, "{result}");
    let inputs: Vec<u64> = input_and_output_tokens(&result)
        .into_iter()
        .map(|(input, _)| input)
        .collect();
    assert_eq!(
        inputs,
        vec![13_006, 13_101, 13_180],
        "the final attempt's calls alone: {result}"
    );
    assert!(diagnostics_for(&result, "calls").is_empty(), "{result}");
}
// llmlint: ignore-end[e2e_not_mocked]

/// One OpenCode `run --format json` turn answering `text` in one step, in the
/// shape of `tests/fixtures/opencode_run.jsonl`'s `text` and `step_finish`
/// events (composed: no real re-asked OpenCode capture is in the repository).
fn opencode_turn(text: &str, input: u64, output: u64) -> String {
    [
        serde_json::json!({"type": "text", "sessionID": "ses_1", "part": {"type": "text", "text": text}}),
        serde_json::json!({"type": "step_finish", "sessionID": "ses_1", "part": {"type": "step-finish", "reason": "stop", "tokens": {"input": input, "output": output}, "cost": 0.01}}),
    ]
    .map(|event| event.to_string())
    .join("\n")
}

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its structured-output loop run; only the harness process (opencode) is the mock harness, replaying events in the shape of the real capture `tests/fixtures/opencode_run.jsonl`, held to its real CLI by `live-opencode` (module doc).
/// A re-asked OpenCode schema run lists every attempt's calls, in order, both
/// alone and as the candidate a fallback chain runs once the one before it
/// could not start.
#[test]
fn a_re_asked_opencode_run_lists_every_attempts_calls_alone_and_in_a_fallback_chain() {
    let scratch = ScratchDir::new("lean-opencode-retry-calls").unwrap();
    let schema = scratch.join("person.json");
    std::fs::write(&schema, PERSON).unwrap();
    let schema = schema.display().to_string();
    let (first, second) = (
        opencode_turn(r#"{"name":"Ada"}"#, 11, 3),
        opencode_turn(r#"{"name":"Ada","age":36}"#, 13, 5),
    );
    let o = bin("opencode");
    let missing = format!("goose={}", scratch.join("no-such-goose").display());
    for chain in [false, true] {
        let counter = scratch.join(format!("{chain}.count"));
        let mut args = vec!["run", "--prompt", "who is this?", "--schema", &schema];
        if chain {
            args.extend(["--harness", "goose,opencode", "--bin", &missing]);
        } else {
            args.extend(["--harness", "opencode"]);
        }
        args.extend(["--bin", &o, "--mode", "bypass"]);
        let out = oh(
            &args,
            &[
                ("MOCK_ATTEMPT_FILE", counter.to_str().unwrap()),
                ("MOCK_STDOUT_1", &first),
                ("MOCK_STDOUT_2", &second),
            ],
            None,
        );
        assert!(out.status.success(), "chain={chain}: {out:?}");
        let report = json(&out);
        let results = report["results"].as_array().unwrap();
        let result = results.last().unwrap();
        assert_eq!(result["harness"], "opencode", "{report}");
        if chain {
            assert_eq!(report["fallback"]["ran"], "opencode", "{report}");
            assert_eq!(report["fallback"]["fell_through"][0]["harness"], "goose");
        }
        assert_eq!(result["schema_attempts"], 2, "chain={chain}: {result}");
        assert_eq!(result["structured"]["age"], 36, "chain={chain}: {result}");
        assert_eq!(
            input_and_output_tokens(result),
            vec![(11, 3), (13, 5)],
            "chain={chain}: both attempts' calls, in order: {result}"
        );
        assert!(diagnostics_for(result, "calls").is_empty(), "{result}");
    }
}
// llmlint: ignore-end[e2e_not_mocked]

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its translation run; only the harness process (claude-code, goose) is the mock harness, held to its real CLI by the request captures the `replace_system` rows cite (module doc).
/// `replace_system` with no system text is a no-op: Claude Code is handed no
/// system flag at all, and a harness with no replacing knob runs rather than
/// being refused, since there is no text it would have to replace.
#[test]
fn replace_system_without_system_text_changes_nothing() {
    let scratch = ScratchDir::new("lean-replace-no-text").unwrap();
    for id in ["claude-code", "goose"] {
        let log = scratch.join(format!("{id}.jsonl"));
        let log_env = format!("MOCK_INVOCATION_LOG={}", log.display());
        let b = bin(id);
        let out = oh(
            &[
                "run",
                "--harness",
                id,
                "--prompt",
                "status?",
                "--replace-system",
                "--bin",
                &b,
                "--mode",
                "bypass",
                "--env",
                &log_env,
            ],
            &[],
            None,
        );
        assert!(out.status.success(), "{id}: {out:?}");
        let result = json(&out)["results"][0].clone();
        assert_eq!(result["status"], "ok", "{id}: {result}");
        assert!(result["failure_kind"].is_null(), "{id}: {result}");
        let calls = invocations(&log);
        assert_eq!(calls.len(), 1, "{id} ran once");
        let argv = strings(&calls[0]["argv"]);
        assert!(
            !argv.iter().any(|a| a.contains("system-prompt")),
            "{id}: {argv:?}"
        );
    }
}
// llmlint: ignore-end[e2e_not_mocked]

// llmlint: ignore-block[e2e_not_mocked] The real `oneharness` binary and its structured-output loop run; only the harness process (codex) is the mock harness, a session double recording the captured rollout token counts (`tests/fixtures/codex-rollout-token-counts.jsonl`), held to its real CLI by the 0.159.2 refusals the `schema` row cites (module doc).
/// A schema strict mode would refuse keeps codex on the prompt path it took
/// before: no `--output-schema`, the instruction in the prompt, an invalid
/// answer re-asked with every attempt's calls listed, and a still-invalid one
/// reported as a schema failure. One shape per refusal the probe saw: an
/// optional property, and `oneOf`.
#[test]
fn codex_keeps_a_schema_strict_mode_refuses_on_the_prompt_path() {
    let scratch = ScratchDir::new("lean-codex-loose-schema").unwrap();
    let b = bin("codex");
    let loose = [
        (
            "optional",
            r#"{"type":"object","properties":{"name":{"type":"string"},"age":{"type":"integer"}},"required":["name"]}"#,
        ),
        (
            "one-of",
            r#"{"type":"object","properties":{"name":{"type":"string"},"age":{"oneOf":[{"type":"integer"},{"type":"null"}]}},"required":["name","age"],"additionalProperties":false}"#,
        ),
    ];
    for (tag, text) in loose {
        let schema = scratch.join(format!("{tag}.json"));
        std::fs::write(&schema, text).unwrap();
        let schema = schema.display().to_string();
        let home_env = format!(
            "CODEX_HOME={}",
            scratch.join(format!("{tag}-home")).display()
        );
        let run = |answers: &str, attempt: &str, log: Option<&Path>| {
            let counter = scratch.join(format!("{tag}-{attempt}.count"));
            let mut env = vec![
                ("MOCK_ATTEMPT_FILE", counter.to_str().unwrap().to_string()),
                ("MOCK_CODEX_ANSWERS", answers.to_string()),
            ];
            if let Some(log) = log {
                env.push(("MOCK_INVOCATION_LOG", log.to_str().unwrap().to_string()));
            }
            let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
            oh(
                &[
                    "run",
                    "--harness",
                    "codex",
                    "--prompt",
                    "who is this?",
                    "--schema",
                    &schema,
                    "--schema-max-retries",
                    "1",
                    "--bin",
                    &b,
                    "--mode",
                    "bypass",
                    "--env",
                    &home_env,
                    "--env",
                    "MOCK_CODEX_SESSIONS=1",
                ],
                &env,
                None,
            )
        };

        let log = scratch.join(format!("{tag}.jsonl"));
        let out = run(
            r#"["{\"age\":36}","{\"name\":\"Ada\",\"age\":36}"]"#,
            "reask",
            Some(&log),
        );
        assert!(out.status.success(), "{tag}: {out:?}");
        let result = json(&out)["results"][0].clone();
        assert_eq!(result["structured"]["name"], "Ada", "{tag}: {result}");
        assert_eq!(result["schema_valid"], true, "{tag}");
        assert!(result["schema_error"].is_null(), "{tag}");
        assert_eq!(result["schema_attempts"], 2, "{tag}");
        let inputs: Vec<u64> = input_and_output_tokens(&result)
            .into_iter()
            .map(|(input, _)| input)
            .collect();
        assert_eq!(
            inputs,
            vec![13_006, 13_101, 13_180, 13_006, 13_101, 13_180],
            "{tag}: each turn's recorded calls, both attempts, in order"
        );
        let calls = invocations(&log);
        assert_eq!(calls.len(), 2, "{tag}");
        for call in &calls {
            let argv = strings(&call["argv"]);
            assert!(
                !argv.iter().any(|a| a == "--output-schema"),
                "{tag}: {argv:?}"
            );
            assert!(
                call["stdin"]
                    .as_str()
                    .unwrap()
                    .contains(PROMPT_SCHEMA_INSTRUCTION),
                "{tag}: {call}"
            );
        }

        let out = run(r#"["{\"age\":36}"]"#, "exhausted", None);
        assert_eq!(out.status.code(), Some(1), "{tag}: {out:?}");
        let result = json(&out)["results"][0].clone();
        assert_eq!(result["schema_valid"], false, "{tag}: {result}");
        assert_eq!(result["schema_attempts"], 2, "{tag}");
        assert!(
            result["schema_error"]
                .as_str()
                .unwrap()
                .contains("\"name\""),
            "{tag}: names the violation: {result}"
        );
    }
}
// llmlint: ignore-end[e2e_not_mocked]
