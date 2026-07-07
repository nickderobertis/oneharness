//! Streaming, reading, and managing the standardized run history on disk. This
//! is the I/O half of the feature: it reads the clock (to mint session ids and
//! record timestamps), resolves the platform state directory, and writes/reads
//! the JSONL history files. The record *shape* and all string formatting stay
//! pure in `src/domain/history.rs`.
//!
//! Layout: `<dir>/<project-slug>/<session>.jsonl`. One file per `oneharness run`
//! invocation (the "session"), partitioned by a slug of the project directory, so
//! runs from different projects never interleave. Each line is one
//! [`crate::domain::history::HistoryRecord`], appended as a harness run finalizes.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::Value;

use crate::domain::history::{self, HistoryRecord};
use crate::domain::mode::PermissionMode;
use crate::domain::report::RunResult;
use crate::errors::OneharnessError;

/// The file extension for every session log (line-delimited JSON).
const SESSION_EXT: &str = "jsonl";

/// Seconds since the UNIX epoch, UTC. The single clock read the history feature
/// makes — kept here in the I/O layer so `domain::history` stays pure.
fn now_epoch_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        // A clock before 1970 is implausible; fall back to the epoch rather than
        // panic — history is a best-effort side channel.
        .unwrap_or(0)
}

/// The per-user state directory, resolved like [`crate::io::config`]'s config
/// dir but for state/logs: `%LOCALAPPDATA%` on Windows; `$XDG_STATE_HOME` (else
/// `~/.local/state`) everywhere else.
fn state_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        return std::env::var_os("LOCALAPPDATA")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
    }
    if let Some(xdg) = std::env::var_os("XDG_STATE_HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(xdg));
    }
    std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .map(|home| PathBuf::from(home).join(".local").join("state"))
}

/// The effective history directory: the configured path if given, else the
/// platform default `<state dir>/oneharness/history`. `None` only when no path
/// was configured and the platform state dir cannot be resolved (no `$HOME`).
pub fn resolve_dir(configured: Option<&str>) -> Option<PathBuf> {
    match configured {
        Some(p) if !p.is_empty() => Some(PathBuf::from(p)),
        _ => state_dir().map(|d| d.join("oneharness").join("history")),
    }
}

/// A handle to one session's history file, opened once per run and appended to as
/// each harness result finalizes.
pub struct HistoryWriter {
    path: PathBuf,
    session: String,
    name: String,
    project: String,
}

impl HistoryWriter {
    /// Open (create) the session file under `dir` for a run in `project`. Mints
    /// the session id from the sanitized `name`, the current instant, and the pid
    /// so concurrent runs never collide: `<name>-<YYYYMMDDThhmmssZ>-<pid>`. The
    /// project subdirectory is created now; the file itself is created on the
    /// first [`append`](Self::append).
    pub fn open(dir: &Path, project: &Path, name: &str) -> std::io::Result<HistoryWriter> {
        let name = history::sanitize_name(name);
        let project_display = project.display().to_string();
        let slug = history::project_slug(&project_display);
        let session = format!(
            "{name}-{}-{}",
            history::format_compact_utc(now_epoch_secs()),
            std::process::id()
        );
        let project_dir = dir.join(&slug);
        fs::create_dir_all(&project_dir)?;
        let path = project_dir.join(format!("{session}.{SESSION_EXT}"));
        Ok(HistoryWriter {
            path,
            session,
            name,
            project: project_display,
        })
    }

    /// The absolute-or-relative path of the session file (as opened).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append one harness result as a normalized JSONL record, stamped with the
    /// current instant. Creates the file on first write.
    pub fn append(
        &self,
        mode: PermissionMode,
        model: Option<&str>,
        run_prompt: &str,
        result: &RunResult,
    ) -> std::io::Result<()> {
        let record = HistoryRecord::from_result(
            &self.session,
            &self.name,
            &self.project,
            history::format_rfc3339(now_epoch_secs()),
            mode,
            model,
            run_prompt,
            result,
        );
        let mut line = serde_json::to_string(&record)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        line.push('\n');
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        file.write_all(line.as_bytes())
    }
}

/// A one-line summary of a session, for `oneharness history list`. Read from the
/// session's records: `name`/`project`/`started` come from the first record,
/// `harnesses` is the distinct set across all records.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionSummary {
    /// The session id (the file stem), unique and sortable by start time.
    pub id: String,
    /// The human-meaningful session name (non-unique).
    pub name: String,
    /// The project directory the run operated in.
    pub project: String,
    /// The RFC3339 UTC start time (first record's timestamp); empty if unknown.
    pub started: String,
    /// How many harness-run records the session holds.
    pub record_count: usize,
    /// The distinct harness ids the session touched, in first-seen order.
    pub harnesses: Vec<String>,
    /// The absolute path of the session file.
    pub path: String,
}

