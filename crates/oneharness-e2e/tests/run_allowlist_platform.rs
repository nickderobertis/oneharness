//! The run-level allowlist where it is withheld, on every platform: each
//! harness that withholds it on this host — every harness off Linux (macOS,
//! Windows), where no test against a real CLI proves the grant; the six with no
//! proven mechanism on Linux — runs exactly as it does with no run-level
//! allowlist, writes nothing, and says so in its report; `oneharness list`
//! reports the same.
//!
//! Not platform-gated, unlike `run_allowlist.rs`: this is the journey the
//! macOS and windows-latest legs run, so their CI exercises what a run does
//! there. It drives the real built `oneharness` with the mock harness standing
//! in for each harness's process, at the existing `--bin` seam, because the
//! deterministic gate cannot run the paid, authenticated CLIs.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use oneharness_core::io::scratch::ScratchDir;
use serde_json::Value;

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

const HARNESSES: [&str; 8] = [
    "claude-code",
    "codex",
    "opencode",
    "goose",
    "qwen",
    "crush",
    "copilot",
    "cursor",
];

/// The harnesses that withhold the run-level allowlist on this host.
fn withholding() -> Vec<&'static str> {
    HARNESSES
        .into_iter()
        .filter(|id| !cfg!(target_os = "linux") || !matches!(*id, "claude-code" | "codex"))
        .collect()
}

/// The platform a diagnostic names where every harness withholds, or `None`
/// on Linux, the one platform where the grant is proven.
fn withheld_platform() -> Option<&'static str> {
    if cfg!(target_os = "linux") {
        None
    } else if cfg!(target_os = "macos") {
        Some("macOS")
    } else if cfg!(windows) {
        Some("Windows")
    } else {
        Some(std::env::consts::OS)
    }
}

/// An absolute executable on the host's convention, one plain word: a rule
/// that is valid everywhere, so what differs is only whether it applies.
fn granted() -> &'static str {
    if cfg!(windows) {
        "C:/oneharness-grant/grant-tool"
    } else {
        "/oneharness-grant/grant-tool"
    }
}

/// Read-only where the harness has it; Goose and Crush have none.
fn mode(id: &str) -> &'static str {
    match id {
        "goose" | "crush" => "bypass",
        _ => "read-only",
    }
}

struct World {
    dir: ScratchDir,
}

/// What one run delivered to its harness, and what it reported.
struct Spawn {
    report: Value,
    argv: String,
    env: serde_json::Map<String, Value>,
    stdin: String,
}

impl World {
    fn new(tag: &str) -> World {
        let dir = ScratchDir::new(&format!("run-allowlist-platform-{tag}")).unwrap();
        for sub in ["project", "tmp", "state", "home", "codex-home", "out"] {
            std::fs::create_dir_all(dir.path().join(sub)).unwrap();
        }
        std::fs::write(dir.path().join("user.toml"), "").unwrap();
        // What a grant written anywhere would overwrite.
        std::fs::create_dir_all(dir.path().join("codex-home/rules")).unwrap();
        std::fs::write(dir.path().join("codex-home/config.toml"), "model = \"m\"\n").unwrap();
        std::fs::write(dir.path().join("codex-home/rules/user.rules"), "# user\n").unwrap();
        std::fs::write(dir.path().join("project/README.md"), "project\n").unwrap();
        World { dir }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    fn command(&self, args: &[&str], config: &Path) -> Command {
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
            .env("USERPROFILE", self.path("home"))
            .env("XDG_STATE_HOME", self.path("state"))
            .env("CODEX_HOME", self.path("codex-home"))
            .args(args);
        if args.first() == Some(&"run") {
            cmd.args(["--config", &config.display().to_string()]);
        }
        cmd.args(["--format", "json"]);
        cmd
    }

    fn spawn(&self, harness: &str, toml: &str) -> Spawn {
        let config = self.path("oneharness.toml");
        std::fs::write(&config, toml).unwrap();
        let (argv, env, stdin) = (
            self.path("out/argv"),
            self.path("out/env.json"),
            self.path("out/stdin"),
        );
        for file in [&argv, &env, &stdin] {
            let _ = std::fs::remove_file(file);
        }
        let output = self.run(harness, mode(harness), &config, &argv, &env, &stdin);
        assert!(
            output.status.code().is_some_and(|code| code < 2),
            "{harness}: {output:?}"
        );
        let report: Value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|e| panic!("{harness}: stdout was not JSON ({e}): {output:?}"));
        let mut env: serde_json::Map<String, Value> =
            serde_json::from_str(&std::fs::read_to_string(&env).unwrap()).unwrap();
        // Per-invocation by construction: the coverage profile path.
        env.remove("LLVM_PROFILE_FILE");
        Spawn {
            report,
            argv: std::fs::read_to_string(&argv).unwrap(),
            env,
            stdin: std::fs::read_to_string(&stdin).unwrap_or_default(),
        }
    }

