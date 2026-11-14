//! A sync-policy field outside `[sync]` through the library: the public config
//! load refuses it, naming the file, the field and its `[sync]` spelling,
//! whether the field sits in the file the caller named or in a parent that file
//! `extends`. A sync request over the same file is refused before it writes
//! anything. `crates/oneharness-e2e/tests/cli.rs` drives the same refusal
//! through the binary.
//!
//! These loads read the environment layer, so this is its own test binary: it
//! strips every ambient `ONEHARNESS_*` override once and serializes its tests
//! over [`ENV_LOCK`].

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use oneharness_core::io::config;
use oneharness_core::io::scratch::ScratchDir;
use oneharness_core::io::sync::{self, SyncRequest};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn hermetic() -> MutexGuard<'static, ()> {
    let guard = ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
    for (name, _) in std::env::vars() {
        if name.starts_with("ONEHARNESS_") {
            std::env::remove_var(name);
        }
    }
    guard
}

/// Each misplaced field, at each place outside `[sync]` it can sit, with the
/// field name and the `[sync]` spelling the refusal must give for it.
const MISPLACED: &[(&str, &str, &str)] = &[
    (
        "denied_tools = [\"Bash(rm:*)\"]\n",
        "`denied_tools` at the top level",
        "`[sync] denied_tools`",
    ),
    (
        "[[hooks]]\ncommand = \"gate {harness}\"\n",
        "`hooks` at the top level",
        "`[[sync.hooks]]`",
    ),
    (
        "[harness.claude-code]\ndenied_tools = [\"Bash(rm:*)\"]\n",
        "`denied_tools` in `[harness.claude-code]`",
        "`[sync.harness.claude-code] denied_tools`",
    ),
    (
        "[harness.claude-code.hooks]\nPreToolUse = []\n",
        "`hooks` in `[harness.claude-code]`",
        "`[sync.harness.claude-code.hooks]`",
    ),
    (
        "[harness.opencode.settings.permission]\nedit = \"deny\"\n",
        "`settings` in `[harness.opencode]`",
        "`[sync.harness.opencode.settings]`",
    ),
    (
        "[harness.claude-code.variant.work]\ndenied_tools = [\"Bash(rm:*)\"]\n",
        "`denied_tools` in `[harness.claude-code.variant.work]`",
        "`[sync.harness.claude-code.variant.work] denied_tools`",
    ),
    (
        "[harness.claude-code.variant.work.hooks]\nPreToolUse = []\n",
        "`hooks` in `[harness.claude-code.variant.work]`",
        "`[sync.harness.claude-code.variant.work.hooks]`",
    ),
    (
        "[harness.claude-code.variant.work.settings]\nmodel = \"x\"\n",
        "`settings` in `[harness.claude-code.variant.work]`",
        "`[sync.harness.claude-code.variant.work.settings]`",
    ),
];

fn assert_names(message: &str, file: &Path, field: &str, spelling: &str) {
    let file = file.display().to_string();
    for expected in [file.as_str(), field, spelling] {
        assert!(message.contains(expected), "missing {expected}: {message}");
    }
}

/// Every file under `dir`, with its bytes, so a refusal can be shown to have
/// written nothing.
fn tree(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut files = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in std::fs::read_dir(&next).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let bytes = std::fs::read(&path).unwrap();
                files.push((path, bytes));
            }
        }
    }
    files.sort();
    files
}

#[test]
fn the_library_config_load_refuses_a_sync_policy_field_outside_sync() {
    let _env = hermetic();
    for (index, (toml, field, spelling)) in MISPLACED.iter().enumerate() {
        let dir = ScratchDir::new(&format!("library-refusal-{index}")).unwrap();
        let file = dir.join("oneharness.toml");
        std::fs::write(&file, toml).unwrap();

        let loaded = config::load(std::slice::from_ref(&file), false, &dir);
        let message = loaded.expect_err(toml).to_string();
        assert_names(&message, &file, field, spelling);

        let layers = config::load_layers(std::slice::from_ref(&file), false, &dir);
        let message = layers.expect_err(toml).to_string();
        assert_names(&message, &file, field, spelling);

        let before = tree(&dir);
        let synced = sync::sync(&SyncRequest {
            config: vec![file.clone()],
            cwd: Some(dir.to_path_buf()),
            harness: vec!["claude-code".to_string()],
            ..SyncRequest::default()
        });
        assert_names(&synced.expect_err(toml).to_string(), &file, field, spelling);
        assert_eq!(tree(&dir), before, "a refused sync wrote: {toml}");
    }
}

#[test]
fn the_library_config_load_refuses_a_sync_policy_field_in_an_extends_parent() {
    let _env = hermetic();
    for (index, (toml, field, spelling)) in MISPLACED.iter().enumerate() {
        let dir = ScratchDir::new(&format!("library-refusal-parent-{index}")).unwrap();
        let parent = dir.join("shared").join("base.toml");
        std::fs::create_dir_all(parent.parent().unwrap()).unwrap();
        std::fs::write(&parent, toml).unwrap();
        // The child itself is valid: its run-level field and its `[sync]`
        // section are both where they belong.
        let child = dir.join("oneharness.toml");
        std::fs::write(
            &child,
            "extends = \"shared/base.toml\"\nmodel = \"m\"\n[sync]\nallowed_tools = [\"Read\"]\n",
        )
        .unwrap();

        let loaded = config::load(std::slice::from_ref(&child), false, &dir);
        let message = loaded.expect_err(toml).to_string();
        assert_names(&message, &parent, field, spelling);

        let before = tree(&dir);
        let synced = sync::sync(&SyncRequest {
            config: vec![child.clone()],
            cwd: Some(dir.to_path_buf()),
            harness: vec!["claude-code".to_string()],
            ..SyncRequest::default()
        });
        assert_names(
            &synced.expect_err(toml).to_string(),
            &parent,
            field,
            spelling,
        );
        assert_eq!(tree(&dir), before, "a refused sync wrote: {toml}");
    }
}

#[test]
fn the_same_policy_inside_sync_loads() {
    // The control: the moved spellings load, so the refusals above are about
    // where each field sits, not about the field.
    let _env = hermetic();
    let dir = ScratchDir::new("library-refusal-control").unwrap();
    let file = dir.join("oneharness.toml");
    std::fs::write(
        &file,
        "[sync]\ndenied_tools = [\"Bash(rm:*)\"]\n[[sync.hooks]]\ncommand = \"gate {harness}\"\n\
         [sync.harness.claude-code]\ndenied_tools = [\"Bash(rm:*)\"]\n\
         [sync.harness.claude-code.hooks]\nPreToolUse = []\n\
         [sync.harness.opencode.settings.permission]\nedit = \"deny\"\n\
         [sync.harness.claude-code.variant.work]\ndenied_tools = [\"Bash(rm:*)\"]\n\
         [sync.harness.claude-code.variant.work.settings]\nmodel = \"x\"\n",
    )
    .unwrap();
    let loaded = config::load(std::slice::from_ref(&file), false, &dir).expect("loads");
    assert_eq!(
        loaded.config.sync.denied_tools_for("claude-code:work"),
        ["Bash(rm:*)".to_string()]
    );
}
