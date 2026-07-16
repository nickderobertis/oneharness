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

use std::collections::{BTreeSet, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use fs2::FileExt;

use crate::domain::history::{self, HistoryId, HistoryLabels, HistoryRecord};
use crate::domain::mode::PermissionMode;
use crate::domain::report::RunResult;
use crate::errors::OneharnessError;

/// The file extension for every session log (line-delimited JSON).
const SESSION_EXT: &str = "jsonl";
const INDEX_FILE: &str = ".index.jsonl";
const INDEX_LOCK_FILE: &str = ".index.lock";

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
    dir: PathBuf,
    path: PathBuf,
    relative_path: String,
    session: String,
    name: String,
    labels: HistoryLabels,
    project: String,
}

impl HistoryWriter {
    /// Open (create) the session file under `dir` for a run in `project`. Mints
    /// the session id from the sanitized `name`, the current instant, and the pid
    /// so concurrent runs never collide: `<name>-<YYYYMMDDThhmmssZ>-<pid>`. The
    /// project subdirectory is created now; the file itself is created on the
    /// first [`append`](Self::append).
    pub fn open(
        dir: &Path,
        project: &Path,
        name: &str,
        labels: HistoryLabels,
    ) -> std::io::Result<HistoryWriter> {
        let name = history::sanitize_name(name);
        let project = fs::canonicalize(project)?;
        let project_display = project.display().to_string();
        let slug = history::project_slug(&project_display);
        let session = format!(
            "{name}-{}-{}",
            history::format_compact_utc(now_epoch_secs()),
            std::process::id()
        );
        fs::create_dir_all(dir)?;
        let dir = fs::canonicalize(dir)?;
        let project_dir = dir.join(&slug);
        fs::create_dir_all(&project_dir)?;
        let path = project_dir.join(format!("{session}.{SESSION_EXT}"));
        let relative_path = path
            .strip_prefix(&dir)
            .unwrap_or(&path)
            .display()
            .to_string();
        reconcile_index(&dir).map_err(history_error_to_io)?;
        Ok(HistoryWriter {
            dir,
            path,
            relative_path,
            session,
            name,
            labels,
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
            HistoryId::from_uuid(uuid::Uuid::now_v7()),
            &self.session,
            &self.name,
            &self.labels,
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
        file.write_all(line.as_bytes())?;
        file.flush()?;
        append_index_entry(
            &self.dir,
            &HistoryIndexEntry {
                session_path: self.relative_path.clone(),
                record,
            },
        )
    }
}

/// One append-only index entry. The session JSONL remains authoritative; the
/// relative path lets reconciliation suppress entries whose session was cleared.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct HistoryIndexEntry {
    session_path: String,
    record: HistoryRecord,
}

struct ReconciledIndex {
    entries: Vec<HistoryIndexEntry>,
    active_ids: BTreeSet<HistoryId>,
    offset: u64,
}

/// A reconciled, resumable reader over the append-only index. Opening performs
/// the only full history-tree scan; [`poll`](Self::poll) tails the index file by
/// byte offset and never scans the tree again.
pub struct HistoryWatcher {
    index_path: PathBuf,
    offset: u64,
    pending: VecDeque<HistoryRecord>,
    seen: BTreeSet<HistoryId>,
    labels: HistoryLabels,
    project_slug: Option<String>,
}

impl HistoryWatcher {
    /// Reconcile the index and prepare to emit records strictly after `after`.
    /// Without a cursor, every active indexed record is initially pending.
    pub fn open(
        dir: &Path,
        after: Option<HistoryId>,
        labels: HistoryLabels,
        project_slug: Option<String>,
    ) -> Result<Self, OneharnessError> {
        let reconciled = reconcile_index(dir)?;
        let start = match after {
            Some(cursor) => reconciled
                .entries
                .iter()
                .position(|entry| entry.record.history_id == cursor)
                .map(|index| index + 1)
                .ok_or_else(|| OneharnessError::HistoryNotFound {
                    id: cursor.to_string(),
                })?,
            None => 0,
        };

        let mut watcher = Self {
            index_path: dir.join(INDEX_FILE),
            offset: reconciled.offset,
            pending: VecDeque::new(),
            seen: reconciled
                .entries
                .iter()
                .take(start)
                .map(|entry| entry.record.history_id)
                .collect(),
            labels,
            project_slug,
        };
        for entry in reconciled.entries.into_iter().skip(start) {
            if reconciled.active_ids.contains(&entry.record.history_id) {
                watcher.accept(entry);
            }
        }
        Ok(watcher)
    }

