//! The lean-session surface as a Rust dependent drives it: `RunRequest`'s
//! `auto_compact_tokens`, `tool_set`, `load_skills`, `load_mcp` and `images`,
//! and `compact_session` — completed, and cancelled through
//! `RunControls::cancel` mid-rewrite — against the mock harness standing in
//! for each harness's process.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Mutex;
#[cfg(unix)]
use std::time::{Duration, Instant};

use oneharness_core::domain::compact::CompactStatus;
use oneharness_core::domain::lean::{Setting, ToolClass};
#[cfg(unix)]
use oneharness_core::io::cancel::CancelToken;
use oneharness_core::io::compact::{compact_session, CompactRequest};
use oneharness_core::io::run::{run, RunControls, RunRequest};
use oneharness_core::io::runner::ProcessSupervisor;
use oneharness_core::io::scratch::ScratchDir;
use serde_json::Value;

#[path = "common/library_fixture.rs"]
mod fixture;
use fixture::{bin_override, ProfileScratch};

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).expect("the mock recorded it")).unwrap()
}

fn png(path: &Path) {
    std::fs::write(path, b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR").unwrap();
}

#[test]
fn a_run_request_delivers_every_lean_setting_and_image_to_claude_code() {
    let profiles = ProfileScratch::new();
    let scratch = ScratchDir::new("library-lean-claude").unwrap();
    let input = scratch.join("input.json");
    let env_file = scratch.join("env.json");
    let image = scratch.join("frame.png");
    png(&image);
    let input_env = format!("{}", input.display());
    let env_env = format!("{}", env_file.display());
    let req = RunRequest {
        harness: vec!["claude-code".to_string()],
        prompt: vec!["hi".to_string()],
        bin: vec![bin_override("claude-code")],
        env: vec![
            format!("MOCK_INPUT_FILE={input_env}"),
            format!("MOCK_ENV_FILE={env_env}"),
            profiles.as_str().to_string(),
        ],
        no_config: true,
        timeout: Some(60),
        auto_compact_tokens: std::num::NonZeroU64::new(200_000),
        tool_set: Some(vec![ToolClass::Shell, ToolClass::Read]),
        load_skills: Some(false),
        load_mcp: Some(false),
        images: vec![image.clone()],
        ..RunRequest::default()
    };
    let outcome = run(&req, RunControls::default()).expect("a valid request");
    assert_eq!(outcome.exit_code, 0, "{:?}", outcome.report.results[0]);

    let recorded = read_json(&input);
    let argv: Vec<&str> = recorded["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap())
        .collect();
    let at = argv.iter().position(|a| *a == "--tools").unwrap();
    assert_eq!(&argv[at + 1..at + 3], ["Bash", "Read"]);
    assert!(argv.contains(&"--disable-slash-commands"));
    assert!(argv.contains(&"--strict-mcp-config"));
    let frame: Value = serde_json::from_str(recorded["stdin"].as_str().unwrap().trim()).unwrap();
    assert_eq!(frame["message"]["content"][0]["type"], "image");
    assert_eq!(
        frame["message"]["content"][0]["source"]["media_type"],
        "image/png"
    );
    assert_eq!(
        frame["message"]["content"][0]["source"]["data"], "iVBORw0KGgoAAAANSUhEUg==",
        "the file's own bytes, base64-encoded"
    );
    assert_eq!(frame["message"]["content"][1]["text"], "hi");
    let env = read_json(&env_file);
    assert_eq!(env["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "233000");
}

#[test]
fn a_run_request_delivers_codex_its_overrides_and_image_flag() {
    let profiles = ProfileScratch::new();
    let scratch = ScratchDir::new("library-lean-codex").unwrap();
    let input = scratch.join("input.json");
    let image = scratch.join("frame.png");
    png(&image);
    let input_env = format!("{}", input.display());
    let req = RunRequest {
        harness: vec!["codex".to_string()],
        prompt: vec!["hi".to_string()],
        bin: vec![bin_override("codex")],
        env: vec![
            format!("MOCK_INPUT_FILE={input_env}"),
            profiles.as_str().to_string(),
        ],
        no_config: true,
        timeout: Some(60),
        auto_compact_tokens: std::num::NonZeroU64::new(50_000),
        load_skills: Some(false),
        images: vec![image.clone()],
        ..RunRequest::default()
    };
    let outcome = run(&req, RunControls::default()).expect("a valid request");
    assert_eq!(outcome.exit_code, 0, "{:?}", outcome.report.results[0]);
    let argv: Vec<String> = read_json(&input)["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap().to_string())
        .collect();
    let pair = |value: &str| argv.windows(2).any(|w| w[0] == "-c" && w[1] == value);
    assert!(pair("model_auto_compact_token_limit=50000"), "{argv:?}");
    assert!(pair("skills.include_instructions=false"), "{argv:?}");
    let absolute = std::path::absolute(&image).unwrap();
    assert!(
        argv.contains(&format!("--image={}", absolute.display())),
        "{argv:?}"
    );
    let applied = &outcome.report.results[0].diagnostics;
    assert!(
        applied.iter().all(|d| d.setting == Setting::Calls),
        "codex applied both settings: {applied:?}"
    );
}

/// A session kept by a session-keeping mock of `id`, driven as a library.
struct Kept {
    dir: ScratchDir,
    id: &'static str,
}

impl Kept {
    fn new(id: &'static str, tag: &str) -> Self {
        let dir = ScratchDir::new(&format!("library-compact-{tag}")).unwrap();
        for sub in ["proj", "store", "home"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        Self { dir, id }
    }

    fn env(&self, profiles: &ProfileScratch) -> Vec<String> {
        let home = self.dir.join("home").display().to_string();
        let mut env = match self.id {
            "claude-code" => vec![
                format!("CLAUDE_CONFIG_DIR={home}"),
                "MOCK_CLAUDE_SESSIONS=1".to_string(),
            ],
            _ => vec![
                format!("CODEX_HOME={home}"),
                "MOCK_CODEX_SESSIONS=1".to_string(),
            ],
        };
        env.push(profiles.as_str().to_string());
        env
    }

    fn turn(&self, prompt: &str) -> (String, String) {
        let profiles = ProfileScratch::new();
        let req = RunRequest {
            harness: vec![self.id.to_string()],
            prompt: vec![prompt.to_string()],
            session: Some("watch".to_string()),
            session_dir: Some(self.dir.join("store")),
            cwd: Some(self.dir.join("proj")),
            bin: vec![bin_override(self.id)],
            env: self.env(&profiles),
            mode: Some(oneharness_core::domain::mode::PermissionMode::Bypass),
            no_config: true,
            timeout: Some(60),
            ..RunRequest::default()
        };
        let outcome = run(&req, RunControls::default()).expect("a valid turn");
        let result = &outcome.report.results[0];
        assert_eq!(outcome.exit_code, 0, "{result:?}");
        (
            result.text.clone().unwrap_or_default(),
            outcome
                .report
                .session
                .as_ref()
                .unwrap()
                .token
                .clone()
                .unwrap(),
        )
    }

    fn request(&self, profiles: &ProfileScratch, extra: &[String]) -> CompactRequest {
        let mut env = self.env(profiles);
        env.extend(extra.iter().cloned());
        CompactRequest {
            harness: Some(self.id.parse().unwrap()),
            session_dir: Some(self.dir.join("store")),
            cwd: Some(self.dir.join("proj")),
            env,
            bin: vec![bin_override(self.id)],
            no_config: true,
            timeout: Some(60),
            ..CompactRequest::new("watch".try_into().unwrap())
        }
    }

    /// Every file under the harness's own store and oneharness's, with bytes.
    #[cfg(unix)]
    fn stored(&self) -> Vec<(PathBuf, Vec<u8>)> {
        let mut out = Vec::new();
        let mut stack = vec![self.dir.join("home"), self.dir.join("store")];
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
fn compact_session_completes_on_claude_and_codex() {
    for id in ["claude-code", "codex"] {
        let kept = Kept::new(id, &format!("done-{id}"));
        kept.turn("alpha");
        let (_, original) = kept.turn("beta");
        let profiles = ProfileScratch::new();
        let outcome = compact_session(&kept.request(&profiles, &[]), RunControls::default(), None)
            .expect("a valid compaction");
        assert_eq!(
            outcome.status,
            CompactStatus::Completed,
            "{id}: {outcome:?}"
        );
        assert_ne!(outcome.token.as_deref(), Some(original.as_str()));
        if id == "claude-code" {
            assert_eq!(outcome.context_tokens_before, Some(6_809));
            assert_eq!(
                outcome.usage.input_tokens,
                Some(58_305),
                "the compaction's own usage"
            );
            assert_eq!(outcome.usage.output_tokens, Some(1_538));
            assert_eq!(outcome.usage.cache_write_tokens, Some(13_711));
            assert_eq!(outcome.usage.cost_usd, Some(0.008_313_375));
        }
        let (text, token) = kept.turn("gamma");
        assert_eq!(text, "context: [summary of: alpha, beta, gamma]", "{id}");
        assert_eq!(Some(token), outcome.token);
    }
}

/// Records what a caller's supervisor is handed: the command about to spawn
/// and the live child's pid.
#[derive(Default)]
struct Watcher {
    commands: Mutex<Vec<(String, Vec<String>)>>,
    pids: Mutex<Vec<u32>>,
}

impl ProcessSupervisor for Watcher {
    fn spawning(&self, command: &mut Command) {
        let program = command.get_program().to_string_lossy().into_owned();
        let args = command
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        self.commands.lock().unwrap().push((program, args));
    }

    fn spawned(&self, child: &Child) {
        self.pids.lock().unwrap().push(child.id());
    }
}

#[test]
fn a_callers_supervisor_is_handed_the_compaction_process() {
    for id in ["claude-code", "codex"] {
        let kept = Kept::new(id, &format!("supervised-{id}"));
        kept.turn("alpha");
        let profiles = ProfileScratch::new();
        let watcher = Watcher::default();
        let outcome = compact_session(
            &kept.request(&profiles, &[]),
            RunControls::default(),
            Some(&watcher),
        )
        .expect("a valid compaction");
        assert_eq!(
            outcome.status,
            CompactStatus::Completed,
            "{id}: {outcome:?}"
        );
        let commands = watcher.commands.lock().unwrap();
        assert_eq!(
            commands.len(),
            1,
            "{id}: one compaction process {commands:?}"
        );
        assert_eq!(Path::new(&commands[0].0), fixture::mock_bin(), "{id}");
        assert!(!commands[0].1.is_empty(), "{id}: the harness's own argv");
        let pids = watcher.pids.lock().unwrap();
        assert_eq!(pids.len(), 1, "{id}: the live child handed over");
        assert_ne!(pids[0], std::process::id(), "{id}");
    }
}

#[test]
fn compact_session_takes_its_binary_from_config_and_an_explicit_bin_over_it() {
    let kept = Kept::new("claude-code", "config-bin");
    kept.turn("alpha");
    let profiles = ProfileScratch::new();
    let write_config = |bin: &Path| {
        let path = kept.dir.join("oneharness.toml");
        let bin = bin.display().to_string().replace('\\', "/");
        std::fs::write(&path, format!("[harness.claude-code]\nbin = \"{bin}\"\n")).unwrap();
        path
    };
    let configured = |config: PathBuf, bin: Vec<String>| CompactRequest {
        config: vec![config],
        no_config: false,
        bin,
        ..kept.request(&profiles, &[])
    };

    // A configured binary that does not exist is the one tried.
    let missing = kept.dir.join("no-such-claude");
    let outcome = compact_session(
        &configured(write_config(&missing), Vec::new()),
        RunControls::default(),
        None,
    )
    .expect("a valid compaction");
    assert_eq!(outcome.status, CompactStatus::Failed, "{outcome:?}");
    assert!(
        outcome
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("no-such-claude"),
        "{outcome:?}"
    );

    // An explicit `bin` beats it.
    let outcome = compact_session(
        &configured(write_config(&missing), vec![bin_override("claude-code")]),
        RunControls::default(),
        None,
    )
    .expect("a valid compaction");
    assert_eq!(outcome.status, CompactStatus::Completed, "{outcome:?}");

    // A configured binary that exists runs the compaction.
    let outcome = compact_session(
        &configured(write_config(&fixture::mock_bin()), Vec::new()),
        RunControls::default(),
        None,
    )
    .expect("a valid compaction");
    assert_eq!(outcome.status, CompactStatus::Completed, "{outcome:?}");
}

#[cfg(unix)]
fn alive(pid: i32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(unix)]
#[test]
fn a_cancelled_compaction_ends_the_harness_and_leaves_the_session_as_it_was() {
    for id in ["claude-code", "codex"] {
        let kept = Kept::new(id, &format!("cancel-{id}"));
        kept.turn("alpha");
        let (_, original) = kept.turn("beta");
        let before = kept.stored();

        let begun = kept.dir.join("begun");
        let profiles = ProfileScratch::new();
        let req = kept.request(
            &profiles,
            &[format!("MOCK_COMPACT_BEGUN={}", begun.display())],
        );
        let cancel = CancelToken::new();
        let watcher = {
            let cancel = cancel.clone();
            let begun = begun.clone();
            std::thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(30);
                while !begun.is_file()
                    || std::fs::read_to_string(&begun)
                        .unwrap_or_default()
                        .is_empty()
                {
                    assert!(
                        Instant::now() < deadline,
                        "the compaction never began rewriting"
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
                cancel.cancel();
            })
        };
        let controls = RunControls {
            cancel,
            ..RunControls::default()
        };
        let outcome = compact_session(&req, controls, None).expect("a valid compaction");
        watcher.join().unwrap();
        assert_eq!(
            outcome.status,
            CompactStatus::Cancelled,
            "{id}: {outcome:?}"
        );
        assert_eq!(outcome.token.as_deref(), Some(original.as_str()));
        let pid: i32 = std::fs::read_to_string(&begun)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(!alive(pid), "{id}: the harness process {pid} was ended");

        // The session's stored files are byte for byte as before; only the
        // abandoned fork (a file the session never named) was added.
        let after = kept.stored();
        for (path, bytes) in &before {
            let now = after.iter().find(|(p, _)| p == path).map(|(_, b)| b);
            assert_eq!(now, Some(bytes), "{id}: {} unchanged", path.display());
        }
        let (text, token) = kept.turn("gamma");
        assert_eq!(
            text, "context: [alpha, beta, gamma]",
            "{id}: the original context"
        );
        assert_eq!(token, original);
    }
}

/// An image path that is missing, unreadable or not an image is refused as a
/// usage error before any harness starts: the request is an `Err`, and the
/// harness process never ran to record what it was handed.
#[test]
fn a_run_request_naming_an_unusable_image_spawns_nothing() {
    use oneharness_core::errors::OneharnessError;
    let profiles = ProfileScratch::new();
    let scratch = ScratchDir::new("library-lean-bad-image").unwrap();
    let not_an_image = scratch.join("notes.png");
    std::fs::write(&not_an_image, "plain text").unwrap();
    // Unreadable-by-permission is a unix-only case: Windows has no mode bits.
    #[cfg(unix)]
    let locked = {
        use std::os::unix::fs::PermissionsExt;
        let locked = scratch.join("locked.png");
        png(&locked);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        Some(locked)
    };
    #[cfg(not(unix))]
    let locked: Option<PathBuf> = None;
    let cases: Vec<PathBuf> = [scratch.join("missing.png"), not_an_image]
        .into_iter()
        .chain(locked.clone())
        .collect();
    for (index, bad) in cases.iter().enumerate() {
        let input = scratch.join(format!("input-{index}.json"));
        let good = scratch.join(format!("good-{index}.png"));
        png(&good);
        let req = RunRequest {
            harness: vec!["codex".to_string()],
            prompt: vec!["hi".to_string()],
            bin: vec![bin_override("codex")],
            env: vec![
                format!("MOCK_INPUT_FILE={}", input.display()),
                profiles.as_str().to_string(),
            ],
            no_config: true,
            timeout: Some(60),
            // A valid image beside the bad one: one unusable path refuses all.
            images: vec![good, bad.clone()],
            ..RunRequest::default()
        };
        let refused = run(&req, RunControls::default());
        match refused {
            Err(OneharnessError::ImageUnreadable { path, .. }) => {
                assert_eq!(path, bad.display().to_string());
            }
            Err(other) => panic!("{}: expected ImageUnreadable, got {other}", bad.display()),
            Ok(_) => panic!("{}: the run was not refused", bad.display()),
        }
        assert!(!input.exists(), "{}: no harness was started", bad.display());
    }
    #[cfg(unix)]
    if let Some(locked) = &locked {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(locked, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
}
