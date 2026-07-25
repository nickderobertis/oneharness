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

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use fs2::FileExt;

use crate::domain::history::{
    self, HistoryEventLine, HistoryId, HistoryLabels, HistoryLine, HistoryRecord, HistoryRunRecord,
};
use crate::domain::mode::PermissionMode;
use crate::domain::report::RunResult;
use crate::errors::OneharnessError;

/// The file extension for every session log (line-delimited JSON).
const SESSION_EXT: &str = "jsonl";
const INDEX_FILE: &str = ".index.jsonl";
const INDEX_LOCK_FILE: &str = ".index.lock";
const EVENT_INDEX_FILE: &str = ".event-index.jsonl";

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

#[derive(Debug)]
pub struct EventAppendOutcome {
    pub index_error: Option<std::io::Error>,
}

impl HistoryWriter {
    /// Mint the id shared by a live run's incremental event lines and closing run line.
    pub fn begin_run(&self) -> HistoryId {
        HistoryId::from_uuid(uuid::Uuid::now_v7())
    }

    /// Durably append one live event and make it visible to event-mode watchers.
    pub fn append_event(
        &self,
        run_id: HistoryId,
        harness: &str,
        event: crate::domain::events::ActionEvent,
    ) -> std::io::Result<()> {
        match self
            .append_event_tracked(run_id, harness, event)?
            .index_error
        {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Append an event while distinguishing session durability from a later
    /// best-effort event-index failure.
    pub fn append_event_tracked(
        &self,
        run_id: HistoryId,
        harness: &str,
        event: crate::domain::events::ActionEvent,
    ) -> std::io::Result<EventAppendOutcome> {
        let (base, variant) = harness
            .split_once(':')
            .map_or((harness, None), |(base, variant)| (base, Some(variant)));
        let line = HistoryEventLine {
            schema_version: history::SCHEMA_VERSION.to_string(),
            run_id,
            harness: base.to_string(),
            variant: variant.map(str::to_string),
            harness_id: Some(harness.to_string()),
            event,
        };
        let mut file = open_session_for_append(&self.path)?;
        write_session_line(&mut file, &HistoryLine::Event(line.clone()))?;
        let index_error = append_event_index_entry(
            &self.dir,
            &HistoryEventIndexEntry {
                session_path: self.relative_path.clone(),
                labels: self.labels.clone(),
                line,
            },
        )
        .err();
        Ok(EventAppendOutcome { index_error })
    }

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

    /// Append one harness result as ordered event lines followed by its terminal
    /// run line, stamped with the current instant. Creates the file on first write.
    pub fn append(
        &self,
        mode: PermissionMode,
        model: Option<&str>,
        run_prompt: &str,
        result: &RunResult,
    ) -> std::io::Result<()> {
        self.append_with_id(self.begin_run(), mode, model, run_prompt, result, None)
    }

    /// Append the terminal line for a run whose events were already persisted live.
    pub fn append_streamed(
        &self,
        run_id: HistoryId,
        mode: PermissionMode,
        model: Option<&str>,
        run_prompt: &str,
        result: &RunResult,
        persisted_event_indexes: &std::collections::BTreeSet<usize>,
    ) -> std::io::Result<()> {
        self.append_with_id(
            run_id,
            mode,
            model,
            run_prompt,
            result,
            Some(persisted_event_indexes),
        )
    }

    fn append_with_id(
        &self,
        run_id: HistoryId,
        mode: PermissionMode,
        model: Option<&str>,
        run_prompt: &str,
        result: &RunResult,
        persisted_event_indexes: Option<&std::collections::BTreeSet<usize>>,
    ) -> std::io::Result<()> {
        let record = HistoryRecord::from_result(
            run_id,
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
        let run = HistoryRunRecord::from_record(&record);
        if !record.complete() || !run.valid() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "new history run lacks complete v1.0 telemetry",
            ));
        }
        let mut lines = result.events.clone().unwrap_or_default();
        if let Some(persisted) = persisted_event_indexes {
            lines.retain(|event| !persisted.contains(&event.index));
        }
        lines.sort_by_key(|event| event.index);
        let mut file = open_session_for_append(&self.path)?;
        for event in lines {
            write_session_line(
                &mut file,
                &HistoryLine::Event(HistoryEventLine {
                    schema_version: history::SCHEMA_VERSION.to_string(),
                    run_id: run.history_id,
                    harness: run.harness.clone(),
                    variant: run.variant.clone(),
                    harness_id: run.harness_id.clone(),
                    event,
                }),
            )?;
        }
        write_session_line(&mut file, &HistoryLine::Run(run))?;
        append_index_entry(
            &self.dir,
            &HistoryIndexEntry {
                session_path: self.relative_path.clone(),
                record: HistoryRunRecord::from_record(&record),
            },
        )
    }
}