    /// Return all records currently available, preserving append order.
    pub fn drain_available(&mut self) -> Vec<HistoryRecord> {
        self.pending.drain(..).collect()
    }

    /// Read newly appended complete index lines. A concurrent partial write is
    /// retained at the current offset and retried only after its newline lands.
    pub fn poll(&mut self) -> Result<Vec<HistoryRecord>, OneharnessError> {
        let mut file = OpenOptions::new()
            .read(true)
            .open(&self.index_path)
            .map_err(|source| history_io_error(&self.index_path, source))?;
        file.seek(SeekFrom::Start(self.offset))
            .map_err(|source| history_io_error(&self.index_path, source))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|source| history_io_error(&self.index_path, source))?;
        let Some(last_newline) = bytes.iter().rposition(|byte| *byte == b'\n') else {
            return Ok(Vec::new());
        };
        let complete = &bytes[..=last_newline];
        self.offset += complete.len() as u64;
        for line in complete.split(|byte| *byte == b'\n') {
            if line.is_empty() {
                continue;
            }
            if let Ok(entry) = serde_json::from_slice::<HistoryIndexEntry>(line) {
                self.accept(entry);
            }
        }
        Ok(self.drain_available())
    }

    fn accept(&mut self, entry: HistoryIndexEntry) {
        let in_project = self.project_slug.as_ref().is_none_or(|slug| {
            Path::new(&entry.session_path)
                .components()
                .next()
                .is_some_and(|component| component.as_os_str() == slug.as_str())
        });
        if self.seen.insert(entry.record.history_id)
            && in_project
            && entry.record.labels.matches(&self.labels)
        {
            self.pending.push_back(entry.record);
        }
    }
}

/// A one-line summary of a session, for `oneharness history list`. Read from the
/// session's records: `name`/`project`/`started` come from the first record,
/// `harnesses` is the distinct set across all records.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[schemars(rename = "HistorySessionSummary")]
pub struct SessionSummary {
    /// The session id (the file stem), unique and sortable by start time.
    pub id: String,
    /// The human-meaningful session name (non-unique).
    pub name: String,
    /// Labels shared by every record in the session. Omitted when empty.
    #[serde(default, skip_serializing_if = "HistoryLabels::is_empty")]
    pub labels: HistoryLabels,
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

/// Read a session file into typed current records. Legacy v0.1 lines are
/// migrated deterministically; malformed or partial lines are skipped.
pub fn read_session(path: &Path) -> Result<Vec<HistoryRecord>, OneharnessError> {
    let text = fs::read_to_string(path).map_err(|source| OneharnessError::HistoryIo {
        path: path.display().to_string(),
        source,
    })?;
    Ok(parse_records(path, &text))
}

/// Find exactly one history record by its UUID, across all projects. A missing
/// id is a typed error so library callers need not parse diagnostics.
pub fn find_record_by_id(dir: &Path, id: HistoryId) -> Result<HistoryRecord, OneharnessError> {
    let reconciled = reconcile_index(dir)?;
    if reconciled.active_ids.contains(&id) {
        if let Some(entry) = reconciled
            .entries
            .into_iter()
            .find(|entry| entry.record.history_id == id)
        {
            return Ok(entry.record);
        }
    }
    Err(OneharnessError::HistoryNotFound { id: id.to_string() })
}

fn reconcile_index(dir: &Path) -> Result<ReconciledIndex, OneharnessError> {
    fs::create_dir_all(dir).map_err(|source| history_io_error(dir, source))?;
    let lock_path = dir.join(INDEX_LOCK_FILE);
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|source| history_io_error(&lock_path, source))?;
    FileExt::lock_exclusive(&lock).map_err(|source| history_io_error(&lock_path, source))?;
    let result = reconcile_index_locked(dir);
    let unlock = FileExt::unlock(&lock).map_err(|source| history_io_error(&lock_path, source));
    match (result, unlock) {
        (Err(error), _) | (Ok(_), Err(error)) => Err(error),
        (Ok(index), Ok(())) => Ok(index),
    }
}

