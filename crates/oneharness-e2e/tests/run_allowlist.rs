//! End-to-end journeys for the run-level allowlist: `allowed_tools` outside
//! `[sync]`, applied by `oneharness run` to that one run and written nowhere
//! that outlives it.
//!
//! Each drives the real `oneharness` binary over a TOML config, with the mock
//! harness standing in for the harness process, and asserts what that process
//! received — its argv, its environment and, for Codex, what its per-run
//! overlay home held while it ran (`MOCK_HOME_SNAPSHOT`) — and what the report
//! and the disk say afterwards.
//!
//! Why the harness process is the mock, stated once for every journey's
//! `e2e_not_mocked` suppression below: the deterministic gate cannot run the
//! paid, authenticated CLIs, so only that process is replaced, at the existing
//! `--bin` seam. What holds the mechanism to the real harnesses is
//! `live/control/run_allowlist_live.py` (`just live-run-allowlist`), which drives
//! the same configs through the installed Claude Code and Codex against a
//! scripted model endpoint and asserts what each CLI's own permission check,
//! sandbox and execution did.
//!
//! Linux only: these are the journeys of a grant that applies, and on every
//! other platform every harness withholds it (`grant::platform_withheld_reason`),
//! which `run_allowlist_platform.rs` proves on every leg, macOS and
//! windows-latest included.
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use oneharness_core::io::scratch::ScratchDir;
use serde_json::Value;

#[path = "../../../tests/profile-scratch/profile_scratch.rs"]
mod profile_scratch;
use profile_scratch::ProfileScratch;

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

// llmlint: ignore-block[e2e_not_mocked] These journeys must drive the real built `oneharness` against the repository's mock harness standing in for each harness's process: that is this change's stated acceptance bar, since the paid, authenticated CLIs cannot run in the deterministic gate. Only the harness subprocess is replaced, at the existing `--bin` seam; live/control/run_allowlist_live.py holds the same configs to the real Claude Code and Codex.
fn mock() -> String {
    workspace_bin("oneharness-mock-harness")
        .display()
        .to_string()
}

fn bin(id: &str) -> String {
    format!("{id}={}", mock())
}
// llmlint: ignore-end[e2e_not_mocked]

/// One journey's world: a project directory, a temp dir the binary is pointed
/// at (so the overlays it makes, and must remove, are this journey's alone), a
/// real Codex home, and the granted executable.
struct World {
    dir: ScratchDir,
}

/// The executable every journey grants.
const TOOL: &str = "grant-tool";

impl World {
    fn new(tag: &str) -> World {
        let dir = ScratchDir::new(&format!("run-allowlist-{tag}")).unwrap();
        for sub in [
            "project",
            "tmp",
            "state",
            "home",
            "bin",
            "codex-home/sessions",
            "codex-home/rules",
        ] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        std::fs::write(
            dir.join("codex-home/auth.json"),
            "{\"auth_mode\":\"apikey\"}\n",
        )
        .unwrap();
        std::fs::write(dir.join("codex-home/config.toml"), "model = \"m\"\n").unwrap();
        std::fs::write(
            dir.join("codex-home/rules/user.rules"),
            "prefix_rule(pattern=[\"ls\"], decision=\"allow\")\n",
        )
        .unwrap();
        std::fs::write(dir.join("bin").join(TOOL), "#!/bin/sh\n").unwrap();
        std::fs::write(dir.join("user.toml"), "").unwrap();
        World { dir }
    }

    fn tool(&self) -> String {
        self.dir.join("bin").join(TOOL).display().to_string()
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.dir.join(rel)
    }

    fn s(&self, rel: &str) -> String {
        self.path(rel).display().to_string()
    }

    fn config(&self, toml: &str) -> PathBuf {
        let path = self.dir.join("oneharness.toml");
        std::fs::write(&path, toml).unwrap();
        path
    }

    /// `oneharness <args>` under `config`, with the world's temp dir, home and
    /// Codex home, and `envs` on the process.
    fn oh(&self, args: &[&str], config: &Path, envs: &[(&str, &str)]) -> Output {
        let profiles = ProfileScratch::new();
        self.command(args, config, envs, &profiles)
            .output()
            .expect("run oneharness")
    }

    /// The `oneharness <args>` [`World::oh`] runs, to spawn instead.
    fn command(
        &self,
        args: &[&str],
        config: &Path,
        envs: &[(&str, &str)],
        profiles: &ProfileScratch,
    ) -> Command {
        let mut cmd = Command::new(workspace_bin("oneharness"));
        for (var, _) in std::env::vars().filter(|(var, _)| {
            var.starts_with("ONEHARNESS_") || var.starts_with("MOCK_") || var == "CODEX_HOME"
        }) {
            cmd.env_remove(var);
        }
        cmd.env("ONEHARNESS_CONFIG", self.path("user.toml"))
            .env("TMPDIR", self.path("tmp"))
            .env("TMP", self.path("tmp"))
            .env("TEMP", self.path("tmp"))
            .env("HOME", self.path("home"))
            .env("XDG_STATE_HOME", self.path("state"))
            .env("CODEX_HOME", self.path("codex-home"));
        let mut argv: Vec<String> = args.iter().map(|a| (*a).to_string()).collect();
        if args.first() == Some(&"run") {
            argv.extend([
                "--env".into(),
                profiles.as_str().into(),
                "--cwd".into(),
                self.s("project"),
                "--config".into(),
                config.display().to_string(),
            ]);
            for id in [
                "claude-code",
                "codex",
                "opencode",
                "goose",
                "qwen",
                "crush",
                "copilot",
                "cursor",
            ] {
                argv.extend(["--bin".into(), bin(id)]);
            }
        } else if args.first() != Some(&"list") {
            argv.extend(["--config".into(), config.display().to_string()]);
            argv.extend(["--cwd".into(), self.s("project")]);
        }
        argv.extend(["--format".into(), "json".into()]);
        cmd.args(&argv);
        for (key, value) in envs {
            cmd.env(key, value);
        }
        cmd
    }

    /// Where the binary makes its run overlays in this world.
    fn overlay_root(&self) -> PathBuf {
        self.path("state/oneharness/run-overlays")
    }

    /// Every run overlay still in the world, wherever one could be made.
    fn overlays(&self) -> Vec<String> {
        [self.path("tmp"), self.overlay_root()]
            .iter()
            .filter_map(|dir| std::fs::read_dir(dir).ok())
            .flatten()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains("run-overlay"))
            .collect()
    }
}

/// The `MOCK_HOME_SNAPSHOT` line a harness took of its run overlay's home (a
/// run's helper app-servers snapshot their own homes beside it).
fn overlay_snapshot(path: &Path) -> Value {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{} was not recorded: {e}", path.display()))
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|snap| {
            snap["home"]
                .as_str()
                .is_some_and(|home| home.contains("run-overlay") && home.ends_with("codex-home"))
        })
        .next_back()
        .unwrap_or_else(|| panic!("{} holds no overlay home's snapshot", path.display()))
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

fn argv_of(file: &Path) -> Vec<String> {
    std::fs::read_to_string(file)
        .unwrap_or_else(|e| panic!("{} was not recorded: {e}", file.display()))
        .lines()
        .map(str::to_string)
        .collect()
}

fn env_of(file: &Path) -> serde_json::Map<String, Value> {
    let mut env: serde_json::Map<String, Value> =
        serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
    // Per-invocation by construction: the coverage profile path.
    env.remove("LLVM_PROFILE_FILE");
    env
}

/// What one granted (or ungranted) run delivered to its harness.
struct Spawn {
    report: Value,
    argv: Vec<String>,
    env: serde_json::Map<String, Value>,
}

