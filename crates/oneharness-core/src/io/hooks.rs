//! Installing a normalized hook into a harness's project config — the I/O half
//! of cross-harness hook management. The shape rendering is pure
//! (`src/domain/hooks.rs`); this layer resolves the target file(s), seeds a new
//! file's scaffolding, deep-merges (or writes a JS shim), and writes atomically.
//!
//! Every write is non-destructive and idempotent, the same contract `oneharness
//! sync` already honours for permission rules: unrelated keys are preserved,
//! hook lists union, an unparseable target is refused and left intact, and the
//! write goes through a temp file + rename. The four [`HookBinding`] variants
//! cover all eight harnesses — three deep-merge JSON, OpenCode writes a shim.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::domain::harness::{HarnessSpec, HookBinding};
use crate::domain::hooks::{render, HookSpec};
use crate::domain::sync::deep_merge;
use crate::errors::OneharnessError;
use crate::io::sync::{write_atomically, FileStatus};

/// The plugin/file identity used when a [`HookSpec`] names none.
const DEFAULT_PLUGIN_NAME: &str = "oneharness";

/// One file `install` created, updated, or found already current.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookWrite {
    pub path: PathBuf,
    pub status: FileStatus,
}

/// Install `hook` into `spec`'s project config under `project_dir`, returning a
/// write per file touched (a Goose install touches two). `check` plans without
/// writing. A harness with no [`HookBinding`] is a loud [`OneharnessError`],
/// never a silent no-op.
pub fn install(
    project_dir: &Path,
    spec: &HarnessSpec,
    hook: &HookSpec,
    check: bool,
) -> Result<Vec<HookWrite>, OneharnessError> {
    let Some(binding) = &spec.hooks else {
        return Err(OneharnessError::HookUnsupported { id: spec.id.into() });
    };
    let name = hook.plugin_name.as_deref().unwrap_or(DEFAULT_PLUGIN_NAME);

    match binding {
        HookBinding::SameFile { shape, path } => {
            // Hooks share the permissions file, so honour the same alt_files
            // precedence `sync` does (e.g. crush's `.crush.json`).
            let sync = spec
                .sync
                .as_ref()
                .expect("SameFile hooks require a sync config file");
            let target = sync
                .alt_files
                .iter()
                .map(|f| project_dir.join(f))
                .find(|p| p.is_file())
                .unwrap_or_else(|| project_dir.join(sync.file));
            let fragment = wrap(path, render(hook, *shape));
            Ok(vec![merge_json(&target, &fragment, None, check)?])
        }
        HookBinding::File {
            shape,
            file,
            path,
            seed,
        } => {
            let target = project_dir.join(file.replace("{name}", name));
            let fragment = wrap(path, render(hook, *shape));
            Ok(vec![merge_json(&target, &fragment, *seed, check)?])
        }
        HookBinding::GoosePlugin {
            shape,
            plugins_dir,
            manifest,
            path,
        } => {
            let base = project_dir.join(plugins_dir).join(name);
            let manifest_json = parse_seed(&manifest.replace("{name}", name));
            let manifest_write =
                merge_json(&base.join("plugin.json"), &manifest_json, None, check)?;
            let fragment = wrap(path, render(hook, *shape));
            let hooks_write = merge_json(
                &base.join("hooks").join("hooks.json"),
                &fragment,
                None,
                check,
            )?;
            Ok(vec![manifest_write, hooks_write])
        }
        HookBinding::JsPlugin {
            plugin_dir,
            template,
        } => {
            let target = project_dir.join(plugin_dir).join(format!("{name}.js"));
            let content = render_shim(template, name, &hook.command);
            Ok(vec![write_text(&target, &content, check)?])
        }
    }
}

/// Nest `value` under `path` (e.g. `["hooks"]` -> `{"hooks": value}`), the
/// object the harness reads at its top level.
fn wrap(path: &[&str], value: Value) -> Value {
    let mut node = value;
    for key in path.iter().rev() {
        let mut map = Map::new();
        map.insert((*key).to_string(), node);
        node = Value::Object(map);
    }
    node
}

/// Render the OpenCode shim: substitute the JS-safe export identifier, the
/// command as a JSON argv array, and the display name.
fn render_shim(template: &str, name: &str, command: &str) -> String {
    let argv: Vec<&str> = command.split_whitespace().collect();
    let argv_json = serde_json::to_string(&argv).expect("argv of strings serializes");
    template
        .replace("{export}", &js_identifier(name))
        .replace("{argv}", &argv_json)
        .replace("{name}", name)
}