/// List the sessions under `dir`, newest first. When `project_slug` is `Some`,
/// only that project's subdirectory is scanned; `None` scans every project. A
/// missing `dir` is not an error — it just means no history yet (empty list).
pub fn list_sessions(
    dir: &Path,
    project_slug: Option<&str>,
) -> Result<Vec<SessionSummary>, OneharnessError> {
    let mut sessions = Vec::new();
    if !dir.exists() {
        return Ok(sessions);
    }
    let project_dirs: Vec<PathBuf> = match project_slug {
        Some(slug) => vec![dir.join(slug)],
        None => read_subdirs(dir)?,
    };
    for pdir in project_dirs {
        if !pdir.is_dir() {
            continue;
        }
        for path in read_session_files(&pdir)? {
            sessions.push(summarize(&path)?);
        }
    }
    // Newest first. RFC3339 sorts lexically as chronologically; a session with no
    // readable timestamp (empty `started`) sorts last.
    sessions.sort_by(|a, b| b.started.cmp(&a.started).then(a.id.cmp(&b.id)));
    Ok(sessions)
}

/// The sessions whose id OR name equals `needle`, newest first (a name is
/// non-unique). Pure over an already-listed set, so the caller walks the fs once.
pub fn match_sessions<'a>(sessions: &'a [SessionSummary], needle: &str) -> Vec<&'a SessionSummary> {
    sessions
        .iter()
        .filter(|s| s.id == needle || s.name == needle)
        .collect()
}

/// Read a session file into its records (one JSON value per non-empty line).
pub fn read_session(path: &Path) -> Result<Vec<Value>, OneharnessError> {
    let text = fs::read_to_string(path).map_err(|source| OneharnessError::HistoryIo {
        path: path.display().to_string(),
        source,
    })?;
    Ok(parse_lines(&text))
}

/// Delete every session file under `dir` (optionally restricted to one project
/// slug), returning the paths removed. Empty project subdirectories left behind
/// are pruned. A missing `dir` removes nothing.
pub fn remove_sessions(
    dir: &Path,
    project_slug: Option<&str>,
) -> Result<Vec<String>, OneharnessError> {
    let mut removed = Vec::new();
    if !dir.exists() {
        return Ok(removed);
    }
    let project_dirs: Vec<PathBuf> = match project_slug {
        Some(slug) => vec![dir.join(slug)],
        None => read_subdirs(dir)?,
    };
    for pdir in project_dirs {
        if !pdir.is_dir() {
            continue;
        }
        for path in read_session_files(&pdir)? {
            fs::remove_file(&path).map_err(|source| OneharnessError::HistoryIo {
                path: path.display().to_string(),
                source,
            })?;
            removed.push(path.display().to_string());
        }
        // Prune the project subdir if it is now empty (best-effort).
        if read_session_files(&pdir)?.is_empty() {
            let _ = fs::remove_dir(&pdir);
        }
    }
    removed.sort();
    Ok(removed)
}

/// The immediate subdirectories of `dir` (the project slugs).
fn read_subdirs(dir: &Path) -> Result<Vec<PathBuf>, OneharnessError> {
    let mut dirs = Vec::new();
    for entry in read_dir(dir)? {
        let path = entry.path();
        if path.is_dir() {
            dirs.push(path);
        }
    }
    Ok(dirs)
}

/// The `*.jsonl` session files directly inside a project subdirectory.
fn read_session_files(pdir: &Path) -> Result<Vec<PathBuf>, OneharnessError> {
    let mut files = Vec::new();
    for entry in read_dir(pdir)? {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some(SESSION_EXT) {
            files.push(path);
        }
    }
    Ok(files)
}

fn read_dir(dir: &Path) -> Result<Vec<fs::DirEntry>, OneharnessError> {
    fs::read_dir(dir)
        .map_err(|source| OneharnessError::HistoryIo {
            path: dir.display().to_string(),
            source,
        })?
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|source| OneharnessError::HistoryIo {
            path: dir.display().to_string(),
            source,
        })
}

