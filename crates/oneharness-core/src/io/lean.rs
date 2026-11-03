//! The I/O halves of [`crate::domain::lean`]: reading the image files a run
//! attaches, the MCP server names a codex configuration declares, and the
//! per-call figures a codex rollout record holds. Everything they decide is
//! pure and lives in the domain module.

use std::path::{Path, PathBuf};

use crate::domain::lean::{self, Diagnostic, ImageInput, Setting};
use crate::domain::report::{OutputFormat, RunResult, Status};
use crate::domain::signals::Usage;
use crate::errors::OneharnessError;

/// Read and check every image a run attaches, before anything spawns. A
/// relative path is resolved against the process's working directory (where
/// the caller named it), so a harness running elsewhere (`--cwd`) still finds
/// it.
///
/// # Errors
///
/// [`OneharnessError::ImageUnreadable`] for a file that is missing,
/// unreadable, or not a PNG, JPEG, GIF or WebP image.
pub fn read_images(paths: &[PathBuf]) -> Result<Vec<ImageInput>, OneharnessError> {
    paths
        .iter()
        .map(|path| {
            let unreadable = |reason: String| OneharnessError::ImageUnreadable {
                path: path.display().to_string(),
                reason,
            };
            let absolute = std::path::absolute(path).map_err(|e| unreadable(e.to_string()))?;
            let bytes = std::fs::read(&absolute).map_err(|e| unreadable(e.to_string()))?;
            ImageInput::from_bytes(&absolute, &bytes)
                .ok_or_else(|| unreadable("not a PNG, JPEG, GIF or WebP image".to_string()))
        })
        .collect()
}

/// The codex home a launch with `env`, `env_remove` and `cwd` uses — the
/// same resolution the model catalog reads (`model_catalog`): its
/// `CODEX_HOME` unless removed, else `HOME` (or `USERPROFILE`)`/.codex`.
#[must_use]
pub fn codex_home(
    env: &[(String, String)],
    env_remove: &[String],
    cwd: Option<&Path>,
) -> Option<PathBuf> {
    crate::io::model_catalog::resolve_codex_home(env, env_remove, cwd)
}

/// The MCP server names codex would load for a launch in `cwd` with `home`:
/// those of `<home>/config.toml` and of every `.codex/config.toml` from `cwd`
/// upward, deduplicated. A file that cannot be read declares none.
#[must_use]
pub fn codex_mcp_servers(home: Option<&Path>, cwd: &Path) -> Vec<String> {
    let mut files: Vec<PathBuf> = home.map(|h| h.join("config.toml")).into_iter().collect();
    let start = std::path::absolute(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    files.extend(
        start
            .ancestors()
            .map(|dir| dir.join(".codex").join("config.toml")),
    );
    let mut names: Vec<String> = Vec::new();
    for file in files {
        if let Ok(text) = std::fs::read_to_string(&file) {
            for name in lean::codex_mcp_server_names(&text) {
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
    }
    names
}

/// The rollout record codex keeps for `thread` under `home`
/// (`sessions/YYYY/MM/DD/rollout-<stamp>-<thread>.jsonl`).
#[must_use]
pub fn find_codex_rollout(home: &Path, thread: &str) -> Option<PathBuf> {
    let suffix = format!("-{thread}.jsonl");
    let mut stack = vec![home.join("sessions")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("rollout-") && n.ends_with(&suffix))
            {
                return Some(path);
            }
        }
    }
    None
}

/// What a run knew before it spawned that its per-call reading needs.
#[derive(Debug, Clone, Default)]
pub struct CallsPlan {
    /// The settings the harness could not apply, attached to its result.
    pub diagnostics: Vec<Diagnostic>,
    /// The codex home the launch used (codex only).
    pub codex_home: Option<PathBuf>,
    /// A resumed codex thread's rollout and its length before the run, so only
    /// the calls this run appended are read.
    pub codex_resumed_from: Option<(String, u64)>,
}

impl CallsPlan {
    /// Record a resumed codex thread's rollout length before the run.
    pub fn note_codex_resume(&mut self, thread: &str) {
        if let Some(home) = &self.codex_home {
            let length = find_codex_rollout(home, thread)
                .and_then(|path| std::fs::metadata(path).ok())
                .map_or(0, |meta| meta.len());
            self.codex_resumed_from = Some((thread.to_string(), length));
        }
    }
}

/// Attach a finished result's diagnostics and per-call usage.
pub fn attach(result: &mut RunResult, plan: &CallsPlan) {
    if !matches!(
        result.status,
        Status::Ok | Status::Nonzero | Status::Timeout | Status::Cancelled
    ) {
        return;
    }
    result.diagnostics.extend(plan.diagnostics.iter().cloned());
    let id = result.harness.clone();
    let calls = match id.as_str() {
        "claude-code" if result.output_format == OutputFormat::StreamJson => {
            lean::claude_stream_calls(&result.stdout).ok_or_else(|| {
                "the stream-json output carried no assistant usage to read per call".to_string()
            })
        }
        "claude-code" => Err(format!(
            "claude-code reports per-call usage only under `--output-format stream-json`; this \
             run used `{}`",
            result.output_format.as_str()
        )),
        "codex" => codex_calls(result, plan),
        "opencode" => lean::opencode_step_calls(&result.stdout).ok_or_else(|| {
            format!(
                "the `{}` output carried no `step_finish` token figures to read per call",
                result.output_format.as_str()
            )
        }),
        _ => Err(format!(
            "{id} exposes no per-call usage ({})",
            lean::translation(&id, Setting::Calls).source
        )),
    };
    match calls {
        Ok(calls) => result.calls = Some(calls),
        Err(reason) => result
            .diagnostics
            .push(lean::calls_unavailable(&id, &reason)),
    }
}

fn codex_calls(result: &RunResult, plan: &CallsPlan) -> Result<Vec<Usage>, String> {
    let thread = result
        .session_id
        .as_deref()
        .ok_or("codex named no thread, so its rollout record cannot be found")?;
    let home = plan.codex_home.as_deref().ok_or("no codex home to read")?;
    let path = find_codex_rollout(home, thread).ok_or_else(|| {
        format!(
            "no rollout record for thread `{thread}` under {}",
            home.display()
        )
    })?;
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let start = match &plan.codex_resumed_from {
        Some((resumed, length)) if resumed == thread => usize::try_from(*length).unwrap_or(0),
        _ => 0,
    };
    let appended = String::from_utf8_lossy(bytes.get(start..).unwrap_or_default());
    let calls = lean::codex_rollout_calls(&appended);
    if calls.is_empty() {
        Err(format!(
            "the rollout record {} holds no token_count for this run",
            path.display()
        ))
    } else {
        Ok(calls)
    }
}