/// A safe JS identifier from a plugin name: non-alphanumeric runs become `_`,
/// and a leading digit is prefixed, so `my-tool` -> `my_tool`.
fn js_identifier(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if out.chars().next().is_none_or(|c| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

/// Parse a registry seed/manifest, which is test-pinned valid JSON.
fn parse_seed(text: &str) -> Value {
    serde_json::from_str(text).expect("registry seed/manifest is valid JSON (test-pinned)")
}

/// Deep-merge `fragment` into `target`'s JSON. A missing file is created from
/// `seed` (or an empty object) then merged; an existing one is merged in place.
/// An unparseable target is refused and left untouched.
fn merge_json(
    target: &Path,
    fragment: &Value,
    seed: Option<&str>,
    check: bool,
) -> Result<HookWrite, OneharnessError> {
    let existing: Option<Value> = match std::fs::read_to_string(target) {
        Ok(text) => Some(serde_json::from_str(&text).map_err(|e| {
            OneharnessError::HarnessConfigUnmergeable {
                path: target.display().to_string(),
                message: format!("not valid JSON ({e}); fix or remove it and re-run"),
            }
        })?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(source) => {
            return Err(OneharnessError::HarnessConfigRead {
                path: target.display().to_string(),
                source,
            })
        }
    };

    let (merged, status) = match &existing {
        Some(existing) => {
            let merged = deep_merge(existing, fragment);
            let status = if &merged == existing {
                FileStatus::Unchanged
            } else {
                FileStatus::Updated
            };
            (merged, status)
        }
        None => {
            let base = seed.map(parse_seed).unwrap_or(Value::Object(Map::new()));
            (deep_merge(&base, fragment), FileStatus::Created)
        }
    };

    if !check && status != FileStatus::Unchanged {
        write_atomically(target, &merged)?;
    }
    Ok(HookWrite {
        path: target.to_path_buf(),
        status,
    })
}

/// Write a text file (the JS shim) idempotently and atomically: unchanged when
/// the bytes already match, else created/updated via a temp file + rename.
fn write_text(target: &Path, content: &str, check: bool) -> Result<HookWrite, OneharnessError> {
    let existing = match std::fs::read_to_string(target) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(source) => {
            return Err(OneharnessError::HarnessConfigRead {
                path: target.display().to_string(),
                source,
            })
        }
    };
    let status = match &existing {
        Some(text) if text == content => FileStatus::Unchanged,
        Some(_) => FileStatus::Updated,
        None => FileStatus::Created,
    };

    if !check && status != FileStatus::Unchanged {
        let write_err = |source: std::io::Error| OneharnessError::HarnessConfigWrite {
            path: target.display().to_string(),
            source,
        };
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(write_err)?;
        }
        let tmp = target.with_extension("oneharness.tmp");
        std::fs::write(&tmp, content).map_err(write_err)?;
        std::fs::rename(&tmp, target).map_err(write_err)?;
    }
    Ok(HookWrite {
        path: target.to_path_buf(),
        status,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::harness;
    use serde_json::json;

    fn temp_project(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oneharness-hooks-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn install_one(dir: &Path, id: &str, hook: &HookSpec) -> Vec<HookWrite> {
        install(dir, harness::by_id(id).unwrap(), hook, false).expect("install should succeed")
    }

    fn read_json(path: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    /// Claude shares its permissions file: the hook lands under `hooks` in
    /// `.claude/settings.json` without disturbing existing keys.
    #[test]
    fn same_file_merges_into_claude_settings_without_clobbering() {
        let dir = temp_project("claude");
        std::fs::create_dir_all(dir.join(".claude")).unwrap();
        std::fs::write(
            dir.join(".claude/settings.json"),
            r#"{"permissions":{"allow":["Read"]}}"#,
        )
        .unwrap();
        let hook = HookSpec {
            command: "guard hook claude-code".into(),
            matcher: Some("Bash".into()),
            timeout: None,
            plugin_name: None,
        };
        let writes = install_one(&dir, "claude-code", &hook);
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].status, FileStatus::Updated);
        assert_eq!(
            read_json(&dir.join(".claude/settings.json")),
            json!({
                "permissions": { "allow": ["Read"] },
                "hooks": {
                    "PreToolUse": [
                        { "matcher": "Bash", "hooks": [{ "type": "command", "command": "guard hook claude-code" }] }
                    ]
                }
            }),
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Codex hooks live in a dedicated file that is created on first install.
    #[test]
    fn file_strategy_creates_codex_hooks_file() {
        let dir = temp_project("codex");
        let writes = install_one(&dir, "codex", &HookSpec::command("guard hook codex"));
        assert_eq!(writes[0].status, FileStatus::Created);
        assert_eq!(writes[0].path, dir.join(".codex/hooks.json"));
        assert_eq!(
            read_json(&dir.join(".codex/hooks.json")),
            json!({
                "hooks": { "PreToolUse": [{ "hooks": [{ "type": "command", "command": "guard hook codex" }] }] }
            }),
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Cursor's dedicated file is seeded with its required `version` and fans
    /// the command across the three `before*` events.
    #[test]
    fn file_strategy_seeds_cursor_version() {
        let dir = temp_project("cursor");
        install_one(&dir, "cursor", &HookSpec::command("guard hook cursor"));
        assert_eq!(
            read_json(&dir.join(".cursor/hooks.json")),
            json!({
                "version": 1,
                "hooks": {
                    "beforeShellExecution": [{ "command": "guard hook cursor" }],
                    "beforeReadFile": [{ "command": "guard hook cursor" }],
                    "beforeMCPExecution": [{ "command": "guard hook cursor" }],
                }
            }),
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Copilot's per-owner file name comes from the plugin identity.
    #[test]
    fn file_strategy_names_copilot_file_after_plugin() {
        let dir = temp_project("copilot");
        let hook = HookSpec {
            plugin_name: Some("guard".into()),
            ..HookSpec::command("guard hook copilot")
        };
        let writes = install_one(&dir, "copilot", &hook);
        assert_eq!(writes[0].path, dir.join(".github/hooks/guard.json"));
        assert_eq!(
            read_json(&dir.join(".github/hooks/guard.json")),
            json!({
                "version": 1,
                "hooks": {
                    "preToolUse": [
                        { "type": "command", "bash": "guard hook copilot", "powershell": "guard hook copilot" }
                    ]
                }
            }),
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Goose installs two files: a one-time manifest and the hooks json, both
    /// under a plugin dir named for the plugin identity.
    #[test]
    fn goose_plugin_writes_manifest_and_hooks() {
        let dir = temp_project("goose");
        let hook = HookSpec {
            command: "guard hook goose".into(),
            matcher: Some("^(shell|read)$".into()),
            timeout: Some(10),
            plugin_name: None,
        };
        let writes = install_one(&dir, "goose", &hook);
        assert_eq!(writes.len(), 2);
        let plugin = dir.join(".agents/plugins/oneharness");
        assert_eq!(
            read_json(&plugin.join("plugin.json")),
            json!({
                "name": "oneharness",
                "version": "0.1.0",
                "description": "Pre-tool hook installed by oneharness.",
            }),
        );
        assert_eq!(
            read_json(&plugin.join("hooks/hooks.json")),
            json!({
                "hooks": {
                    "PreToolUse": [
                        {
                            "matcher": "^(shell|read)$",
                            "hooks": [{ "type": "command", "command": "guard hook goose", "timeout": 10 }]
                        }
                    ]
                }
            }),
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// OpenCode gets a JS shim that spawns the command as an argv array under
    /// its plugin name; the command is wired in, never hardcoded.
    #[test]
    fn js_plugin_writes_shim_with_command_argv() {
        let dir = temp_project("opencode");
        let hook = HookSpec {
            plugin_name: Some("guard".into()),
            ..HookSpec::command("guard hook opencode")
        };
        let writes = install_one(&dir, "opencode", &hook);
        assert_eq!(writes[0].path, dir.join(".opencode/plugin/guard.js"));
        let shim = std::fs::read_to_string(writes[0].path.clone()).unwrap();
        assert!(
            shim.contains(r#"Bun.spawn(["guard","hook","opencode"]"#),
            "command must be wired into the shim as an argv array:\n{shim}"
        );
        assert!(
            shim.contains("export const guard ="),
            "export uses the plugin identity:\n{shim}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Re-installing the same hook is a no-op everywhere — across plain files,
    /// the seeded Cursor file, the Goose plugin pair, and the JS shim.
    #[test]
    fn reinstall_is_idempotent_across_strategies() {
        for id in [
            "claude-code",
            "codex",
            "cursor",
            "copilot",
            "goose",
            "opencode",
        ] {
            let dir = temp_project(&format!("idem-{id}"));
            let hook = HookSpec {
                command: format!("guard hook {id}"),
                matcher: Some("Bash".into()),
                timeout: Some(10),
                plugin_name: None,
            };
            install_one(&dir, id, &hook);
            let second = install(dir.as_path(), harness::by_id(id).unwrap(), &hook, false).unwrap();
            assert!(
                second.iter().all(|w| w.status == FileStatus::Unchanged),
                "second install of `{id}` must be all-unchanged, got {second:?}"
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// `check` plans the writes without creating any file.
    #[test]
    fn check_mode_writes_nothing() {
        let dir = temp_project("check");
        let writes = install(
            &dir,
            harness::by_id("codex").unwrap(),
            &HookSpec::command("x"),
            true,
        )
        .unwrap();
        assert_eq!(writes[0].status, FileStatus::Created);
        assert!(!dir.join(".codex/hooks.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An unparseable target is refused and left exactly as it was.
    #[test]
    fn unparseable_target_is_refused_and_untouched() {
        let dir = temp_project("bad");
        std::fs::create_dir_all(dir.join(".codex")).unwrap();
        let path = dir.join(".codex/hooks.json");
        std::fs::write(&path, "{ not json").unwrap();
        let err = install(
            &dir,
            harness::by_id("codex").unwrap(),
            &HookSpec::command("x"),
            false,
        )
        .unwrap_err();
        assert!(err.to_string().contains("not valid JSON"), "{err}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Every harness in the registry can take a hook — the cross-harness
    /// promise — and each install produces at least one write.
    #[test]
    fn every_harness_supports_hook_install() {
        for spec in harness::all() {
            let dir = temp_project(&format!("all-{}", spec.id));
            let writes = install(&dir, spec, &HookSpec::command("guard hook x"), false)
                .unwrap_or_else(|e| panic!("{}: {e}", spec.id));
            assert!(!writes.is_empty(), "{}: no writes", spec.id);
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}