fn spawn(world: &World, harness: &str, mode: &str, toml: &str, extra: &[&str]) -> Spawn {
    let config = world.config(toml);
    let argv_file = world.path("argv");
    let env_file = world.path("env.json");
    let _ = std::fs::remove_file(&argv_file);
    let _ = std::fs::remove_file(&env_file);
    let mut args = vec![
        "run",
        "--harness",
        harness,
        "--mode",
        mode,
        "--prompt",
        "judge it",
    ];
    args.extend_from_slice(extra);
    let argv_s = argv_file.display().to_string();
    let env_s = env_file.display().to_string();
    let output = world.oh(
        &args,
        &config,
        &[("MOCK_ARGV_FILE", &argv_s), ("MOCK_ENV_FILE", &env_s)],
    );
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        output.status.code().is_some_and(|c| c == 0 || c == 1),
        "{harness} {mode}: {stderr}"
    );
    if harness == "codex"
        && toml.contains("allowed_tools")
        && world.path("codex-home/state_5.sqlite").exists()
    {
        // The silent helper's warnings, not a failure.
        assert!(
            stderr.contains("could not build the run overlay's Codex thread index"),
            "{stderr}"
        );
        assert!(
            stderr.contains("could not record the granted run's Codex thread"),
            "{stderr}"
        );
    }
    Spawn {
        report: json(&output),
        argv: argv_of(&argv_file),
        env: env_of(&env_file),
    }
}

const READ_TOOLS: [&str; 5] = ["Read", "Grep", "Glob", "WebFetch", "WebSearch"];

/// The flag a Claude Code argv carries and every value after it up to the
/// next flag.
fn flag_values<'a>(argv: &'a [String], flag: &str) -> Vec<&'a str> {
    let Some(at) = argv.iter().position(|a| a == flag) else {
        return Vec::new();
    };
    argv[at + 1..]
        .iter()
        .take_while(|a| !a.starts_with("--"))
        .map(String::as_str)
        .collect()
}

fn flag_value<'a>(argv: &'a [String], flag: &str) -> Option<&'a str> {
    argv.iter()
        .position(|a| a == flag)
        .and_then(|at| argv.get(at + 1))
        .map(String::as_str)
}

#[test]
fn claude_code_gets_the_resolved_grant_on_its_own_flags_and_nothing_written() {
    let world = World::new("claude");
    let tool = world.tool();
    let toml = format!("allowed_tools = [\"Bash($GRANTED)\"]\n[env]\nGRANTED = \"{tool}\"\n");
    let before = tree(&world.path("project"));
    let run = spawn(&world, "claude-code", "read-only", &toml, &[]);
    let argv = &run.argv;
    assert_eq!(
        flag_value(argv, "--permission-mode"),
        Some("dontAsk"),
        "{argv:?}"
    );
    let mut tools = READ_TOOLS.to_vec();
    tools.push("Bash");
    assert_eq!(flag_values(argv, "--tools"), tools);
    let mut allowed = READ_TOOLS.to_vec();
    let rule = format!("Bash({tool}:*)");
    allowed.push(&rule);
    assert_eq!(flag_values(argv, "--allowedTools"), allowed);
    assert!(argv.iter().all(|a| !a.contains("$GRANTED")), "{argv:?}");
    // Claude Code gets its grant on the argv alone: no overlay, no env.
    assert!(!run.env.contains_key("ONEHARNESS_RUN_OVERLAY"));
    assert!(run.report["results"][0].get("overlay_env").is_none());
    assert_eq!(
        tree(&world.path("project")),
        before,
        "the project gained a file"
    );
    assert!(world.overlays().is_empty());
    assert!(
        run.report["results"][0]["diagnostics"]
            .as_array()
            .is_none_or(|d| d.iter().all(|d| d["setting"] != "allowed_tools")),
        "{}",
        run.report
    );
}

#[test]
fn outside_read_only_the_rules_join_claude_codes_allowed_tools() {
    let world = World::new("claude-modes");
    let tool = world.tool();
    let toml = format!("allowed_tools = [\"Bash({tool}:*)\", \"Edit\"]\n");
    for (mode, native) in [
        ("default", "dontAsk"),
        ("plan", "plan"),
        ("auto", "auto"),
        ("bypass", "bypassPermissions"),
    ] {
        let run = spawn(&world, "claude-code", mode, &toml, &[]);
        assert_eq!(
            flag_value(&run.argv, "--permission-mode"),
            Some(native),
            "{mode}"
        );
        assert_eq!(
            flag_values(&run.argv, "--allowedTools"),
            [format!("Bash({tool}:*)").as_str(), "Edit"],
            "{mode}"
        );
        assert!(
            flag_values(&run.argv, "--tools").is_empty(),
            "{mode}: {:?}",
            run.argv
        );
    }
}

/// Every entry under `dir`, with its bytes (or link target), for a
/// before/after comparison.
fn tree(dir: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let rel = path.strip_prefix(root).unwrap().display().to_string();
            let meta = std::fs::symlink_metadata(&path).unwrap();
            if meta.file_type().is_symlink() {
                out.push((
                    rel,
                    format!("-> {}", std::fs::read_link(&path).unwrap().display()),
                ));
            } else if meta.is_dir() {
                out.push((rel.clone(), "/".into()));
                walk(root, &path, out);
            } else {
                out.push((
                    rel,
                    String::from_utf8_lossy(&std::fs::read(&path).unwrap()).into_owned(),
                ));
            }
        }
    }
    walk(dir, dir, &mut out);
    out.sort();
    out
}