fn reconcile_index_locked(dir: &Path) -> Result<ReconciledIndex, OneharnessError> {
    let index_path = dir.join(INDEX_FILE);
    let mut index = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&index_path)
        .map_err(|source| history_io_error(&index_path, source))?;
    recover_partial_tail(&mut index, &index_path)?;

    index
        .seek(SeekFrom::Start(0))
        .map_err(|source| history_io_error(&index_path, source))?;
    let mut bytes = Vec::new();
    index
        .read_to_end(&mut bytes)
        .map_err(|source| history_io_error(&index_path, source))?;
    let mut entries = parse_index_entries(&bytes);
    let mut indexed_ids: BTreeSet<HistoryId> = entries
        .iter()
        .map(|entry| entry.record.history_id)
        .collect();
    let mut active_ids = BTreeSet::new();
    let mut missing = Vec::new();

    for project_dir in read_subdirs_if_present(dir)? {
        for path in read_session_files(&project_dir)? {
            let relative_path = path
                .strip_prefix(dir)
                .unwrap_or(&path)
                .display()
                .to_string();
            for record in read_session(&path)? {
                active_ids.insert(record.history_id);
                if indexed_ids.insert(record.history_id) {
                    missing.push(HistoryIndexEntry {
                        session_path: relative_path.clone(),
                        record,
                    });
                }
            }
        }
    }

    index
        .seek(SeekFrom::End(0))
        .map_err(|source| history_io_error(&index_path, source))?;
    for entry in &missing {
        write_index_line(&mut index, &index_path, entry)?;
    }
    index
        .flush()
        .map_err(|source| history_io_error(&index_path, source))?;
    entries.extend(missing);
    let offset = index
        .stream_position()
        .map_err(|source| history_io_error(&index_path, source))?;
    Ok(ReconciledIndex {
        entries,
        active_ids,
        offset,
    })
}

fn append_index_entry(dir: &Path, entry: &HistoryIndexEntry) -> std::io::Result<()> {
    let lock_path = dir.join(INDEX_LOCK_FILE);
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)?;
    FileExt::lock_exclusive(&lock)?;
    let result = (|| {
        let index_path = dir.join(INDEX_FILE);
        let mut index = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(index_path)?;
        recover_partial_tail_io(&mut index)?;
        index.seek(SeekFrom::Start(0))?;
        let mut existing = Vec::new();
        index.read_to_end(&mut existing)?;
        if parse_index_entries(&existing)
            .iter()
            .any(|indexed| indexed.record.history_id == entry.record.history_id)
        {
            return Ok(());
        }
        index.seek(SeekFrom::End(0))?;
        let mut line = serde_json::to_vec(entry)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        line.push(b'\n');
        index.write_all(&line)?;
        index.flush()
    })();
    let unlock = FileExt::unlock(&lock);
    result.and(unlock)
}

fn recover_partial_tail(file: &mut File, path: &Path) -> Result<(), OneharnessError> {
    recover_partial_tail_io(file).map_err(|source| history_io_error(path, source))
}

fn recover_partial_tail_io(file: &mut File) -> std::io::Result<()> {
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    if bytes.last().is_some_and(|byte| *byte != b'\n') {
        let valid_len = bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |index| index + 1);
        file.set_len(valid_len as u64)?;
    }
    Ok(())
}

fn parse_index_entries(bytes: &[u8]) -> Vec<HistoryIndexEntry> {
    let mut seen = BTreeSet::new();
    bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .filter_map(|line| serde_json::from_slice::<HistoryIndexEntry>(line).ok())
        .filter(|entry| seen.insert(entry.record.history_id))
        .collect()
}

fn write_index_line(
    index: &mut File,
    path: &Path,
    entry: &HistoryIndexEntry,
) -> Result<(), OneharnessError> {
    let mut line = serde_json::to_vec(entry).map_err(OneharnessError::Serialize)?;
    line.push(b'\n');
    index
        .write_all(&line)
        .map_err(|source| history_io_error(path, source))
}