/// Build a [`SessionSummary`] by reading a session file. Robust to a partial or
/// empty file: fields the records don't supply fall back to the file stem / the
/// slug / empty.
fn summarize(path: &Path) -> Result<SessionSummary, OneharnessError> {
    let id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();
    let slug = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();
    let records = read_session(path)?;

    let field = |key: &str| -> Option<String> {
        records
            .first()
            .and_then(|r| r.get(key))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    };
    let mut harnesses: Vec<String> = Vec::new();
    for r in &records {
        if let Some(h) = r.get("harness").and_then(|v| v.as_str()) {
            if !harnesses.iter().any(|x| x == h) {
                harnesses.push(h.to_string());
            }
        }
    }
    Ok(SessionSummary {
        name: field("name").unwrap_or_else(|| id.clone()),
        project: field("project").unwrap_or(slug),
        started: field("timestamp").unwrap_or_default(),
        record_count: records.len(),
        harnesses,
        path: path.display().to_string(),
        id,
    })
}

/// Parse a JSONL blob into one value per non-empty line, skipping lines that do
/// not parse (a truncated last line from an interrupted run is not fatal).
fn parse_lines(text: &str) -> Vec<Value> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::report::{OutputFormat, Status};
    use crate::domain::signals::Usage;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oneharness-hist-{tag}-{}-{}",
            std::process::id(),
            now_epoch_secs()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn result(harness: &str) -> RunResult {
        RunResult {
            harness: harness.to_string(),
            bin: "bin".to_string(),
            available: true,
            status: Status::Ok,
            prompt: None,
            exit_code: Some(0),
            duration_ms: Some(10),
            command: vec!["bin".to_string()],
            output_format: OutputFormat::Json,
            text: Some("hi".to_string()),
            text_source: Some("raw".to_string()),
            usage: Usage::default(),
            usage_source: None,
            session_id: None,
            events: None,
            events_source: None,
            structured: None,
            schema_valid: None,
            schema_attempts: None,
            schema_error: None,
            failure_kind: None,
            failure_kind_source: None,
            stdout: "hi".to_string(),
            stderr: String::new(),
            error: None,
        }
    }

    #[test]
    fn resolve_dir_prefers_configured_then_default() {
        assert_eq!(
            resolve_dir(Some("/tmp/custom")),
            Some(PathBuf::from("/tmp/custom"))
        );
        // Empty configured value falls through to the platform default (if any).
        // We can't assert the default path portably, but it must end with the
        // history segments when a state dir is resolvable.
        if let Some(def) = resolve_dir(None) {
            assert!(def.ends_with("oneharness/history"));
        }
    }

    #[test]
    fn open_creates_project_subdir_and_expected_path() {
        let dir = temp_dir("open");
        let project = PathBuf::from("/home/user/My Proj");
        let w = HistoryWriter::open(&dir, &project, "Fix Bug!").unwrap();
        // Project slug subdir exists.
        assert!(dir.join("home-user-My-Proj").is_dir());
        // Session id: sanitized name, compact UTC, pid.
        let stem = w.path().file_stem().unwrap().to_str().unwrap();
        assert!(stem.starts_with("fix-bug-"), "{stem}");
        assert!(w.path().extension().unwrap() == "jsonl");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn append_writes_parseable_jsonl_lines() {
        let dir = temp_dir("append");
        let project = PathBuf::from("/proj/a");
        let w = HistoryWriter::open(&dir, &project, "my-session").unwrap();
        w.append(
            PermissionMode::Bypass,
            Some("sonnet"),
            "do it",
            &result("claude-code"),
        )
        .unwrap();
        w.append(
            PermissionMode::Bypass,
            Some("sonnet"),
            "do it",
            &result("codex"),
        )
        .unwrap();
        let records = read_session(w.path()).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["harness"], "claude-code");
        assert_eq!(records[0]["name"], "my-session");
        assert_eq!(records[0]["project"], "/proj/a");
        assert_eq!(records[0]["model"], "sonnet");
        assert_eq!(records[1]["harness"], "codex");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_sessions_summarizes_and_orders_newest_first() {
        let dir = temp_dir("list");
        // Two projects, hand-written so we control timestamps/order.
        fs::create_dir_all(dir.join("proj-a")).unwrap();
        fs::create_dir_all(dir.join("proj-b")).unwrap();
        fs::write(
            dir.join("proj-a").join("old-20240101T000000Z-1.jsonl"),
            "{\"name\":\"old\",\"project\":\"/proj/a\",\"timestamp\":\"2024-01-01T00:00:00Z\",\"harness\":\"codex\"}\n",
        )
        .unwrap();
        fs::write(
            dir.join("proj-b").join("new-20260101T000000Z-2.jsonl"),
            "{\"name\":\"new\",\"project\":\"/proj/b\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"harness\":\"claude-code\"}\n\
             {\"name\":\"new\",\"project\":\"/proj/b\",\"timestamp\":\"2026-01-01T00:00:01Z\",\"harness\":\"codex\"}\n",
        )
        .unwrap();
        let all = list_sessions(&dir, None).unwrap();
        assert_eq!(all.len(), 2);
        // Newest first.
        assert_eq!(all[0].name, "new");
        assert_eq!(all[0].record_count, 2);
        assert_eq!(all[0].harnesses, vec!["claude-code", "codex"]);
        assert_eq!(all[1].name, "old");
        // Project filter restricts to one subdir.
        let just_a = list_sessions(&dir, Some("proj-a")).unwrap();
        assert_eq!(just_a.len(), 1);
        assert_eq!(just_a[0].name, "old");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn summarize_falls_back_on_an_empty_session_file() {
        // A session file with no readable records (e.g. an interrupted first
        // write) still summarizes: name/project fall back to the stem/slug and
        // the count is zero, rather than erroring.
        let dir = temp_dir("emptyfile");
        fs::create_dir_all(dir.join("some-proj")).unwrap();
        fs::write(dir.join("some-proj").join("stub-1.jsonl"), "\n").unwrap();
        let sessions = list_sessions(&dir, None).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "stub-1");
        assert_eq!(sessions[0].name, "stub-1");
        assert_eq!(sessions[0].project, "some-proj");
        assert_eq!(sessions[0].started, "");
        assert_eq!(sessions[0].record_count, 0);
        assert!(sessions[0].harnesses.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_dir_lists_nothing() {
        let dir = std::env::temp_dir().join("oneharness-hist-absent-does-not-exist-xyz");
        let _ = fs::remove_dir_all(&dir);
        assert!(list_sessions(&dir, None).unwrap().is_empty());
        assert!(remove_sessions(&dir, None).unwrap().is_empty());
    }

    #[test]
    fn match_sessions_by_id_or_name_newest_first() {
        let sessions = list_from(&[
            ("dup-20240101T000000Z-1", "dup", "2024-01-01T00:00:00Z"),
            ("dup-20260101T000000Z-2", "dup", "2026-01-01T00:00:00Z"),
            ("other-20250101T000000Z-3", "other", "2025-01-01T00:00:00Z"),
        ]);
        // By name: both `dup` sessions, newest first.
        let m = match_sessions(&sessions, "dup");
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].id, "dup-20260101T000000Z-2");
        // By exact id: just that one.
        let m = match_sessions(&sessions, "other-20250101T000000Z-3");
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].name, "other");
        assert!(match_sessions(&sessions, "nope").is_empty());
    }

    /// Build a pre-sorted (newest-first) summary list for the pure matcher test.
    fn list_from(rows: &[(&str, &str, &str)]) -> Vec<SessionSummary> {
        let mut v: Vec<SessionSummary> = rows
            .iter()
            .map(|(id, name, started)| SessionSummary {
                id: id.to_string(),
                name: name.to_string(),
                project: "/p".to_string(),
                started: started.to_string(),
                record_count: 1,
                harnesses: vec!["codex".to_string()],
                path: format!("/h/{id}.jsonl"),
            })
            .collect();
        v.sort_by(|a, b| b.started.cmp(&a.started));
        v
    }

    #[test]
    fn remove_sessions_deletes_and_prunes() {
        let dir = temp_dir("remove");
        let w = HistoryWriter::open(&dir, &PathBuf::from("/proj/x"), "s").unwrap();
        w.append(PermissionMode::Default, None, "p", &result("codex"))
            .unwrap();
        assert_eq!(list_sessions(&dir, None).unwrap().len(), 1);
        let removed = remove_sessions(&dir, None).unwrap();
        assert_eq!(removed.len(), 1);
        assert!(list_sessions(&dir, None).unwrap().is_empty());
        // The now-empty project subdir was pruned.
        assert!(!dir.join("proj-x").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_lines_skips_blank_and_bad_lines() {
        let text = "{\"a\":1}\n\n  \nnot json\n{\"b\":2}\n";
        let vals = parse_lines(text);
        assert_eq!(vals.len(), 2);
        assert_eq!(vals[0]["a"], 1);
        assert_eq!(vals[1]["b"], 2);
    }
}