#[cfg(unix)]
#[test]
fn codex_runs_on_an_overlay_home_linking_the_real_one_with_one_rules_file_of_its_own() {
    let world = World::new("codex");
    let tool = world.tool();
    let toml = format!("allowed_tools = [\"Bash($GRANTED:*)\"]\n[env]\nGRANTED = \"{tool}\"\n");
    let config = world.config(&toml);
    let snapshot = world.path("snapshot.json");
    let env_file = world.path("env.json");
    let real_before = tree(&world.path("codex-home"));
    let project_before = tree(&world.path("project"));
    let output = world.oh(
        &[
            "run",
            "--harness",
            "codex",
            "--mode",
            "read-only",
            "--prompt",
            "judge it",
        ],
        &config,
        &[
            ("MOCK_HOME_SNAPSHOT", snapshot.to_str().unwrap()),
            ("MOCK_ENV_FILE", env_file.to_str().unwrap()),
        ],
    );
    let report = json(&output);
    let result = &report["results"][0];
    let env = env_of(&env_file);
    let home = env["CODEX_HOME"].as_str().unwrap();
    assert_eq!(result["overlay_env"]["CODEX_HOME"], home);
    for phase in ["build_ms", "teardown_ms"] {
        assert!(
            result["overlay_timings"][phase].is_u64(),
            "{phase}: {result}"
        );
    }
    // The overlay home is the run's own: under oneharness's state dir,
    // outside the project, the real home and the temp dir.
    assert!(
        home.starts_with(&world.overlay_root().display().to_string()),
        "{home}"
    );
    assert!(home.contains("run-overlay"), "{home}");
    assert!(env["ONEHARNESS_RUN_OVERLAY"]
        .as_str()
        .is_some_and(|m| home.contains(m)));
    let snap = overlay_snapshot(&snapshot);
    let entries = &snap["entries"];
    for name in ["auth.json", "config.toml", "sessions"] {
        assert_eq!(
            entries[name]["link"],
            world.s(&format!("codex-home/{name}")),
            "{name}: {entries}"
        );
    }
    let rules = &entries["rules"]["dir"];
    assert_eq!(
        rules["user.rules"]["link"],
        world.s("codex-home/rules/user.rules")
    );
    let run_rules = rules["oneharness-run.rules"]["file"].as_str().unwrap();
    assert!(
        run_rules.contains(&format!(
            "prefix_rule(pattern=[\"{tool}\"], decision=\"allow\")"
        )),
        "{run_rules}"
    );
    assert!(
        !run_rules.contains('$'),
        "a rule names a variable: {run_rules}"
    );
    // Today's sandbox is unchanged: the grant rides the rules file alone.
    let argv: Vec<&str> = result["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap())
        .collect();
    assert!(
        argv.windows(2).any(|w| w == ["--sandbox", "read-only"]),
        "{argv:?}"
    );
    // Nothing outlives the run, and nothing reached the project or the home.
    assert!(world.overlays().is_empty(), "{:?}", world.overlays());
    assert!(!Path::new(home).exists());
    assert!(
        !world.overlay_root().exists(),
        "the emptied overlay root outlived the run"
    );
    assert_eq!(tree(&world.path("codex-home")), real_before);
    assert_eq!(tree(&world.path("project")), project_before);
}

#[cfg(unix)]
#[test]
fn outside_read_only_codex_takes_its_rules_through_the_same_overlay() {
    let world = World::new("codex-modes");
    let tool = world.tool();
    let toml = format!("allowed_tools = [\"Bash({tool}:*)\", \"Edit\"]\n");
    let config = world.config(&toml);
    for mode in ["default", "auto", "bypass"] {
        let snapshot = world.path(&format!("snapshot-{mode}.json"));
        let output = world.oh(
            &[
                "run",
                "--harness",
                "codex",
                "--mode",
                mode,
                "--prompt",
                "judge it",
            ],
            &config,
            &[("MOCK_HOME_SNAPSHOT", snapshot.to_str().unwrap())],
        );
        let report = json(&output);
        if report["results"][0]["status"] == "error" || !snapshot.exists() {
            // `auto` is not a mode codex supports: refused before spawning.
            assert_eq!(output.status.code(), Some(2), "{mode}: {report}");
            continue;
        }
        let snap = overlay_snapshot(&snapshot);
        let run_rules = snap["entries"]["rules"]["dir"]["oneharness-run.rules"]["file"]
            .as_str()
            .unwrap_or_else(|| panic!("{mode}: {snap}"));
        assert!(
            run_rules.contains(&format!("pattern=[\"{tool}\"]")),
            "{mode}: {run_rules}"
        );
        let diagnostics = report["results"][0]["diagnostics"].as_array().unwrap();
        assert!(
            diagnostics.iter().any(|d| d["setting"] == "allowed_tools"
                && d["message"].as_str().unwrap().contains("`Edit`")),
            "{mode}: {diagnostics:?}"
        );
    }
    assert!(world.overlays().is_empty());
}

#[test]
fn the_grant_layers_as_allowed_tools_always_has() {
    let world = World::new("layering");
    let tool = world.tool();
    let parent = world.path("parent.toml");
    std::fs::write(
        &parent,
        format!(
            "model = \"inherited-model\"\nallowed_tools = [\"Bash({tool}-parent)\"]\n\
             [sync]\nallowed_tools = [\"Bash(git status:*)\"]\n"
        ),
    )
    .unwrap();
    let rules_of = |toml: &str, harness: &str| -> (Vec<String>, Vec<String>) {
        let run = spawn(&world, harness, "read-only", toml, &[]);
        let allowed = flag_values(&run.argv, "--allowedTools")
            .into_iter()
            .filter(|r| r.starts_with("Bash("))
            .map(str::to_string)
            .collect();
        (allowed, run.argv.clone())
    };
    // A child that sets only its grant keeps every key it inherits.
    let child = format!("extends = \"{}\"\n", parent.display());
    let (allowed, argv) = rules_of(&child, "claude-code");
    assert_eq!(allowed, [format!("Bash({tool}-parent:*)")]);
    assert_eq!(flag_value(&argv, "--model"), Some("inherited-model"));
    let child = format!(
        "extends = \"{}\"\nallowed_tools = [\"Bash({tool})\"]\n",
        parent.display()
    );
    let (allowed, argv) = rules_of(&child, "claude-code");
    assert_eq!(allowed, [format!("Bash({tool}:*)")]);
    assert_eq!(flag_value(&argv, "--model"), Some("inherited-model"));
    // `[harness.<id>]` beats top level, and a variant beats its harness.
    let toml = format!(
        "allowed_tools = [\"Bash({tool}-top)\"]\n\
         [harness.claude-code]\nallowed_tools = [\"Bash({tool}-harness)\"]\n\
         [harness.claude-code.variant.work]\nallowed_tools = [\"Bash({tool}-variant)\"]\n"
    );
    assert_eq!(
        rules_of(&toml, "claude-code").0,
        [format!("Bash({tool}-harness:*)")]
    );
    assert_eq!(
        rules_of(&toml, "claude-code:work").0,
        [format!("Bash({tool}-variant:*)")]
    );
    let toml = format!(
        "allowed_tools = [\"Bash({tool}-top)\"]\n[harness.claude-code.variant.work]\nmodel = \"w\"\n"
    );
    assert_eq!(
        rules_of(&toml, "claude-code:work").0,
        [format!("Bash({tool}-top:*)")]
    );
}

#[test]
fn a_variable_resolves_from_the_runs_own_environment() {
    let world = World::new("resolve");
    let a = world.s("bin/from-env-table");
    let b = world.s("bin/from-flag");
    let c = world.s("bin/from-env-from");
    // An absolute `env_from` value names an identity home, which must exist.
    std::fs::write(&c, "").unwrap();
    let allowed = |run: &Spawn| -> Vec<String> {
        flag_values(&run.argv, "--allowedTools")
            .into_iter()
            .filter(|r| r.starts_with("Bash("))
            .map(str::to_string)
            .collect()
    };
    let toml = format!("allowed_tools = [\"Bash($GRANTED)\"]\n[env]\nGRANTED = \"{a}\"\n");
    let run = spawn(&world, "claude-code", "read-only", &toml, &[]);
    assert_eq!(allowed(&run), [format!("Bash({a}:*)")]);
    // `--env` wins over `[env]`, as it does for the spawn.
    let flag = format!("GRANTED={b}");
    let run = spawn(&world, "claude-code", "read-only", &toml, &["--env", &flag]);
    assert_eq!(allowed(&run), [format!("Bash({b}:*)")]);
    // A variant's `env_from` maps the name from another parent variable.
    let toml = String::from(
        "[harness.claude-code.variant.work]\nallowed_tools = [\"Bash($GRANTED)\"]\n\
         env_from = { GRANTED = \"SOURCE_OF_GRANT\" }\n",
    );
    let config = world.config(&toml);
    let argv_file = world.path("argv-env-from");
    let output = world.oh(
        &[
            "run",
            "--harness",
            "claude-code:work",
            "--mode",
            "read-only",
            "--prompt",
            "p",
        ],
        &config,
        &[
            ("SOURCE_OF_GRANT", &c),
            ("MOCK_ARGV_FILE", argv_file.to_str().unwrap()),
        ],
    );
    assert!(
        output.status.code().is_some_and(|code| code < 2),
        "{output:?}"
    );
    let argv = argv_of(&argv_file);
    assert!(flag_values(&argv, "--allowedTools").contains(&format!("Bash({c}:*)").as_str()));
    // `unset_env` removes the name the rule reads, which is then unset.
    let toml = format!(
        "allowed_tools = [\"Bash($GRANTED)\"]\n[env]\nGRANTED = \"{a}\"\n\
         [harness.claude-code.variant.bare]\nunset_env = [\"GRANTED\"]\n"
    );
    let config = world.config(&toml);
    let output = world.oh(
        &[
            "run",
            "--harness",
            "claude-code:bare",
            "--mode",
            "read-only",
            "--prompt",
            "p",
        ],
        &config,
        &[],
    );
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("`$GRANTED` is unset or empty"));
}