fn history_io_error(path: &Path, source: std::io::Error) -> OneharnessError {
    OneharnessError::HistoryIo {
        path: path.display().to_string(),
        source,
    }
}

fn history_error_to_io(error: OneharnessError) -> std::io::Error {
    std::io::Error::other(error)
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

fn read_subdirs_if_present(dir: &Path) -> Result<Vec<PathBuf>, OneharnessError> {
    if dir.exists() {
        read_subdirs(dir)
    } else {
        Ok(Vec::new())
    }
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
    let records = read_values(path)?;

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
        labels: records
            .first()
            .and_then(|record| record.get("labels").cloned())
            .and_then(|labels| serde_json::from_value(labels).ok())
            .unwrap_or_default(),
        project: field("project").unwrap_or(slug),
        started: field("timestamp").unwrap_or_default(),
        record_count: records.len(),
        harnesses,
        path: path.display().to_string(),
        id,
    })
}

fn read_values(path: &Path) -> Result<Vec<Value>, OneharnessError> {
    let text = fs::read_to_string(path).map_err(|source| OneharnessError::HistoryIo {
        path: path.display().to_string(),
        source,
    })?;
    Ok(parse_values(&text))
}

/// Parse a JSONL blob into values, skipping malformed or partial lines.
fn parse_values(text: &str) -> Vec<Value> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn parse_records(path: &Path, text: &str) -> Vec<HistoryRecord> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .filter_map(|(index, line)| {
            let value = serde_json::from_str(line).ok()?;
            let identity = format!(
                "{}:{}",
                path.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default(),
                index + 1
            );
            HistoryRecord::from_value_with_legacy_identity(value, Some(&identity)).ok()
        })
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
            model: None,
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
        let project = dir.join("My Proj");
        fs::create_dir_all(&project).unwrap();
        let canonical = fs::canonicalize(&project).unwrap();
        let w = HistoryWriter::open(&dir, &project, "Fix Bug!", HistoryLabels::default()).unwrap();
        // Project slug subdir exists.
        assert!(dir
            .join(history::project_slug(&canonical.display().to_string()))
            .is_dir());
        // Session id: sanitized name, compact UTC, pid.
        let stem = w.path().file_stem().unwrap().to_str().unwrap();
        assert!(stem.starts_with("fix-bug-"), "{stem}");
        assert!(w.path().extension().unwrap() == "jsonl");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn append_writes_parseable_jsonl_lines() {
        let dir = temp_dir("append");
        let project = dir.join("project-a");
        fs::create_dir_all(&project).unwrap();
        let w = HistoryWriter::open(
            &dir,
            &project,
            "my-session",
            history::parse_labels(["graph=deploy"]).unwrap(),
        )
        .unwrap();
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
        assert_eq!(records[0].harness, "claude-code");
        assert_eq!(records[0].name, "my-session");
        assert_eq!(
            records[0].project,
            fs::canonicalize(&project).unwrap().display().to_string()
        );
        assert_eq!(records[0].model.as_deref(), Some("sonnet"));
        assert_eq!(records[0].labels.as_map().get("graph").unwrap(), "deploy");
        assert_eq!(records[1].harness, "codex");
        assert_eq!(records[0].history_id.as_uuid().get_version_num(), 7);
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
                labels: HistoryLabels::default(),
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
        let project = dir.join("project-x");
        fs::create_dir_all(&project).unwrap();
        let w = HistoryWriter::open(&dir, &project, "s", HistoryLabels::default()).unwrap();
        w.append(PermissionMode::Default, None, "p", &result("codex"))
            .unwrap();
        assert_eq!(list_sessions(&dir, None).unwrap().len(), 1);
        let removed = remove_sessions(&dir, None).unwrap();
        assert_eq!(removed.len(), 1);
        assert!(list_sessions(&dir, None).unwrap().is_empty());
        // The now-empty project subdir was pruned.
        assert!(list_sessions(&dir, None).unwrap().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_values_skips_blank_and_bad_lines() {
        let text = "{\"a\":1}\n\n  \nnot json\n{\"b\":2}\n";
        let vals = parse_values(text);
        assert_eq!(vals.len(), 2);
        assert_eq!(vals[0]["a"], 1);
        assert_eq!(vals[1]["b"], 2);
    }

    #[test]
    fn concurrent_index_appends_are_complete_and_unique() {
        let dir = temp_dir("concurrent-index");
        let project = temp_dir("concurrent-project");
        let mut threads = Vec::new();
        for index in 0..12 {
            let dir = dir.clone();
            let project = project.clone();
            threads.push(std::thread::spawn(move || {
                let writer = HistoryWriter::open(
                    &dir,
                    &project,
                    &format!("session-{index}"),
                    HistoryLabels::default(),
                )
                .unwrap();
                writer
                    .append(
                        PermissionMode::Default,
                        None,
                        &format!("prompt-{index}"),
                        &result("codex"),
                    )
                    .unwrap();
            }));
        }
        for thread in threads {
            thread.join().unwrap();
        }

        let bytes = fs::read(dir.join(INDEX_FILE)).unwrap();
        assert!(bytes.ends_with(b"\n"));
        let entries = parse_index_entries(&bytes);
        assert_eq!(entries.len(), 12);
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.record.history_id)
                .collect::<BTreeSet<_>>()
                .len(),
            12
        );
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&project);
    }

    #[test]
    fn watcher_resumes_after_cursor_without_duplication_and_filters_labels() {
        let dir = temp_dir("watch-resume");
        let project = temp_dir("watch-project");
        let writer = HistoryWriter::open(
            &dir,
            &project,
            "watched",
            history::parse_labels(["graph=release", "task=test"]).unwrap(),
        )
        .unwrap();
        writer
            .append(PermissionMode::Default, None, "first", &result("codex"))
            .unwrap();
        let first_id = read_session(writer.path()).unwrap()[0].history_id;
        let mut watcher = HistoryWatcher::open(
            &dir,
            Some(first_id),
            history::parse_labels(["graph=release"]).unwrap(),
            None,
        )
        .unwrap();
        assert!(watcher.drain_available().is_empty());

        writer
            .append(PermissionMode::Default, None, "second", &result("codex"))
            .unwrap();
        let records = watcher.poll().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].prompt, "second");
        assert_ne!(records[0].history_id, first_id);
        assert!(watcher.poll().unwrap().is_empty());

        let mut no_match = HistoryWatcher::open(
            &dir,
            None,
            history::parse_labels(["graph=other"]).unwrap(),
            None,
        )
        .unwrap();
        assert!(no_match.drain_available().is_empty());
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&project);
    }

    #[test]
    fn reconciliation_recovers_a_partial_final_index_line() {
        let dir = temp_dir("partial-index");
        let project = temp_dir("partial-project");
        let writer =
            HistoryWriter::open(&dir, &project, "partial", HistoryLabels::default()).unwrap();
        writer
            .append(PermissionMode::Default, None, "prompt", &result("codex"))
            .unwrap();
        let index_path = dir.join(INDEX_FILE);
        let len = fs::metadata(&index_path).unwrap().len();
        OpenOptions::new()
            .write(true)
            .open(&index_path)
            .unwrap()
            .set_len(len / 2)
            .unwrap();

        let mut watcher = HistoryWatcher::open(&dir, None, HistoryLabels::default(), None).unwrap();
        let recovered = watcher.drain_available();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].prompt, "prompt");
        let repaired = fs::read(&index_path).unwrap();
        assert!(repaired.ends_with(b"\n"));
        assert_eq!(parse_index_entries(&repaired).len(), 1);
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&project);
    }

    #[test]
    fn exact_id_lookup_has_a_typed_not_found_error() {
        let dir = temp_dir("exact-id");
        let missing = HistoryId::from_uuid(uuid::Uuid::now_v7());
        let error = find_record_by_id(&dir, missing).unwrap_err();
        assert!(matches!(
            error,
            OneharnessError::HistoryNotFound { id } if id == missing.to_string()
        ));
        let _ = fs::remove_dir_all(&dir);
    }
}