    // llmlint: ignore-block[e2e_not_mocked] The journey drives the real built `oneharness` over a real TOML config; only each harness's own process is the repository's mock harness, at the existing `--bin` seam, because the deterministic gate (windows-latest among its legs) cannot run the paid, authenticated CLIs. A withheld grant reaches no harness at all, so what it must prove — the spawn is the ungranted one — is fully observed at that seam.
    fn run(
        &self,
        harness: &str,
        mode: &str,
        config: &Path,
        argv: &Path,
        env: &Path,
        stdin: &Path,
    ) -> Output {
        let project = self.path("project").display().to_string();
        let bin = format!(
            "{harness}={}",
            workspace_bin("oneharness-mock-harness").display()
        );
        self.command(
            &[
                "run",
                "--harness",
                harness,
                "--mode",
                mode,
                "--prompt",
                "p",
                "--cwd",
                &project,
                "--bin",
                &bin,
            ],
            config,
        )
        .env("MOCK_ARGV_FILE", argv)
        .env("MOCK_ENV_FILE", env)
        .env("MOCK_INPUT_FILE", stdin)
        .output()
        .expect("run oneharness")
    }
    // llmlint: ignore-end[e2e_not_mocked]

    /// Every file under the world but the recorder's own output, with its
    /// contents — but under `state`, whose history store every run appends to,
    /// granted or not, by path alone.
    fn files(&self) -> Vec<(PathBuf, Option<Vec<u8>>)> {
        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else {
                    out.push(path);
                }
            }
        }
        let mut out = Vec::new();
        for sub in ["project", "tmp", "home", "codex-home", "state"] {
            let mut paths = Vec::new();
            walk(&self.path(sub), &mut paths);
            out.extend(paths.into_iter().map(|path| {
                let contents = (sub != "state").then(|| std::fs::read(&path).unwrap());
                (path, contents)
            }));
        }
        out.sort();
        out
    }
}

fn allowlist_diagnostics(spawn: &Spawn) -> Vec<Value> {
    spawn.report["results"][0]
        .get("diagnostics")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|d| d["setting"] == "allowed_tools")
        .cloned()
        .collect()
}

#[test]
fn a_withheld_run_level_allowlist_changes_nothing_a_run_spawns_or_writes_and_says_so() {
    let world = World::new("withheld");
    let toml = format!("allowed_tools = [\"Bash({})\"]\n", granted());
    for harness in withholding() {
        let without = world.spawn(harness, "");
        let before = world.files();
        let with = world.spawn(harness, &toml);
        assert_eq!(with.argv, without.argv, "{harness}");
        assert_eq!(with.env, without.env, "{harness}");
        assert_eq!(with.stdin, without.stdin, "{harness}");
        assert!(!with.argv.contains(granted()), "{harness}: {}", with.argv);
        assert_eq!(world.files(), before, "{harness}: the withheld run wrote");
        assert!(allowlist_diagnostics(&without).is_empty(), "{harness}");
        let withheld = allowlist_diagnostics(&with);
        assert_eq!(withheld.len(), 1, "{harness}: {withheld:?}");
        assert_eq!(withheld[0]["harness"], harness);
        let message = withheld[0]["message"].as_str().unwrap();
        assert!(
            message.contains(&format!("withheld on `{harness}`")),
            "{message}"
        );
        assert!(
            message.contains(&format!("`Bash({})`", granted())),
            "{message}"
        );
        if let Some(platform) = withheld_platform() {
            assert!(message.contains(&format!("on {platform}")), "{message}");
            assert!(message.contains("proven on Linux only"), "{message}");
        }
    }
}

#[test]
fn list_reports_the_run_level_allowlist_as_this_platform_applies_it() {
    let world = World::new("list");
    let config = world.path("oneharness.toml");
    std::fs::write(&config, "").unwrap();
    let output = world.command(&["list"], &config).output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let harnesses = report["harnesses"].as_array().unwrap();
    // Every registered harness, no more: one added later is held here too.
    let mut listed: Vec<&str> = harnesses.iter().filter_map(|h| h["id"].as_str()).collect();
    let mut known = HARNESSES.to_vec();
    listed.sort_unstable();
    known.sort_unstable();
    assert_eq!(listed, known, "update HARNESSES to the registry");
    for id in HARNESSES {
        let h = harnesses
            .iter()
            .find(|h| h["id"] == id)
            .unwrap_or_else(|| panic!("{id} missing from list"));
        let withheld = withholding().contains(&id);
        // Codex also withholds where its overlay's links cannot be made,
        // which a Linux host always can.
        assert_eq!(h["supports_run_allowed_tools"], !withheld, "{id}");
    }
}

/// Withholding never relaxes validation: on every platform, macOS and Windows included,
/// a rule that cannot be one is refused before anything spawns, as it is where
/// the grant applies.
#[test]
fn a_rule_that_cannot_be_granted_is_refused_before_spawn_wherever_it_is_withheld() {
    let world = World::new("refused");
    let (argv, env, stdin) = (
        world.path("out/argv"),
        world.path("out/env.json"),
        world.path("out/stdin"),
    );
    for (toml, wants) in [
        (
            "allowed_tools = [\"Bash($ONEHARNESS_TEST_UNSET_GRANT)\"]\n",
            "`$ONEHARNESS_TEST_UNSET_GRANT` is unset or empty",
        ),
        ("allowed_tools = [\"Edit\"]\n", "--mode read-only"),
    ] {
        let config = world.path("oneharness.toml");
        std::fs::write(&config, toml).unwrap();
        for harness in ["claude-code", "codex", "opencode"] {
            let _ = std::fs::remove_file(&argv);
            let output = world.run(harness, "read-only", &config, &argv, &env, &stdin);
            let said = String::from_utf8_lossy(&output.stderr);
            assert_eq!(output.status.code(), Some(2), "{harness}: {output:?}");
            assert!(said.contains(wants), "{harness}: {said}");
            assert!(!argv.exists(), "{harness}: the harness was spawned");
        }
    }
}