#[test]
fn an_unusable_rule_is_refused_before_anything_spawns() {
    let world = World::new("refusals");
    let argv_file = world.path("argv");
    let check = |toml: &str, mode: &str, envs: &[(&str, &str)], wants: &[&str]| {
        let config = world.config(toml);
        let mut envs = envs.to_vec();
        let argv_s = argv_file.display().to_string();
        envs.push(("MOCK_ARGV_FILE", &argv_s));
        for harness in ["claude-code", "codex", "opencode"] {
            let output = world.oh(
                &["run", "--harness", harness, "--mode", mode, "--prompt", "p"],
                &config,
                &envs,
            );
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert_eq!(output.status.code(), Some(2), "{harness} {toml}: {stderr}");
            for want in wants {
                assert!(
                    stderr.contains(want),
                    "{harness} {toml}: {want:?} not in {stderr}"
                );
            }
            assert!(stderr.contains("Nothing was spawned"), "{stderr}");
            assert!(
                !argv_file.exists(),
                "{harness} {toml}: a harness was spawned"
            );
            assert!(output.stdout.is_empty());
        }
    };
    check(
        "allowed_tools = [\"Bash(${GRANTED}:*)\"]\n",
        "default",
        &[],
        &["rule `Bash(${GRANTED}:*)`", "is not a variable name"],
    );
    check(
        "allowed_tools = [\"Bash($NOT_SET_ANYWHERE)\"]\n",
        "default",
        &[],
        &[
            "rule `Bash($NOT_SET_ANYWHERE)`",
            "`$NOT_SET_ANYWHERE` is unset or empty",
        ],
    );
    check(
        "allowed_tools = [\"Bash($GRANTED:*)\"]\n",
        "read-only",
        &[("GRANTED", "")],
        &["rule `Bash($GRANTED:*)`", "`$GRANTED` is unset or empty"],
    );
    check(
        "allowed_tools = [\"Bash($GRANTED)\"]\n",
        "default",
        &[("GRANTED", "bin/decline")],
        &[
            "rule `Bash($GRANTED)`",
            "holds `bin/decline`, which is not one absolute path",
        ],
    );
    for (rule, why) in [
        ("Bash(decline:*)", "not an absolute path"),
        ("Edit", "not a `Bash(...)` rule"),
        ("Write", "not a `Bash(...)` rule"),
        ("Read", "not a `Bash(...)` rule"),
        ("Bash(/usr/bin/*)", "wildcard"),
        ("Bash(/usr/bin/git status:*)", "several words"),
        ("Bash", "the whole shell"),
        ("Bash(:*)", "names no command"),
        ("Bash(/usr/bin/a;b)", "shell syntax"),
    ] {
        check(
            &format!("allowed_tools = [\"{rule}\"]\n"),
            "read-only",
            &[],
            &[&format!("rule `{rule}`"), "under `--mode read-only`", why],
        );
    }
}

#[test]
fn the_turn_is_told_the_granted_path_only_where_the_grant_applies() {
    let world = World::new("statement");
    let tool = world.tool();
    let toml = format!("allowed_tools = [\"Bash({tool})\"]\nsystem = \"be a judge\"\n");
    // Claude Code: the native system file carries the statement.
    let record = world.path("system.json");
    let config = world.config(&toml);
    let output = world.oh(
        &[
            "run",
            "--harness",
            "claude-code",
            "--mode",
            "read-only",
            "--prompt",
            "p",
        ],
        &config,
        &[("MOCK_SYSTEM_FILE_RECORD", record.to_str().unwrap())],
    );
    assert!(output.status.code().is_some_and(|c| c < 2), "{output:?}");
    let system: Value = serde_json::from_str(&std::fs::read_to_string(&record).unwrap()).unwrap();
    let contents = system["contents"].as_str().unwrap();
    assert!(contents.starts_with("be a judge"), "{contents}");
    assert!(contents.contains(&format!("`{tool}`")), "{contents}");
    // Codex: the statement is folded into the turn it is sent.
    let input = world.path("input.json");
    let output = world.oh(
        &[
            "run",
            "--harness",
            "codex",
            "--mode",
            "read-only",
            "--prompt",
            "p",
        ],
        &config,
        &[("MOCK_INPUT_FILE", input.to_str().unwrap())],
    );
    assert!(output.status.code().is_some_and(|c| c < 2), "{output:?}");
    let sent = std::fs::read_to_string(&input).unwrap();
    assert!(sent.contains(&format!("`{tool}`")), "{sent}");
    // A withheld grant, and a run with none, add nothing.
    for (harness, toml) in [
        ("opencode", toml.as_str()),
        ("claude-code", "system = \"be a judge\"\n"),
    ] {
        let config = world.config(toml);
        let _ = std::fs::remove_file(&input);
        let output = world.oh(
            &[
                "run",
                "--harness",
                harness,
                "--mode",
                "read-only",
                "--prompt",
                "p",
            ],
            &config,
            &[("MOCK_INPUT_FILE", input.to_str().unwrap())],
        );
        assert!(output.status.code().is_some_and(|c| c < 2), "{output:?}");
        let sent = std::fs::read_to_string(&input).unwrap();
        assert!(!sent.contains(&tool), "{harness}: {sent}");
        assert!(!sent.contains("This run may execute"), "{harness}: {sent}");
    }
}

#[test]
fn a_harness_that_withholds_runs_as_it_would_with_none_and_says_so() {
    let world = World::new("withheld");
    let tool = world.tool();
    // Read-only where the harness has it; the three that do not are held to
    // the same withholding under the mode they share with every harness.
    for (harness, mode) in [
        ("opencode", "read-only"),
        ("copilot", "read-only"),
        ("cursor", "read-only"),
        ("qwen", "read-only"),
        ("goose", "bypass"),
        ("crush", "bypass"),
    ] {
        let without = spawn(&world, harness, mode, "", &[]);
        let with = spawn(
            &world,
            harness,
            mode,
            &format!("allowed_tools = [\"Bash({tool})\"]\n"),
            &[],
        );
        assert_eq!(with.argv, without.argv, "{harness}");
        assert_eq!(with.env, without.env, "{harness}");
        let diagnostics = with.report["results"][0]["diagnostics"].as_array().unwrap();
        let withheld: Vec<&Value> = diagnostics
            .iter()
            .filter(|d| d["setting"] == "allowed_tools")
            .collect();
        assert_eq!(withheld.len(), 1, "{harness}: {diagnostics:?}");
        let message = withheld[0]["message"].as_str().unwrap();
        assert!(message.contains(&format!("`Bash({tool})`")), "{message}");
        assert!(
            message.contains(&format!("withheld on `{harness}`")),
            "{message}"
        );
        assert_eq!(withheld[0]["harness"], harness);
        assert!(without.report["results"][0]
            .get("diagnostics")
            .is_none_or(|d| {
                d.as_array()
                    .unwrap()
                    .iter()
                    .all(|d| d["setting"] != "allowed_tools")
            }));
    }
}

#[cfg(unix)]
#[test]
fn a_codex_login_in_the_keyring_withholds_the_grant() {
    let world = World::new("keyring");
    std::fs::write(
        world.path("codex-home/config.toml"),
        "cli_auth_credentials_store = \"keyring\"\n",
    )
    .unwrap();
    let tool = world.tool();
    let without = spawn(&world, "codex", "read-only", "", &[]);
    let with = spawn(
        &world,
        "codex",
        "read-only",
        &format!("allowed_tools = [\"Bash({tool})\"]\n"),
        &[],
    );
    assert_eq!(with.argv, without.argv);
    assert_eq!(with.env, without.env);
    assert_eq!(with.env["CODEX_HOME"], world.s("codex-home"));
    let message = with.report["results"][0]["diagnostics"][0]["message"]
        .as_str()
        .unwrap();
    assert!(message.contains("keyring"), "{message}");
    assert!(world.overlays().is_empty());
}