fn write_session_line(file: &mut File, line: &HistoryLine) -> std::io::Result<()> {
    let mut bytes = serde_json::to_vec(line)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    bytes.push(b'\n');
    file.write_all(&bytes)?;
    file.flush()
}

fn open_session_for_append(path: &Path) -> std::io::Result<File> {
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?;
    recover_partial_tail_io(&mut file)?;
    file.seek(SeekFrom::End(0))?;
    Ok(file)
}

/// One append-only index entry. The session JSONL remains authoritative; the
/// relative path lets reconciliation suppress entries whose session was cleared.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct HistoryIndexEntry {
    session_path: String,
    record: HistoryRunRecord,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct HistoryEventIndexEntry {
    session_path: String,
    labels: HistoryLabels,
    line: HistoryEventLine,
}

/// Outcome for one session file processed by [`migrate`]. Counts refer to
/// whole legacy records and current v1.0 lines; unreadable lines are preserved
/// byte-for-byte and reported as skipped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MigrationSummary {
    pub path: String,
    pub records_migrated: usize,
    pub skipped: usize,
    pub already_current: usize,
}

/// Rewrite every legacy session in a history store to v1.0 and rebuild its
/// index. Each session and the index are replaced from a fully flushed sibling
/// temp file, so a failed conversion never leaves a partially written target.
pub fn migrate(dir: &Path) -> Result<Vec<MigrationSummary>, OneharnessError> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let lock_path = dir.join(INDEX_LOCK_FILE);
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|source| history_io_error(&lock_path, source))?;
    FileExt::lock_exclusive(&lock).map_err(|source| history_io_error(&lock_path, source))?;
    let result = migrate_locked(dir);
    let unlock = FileExt::unlock(&lock).map_err(|source| history_io_error(&lock_path, source));
    match (result, unlock) {
        (Err(error), _) | (Ok(_), Err(error)) => Err(error),
        (Ok(summaries), Ok(())) => Ok(summaries),
    }
}

fn migrate_locked(dir: &Path) -> Result<Vec<MigrationSummary>, OneharnessError> {
    let mut summaries = Vec::new();
    for project_dir in read_subdirs_if_present(dir)? {
        for path in read_session_files(&project_dir)? {
            summaries.push(migrate_file(dir, &path)?);
        }
    }
    summaries.sort_by(|a, b| a.path.cmp(&b.path));
    rebuild_index_locked(dir)?;
    Ok(summaries)
}

