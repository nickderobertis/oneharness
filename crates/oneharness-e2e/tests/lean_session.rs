//! End-to-end journeys for the settings that keep a long continued session
//! cheap: `auto_compact_tokens`, the lean tool set (`tool_set`, `load_skills`,
//! `load_mcp`), `images`, per-call `calls`, and `oneharness compact`.
//!
//! Each drives the real `oneharness` binary with the mock harness standing in
//! for the harness process, and asserts what that process received (its argv,
//! environment and stdin, recorded by the mock) or what the report says.

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

/// The `ONEHARNESS_*` overrides a hermetic config journey strips.
const AMBIENT: &[&str] = &[
    "ONEHARNESS_AUTO_COMPACT_TOKENS",
    "ONEHARNESS_TOOL_SET",
    "ONEHARNESS_LOAD_SKILLS",
    "ONEHARNESS_LOAD_MCP",
    "ONEHARNESS_TIMEOUT",
    "ONEHARNESS_HARNESSES",
    "ONEHARNESS_MODE",
    "ONEHARNESS_OUTPUT_FORMAT",
    "ONEHARNESS_RUN_MODE",
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

fn mock() -> String {
    workspace_bin("oneharness-mock-harness")
        .display()
        .to_string()
}

fn bin(id: &str) -> String {
    format!("{id}={}", mock())
}

/// One invocation of the binary. `config` pins the user config (and lets
/// project discovery run); `None` is hermetic (`ONEHARNESS_NO_CONFIG=1`).
/// Every `run`/`compact` child gets the coverage-profile redirect, because a
/// cancelled or timed-out mock never flushes its profile.
fn oh(args: &[&str], envs: &[(&str, &str)], config: Option<&Path>) -> Output {
    let profiles = ProfileScratch::new();
    let mut cmd = Command::new(workspace_bin("oneharness"));
    match config {
        Some(user) => {
            cmd.env("ONEHARNESS_CONFIG", user);
        }
        None => {
            cmd.env("ONEHARNESS_NO_CONFIG", "1");
        }
    }
    for var in AMBIENT {
        cmd.env_remove(var);
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

// ---------------------------------------------------------------- config ----

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

    // The config report attributes each to its layer.
    let cwd = file.cwd();
    let out = oh(&["config", "--cwd", &cwd], &[], Some(&file.user()));
    let report = json(&out);
    assert_eq!(report["auto_compact_tokens"]["value"], 1000);
}

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

// --------------------------------------------------------------- delivery ----

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

// ----------------------------------------------------------------- images ----

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

// ------------------------------------------------------------------ calls ----

#[test]
fn calls_list_one_usage_per_model_call_where_the_harness_records_them() {
    let scratch = ScratchDir::new("lean-calls").unwrap();
    let claude_stream = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/claude-stream-json-turn.jsonl"),
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
}

// ---------------------------------------------------------------- compact ----

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
        assert_eq!(
            after["results"][0]["text"], "context: [summary of: alpha, beta, gamma]",
            "{id}: the compacted context, not the original"
        );
    }
}

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
}