#[test]
fn with_no_run_level_allowlist_a_sync_section_changes_nothing_a_run_spawns() {
    let world = World::new("byte-identical");
    let tool = world.tool();
    let sync = format!("[sync]\nallowed_tools = [\"Bash({tool}:*)\", \"Bash(touch:*)\"]\n");
    for (harness, modes) in [
        (
            "claude-code",
            &["read-only", "default", "plan", "auto", "bypass"][..],
        ),
        ("codex", &["read-only", "default", "bypass"][..]),
    ] {
        for mode in modes {
            let project_before = tree(&world.path("project"));
            let bare = spawn(&world, harness, mode, "", &[]);
            let synced = spawn(&world, harness, mode, &sync, &[]);
            assert_eq!(synced.argv, bare.argv, "{harness} {mode}");
            assert_eq!(synced.env, bare.env, "{harness} {mode}");
            assert_eq!(
                tree(&world.path("project")),
                project_before,
                "{harness} {mode}"
            );
            assert!(synced.report["results"][0].get("overlay_env").is_none());
            assert!(world.overlays().is_empty());
        }
    }
}

#[test]
fn sync_writes_only_the_sync_section_and_run_applies_only_the_run_level_grant() {
    let world = World::new("both");
    let tool = world.tool();
    let toml = format!(
        "allowed_tools = [\"Bash({tool})\"]\n[sync]\nallowed_tools = [\"Bash(touch:*)\"]\n"
    );
    let config = world.config(&toml);
    let synced = world.oh(&["sync", "--harness", "claude-code,codex"], &config, &[]);
    assert!(synced.status.success(), "{synced:?}");
    let settings = std::fs::read_to_string(world.path("project/.claude/settings.json")).unwrap();
    let settings: Value = serde_json::from_str(&settings).unwrap();
    assert_eq!(
        settings["permissions"]["allow"],
        serde_json::json!(["Bash(touch:*)"])
    );
    let rules =
        std::fs::read_to_string(world.path("project/.codex/rules/oneharness.rules")).unwrap();
    assert!(rules.contains("pattern=[\"touch\"]"), "{rules}");
    assert!(!rules.contains(&tool), "{rules}");

    let run = spawn(&world, "claude-code", "read-only", &toml, &[]);
    let allowed = flag_values(&run.argv, "--allowedTools");
    assert!(
        allowed.contains(&format!("Bash({tool}:*)").as_str()),
        "{allowed:?}"
    );
    assert!(!allowed.iter().any(|r| r.contains("touch")), "{allowed:?}");
    #[cfg(unix)]
    {
        let snapshot = world.path("snapshot.json");
        let output = world.oh(
            &[
                "run",
                "--harness",
                "codex",
                "--mode",
                "read-only",
                "--prompt",
                "p",
            ],
            &config,
            &[("MOCK_HOME_SNAPSHOT", snapshot.to_str().unwrap())],
        );
        assert!(output.status.code().is_some_and(|c| c < 2));
        let snap = overlay_snapshot(&snapshot);
        let run_rules = snap["entries"]["rules"]["dir"]["oneharness-run.rules"]["file"]
            .as_str()
            .unwrap();
        assert!(
            run_rules.contains(&tool) && !run_rules.contains("touch"),
            "{run_rules}"
        );
    }
}