fn migrate_file(dir: &Path, path: &Path) -> Result<MigrationSummary, OneharnessError> {
    let text = fs::read_to_string(path).map_err(|source| history_io_error(path, source))?;
    let relative = path.strip_prefix(dir).unwrap_or(path).display().to_string();
    let mut output = Vec::new();
    let mut summary = MigrationSummary {
        path: path.display().to_string(),
        records_migrated: 0,
        skipped: 0,
        already_current: 0,
    };
    for (index, raw) in text.lines().enumerate() {
        if raw.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(raw) else {
            summary.skipped += 1;
            output.extend_from_slice(raw.as_bytes());
            output.push(b'\n');
            continue;
        };
        if serde_json::from_value::<HistoryLine>(value.clone()).is_ok() {
            summary.already_current += 1;
            output.extend_from_slice(raw.as_bytes());
            output.push(b'\n');
            continue;
        }
        let identity = format!("{}:{}", relative, index + 1);
        let Ok(record) = HistoryRecord::from_legacy_value(value, &identity) else {
            summary.skipped += 1;
            output.extend_from_slice(raw.as_bytes());
            output.push(b'\n');
            continue;
        };
        if let Some(events) = &record.events {
            for event in events {
                append_json_line(
                    &mut output,
                    &HistoryLine::Event(HistoryEventLine {
                        schema_version: history::SCHEMA_VERSION.to_string(),
                        run_id: record.history_id,
                        harness: record.harness.clone(),
                        variant: record.variant.clone(),
                        harness_id: Some(record.harness_id.clone()),
                        event: event.clone(),
                    }),
                )?;
            }
        }
        append_json_line(
            &mut output,
            &HistoryLine::Run(HistoryRunRecord::from_record(&record)),
        )?;
        summary.records_migrated += 1;
    }
    atomic_write(path, &output)?;
    Ok(summary)
}