#[test]
fn print_command_shows_the_overlaid_spawn_and_creates_nothing() {
    let world = World::new("print");
    let tool = world.tool();
    let config = world.config(&format!("allowed_tools = [\"Bash({tool})\"]\n"));
    let tmp_before = tree(&world.path("tmp"));
    let state_before = tree(&world.path("state"));
    let home_before = tree(&world.path("codex-home"));
    let output = world.oh(
        &[
            "run",
            "--harness",
            "claude-code,codex",
            "--mode",
            "read-only",
            "--prompt",
            "p",
            "--print-command",
        ],
        &config,
        &[],
    );
    assert!(output.status.success(), "{output:?}");
    let report = json(&output);
    let claude: Vec<String> = report["results"][0]["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap().to_string())
        .collect();
    assert!(flag_values(&claude, "--allowedTools").contains(&format!("Bash({tool}:*)").as_str()));
    assert_eq!(flag_value(&claude, "--permission-mode"), Some("dontAsk"));
    if cfg!(unix) {
        assert_eq!(
            report["results"][1]["overlay_env"]["CODEX_HOME"],
            "<run-overlay>/codex-home"
        );
    }
    assert_eq!(
        tree(&world.path("tmp")),
        tmp_before,
        "a dry run wrote under the temp dir"
    );
    assert_eq!(
        tree(&world.path("state")),
        state_before,
        "a dry run made an overlay"
    );
    assert_eq!(tree(&world.path("codex-home")), home_before);
}

#[cfg(unix)]
#[test]
fn a_granted_controlled_codex_turn_runs_its_own_app_server_and_records_its_thread() {
    let world = World::new("control");
    let tool = world.tool();
    // A home Codex has indexed: the overlay seeds its own index and, after the
    // turn, has the real home read the thread.
    index_the_home(&world);
    let config = world.config(&format!("allowed_tools = [\"Bash({tool})\"]\n"));
    let log = world.path("app-server.log");
    let env_file = world.path("env.json");
    let snapshot = world.path("snapshot.jsonl");
    let output = world.oh(
        &[
            "run",
            "--harness",
            "codex",
            "--mode",
            "read-only",
            "--prompt",
            "judge it",
            "--control",
            "--session",
            "granted",
            "--session-dir",
            &world.s("sessions"),
        ],
        &config,
        &[
            ("MOCK_CODEX_APP_SERVER_LOG", log.to_str().unwrap()),
            ("MOCK_CODEX_COMPLETE_TURN", "1"),
            ("MOCK_CODEX_INDEX", "1"),
            ("MOCK_ENV_FILE", env_file.to_str().unwrap()),
            ("MOCK_HOME_SNAPSHOT", snapshot.to_str().unwrap()),
        ],
    );
    let report = json(&output);
    assert!(
        output.status.success(),
        "{report}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result = &report["results"][0];
    // Not the pooled server: a pooled turn's app-server listens on a socket,
    // and its `app_server` report says `shared`.
    assert_ne!(
        report["control"]["app_server"]["mode"], "shared",
        "{report}"
    );
    let frames: Vec<Value> = std::fs::read_to_string(&log)
        .unwrap()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let methods: Vec<&str> = frames.iter().filter_map(|f| f["method"].as_str()).collect();
    assert!(
        methods.contains(&"thread/list"),
        "the index seed ran: {methods:?}"
    );
    assert!(methods.contains(&"turn/start"), "{methods:?}");
    let read = frames
        .iter()
        .find(|f| f["method"] == "thread/read")
        .unwrap_or_else(|| panic!("the real home never read the thread: {methods:?}"));
    assert_eq!(read["params"]["threadId"], result["session_id"]);
    // The turn's Codex started on an index the overlay seeded from the one
    // its helper built over an empty home — not the real home's, and not one
    // it had to build (and backfill) itself.
    let index = &overlay_snapshot(&snapshot)["entries"]["state_5.sqlite"];
    assert_eq!(
        index["file"], "mock codex thread index built in thread-index-seed",
        "{index}"
    );
    assert!(world.overlays().is_empty());
    assert_eq!(
        std::fs::read_to_string(world.path("codex-home/state_5.sqlite")).unwrap(),
        "mock codex thread index built in codex-home",
        "the real index is Codex's to change, never the overlay's"
    );
}

#[test]
fn list_says_which_harnesses_apply_a_run_level_allowlist() {
    let world = World::new("list");
    let config = world.config("");
    let output = world.oh(&["list"], &config, &[]);
    assert!(output.status.success(), "{output:?}");
    let listed = json(&output);
    for h in listed["harnesses"].as_array().unwrap() {
        let id = h["id"].as_str().unwrap();
        let applied = id == "claude-code" || (id == "codex" && cfg!(unix));
        assert_eq!(h["supports_run_allowed_tools"], applied, "{id}");
    }
}

#[cfg(unix)]
#[test]
fn codex_withholds_over_a_missing_home_or_a_keyring_store_named_in_its_args() {
    let world = World::new("codex-withheld");
    let tool = world.tool();
    let rule = format!("allowed_tools = [\"Bash({tool})\"]\n");
    // A keyring (or auto) store set by the identity's own args, not its file.
    let toml =
        format!("{rule}[harness.codex]\nargs = [\"-c\", \"cli_auth_credentials_store=auto\"]\n");
    let run = spawn(&world, "codex", "read-only", &toml, &[]);
    let message = run.report["results"][0]["diagnostics"][0]["message"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(message.contains("\"auto\""), "{message}");
    assert_eq!(run.env["CODEX_HOME"], world.s("codex-home"));
    // A home that does not exist has no login or sessions to link.
    let gone = world.s("no-such-home");
    let run = spawn(
        &world,
        "codex",
        "read-only",
        &rule,
        &["--env", &format!("CODEX_HOME={gone}")],
    );
    let message = run.report["results"][0]["diagnostics"][0]["message"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(message.contains("does not exist"), "{message}");
    assert!(world.overlays().is_empty());
}

#[cfg(unix)]
#[test]
fn a_helper_app_server_that_never_answers_leaves_the_run_whole() {
    // An indexed home makes the overlay seed its own index and, for the thread
    // the run continues, repair the real one, both through a Codex app-server. Here the "app-server" is a harness
    // double that answers no JSON-RPC: the run still completes, says why on
    // stderr, and leaves nothing behind.
    let world = World::new("helpers-silent");
    index_the_home(&world);
    let tool = world.tool();
    let run = spawn(
        &world,
        "codex",
        "read-only",
        &format!("allowed_tools = [\"Bash({tool})\"]\n"),
        &["--resume", "thread-continued"],
    );
    assert_eq!(run.report["results"][0]["status"], "ok", "{}", run.report);
    assert!(world.overlays().is_empty(), "{:?}", world.overlays());
    assert_eq!(
        std::fs::read_to_string(world.path("codex-home/state_5.sqlite")).unwrap(),
        "mock codex thread index built in codex-home"
    );
}

/// Have the Codex double run once, ungranted, in the world's real home, as a
/// home Codex has used is: its first run opens the thread index.
fn index_the_home(world: &World) {
    let config = world.config("");
    let output = world.oh(
        &[
            "run",
            "--harness",
            "codex",
            "--mode",
            "read-only",
            "--prompt",
            "p",
        ],
        &config,
        &[("MOCK_CODEX_INDEX", "1")],
    );
    assert!(output.status.code().is_some_and(|c| c < 2), "{output:?}");
    assert!(world.path("codex-home/state_5.sqlite").is_file());
}

/// Poll `until` for up to ten seconds.
fn eventually(what: &str, until: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !until() {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// The processes of this user carrying `marker` as their run overlay
/// (Linux, where `/proc` holds each process's environment).
#[cfg(target_os = "linux")]
fn marked(marker: &str) -> Vec<String> {
    let needle = format!("ONEHARNESS_RUN_OVERLAY={marker}\0");
    std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .chars()
                .all(|c| c.is_ascii_digit())
        })
        .filter(|e| {
            std::fs::read(e.path().join("environ"))
                .is_ok_and(|env| String::from_utf8_lossy(&env).contains(&needle))
        })
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect()
}

#[cfg(unix)]
#[test]
fn a_killed_granted_run_is_reclaimed_by_the_next_run_and_a_live_one_beside_it_completes() {
    let world = World::new("killed");
    let tool = world.tool();
    let config = world.config(&format!("allowed_tools = [\"Bash({tool})\"]\n"));
    let granted = [
        "run",
        "--harness",
        "codex",
        "--mode",
        "read-only",
        "--prompt",
        "p",
    ];
    let profiles = ProfileScratch::new();
    // The killed run's Codex creates a top-level file, then waits forever.
    let gate_killed = world.s("gate-killed");
    let mut killed = world
        .command(
            &granted,
            &config,
            &[
                ("MOCK_OUTPUT_GATE", gate_killed.as_str()),
                ("MOCK_HOME_WRITE", "session_index.jsonl={}"),
            ],
            &profiles,
        )
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    eventually("the killed run's overlay", || {
        world.overlays().len() == 1
            && std::fs::read_dir(world.overlay_root())
                .unwrap()
                .flatten()
                .any(|e| e.path().join("codex-home/session_index.jsonl").is_file())
    });
    let killed_overlay = world.overlays().pop().unwrap();
    let gate_live = world.s("gate-live");
    let live = world
        .command(
            &granted,
            &config,
            &[("MOCK_OUTPUT_GATE", gate_live.as_str())],
            &profiles,
        )
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    eventually("the live run's overlay", || world.overlays().len() == 2);
    killed.kill().unwrap();
    killed.wait().unwrap();
    // SIGKILL ran no cleanup: the killed run's overlay is still there.
    assert!(world.overlays().contains(&killed_overlay));

    let next = world.oh(
        &["run", "--harness", "claude-code", "--prompt", "p"],
        &config,
        &[],
    );
    assert!(next.status.code().is_some_and(|c| c < 2), "{next:?}");
    let left = world.overlays();
    assert!(
        !left.contains(&killed_overlay),
        "the killed run's overlay survived: {left:?}"
    );
    assert_eq!(
        left.len(),
        1,
        "the live run's overlay was reclaimed: {left:?}"
    );
    #[cfg(target_os = "linux")]
    assert!(
        marked(&killed_overlay).is_empty(),
        "the killed run's Codex outlived the reclaim"
    );
    // What the killed run's Codex created is in the real home.
    assert_eq!(
        std::fs::read_to_string(world.path("codex-home/session_index.jsonl")).unwrap(),
        "{}"
    );

    std::fs::write(&gate_live, "").unwrap();
    let live = live.wait_with_output().unwrap();
    let report = json(&live);
    assert_eq!(report["results"][0]["status"], "ok", "{report}");
    assert!(world.overlays().is_empty());
}

#[test]
fn a_grant_under_read_only_keeps_a_lean_tool_set_narrowed() {
    let world = World::new("tool-set");
    let tool = world.tool();
    let rule = format!("Bash({tool}:*)");
    for (tool_set, tools, allowed) in [
        // Shell is not a read-only tool, so a lean set naming it loads only
        // what reads — and the grant adds `Bash` for its rule alone.
        (
            "[\"shell\", \"read\"]",
            vec!["Read", "Bash"],
            vec!["Read", rule.as_str()],
        ),
        ("[\"shell\"]", vec!["Bash"], vec![rule.as_str()]),
    ] {
        let toml = format!("allowed_tools = [\"Bash({tool})\"]\ntool_set = {tool_set}\n");
        let run = spawn(&world, "claude-code", "read-only", &toml, &[]);
        assert_eq!(flag_values(&run.argv, "--tools"), tools, "{tool_set}");
        assert_eq!(
            flag_values(&run.argv, "--allowedTools"),
            allowed,
            "{tool_set}"
        );
        assert_eq!(flag_value(&run.argv, "--permission-mode"), Some("dontAsk"));
    }
}

#[test]
fn codex_with_only_rules_it_cannot_express_runs_as_without_and_names_them() {
    let world = World::new("codex-unexpressible");
    let toml = "allowed_tools = [\"Edit\", \"Bash(git status)\"]\n";
    let without = spawn(&world, "codex", "default", "", &[]);
    let with = spawn(&world, "codex", "default", toml, &[]);
    assert_eq!(with.argv, without.argv);
    assert_eq!(with.env, without.env);
    let result = &with.report["results"][0];
    assert!(result.get("overlay_env").is_none());
    let message = result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["setting"] == "allowed_tools")
        .unwrap()["message"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(message.contains("`Edit`, `Bash(git status)`"), "{message}");
    assert!(message.contains("nothing was applied"), "{message}");
    assert!(world.overlays().is_empty());
}

/// A granted run whose Codex creates a directory, replaces a linked file and
/// replaces a linked directory with a file: the first two reach the real home,
/// the third is refused naming both paths, and the run is whole.
#[cfg(unix)]
fn assert_teardown_returns_what_codex_wrote(world: &World, state: &Path) {
    std::fs::create_dir_all(world.path("codex-home/blocked")).unwrap();
    std::fs::write(world.path("codex-home/blocked/kept"), "k").unwrap();
    let tool = world.tool();
    let config = world.config(&format!("allowed_tools = [\"Bash({tool})\"]\n"));
    let state = state.display().to_string();
    let output = world.oh(
        &[
            "run",
            "--harness",
            "codex",
            "--mode",
            "read-only",
            "--prompt",
            "p",
        ],
        &config,
        &[
            ("XDG_STATE_HOME", state.as_str()),
            (
                "MOCK_HOME_WRITE",
                "made/deep/f=c;config.toml=model = \"n\"\n;blocked=file;\
                 rules/default.rules=prefix_rule(pattern=[\"ls\"], decision=\"allow\")\n",
            ),
        ],
    );
    let report = json(&output);
    assert_eq!(report["results"][0]["status"], "ok", "{report}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("could not return") && stderr.contains("a directory is in the way"),
        "{stderr}"
    );
    assert_eq!(
        std::fs::read_to_string(world.path("codex-home/made/deep/f")).unwrap(),
        "c"
    );
    assert_eq!(
        std::fs::read_to_string(world.path("codex-home/config.toml")).unwrap(),
        "model = \"n\"\n"
    );
    assert!(
        !std::fs::symlink_metadata(world.path("codex-home/config.toml"))
            .unwrap()
            .is_symlink()
    );
    assert_eq!(
        std::fs::read_to_string(world.path("codex-home/blocked/kept")).unwrap(),
        "k"
    );
    // A rules file Codex wrote (an approval it saved) is the user's now; the
    // run's own rules file never leaves the overlay.
    assert_eq!(
        std::fs::read_to_string(world.path("codex-home/rules/default.rules")).unwrap(),
        "prefix_rule(pattern=[\"ls\"], decision=\"allow\")\n"
    );
    assert!(!world.path("codex-home/rules/oneharness-run.rules").exists());
    let root = Path::new(&state).join("oneharness/run-overlays");
    assert!(
        !root.exists(),
        "the overlay outlived the run under {}",
        root.display()
    );
}

#[cfg(unix)]
#[test]
fn teardown_returns_what_codex_wrote_to_the_real_home() {
    let world = World::new("teardown");
    assert_teardown_returns_what_codex_wrote(&world, &world.path("state"));
}

/// The same across filesystems: an overlay under a tmpfs (`/dev/shm`) over a
/// real home on disk, where a rename cannot move a file and teardown copies.
#[cfg(target_os = "linux")]
#[test]
fn teardown_returns_what_codex_wrote_across_filesystems() {
    let shm = Path::new("/dev/shm");
    if !shm.is_dir() {
        eprintln!("no /dev/shm on this host: the cross-filesystem teardown is covered elsewhere");
        return;
    }
    let world = World::new("teardown-xfs");
    let state = ScratchDir::under(shm, "run-allowlist-xfs").unwrap();
    assert_teardown_returns_what_codex_wrote(&world, state.path());
}

#[cfg(unix)]
#[test]
fn a_codex_config_that_cannot_be_read_withholds_the_grant() {
    let world = World::new("codex-bad-config");
    std::fs::write(
        world.path("codex-home/config.toml"),
        "this is = = not toml\n",
    )
    .unwrap();
    let tool = world.tool();
    let run = spawn(
        &world,
        "codex",
        "read-only",
        &format!("allowed_tools = [\"Bash({tool})\"]\n"),
        &[],
    );
    let message = run.report["results"][0]["diagnostics"][0]["message"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(message.contains("is not valid TOML"), "{message}");
    assert_eq!(run.env["CODEX_HOME"], world.s("codex-home"));
    assert!(world.overlays().is_empty());
}

#[cfg(unix)]
#[test]
fn every_prompt_of_a_granted_batch_runs_on_an_overlay_of_its_own() {
    let world = World::new("batch");
    let tool = world.tool();
    let config = world.config(&format!("allowed_tools = [\"Bash({tool})\"]\n"));
    let output = world.oh(
        &[
            "run",
            "--harness",
            "codex",
            "--mode",
            "read-only",
            "--prompt",
            "one",
            "--prompt",
            "two",
        ],
        &config,
        &[],
    );
    let report = json(&output);
    let results = report["results"].as_array().unwrap();
    assert_eq!(results.len(), 2, "{report}");
    let homes: Vec<&str> = results
        .iter()
        .map(|r| r["overlay_env"]["CODEX_HOME"].as_str().unwrap())
        .collect();
    assert_ne!(homes[0], homes[1], "two prompts shared one overlay");
    for result in results {
        assert!(
            result["overlay_timings"]["teardown_ms"].is_u64(),
            "{result}"
        );
    }
    assert!(world.overlays().is_empty(), "{:?}", world.overlays());
}

#[cfg(unix)]
#[test]
fn a_fallback_chain_applies_the_grant_to_the_candidate_that_serves() {
    // The first candidate's identity has no home: its grant is withheld and
    // it is skipped (`auth`), so the chain falls through to Claude Code, which
    // runs granted.
    let world = World::new("fallback");
    let tool = world.tool();
    let gone = world.s("no-such-codex-home");
    let config = world.config(&format!(
        "allowed_tools = [\"Bash({tool})\"]\n\
         [harness.codex.variant.gone.env_from]\nCODEX_HOME = \"OH_GONE_CODEX_HOME\"\n"
    ));
    let argv_file = world.path("argv");
    let output = world.oh(
        &[
            "run",
            "--harness",
            "codex:gone,claude-code",
            "--mode",
            "read-only",
            "--prompt",
            "p",
        ],
        &config,
        &[
            ("MOCK_ARGV_FILE", argv_file.to_str().unwrap()),
            ("OH_GONE_CODEX_HOME", gone.as_str()),
        ],
    );
    let report = json(&output);
    assert_eq!(report["fallback"]["ran"], "claude-code", "{report}");
    let results = report["results"].as_array().unwrap();
    assert_eq!(results[0]["status"], "skipped");
    assert!(results[0].get("overlay_env").is_none());
    let argv = argv_of(&argv_file);
    assert!(flag_values(&argv, "--allowedTools").contains(&format!("Bash({tool}:*)").as_str()));
    assert!(world.overlays().is_empty());
}

#[cfg(unix)]
#[test]
fn a_granted_run_keeps_one_overlay_across_its_schema_retries() {
    let world = World::new("schema-retry");
    let tool = world.tool();
    let config = world.config(&format!("allowed_tools = [\"Bash({tool})\"]\n"));
    let attempts = world.path("attempts.log");
    let schema = world.path("schema.json");
    std::fs::write(&schema, r#"{"type":"object","required":["a"]}"#).unwrap();
    let schema = schema.display().to_string();
    let output = world.oh(
        &[
            "run",
            "--harness",
            "codex",
            "--mode",
            "read-only",
            "--prompt",
            "p",
            "--schema",
            &schema,
            "--schema-max-retries",
            "2",
        ],
        &config,
        &[
            ("MOCK_HOME_WRITE", "seen=x"),
            ("MOCK_ATTEMPT_FILE", attempts.to_str().unwrap()),
        ],
    );
    let report = json(&output);
    let result = &report["results"][0];
    assert_eq!(result["schema_attempts"], 3, "{result}");
    assert!(result["overlay_env"]["CODEX_HOME"].is_string(), "{result}");
    assert!(world.overlays().is_empty(), "{:?}", world.overlays());
    // Each attempt wrote through the one overlay home, returned at the end.
    assert_eq!(
        std::fs::read_to_string(world.path("codex-home/seen")).unwrap(),
        "x"
    );
}

#[cfg(unix)]
#[test]
fn a_codex_store_that_cannot_be_determined_withholds_the_grant() {
    use std::os::unix::fs::PermissionsExt;
    let world = World::new("codex-unknown-store");
    let tool = world.tool();
    let rule = format!("allowed_tools = [\"Bash({tool})\"]\n");
    let config = world.path("codex-home/config.toml");
    let message = |run: &Spawn| {
        run.report["results"][0]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["setting"] == "allowed_tools")
            .unwrap()["message"]
            .as_str()
            .unwrap()
            .to_string()
    };
    // A store named as no string.
    std::fs::write(&config, "cli_auth_credentials_store = 5\n").unwrap();
    let run = spawn(&world, "codex", "read-only", &rule, &[]);
    assert!(message(&run).contains("not a store"), "{}", message(&run));
    // A store this release does not know.
    std::fs::write(&config, "cli_auth_credentials_store = \"vault\"\n").unwrap();
    let run = spawn(&world, "codex", "read-only", &rule, &[]);
    assert!(
        message(&run).contains("not one an overlay is known to reach"),
        "{}",
        message(&run)
    );
    assert_eq!(run.env["CODEX_HOME"], world.s("codex-home"));
    // A config the run cannot read (skipped where permissions do not bind,
    // as for root).
    std::fs::write(&config, "model = \"m\"\n").unwrap();
    std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read(&config).is_err() {
        let run = spawn(&world, "codex", "read-only", &rule, &[]);
        assert!(
            message(&run).contains("could not be read"),
            "{}",
            message(&run)
        );
        assert!(run.report["results"][0].get("overlay_env").is_none());
    }
    std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(world.overlays().is_empty());
}

#[cfg(unix)]
#[test]
fn a_helper_that_refuses_answers_badly_or_never_answers_leaves_the_turn_whole() {
    for (reply, says) in [
        ("error", "the mock refuses thread/list"),
        ("no-result", "answered with no result object"),
        ("silent", "no reply to `thread/list`"),
    ] {
        let world = World::new(&format!("helper-{reply}"));
        index_the_home(&world);
        let tool = world.tool();
        let config = world.config(&format!("allowed_tools = [\"Bash({tool})\"]\n"));
        let log = world.path("app-server.log");
        let output = world.oh(
            &[
                "run",
                "--harness",
                "codex",
                "--mode",
                "read-only",
                "--prompt",
                "judge it",
                "--control",
                "--session",
                "granted",
                "--session-dir",
                &world.s("sessions"),
            ],
            &config,
            &[
                ("MOCK_CODEX_APP_SERVER_LOG", log.to_str().unwrap()),
                ("MOCK_CODEX_COMPLETE_TURN", "1"),
                ("MOCK_CODEX_HELPER_REPLY", reply),
            ],
        );
        let report = json(&output);
        assert_eq!(report["results"][0]["status"], "ok", "{reply}: {report}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("could not build the run overlay's Codex thread index")
                && stderr.contains(says),
            "{reply}: {stderr}"
        );
        assert!(
            stderr.contains("could not record the granted run's Codex thread"),
            "{reply}: {stderr}"
        );
        assert!(
            world.overlays().is_empty(),
            "{reply}: {:?}",
            world.overlays()
        );
    }
}

#[cfg(unix)]
#[test]
fn an_overlay_that_cannot_be_made_stops_the_run_before_anything_spawns() {
    // The state directory an overlay lives under is a file here, so no
    // overlay can be made: the run stops with a usage error naming why,
    // launches no harness, and leaves no partial overlay behind.
    let world = World::new("no-overlay-root");
    let state = world.path("state-is-a-file");
    std::fs::write(&state, "").unwrap();
    let tool = world.tool();
    // The shared model catalog's own probe of codex runs before an overlay
    // is opened; off here, so the one launch this journey watches for is the
    // turn's.
    let config = world.config(&format!(
        "allowed_tools = [\"Bash({tool})\"]\n[harness.codex]\nshared_model_catalog = false\n"
    ));
    let argv_file = world.path("argv");
    let state_s = state.display().to_string();
    let output = world.oh(
        &[
            "run",
            "--harness",
            "codex",
            "--mode",
            "read-only",
            "--prompt",
            "p",
        ],
        &config,
        &[
            ("XDG_STATE_HOME", state_s.as_str()),
            ("MOCK_ARGV_FILE", argv_file.to_str().unwrap()),
        ],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(
        stderr
            .contains("could not create the run overlay a run-level allowlist is applied through"),
        "{stderr}"
    );
    assert!(!argv_file.exists(), "a harness was spawned");
    assert!(output.stdout.is_empty());
    assert_eq!(
        std::fs::read_to_string(&state).unwrap(),
        "",
        "the state file was written"
    );
    assert!(world.overlays().is_empty());
}

#[cfg(unix)]
#[test]
fn an_unreadable_user_rules_directory_stops_the_run_rather_than_drop_its_rules() {
    // The overlay's `rules/` carries the user's own rule files beside the
    // run's allows. When the real home's `rules/` exists but cannot be read,
    // an overlay without them would lose their `forbidden` rules while
    // gaining the run's allows — so the run stops before anything spawns, and
    // leaves no overlay behind.
    use std::os::unix::fs::PermissionsExt;
    let world = World::new("unreadable-rules");
    let rules = world.path("codex-home/rules");
    std::fs::set_permissions(&rules, std::fs::Permissions::from_mode(0o000)).unwrap();
    // Skipped where permissions do not bind, as for root.
    if std::fs::read_dir(&rules).is_ok() {
        std::fs::set_permissions(&rules, std::fs::Permissions::from_mode(0o755)).unwrap();
        return;
    }
    let tool = world.tool();
    let config = world.config(&format!(
        "allowed_tools = [\"Bash({tool})\"]\n[harness.codex]\nshared_model_catalog = false\n"
    ));
    let argv_file = world.path("argv");
    let output = world.oh(
        &[
            "run",
            "--harness",
            "codex",
            "--mode",
            "read-only",
            "--prompt",
            "p",
        ],
        &config,
        &[("MOCK_ARGV_FILE", argv_file.to_str().unwrap())],
    );
    std::fs::set_permissions(&rules, std::fs::Permissions::from_mode(0o755)).unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(
        stderr
            .contains("could not create the run overlay a run-level allowlist is applied through"),
        "{stderr}"
    );
    assert!(!argv_file.exists(), "a harness was spawned");
    assert!(output.stdout.is_empty());
    assert!(world.overlays().is_empty(), "{:?}", world.overlays());
}

#[cfg(unix)]
#[test]
fn every_spelling_of_a_codex_store_override_withholds_the_grant() {
    let world = World::new("codex-store-spellings");
    let tool = world.tool();
    for args in [
        r#"["--config=cli_auth_credentials_store=auto"]"#,
        r#"["-ccli_auth_credentials_store=keyring"]"#,
        r#"["--config", "cli_auth_credentials_store = \"keyring\""]"#,
        r#"["-c", "cli_auth_credentials_store='auto'"]"#,
    ] {
        let toml = format!("allowed_tools = [\"Bash({tool})\"]\n[harness.codex]\nargs = {args}\n");
        let run = spawn(&world, "codex", "read-only", &toml, &[]);
        let message = run.report["results"][0]["diagnostics"][0]["message"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        assert!(message.contains("OS keyring"), "{args}: {message}");
        assert!(
            run.report["results"][0].get("overlay_env").is_none(),
            "{args}"
        );
    }
    assert!(world.overlays().is_empty());
}