fn append_json_line<T: Serialize>(output: &mut Vec<u8>, value: &T) -> Result<(), OneharnessError> {
    serde_json::to_writer(&mut *output, value).map_err(OneharnessError::Serialize)?;
    output.push(b'\n');
    Ok(())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), OneharnessError> {
    let tmp = path.with_extension(format!("jsonl.{}.oneharness.tmp", std::process::id()));
    let result = (|| {
        let mut file = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result.map_err(|source| history_io_error(path, source))
}

fn rebuild_index_locked(dir: &Path) -> Result<(), OneharnessError> {
    let mut bytes = Vec::new();
    for project_dir in read_subdirs_if_present(dir)? {
        for path in read_session_files(&project_dir)? {
            let session_path = path
                .strip_prefix(dir)
                .unwrap_or(&path)
                .display()
                .to_string();
            for record in read_run_lines(&path)? {
                append_json_line(
                    &mut bytes,
                    &HistoryIndexEntry {
                        session_path: session_path.clone(),
                        record,
                    },
                )?;
            }
        }
    }
    atomic_write(&dir.join(INDEX_FILE), &bytes)
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
    event_index_path: Option<PathBuf>,
    event_offset: u64,
    pending_events: VecDeque<HistoryEventLine>,
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
        events: bool,
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
            event_index_path: events.then(|| dir.join(EVENT_INDEX_FILE)),
            event_offset: 0,
            pending_events: VecDeque::new(),
            seen: reconciled
                .entries
                .iter()
                .take(start)
                .map(|entry| entry.record.history_id)
                .collect(),
            labels,
            project_slug,
        };
        if events {
            let (event_entries, event_offset) = reconcile_event_index(dir)?;
            watcher.event_offset = event_offset;
            for entry in event_entries {
                watcher.accept_event(entry);
            }
        }
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

    pub fn drain_events(&mut self) -> Vec<HistoryEventLine> {
        self.pending_events.drain(..).collect()
    }

    /// Read newly appended complete index lines. A concurrent partial write is
    /// retained at the current offset and retried only after its newline lands.
    pub fn poll(&mut self) -> Result<Vec<HistoryRecord>, OneharnessError> {
        self.poll_events()?;
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

    fn poll_events(&mut self) -> Result<(), OneharnessError> {
        let Some(path) = self.event_index_path.clone() else {
            return Ok(());
        };
        let mut file = OpenOptions::new()
            .read(true)
            .open(&path)
            .map_err(|source| history_io_error(&path, source))?;
        file.seek(SeekFrom::Start(self.event_offset))
            .map_err(|source| history_io_error(&path, source))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|source| history_io_error(&path, source))?;
        let Some(last_newline) = bytes.iter().rposition(|byte| *byte == b'\n') else {
            return Ok(());
        };
        let complete = &bytes[..=last_newline];
        self.event_offset += complete.len() as u64;
        for line in complete
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
        {
            if let Ok(entry) = serde_json::from_slice::<HistoryEventIndexEntry>(line) {
                self.accept_event(entry);
            }
        }
        Ok(())
    }

    fn accept_event(&mut self, entry: HistoryEventIndexEntry) {
        let in_project = self.project_slug.as_ref().is_none_or(|slug| {
            Path::new(&entry.session_path)
                .components()
                .next()
                .is_some_and(|component| component.as_os_str() == slug.as_str())
        });
        if in_project && entry.labels.matches(&self.labels) {
            self.pending_events.push_back(entry.line);
        }
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
            self.pending.push_back(entry.record.materialize(Vec::new()));
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
            let summary = summarize(&path)?;
            if summary.record_count > 0 {
                sessions.push(summary);
            }
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

/// Read a session file and materialize each completed run with its ordered events.
/// Malformed, partial, and legacy lines are skipped.
pub fn read_session(path: &Path) -> Result<Vec<HistoryRecord>, OneharnessError> {
    let text = fs::read_to_string(path).map_err(|source| OneharnessError::HistoryIo {
        path: path.display().to_string(),
        source,
    })?;
    Ok(parse_records(path, &text))
}

/// Read the display view, including event-only runs whose terminal line has not
/// landed. Completed entries retain the established materialized record shape.
pub fn read_session_display(path: &Path) -> Result<Vec<Value>, OneharnessError> {
    let text = fs::read_to_string(path).map_err(|source| OneharnessError::HistoryIo {
        path: path.display().to_string(),
        source,
    })?;
    let mut dangling: BTreeMap<HistoryId, (String, Vec<_>)> = BTreeMap::new();
    let mut values = Vec::new();
    for line in parse_lines(path, &text) {
        match line {
            HistoryLine::Event(line) => {
                dangling
                    .entry(line.run_id)
                    .or_insert_with(|| (line.harness, Vec::new()))
                    .1
                    .push(line.event);
            }
            HistoryLine::Run(run) => {
                let events = dangling
                    .remove(&run.history_id)
                    .map(|(_, events)| events)
                    .unwrap_or_default();
                values.push(serde_json::to_value(run.materialize(events))?);
            }
        }
    }
    for (run_id, (harness, mut events)) in dangling {
        events.sort_by_key(|event| event.index);
        values.push(serde_json::json!({
            "type": "incomplete",
            "run_id": run_id,
            "harness": harness,
            "events": events,
        }));
    }
    Ok(values)
}

/// Resolve an event-only session by its file id without making it visible to
/// `history list` before a closing run line exists.
pub fn find_session_path(
    dir: &Path,
    project_slug: Option<&str>,
    id: &str,
) -> Result<Option<PathBuf>, OneharnessError> {
    let project_dirs = match project_slug {
        Some(slug) => vec![dir.join(slug)],
        None => read_subdirs_if_present(dir)?,
    };
    for project_dir in project_dirs {
        if !project_dir.is_dir() {
            continue;
        }
        if let Some(path) = read_session_files(&project_dir)?
            .into_iter()
            .find(|path| path.file_stem().and_then(|stem| stem.to_str()) == Some(id))
        {
            return Ok(Some(path));
        }
    }
    Ok(None)
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
            let path = dir.join(&entry.session_path);
            if let Some(record) = read_session(&path)?
                .into_iter()
                .find(|record| record.history_id == id)
            {
                return Ok(record);
            }
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
            for record in read_run_lines(&path)? {
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

fn append_event_index_entry(dir: &Path, entry: &HistoryEventIndexEntry) -> std::io::Result<()> {
    let lock_path = dir.join(INDEX_LOCK_FILE);
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)?;
    FileExt::lock_exclusive(&lock)?;
    let result = (|| {
        let path = dir.join(EVENT_INDEX_FILE);
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)?;
        recover_partial_tail_io(&mut file)?;
        file.seek(SeekFrom::End(0))?;
        let mut bytes = serde_json::to_vec(entry)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        bytes.push(b'\n');
        file.write_all(&bytes)?;
        file.flush()
    })();
    let unlock = FileExt::unlock(&lock);
    result.and(unlock)
}

fn reconcile_event_index(
    dir: &Path,
) -> Result<(Vec<HistoryEventIndexEntry>, u64), OneharnessError> {
    let lock_path = dir.join(INDEX_LOCK_FILE);
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|source| history_io_error(&lock_path, source))?;
    FileExt::lock_exclusive(&lock).map_err(|source| history_io_error(&lock_path, source))?;
    let result = (|| {
        let path = dir.join(EVENT_INDEX_FILE);
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|source| history_io_error(&path, source))?;
        recover_partial_tail(&mut file, &path)?;
        file.seek(SeekFrom::Start(0))
            .map_err(|source| history_io_error(&path, source))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|source| history_io_error(&path, source))?;
        let entries = bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .filter_map(|line| serde_json::from_slice(line).ok())
            .filter(|entry: &HistoryEventIndexEntry| dir.join(&entry.session_path).is_file())
            .collect();
        let offset = file
            .stream_position()
            .map_err(|source| history_io_error(&path, source))?;
        Ok((entries, offset))
    })();
    let unlock = FileExt::unlock(&lock).map_err(|source| history_io_error(&lock_path, source));
    match (result, unlock) {
        (Err(error), _) | (Ok(_), Err(error)) => Err(error),
        (Ok(entries), Ok(())) => Ok(entries),
    }
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
    let records = read_run_lines(path)?;

    let field = |key: &str| -> Option<String> {
        records.first().map(|record| match key {
            "name" => record.name.clone(),
            "project" => record.project.clone(),
            "timestamp" => record.timestamp.clone(),
            _ => String::new(),
        })
    };
    let mut harnesses: Vec<String> = Vec::new();
    for r in &records {
        let harness_id = r.harness_id.as_ref().unwrap_or(&r.harness);
        if !harnesses.iter().any(|x| x == harness_id) {
            harnesses.push(harness_id.clone());
        }
    }
    Ok(SessionSummary {
        name: field("name").unwrap_or_else(|| id.clone()),
        labels: records
            .first()
            .map(|record| record.labels.clone())
            .unwrap_or_default(),
        project: field("project").unwrap_or(slug),
        started: field("timestamp").unwrap_or_default(),
        record_count: records.len(),
        harnesses,
        path: path.display().to_string(),
        id,
    })
}

fn read_run_lines(path: &Path) -> Result<Vec<HistoryRunRecord>, OneharnessError> {
    let text = fs::read_to_string(path).map_err(|source| OneharnessError::HistoryIo {
        path: path.display().to_string(),
        source,
    })?;
    Ok(parse_lines(path, &text)
        .into_iter()
        .filter_map(|line| match line {
            HistoryLine::Run(run) => Some(run),
            HistoryLine::Event(_) => None,
        })
        .collect())
}

/// Parse a JSONL blob into values, skipping malformed or partial lines.
#[cfg(test)]
fn parse_values(text: &str) -> Vec<Value> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn parse_records(path: &Path, text: &str) -> Vec<HistoryRecord> {
    let mut events: BTreeMap<HistoryId, Vec<_>> = BTreeMap::new();
    let mut records = Vec::new();
    for line in parse_lines(path, text) {
        match line {
            HistoryLine::Event(line) => events.entry(line.run_id).or_default().push(line.event),
            HistoryLine::Run(run) => {
                let mut run_events = events.remove(&run.history_id).unwrap_or_default();
                run_events.sort_by_key(|event| event.index);
                records.push(run.materialize(run_events));
            }
        }
    }
    records
}

fn parse_lines(path: &Path, text: &str) -> Vec<HistoryLine> {
    let mut legacy = false;
    let lines = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let value: Value = serde_json::from_str(line).ok()?;
            let line_type = value.get("type").and_then(Value::as_str);
            let version = value.get("schema_version").and_then(Value::as_str);
            if line_type.is_none()
                || version.is_some_and(|version| version < history::SCHEMA_VERSION)
            {
                legacy = true;
                return None;
            }
            serde_json::from_value(value).ok()
        })
        .collect();
    if legacy {
        eprintln!(
            "oneharness: warning: skipped unmigrated history lines in `{}`; run `oneharness history migrate`",
            path.display()
        );
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::report::{ExecutionTelemetry, OutputFormat, Status};
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
            variant: None,
            harness_id: harness.to_string(),
            bin: "bin".to_string(),
            available: true,
            status: Status::Ok,
            prompt: None,
            model: None,
            exit_code: Some(0),
            duration_ms: Some(10),
            telemetry: Some(ExecutionTelemetry {
                started_at: "2026-07-19T00:00:00Z".to_string(),
                finished_at: Some("2026-07-19T00:00:00Z".to_string()),
                model_ms: Some(7),
                tool_ms: Some(0),
                time_to_first_token_ms: None,
            }),
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
        let mut first = result("claude-code");
        first.events = Some(vec![crate::domain::events::ActionEvent {
            kind: "message".to_string(),
            name: None,
            input: None,
            output: Some("observed".to_string()),
            index: 0,
            tool_call_id: None,
            started_at: None,
            finished_at: None,
            duration_ms: None,
            status: None,
        }]);
        w.append(PermissionMode::Bypass, Some("sonnet"), "do it", &first)
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
        assert_eq!(
            records[0].events.as_ref().unwrap()[0].output.as_deref(),
            Some("observed")
        );
        assert_eq!(records[1].harness, "codex");
        assert_eq!(records[0].history_id.as_uuid().get_version_num(), 7);
        // A minted id must be text the public cursor contract accepts back, or
        // `history watch --after` could not resume from the record it just wrote.
        assert_eq!(
            records[0]
                .history_id
                .to_string()
                .parse::<HistoryId>()
                .unwrap(),
            records[0].history_id
        );
        let lines = parse_lines(w.path(), &fs::read_to_string(w.path()).unwrap());
        assert!(matches!(lines[0], HistoryLine::Event(_)));
        assert!(matches!(lines[1], HistoryLine::Run(_)));
        assert!(matches!(lines[2], HistoryLine::Run(_)));
        let index = fs::read(dir.join(INDEX_FILE)).unwrap();
        assert_eq!(parse_index_entries(&index).len(), 2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn failed_live_session_append_is_retried_at_finalization() {
        let dir = temp_dir("live-fallback");
        let project = temp_dir("live-fallback-project");
        let writer = HistoryWriter::open(&dir, &project, "live", HistoryLabels::default()).unwrap();
        let run_id = writer.begin_run();
        let event = crate::domain::events::ActionEvent {
            kind: "message".to_string(),
            name: None,
            input: None,
            output: Some("late".to_string()),
            index: 0,
            tool_call_id: None,
            started_at: None,
            finished_at: None,
            duration_ms: None,
            status: None,
        };
        fs::create_dir(writer.path()).unwrap();
        let error = writer
            .append_event_tracked(run_id, "codex", event.clone())
            .unwrap_err();
        assert_ne!(error.kind(), std::io::ErrorKind::NotFound);
        fs::remove_dir(writer.path()).unwrap();
        let mut completed = result("codex");
        completed.events = Some(vec![event]);
        writer
            .append_streamed(
                run_id,
                PermissionMode::Default,
                None,
                "prompt",
                &completed,
                &BTreeSet::new(),
            )
            .unwrap();
        assert_eq!(
            read_session(writer.path()).unwrap()[0]
                .events
                .as_ref()
                .unwrap()
                .len(),
            1
        );
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&project);
    }

    #[test]
    fn event_index_failure_still_reports_the_session_line_as_persisted() {
        let dir = temp_dir("live-index-failure");
        let project = temp_dir("live-index-project");
        let writer = HistoryWriter::open(&dir, &project, "live", HistoryLabels::default()).unwrap();
        fs::create_dir(dir.join(EVENT_INDEX_FILE)).unwrap();
        let event = crate::domain::events::ActionEvent {
            kind: "message".to_string(),
            name: None,
            input: None,
            output: None,
            index: 0,
            tool_call_id: None,
            started_at: None,
            finished_at: None,
            duration_ms: None,
            status: None,
        };
        let outcome = writer
            .append_event_tracked(writer.begin_run(), "codex", event.clone())
            .unwrap();
        assert!(outcome.index_error.is_some());
        assert!(writer
            .append_event(writer.begin_run(), "codex", event.clone())
            .is_err());
        fs::remove_dir(dir.join(EVENT_INDEX_FILE)).unwrap();
        writer
            .append_event(writer.begin_run(), "codex", event)
            .unwrap();
        assert_eq!(
            parse_lines(writer.path(), &fs::read_to_string(writer.path()).unwrap()).len(),
            3
        );
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&project);
    }

    #[test]
    fn list_sessions_summarizes_and_orders_newest_first() {
        let dir = temp_dir("list");
        fs::create_dir_all(dir.join("proj-a")).unwrap();
        fs::create_dir_all(dir.join("proj-b")).unwrap();
        let line = |name: &str, project: &str, timestamp: &str, harness: &str| {
            let record = HistoryRecord::from_result(
                HistoryId::from_uuid(uuid::Uuid::now_v7()),
                name,
                name,
                &HistoryLabels::default(),
                project,
                timestamp.to_string(),
                PermissionMode::Default,
                None,
                "prompt",
                &result(harness),
            );
            format!(
                "{}\n",
                serde_json::to_string(&HistoryLine::Run(HistoryRunRecord::from_record(&record)))
                    .unwrap()
            )
        };
        fs::write(
            dir.join("proj-a").join("old-20240101T000000Z-1.jsonl"),
            line("old", "/proj/a", "2024-01-01T00:00:00Z", "codex"),
        )
        .unwrap();
        fs::write(
            dir.join("proj-b").join("new-20260101T000000Z-2.jsonl"),
            format!(
                "{}{}",
                line("new", "/proj/b", "2026-01-01T00:00:00Z", "claude-code"),
                line("new", "/proj/b", "2026-01-01T00:00:01Z", "codex")
            ),
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
    fn list_ignores_an_empty_session_file() {
        let dir = temp_dir("emptyfile");
        fs::create_dir_all(dir.join("some-proj")).unwrap();
        fs::write(dir.join("some-proj").join("stub-1.jsonl"), "\n").unwrap();
        let sessions = list_sessions(&dir, None).unwrap();
        assert!(sessions.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn dangling_events_are_displayed_as_incomplete_but_not_listed() {
        let dir = temp_dir("dangling");
        let project_dir = dir.join("project");
        fs::create_dir_all(&project_dir).unwrap();
        let path = project_dir.join("interrupted.jsonl");
        let run_id = HistoryId::from_uuid(uuid::Uuid::now_v7());
        let line = HistoryLine::Event(HistoryEventLine {
            schema_version: history::SCHEMA_VERSION.to_string(),
            run_id,
            harness: "codex".to_string(),
            variant: None,
            harness_id: Some("codex".to_string()),
            event: crate::domain::events::ActionEvent {
                kind: "message".to_string(),
                name: None,
                input: None,
                output: Some("partial".to_string()),
                index: 0,
                tool_call_id: None,
                started_at: None,
                finished_at: None,
                duration_ms: None,
                status: None,
            },
        });
        fs::write(
            &path,
            format!("{}\n", serde_json::to_string(&line).unwrap()),
        )
        .unwrap();

        assert!(list_sessions(&dir, None).unwrap().is_empty());
        let displayed = read_session_display(&path).unwrap();
        assert_eq!(displayed[0]["type"], "incomplete");
        assert_eq!(displayed[0]["run_id"], run_id.to_string());
        assert_eq!(displayed[0]["events"][0]["output"], "partial");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_lines_are_skipped_without_panicking() {
        let dir = temp_dir("legacy");
        let path = dir.join("legacy.jsonl");
        fs::write(
            &path,
            "{\"schema_version\":\"0.3\",\"history_id\":\"0198f0d0-7b31-7000-8000-000000000001\"}\n",
        )
        .unwrap();
        assert!(read_session(&path).unwrap().is_empty());
        assert!(read_session_display(&path).unwrap().is_empty());
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
            false,
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
            false,
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

        let mut watcher =
            HistoryWatcher::open(&dir, None, HistoryLabels::default(), None, false).unwrap();
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
