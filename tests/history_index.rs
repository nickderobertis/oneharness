//! The dated history index, driven the way its consumers drive it: the real
//! `oneharness` binary (with the mock harness standing in for a paid one), the
//! in-process library run, and the library readers — against stores built so
//! that any read the contract does not name is observable. The contract is
//! `docs/history-index.md`.
//!
//! How a stray read shows:
//! - a file the reader has no business opening is a FIFO, so opening it for
//!   reading blocks and the bounded run fails on its deadline;
//! - a file it has no business opening is mode `000` (when not root), so the
//!   open fails loudly;
//! - on Linux, a child's `rchar` (bytes read) and peak RSS are read after it
//!   exits, and a store grown tenfold — sparse files, so the size costs no disk —
//!   must not move them.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, SystemTime};

use oneharness_core::domain::events::ActionEvent;
use oneharness_core::domain::history::{HistoryId, HistoryLabels, HistoryLine};
use oneharness_core::domain::history_index::{
    EventIndexEntry, HistoryIndexEntry, LineSpan, RunIndexEntry, INDEX_SCHEMA_VERSION,
};
use oneharness_core::domain::mode::PermissionMode;
use oneharness_core::domain::report;
use oneharness_core::io::history::{self, HistoryWindow, HistoryWriter, UtcDate};
use oneharness_core::io::scratch::ScratchDir;
use serde_json::Value;

fn oneharness_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_oneharness"))
}

fn mock_bin() -> PathBuf {
    let mut path = oneharness_bin();
    path.set_file_name(format!(
        "oneharness-mock-harness{}",
        std::env::consts::EXE_SUFFIX
    ));
    path
}

const CODEX_TELEMETRY: &str = concat!(
    "{\"type\":\"turn.started\"}\n",
    "{\"type\":\"item.completed\",\"item\":{\"id\":\"m1\",\"type\":\"agent_message\",\"text\":\"indexed\"}}\n",
    "{\"type\":\"turn.completed\"}\n",
);

/// The binary, hermetic: no config file or `ONEHARNESS_*` override reshapes it.
fn oneharness() -> Command {
    let mut command = Command::new(oneharness_bin());
    command.env("ONEHARNESS_NO_CONFIG", "1");
    for (name, _) in std::env::vars() {
        if name.starts_with("ONEHARNESS_") && name != "ONEHARNESS_NO_CONFIG" {
            command.env_remove(name);
        }
    }
    command
}

/// Run to completion, killing the child and failing at `limit` — a read of a
/// FIFO never returns, so this is how such a read fails the test.
fn run_within(mut command: Command, limit: Duration) -> Output {
    use std::io::Read;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("spawn oneharness");
    let drain = |mut pipe: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            pipe.read_to_end(&mut bytes).ok();
            bytes
        })
    };
    let stdout = drain(Box::new(child.stdout.take().unwrap()));
    let stderr = drain(Box::new(child.stderr.take().unwrap()));
    let deadline = std::time::Instant::now() + limit;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().expect("stop the child this test started");
            child.wait().expect("reap it");
            panic!(
                "{command:?} was still running after {limit:?}: it read a file it should not have"
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    Output {
        status,
        stdout: stdout.join().unwrap(),
        stderr: stderr.join().unwrap(),
    }
}

fn json(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "exit {:?}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout is not JSON ({error}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

/// `oneharness history <args…> --history-dir <store> --format json`, bounded.
fn history_verb(store: &Path, args: &[&str]) -> Output {
    let mut command = oneharness();
    command
        .arg("history")
        .args(args)
        .args(["--history-dir", &store.display().to_string()])
        .args(["--format", "json"]);
    run_within(command, Duration::from_secs(60))
}

/// A UUIDv7 history id minted at `secs` past the epoch — a clock seam for the
/// writer, which files an entry under the date its id was minted on.
fn id_at(secs: u64, counter: u64) -> HistoryId {
    let ms = secs * 1000;
    format!(
        "{:08x}-{:04x}-7{:03x}-8{:03x}-{:012x}",
        ms >> 16,
        ms & 0xffff,
        counter & 0xfff,
        (counter >> 12) & 0xfff,
        counter
    )
    .parse()
    .expect("a canonical v7 id")
}

fn epoch_of(date: &str) -> u64 {
    let date: UtcDate = date.parse().unwrap();
    // UtcDate counts days; recover them through its own arithmetic.
    let epoch: UtcDate = "1970-01-01".parse().unwrap();
    let mut days = 0u64;
    let mut probe = epoch;
    while probe < date {
        probe = probe.add_days(1);
        days += 1;
    }
    days * 86_400 + 12 * 3600
}

fn today() -> UtcDate {
    UtcDate::from_epoch_secs(
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64,
    )
}

fn finished_result(prompt_text: &str) -> report::RunResult {
    report::RunResult {
        harness: "codex".to_string(),
        variant: None,
        harness_id: "codex".to_string(),
        bin: "codex".to_string(),
        available: true,
        status: report::Status::Ok,
        prompt: None,
        model: None,
        observed_model: None,
        exit_code: Some(0),
        duration_ms: Some(10),
        telemetry: Some(report::ExecutionTelemetry::ProviderMeasured {
            started_at: "2026-07-19T00:00:00.000Z".parse().unwrap(),
            finished_at: Some("2026-07-19T00:00:00.000Z".parse().unwrap()),
            model_ms: Some(7),
            tool_ms: Some(0),
            time_to_first_token_ms: None,
        }),
        command: vec!["codex".to_string()],
        output_format: report::OutputFormat::Json,
        text: Some(prompt_text.to_string()),
        text_source: Some("raw".to_string()),
        usage: oneharness_core::domain::signals::Usage::default(),
        usage_source: None,
        session_id: None,
        events: None,
        events_source: None,
        structured: None,
        schema_valid: None,
        schema_attempts: None,
        schema_error: None,
        failure_kind: None,
        work: None,
        failure_kind_source: None,
        stdout: "done".to_string(),
        stderr: String::new(),
        error: None,
    }
}

fn event(index: usize, output: &str) -> ActionEvent {
    ActionEvent {
        kind: "message".to_string(),
        name: None,
        input: None,
        output: Some(output.to_string()),
        index,
        tool_call_id: None,
        started_at: None,
        finished_at: None,
        duration_ms: None,
        status: None,
        timing_source: None,
    }
}

/// Record one run through the library writer under a chosen id: an event line,
/// then its closing run line. Returns the session file.
fn seed_run(store: &Path, project: &Path, name: &str, id: HistoryId, prompt: &str) -> PathBuf {
    let writer = HistoryWriter::open(store, project, name, HistoryLabels::default()).unwrap();
    writer.append_event(id, "codex", event(0, prompt)).unwrap();
    writer
        .append_streamed(
            id,
            PermissionMode::Default,
            None,
            prompt,
            &finished_result(prompt),
            &BTreeSet::from([0]),
        )
        .unwrap();
    writer.path().to_path_buf()
}

/// A file's length, modification time, and bytes when readable and small.
type FileState = (u64, Option<SystemTime>, Option<Vec<u8>>);

/// Every file under `dir` with its bytes (or, for one this test cannot read,
/// its length and modification time).
fn snapshot(dir: &Path) -> BTreeMap<PathBuf, FileState> {
    let mut files = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(next) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&next) else {
            continue;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            let meta = std::fs::symlink_metadata(&path).unwrap();
            if meta.is_dir() {
                stack.push(path);
            } else {
                let bytes = if meta.is_file() && meta.len() <= 4 << 20 {
                    std::fs::read(&path).ok()
                } else {
                    None
                };
                files.insert(path, (meta.len(), meta.modified().ok(), bytes));
            }
        }
    }
    files
}

fn segment_files(store: &Path) -> BTreeMap<String, Vec<u8>> {
    let dir = store.join(".index.d");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return BTreeMap::new();
    };
    entries
        .map(|entry| {
            let path = entry.unwrap().path();
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                std::fs::read(&path).unwrap(),
            )
        })
        .collect()
}

fn entries_of(bytes: &[u8]) -> Vec<HistoryIndexEntry> {
    bytes
        .split(|byte| *byte == b'\n')
        .filter_map(|line| serde_json::from_slice(line).ok())
        .collect()
}

#[cfg(unix)]
fn project_slug(project: &Path) -> String {
    oneharness_core::domain::history::project_slug(
        &std::fs::canonicalize(project)
            .unwrap()
            .display()
            .to_string(),
    )
}

#[cfg(unix)]
fn is_root() -> bool {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() == 0 }
}

#[cfg(unix)]
fn make_fifo(path: &Path) {
    use std::os::unix::ffi::OsStrExt;
    let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: a valid NUL-terminated path; mkfifo only creates a node.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o644) }, 0, "{path:?}");
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// A sparse file: one real line, then `size` bytes of hole — a full read of it
/// is `size` bytes of `rchar` and of heap, at no disk cost.
#[cfg(target_os = "linux")]
fn sparse(path: &Path, first_line: &str, size: u64) {
    std::fs::write(path, format!("{first_line}\n")).unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_len(size)
        .unwrap();
}

/// The fields a `docs/history-index.md` table documents: every `` `name` `` in
/// its first column, `a` / `b` rows naming two.
fn documented_fields(doc: &str, marker: &str) -> BTreeSet<String> {
    doc.split(marker)
        .nth(1)
        .and_then(|rest| rest.split("| field | meaning |").nth(1))
        .unwrap_or_else(|| panic!("docs/history-index.md has a table after {marker}"))
        .lines()
        .skip(1)
        .take_while(|line| line.starts_with('|'))
        .filter(|line| line.starts_with("| `"))
        .flat_map(|line| {
            line.trim_start_matches("| ")
                .split(" |")
                .next()
                .unwrap_or_default()
                .split('/')
                .map(|cell| cell.trim().trim_matches('`').to_string())
                .collect::<Vec<_>>()
        })
        .collect()
}

fn serialized_fields(entry: &HistoryIndexEntry) -> BTreeSet<String> {
    serde_json::to_value(entry)
        .unwrap()
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect()
}

#[test]
fn the_contract_documents_exactly_the_fields_each_entry_serializes() {
    let doc = include_str!("../docs/history-index.md").replace("\r\n", "\n");
    let id: HistoryId = "0192b2a0-0000-7000-8000-000000000001".parse().unwrap();
    let labels = HistoryLabels::new(BTreeMap::from([("k".to_string(), "v".to_string())])).unwrap();
    let span = Some(LineSpan {
        offset: 0,
        length: std::num::NonZeroU64::MIN,
    });
    // Fully populated, so every optional field is on the wire.
    let run = HistoryIndexEntry::Run(RunIndexEntry {
        schema_version: INDEX_SCHEMA_VERSION.to_string(),
        history_id: id,
        session_path: "p/s.jsonl".parse().unwrap(),
        session: "s".to_string(),
        name: "s".to_string(),
        project_slug: "p".to_string(),
        harness_id: "codex".to_string(),
        labels: labels.clone(),
        recorded_at: "2026-01-01T00:00:00Z".parse().unwrap(),
        span,
    });
    let event = HistoryIndexEntry::Event(EventIndexEntry {
        schema_version: INDEX_SCHEMA_VERSION.to_string(),
        run_id: id,
        event_index: 0,
        session_path: "p/s.jsonl".parse().unwrap(),
        project_slug: "p".to_string(),
        harness_id: "codex".to_string(),
        labels,
        span,
    });
    assert_eq!(
        documented_fields(&doc, "**Run entry.**"),
        serialized_fields(&run),
        "docs/history-index.md's run-entry table must name exactly the run entry's fields"
    );
    assert_eq!(
        documented_fields(&doc, "**Event entry.**"),
        serialized_fields(&event),
        "docs/history-index.md's event-entry table must name exactly the event entry's fields"
    );
    assert!(doc.contains(&format!("(`{INDEX_SCHEMA_VERSION}`)")));
}

/// A store crowded with everything a recording run must not touch: a legacy
/// index and event index and lock (mode `000`, sparse — sized with the store),
/// `sessions` other sessions' files across ten projects and the run's own
/// (mode `000`, sparse), one FIFO session file in the run's own project, and
/// today's segments already long (sparse) — which the writer appends to after
/// reading one byte.
#[cfg(target_os = "linux")]
struct CrowdedStore {
    _scratch: ScratchDir,
    store: PathBuf,
    project: PathBuf,
    untouchable: Vec<PathBuf>,
}

#[cfg(target_os = "linux")]
impl CrowdedStore {
    fn new(tag: &str, sessions: u64) -> CrowdedStore {
        let scratch = ScratchDir::new(&format!("hindex-crowd-{tag}")).unwrap();
        let store = scratch.join("store");
        let project = scratch.join("project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(store.join(".index.d")).unwrap();
        let own = store.join(project_slug(&project));
        std::fs::create_dir_all(&own).unwrap();
        let mut untouchable = Vec::new();
        let legacy_line = r#"{"session_path":"p/old.jsonl","record":{}}"#;
        for name in [".index.jsonl", ".event-index.jsonl"] {
            let path = store.join(name);
            sparse(&path, legacy_line, sessions * 64 * 1024);
            untouchable.push(path);
        }
        std::fs::write(store.join(".index.lock"), "").unwrap();
        untouchable.push(store.join(".index.lock"));
        for i in 0..sessions {
            let dir = if i % 11 == 0 {
                own.clone()
            } else {
                store.join(format!("project-{}", i % 10))
            };
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join(format!("other-20260101T000000Z-{i}.jsonl"));
            sparse(&path, r#"{"type":"run"}"#, 256 * 1024);
            untouchable.push(path);
        }
        make_fifo(&own.join("fifo-20260101T000000Z-1.jsonl"));
        let day = today();
        for kind in ["runs", "events"] {
            let path = store.join(".index.d").join(format!("{kind}-{day}.ndjson"));
            sparse(&path, r#"{"kind":"run"}"#, sessions * 16 * 1024);
            // End on a newline, as a healthy segment does.
            std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap()
                .write_all_newline();
        }
        if !is_root() {
            for path in &untouchable {
                set_mode(path, 0o000);
            }
        }
        CrowdedStore {
            _scratch: scratch,
            store,
            project,
            untouchable,
        }
    }

    fn untouched(&self) -> Vec<(PathBuf, u64, SystemTime)> {
        self.untouchable
            .iter()
            .map(|path| {
                let meta = std::fs::metadata(path).unwrap();
                (path.clone(), meta.len(), meta.modified().unwrap())
            })
            .collect()
    }
}

#[cfg(target_os = "linux")]
impl Drop for CrowdedStore {
    fn drop(&mut self) {
        // Mode 000 files are the scratch guard's to remove, which needs the
        // directory — not the files — writable; restore them anyway so a failed
        // run leaves nothing a human cannot inspect.
        for path in &self.untouchable {
            set_mode(path, 0o644);
        }
    }
}

#[cfg(target_os = "linux")]
trait WriteNewline {
    fn write_all_newline(self);
}

#[cfg(target_os = "linux")]
impl WriteNewline for std::fs::File {
    fn write_all_newline(mut self) {
        use std::io::Write;
        self.write_all(b"\n").unwrap();
    }
}

#[cfg(target_os = "linux")]
mod measured {
    //! A command's bytes read and peak RSS, measured by a small helper process
    //! (this test binary, re-entered as `measure_helper`) that spawns it,
    //! reaps it with `wait4` and reads its own `/proc/self/io` around the
    //! child's life. Two traps make the helper necessary: an exited child's
    //! `/proc/<pid>/io` is not readable, and `exec` records the *spawning*
    //! process's high-water RSS into the child's `ru_maxrss` — so a child of
    //! this large, multi-threaded test process would report this process's
    //! size. The helper is small and single-purpose, so its children's figures
    //! are theirs, and a reaped child's I/O is folded into the helper's own.
    use super::*;
    use serde::{Deserialize, Serialize};

    pub struct Measured {
        pub output: Output,
        pub rchar: u64,
        pub max_rss_kib: i64,
        /// The largest resident size sampled from the command while it ran.
        pub sampled_rss_kib: u64,
        /// The command's resident size over its life, one sample per
        /// [`SERIES_EVERY`], beside the length of the request's watched file.
        pub series: Vec<Sample>,
    }

    /// How often the helper adds a point to [`Measured::series`].
    pub const SERIES_EVERY: Duration = Duration::from_millis(100);

    /// One point of a command's life: when, how resident, and how long the
    /// watched file (the segment it appends to) was at that moment.
    #[derive(Clone, Copy, Debug, Serialize, Deserialize)]
    pub struct Sample {
        pub at_ms: u64,
        pub rss_kib: u64,
        pub watched_len: u64,
    }

    #[derive(Serialize, Deserialize)]
    pub struct Request {
        pub program: String,
        pub args: Vec<String>,
        pub env: Vec<(String, Option<String>)>,
        pub stdout: PathBuf,
        pub stderr: PathBuf,
        pub limit_ms: u64,
        /// A file whose length each [`Sample`] records, if any.
        pub watch: Option<PathBuf>,
    }

    #[derive(Serialize, Deserialize)]
    pub struct Report {
        pub status: i32,
        pub rchar: u64,
        pub max_rss_kib: i64,
        pub sampled_rss_kib: u64,
        pub series: Vec<Sample>,
        pub timed_out: bool,
    }

    fn rchar_of_self() -> u64 {
        std::fs::read_to_string("/proc/self/io")
            .unwrap()
            .lines()
            .find_map(|line| line.strip_prefix("rchar: "))
            .and_then(|value| value.trim().parse().ok())
            .unwrap()
    }

    fn vm_rss_kib(pid: u32) -> Option<u64> {
        std::fs::read_to_string(format!("/proc/{pid}/status"))
            .ok()?
            .lines()
            .find_map(|line| line.strip_prefix("VmRSS:"))
            .and_then(|rest| rest.trim().trim_end_matches("kB").trim().parse().ok())
    }

    /// The helper's half: run the request and report on it.
    pub fn serve(request: &Request) -> Report {
        let mut command = Command::new(&request.program);
        command
            .args(&request.args)
            .stdin(Stdio::null())
            .stdout(std::fs::File::create(&request.stdout).unwrap())
            .stderr(std::fs::File::create(&request.stderr).unwrap());
        for (key, value) in &request.env {
            match value {
                Some(value) => command.env(key, value),
                None => command.env_remove(key),
            };
        }
        let before = rchar_of_self();
        // Reaped by the `wait4` below, which is what yields its rusage — a
        // `Child::wait` would reap it first and lose exactly that.
        #[allow(clippy::zombie_processes)]
        let child = command.spawn().expect("spawn the measured command");
        let pid = child.id();
        let deadline = std::time::Instant::now() + Duration::from_millis(request.limit_ms);
        let started = std::time::Instant::now();
        let mut sampled_rss_kib = 0;
        let mut series = Vec::new();
        let mut next_point = started;
        let mut timed_out = false;
        loop {
            // SAFETY: a zeroed siginfo_t is a valid out-parameter for waitid.
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            // SAFETY: `pid` is this helper's own child; WNOWAIT leaves it unreaped.
            let waited = unsafe {
                libc::waitid(
                    libc::P_PID,
                    pid as libc::id_t,
                    &mut info,
                    libc::WEXITED | libc::WNOWAIT | libc::WNOHANG,
                )
            };
            // SAFETY: si_pid is valid to read once waitid filled the struct.
            if waited == 0 && unsafe { info.si_pid() } == pid as libc::pid_t {
                break;
            }
            if let Some(rss) = vm_rss_kib(pid) {
                sampled_rss_kib = sampled_rss_kib.max(rss);
                if std::time::Instant::now() >= next_point {
                    next_point += SERIES_EVERY;
                    series.push(Sample {
                        at_ms: started.elapsed().as_millis() as u64,
                        rss_kib: rss,
                        watched_len: request
                            .watch
                            .as_ref()
                            .and_then(|path| std::fs::metadata(path).ok())
                            .map_or(0, |meta| meta.len()),
                    });
                }
            }
            if std::time::Instant::now() >= deadline {
                // SAFETY: signals this helper's own child, by the pid it spawned.
                unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
                timed_out = true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let mut status = 0;
        // SAFETY: a zeroed rusage is a valid out-parameter for wait4.
        let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
        // SAFETY: reaps this helper's own exited child exactly once.
        let reaped = unsafe { libc::wait4(pid as libc::pid_t, &mut status, 0, &mut usage) };
        assert_eq!(reaped, pid as libc::pid_t);
        Report {
            status,
            rchar: rchar_of_self() - before,
            max_rss_kib: usage.ru_maxrss,
            sampled_rss_kib,
            series,
            timed_out,
        }
    }

    /// Measure `command` through the helper.
    pub fn measure(command: Command, limit: Duration) -> Measured {
        measure_watching(command, limit, None)
    }

    /// [`measure`], also sampling the length of `watch` over the command's life.
    pub fn measure_watching(command: Command, limit: Duration, watch: Option<&Path>) -> Measured {
        let scratch = ScratchDir::new(&format!(
            "hindex-measure-{}",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
        .unwrap();
        let request = Request {
            program: command.get_program().to_string_lossy().into_owned(),
            args: command
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect(),
            env: command
                .get_envs()
                .map(|(key, value)| {
                    (
                        key.to_string_lossy().into_owned(),
                        value.map(|value| value.to_string_lossy().into_owned()),
                    )
                })
                .collect(),
            stdout: scratch.join("stdout"),
            stderr: scratch.join("stderr"),
            limit_ms: limit.as_millis() as u64,
            watch: watch.map(Path::to_path_buf),
        };
        let request_file = scratch.join("request.json");
        let report_file = scratch.join("report.json");
        std::fs::write(&request_file, serde_json::to_vec(&request).unwrap()).unwrap();
        let helper = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "measure_helper", "--ignored", "--test-threads=1"])
            .env("HISTORY_INDEX_MEASURE", &request_file)
            .env("HISTORY_INDEX_MEASURE_OUT", &report_file)
            .output()
            .unwrap();
        assert!(
            helper.status.success(),
            "the measuring helper failed: {helper:?}"
        );
        let report: Report = serde_json::from_slice(&std::fs::read(&report_file).unwrap()).unwrap();
        assert!(
            !report.timed_out,
            "{command:?} was still running after {limit:?}"
        );
        use std::os::unix::process::ExitStatusExt;
        Measured {
            output: Output {
                status: std::process::ExitStatus::from_raw(report.status),
                stdout: std::fs::read(&request.stdout).unwrap(),
                stderr: std::fs::read(&request.stderr).unwrap(),
            },
            rchar: report.rchar,
            max_rss_kib: report.max_rss_kib,
            sampled_rss_kib: report.sampled_rss_kib,
            series: report.series,
        }
    }
}

/// The helper half of [`measured::measure`]: run only as its child.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "the measuring helper measured::measure re-enters this binary as"]
fn measure_helper() {
    let (Ok(request), Ok(out)) = (
        std::env::var("HISTORY_INDEX_MEASURE"),
        std::env::var("HISTORY_INDEX_MEASURE_OUT"),
    ) else {
        panic!(
            "measure_helper is entered only by measured::measure, which sets \
             HISTORY_INDEX_MEASURE and HISTORY_INDEX_MEASURE_OUT; run the tests that call it"
        );
    };
    let request: measured::Request =
        serde_json::from_slice(&std::fs::read(request).unwrap()).unwrap();
    let report = measured::serve(&request);
    assert!(
        report.max_rss_kib > 0,
        "wait4 reported no peak RSS for the measured command"
    );
    std::fs::write(out, serde_json::to_vec(&report).unwrap()).unwrap();
}

/// How a recording run is started: the CLI buffered, the CLI streaming, and an
/// in-process library run through `io::run` (this test binary, re-entered).
#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug)]
enum Recorder {
    Cli,
    CliStreaming,
    Library,
}

#[cfg(target_os = "linux")]
fn recording_command(recorder: Recorder, crowd: &CrowdedStore) -> Command {
    let store = crowd.store.display().to_string();
    let project = crowd.project.display().to_string();
    match recorder {
        Recorder::Cli | Recorder::CliStreaming => {
            let mut command = oneharness();
            command
                .env("MOCK_STDOUT", CODEX_TELEMETRY)
                // Long enough to sample the recording process mid-run.
                .env("MOCK_SLEEP_MS", "400")
                .args(["run", "--harness", "codex", "--prompt", "record me"])
                .args(["--bin", &format!("codex={}", mock_bin().display())])
                .args(["--history", "--history-dir", &store, "--cwd", &project])
                .args(["--bypass", "--format", "json"]);
            if matches!(recorder, Recorder::CliStreaming) {
                command.arg("--stream");
            }
            command
        }
        Recorder::Library => {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "library_recording_child",
                    "--ignored",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env("HISTORY_INDEX_CHILD_STORE", &store)
                .env("HISTORY_INDEX_CHILD_CWD", &project);
            command
        }
    }
}

/// The in-process half of [`Recorder::Library`]: one `io::run` with history on,
/// printing its report as JSON. Run only as the measured child of the test
/// below, which names the store through the environment.
#[test]
#[ignore = "the measured child of the recording and long-lived recording tests"]
fn library_recording_child() {
    use oneharness_core::io::run::{run, RunControls, RunRequest};
    let (Ok(store), Ok(cwd)) = (
        std::env::var("HISTORY_INDEX_CHILD_STORE"),
        std::env::var("HISTORY_INDEX_CHILD_CWD"),
    ) else {
        panic!(
            "library_recording_child is entered only by the recording tests, which set \
             HISTORY_INDEX_CHILD_STORE and HISTORY_INDEX_CHILD_CWD; run those instead"
        );
    };
    // Set by the long-lived test: stream this many events over the turn's
    // life instead of a short buffered turn.
    let events = std::env::var("HISTORY_INDEX_CHILD_EVENTS").ok().map(|n| {
        n.parse::<usize>()
            .expect("HISTORY_INDEX_CHILD_EVENTS is a count")
    });
    let env = match events {
        Some(n) => vec![
            format!("MOCK_STDOUT={}", long_turn_stdout(n)),
            format!("MOCK_STREAM_DELAY_MS={LONG_TURN_EVENT_GAP_MS}"),
        ],
        None => vec![
            format!("MOCK_STDOUT={CODEX_TELEMETRY}"),
            "MOCK_SLEEP_MS=400".to_string(),
        ],
    };
    struct Counting(usize);
    impl oneharness_core::io::run::EventSink for Counting {
        fn event(
            &mut self,
            _harness_id: &str,
            _event: &ActionEvent,
        ) -> oneharness_core::io::run::SinkStep {
            self.0 += 1;
            oneharness_core::io::run::SinkStep::Continue
        }
    }
    let mut sink = Counting(0);
    let outcome = run(
        &RunRequest {
            harness: vec!["codex".to_string()],
            prompt: vec!["record me".to_string()],
            bin: vec![format!("codex={}", mock_bin().display())],
            env,
            no_config: true,
            timeout: Some(60),
            history: Some(true),
            history_dir: Some(PathBuf::from(store)),
            cwd: Some(PathBuf::from(cwd)),
            mode: Some(PermissionMode::Bypass),
            stream: events.map(|_| true),
            ..RunRequest::default()
        },
        RunControls {
            events: Some(&mut sink),
            ..RunControls::default()
        },
    )
    .expect("a valid hermetic run");
    assert_eq!(outcome.report.results[0].status, report::Status::Ok);
    if let Some(n) = events {
        // Each tool call and the closing answer, as they happened.
        assert_eq!(sink.0, n + 1, "every event reached the library's sink");
    }
    assert!(
        outcome.report.history_file.is_some(),
        "history was recorded"
    );
    println!("{}", serde_json::to_string(&outcome.report).unwrap());
}

/// The history file a recording's stdout names: the buffered report, the
/// streamed terminal envelope, or the library child's printed report.
#[cfg(target_os = "linux")]
fn recorded_history_file(stdout: &[u8]) -> PathBuf {
    let text = String::from_utf8_lossy(stdout);
    if let Ok(report) = serde_json::from_str::<Value>(&text) {
        return PathBuf::from(report["history_file"].as_str().unwrap());
    }
    // One JSON document per line — after libtest's `test <name> ... ` prefix,
    // for the library child.
    let report = text
        .lines()
        .rev()
        .filter_map(|line| serde_json::from_str::<Value>(&line[line.find('{')?..]).ok())
        .find_map(|value| {
            if value["history_file"].is_string() {
                Some(value)
            } else if value["report"]["history_file"].is_string() {
                Some(value["report"].clone())
            } else {
                None
            }
        })
        .unwrap_or_else(|| panic!("no report naming its history file in: {text}"));
    PathBuf::from(report["history_file"].as_str().unwrap())
}

#[cfg(target_os = "linux")]
#[test]
fn recording_reads_nothing_but_its_own_files_however_large_the_store() {
    for recorder in [Recorder::Cli, Recorder::CliStreaming, Recorder::Library] {
        let mut runs = Vec::new();
        for (size, sessions) in [("small", 200), ("large", 2000)] {
            let crowd = CrowdedStore::new(&format!("{recorder:?}-{size}"), sessions);
            let before = crowd.untouched();
            let segments_before = std::fs::read_dir(crowd.store.join(".index.d"))
                .unwrap()
                .count();
            let measured =
                measured::measure(recording_command(recorder, &crowd), Duration::from_secs(60));
            let stderr = String::from_utf8_lossy(&measured.output.stderr).into_owned();
            assert!(
                measured.output.status.success(),
                "{recorder:?}/{size}: {stderr}"
            );
            assert!(
                !stderr.contains("warning"),
                "{recorder:?}/{size}: history was not recorded cleanly: {stderr}"
            );
            // Nothing else in the store was opened for writing, rewritten or
            // removed — and, the files being mode 000, nothing opened them at
            // all without failing the run.
            assert_eq!(crowd.untouched(), before, "{recorder:?}/{size}");
            assert_eq!(
                std::fs::read_dir(crowd.store.join(".index.d"))
                    .unwrap()
                    .count(),
                segments_before,
                "{recorder:?}/{size}: today's two segments took the entries"
            );
            // The run is recorded, and its id resolves through today's segment.
            let file = recorded_history_file(&measured.output.stdout);
            let records = history::read_session(&file).unwrap();
            assert_eq!(records.len(), 1, "{recorder:?}/{size}");
            let found = history::find_record_by_id(&crowd.store, records[0].history_id)
                .expect("the recorded run is found by id");
            assert_eq!(found.prompt, "record me");
            runs.push((
                size,
                measured.rchar,
                measured.max_rss_kib,
                measured.sampled_rss_kib,
            ));
        }
        let (_, small_read, small_peak, small_sampled) = runs[0];
        let (_, large_read, large_peak, large_sampled) = runs[1];
        // A store ten times larger — 1800 more session files of 256 KiB each,
        // a legacy index 115 MiB longer and today's segments 28 MiB longer —
        // reads the same bytes and holds the same memory.
        assert!(
            large_read <= small_read + 64 * 1024,
            "{recorder:?}: bytes read grew with the store: {small_read} -> {large_read}"
        );
        assert!(
            large_peak <= small_peak + 4 * 1024,
            "{recorder:?}: peak RSS grew with the store: {small_peak} KiB -> {large_peak} KiB"
        );
        assert!(
            large_sampled <= small_sampled + 4 * 1024,
            "{recorder:?}: RSS while the run was live grew with the store: \
             {small_sampled} KiB -> {large_sampled} KiB"
        );
    }
}

/// How many tool events the long-lived recording streams, and the gap between
/// the lines that carry them: a turn of about four seconds (two lines per call)
/// that appends to the event segment the whole time.
#[cfg(target_os = "linux")]
const LONG_TURN_EVENTS: usize = 80;
const LONG_TURN_EVENT_GAP_MS: u64 = 25;

/// A codex turn of `events` tool calls, one line each, streamed by the mock at
/// [`LONG_TURN_EVENT_GAP_MS`] per line.
fn long_turn_stdout(events: usize) -> String {
    let mut stdout = String::from("{\"type\":\"turn.started\"}\n");
    for i in 0..events {
        stdout.push_str(&format!(
            "{{\"type\":\"item.started\",\"item\":{{\"id\":\"c{i}\",\"type\":\"command_execution\",\
             \"command\":\"echo {i}\",\"aggregated_output\":\"\",\"exit_code\":null,\"status\":\"in_progress\"}}}}\n\
             {{\"type\":\"item.completed\",\"item\":{{\"id\":\"c{i}\",\"type\":\"command_execution\",\
             \"command\":\"echo {i}\",\"aggregated_output\":\"{i}\",\"exit_code\":0,\"status\":\"completed\"}}}}\n"
        ));
    }
    stdout.push_str(concat!(
        "{\"type\":\"item.completed\",\"item\":{\"id\":\"m1\",\"type\":\"agent_message\",\"text\":\"indexed\"}}\n",
        "{\"type\":\"turn.completed\"}\n",
    ));
    stdout
}

/// A long-lived streaming recording through the CLI or `io::run`.
#[cfg(target_os = "linux")]
fn long_lived_command(recorder: Recorder, crowd: &CrowdedStore) -> Command {
    let mut command = match recorder {
        Recorder::CliStreaming => {
            let mut command = recording_command(Recorder::CliStreaming, crowd);
            command
                .env_remove("MOCK_SLEEP_MS")
                .env("MOCK_STDOUT", long_turn_stdout(LONG_TURN_EVENTS))
                .env("MOCK_STREAM_DELAY_MS", LONG_TURN_EVENT_GAP_MS.to_string());
            command
        }
        Recorder::Library => recording_command(Recorder::Library, crowd),
        Recorder::Cli => unreachable!("a buffered run appends its events only as it closes"),
    };
    command.env("HISTORY_INDEX_CHILD_EVENTS", LONG_TURN_EVENTS.to_string());
    command
}

/// A recording run that lives for seconds and appends an event entry the whole
/// time holds nothing derived from the store at any point of that life — no
/// parsed index, no set of indexed ids, no cache of either. The run's own work
/// is held fixed and the store is varied tenfold: its resident size, sampled
/// every [`measured::SERIES_EVERY`] while the event segment grows, must trace
/// the same profile against both, phase by phase of the turn. An index loaded
/// lazily after open — on the first event, the tenth, or the last — is ten
/// times larger on the larger store, so it shows in every phase after it.
/// Memory the run spends on its own events is the same on both stores, and
/// this test does not ask it to be absent.
#[cfg(target_os = "linux")]
#[test]
fn a_long_lived_recording_run_holds_no_index_at_any_point_of_its_life() {
    for recorder in [Recorder::CliStreaming, Recorder::Library] {
        let mut lives = Vec::new();
        for (size, sessions) in [("small", 200), ("large", 2000)] {
            let crowd = CrowdedStore::new(&format!("long-{recorder:?}-{size}"), sessions);
            let before = crowd.untouched();
            let events_segment = crowd
                .store
                .join(".index.d")
                .join(format!("events-{}.ndjson", today()));
            let segment_before = std::fs::metadata(&events_segment).unwrap().len();
            let measured = measured::measure_watching(
                long_lived_command(recorder, &crowd),
                Duration::from_secs(60),
                Some(&events_segment),
            );
            let stderr = String::from_utf8_lossy(&measured.output.stderr).into_owned();
            assert!(
                measured.output.status.success(),
                "{recorder:?}/{size}: {stderr}"
            );
            assert!(
                !stderr.contains("warning"),
                "{recorder:?}/{size}: history was not recorded cleanly: {stderr}"
            );
            assert_eq!(crowd.untouched(), before, "{recorder:?}/{size}");

            // Every streamed event landed as one entry of today's segment,
            // and resolves to its event line in the run's own session file.
            let file = recorded_history_file(&measured.output.stdout);
            let appended = std::fs::read(&events_segment).unwrap();
            let entries = entries_of(&appended[segment_before as usize..]);
            // One per tool call, and one for the closing answer.
            assert_eq!(entries.len(), LONG_TURN_EVENTS + 1, "{recorder:?}/{size}");
            let run_id = history::read_session(&file).unwrap()[0].history_id;
            for (i, entry) in entries.iter().enumerate() {
                let HistoryIndexEntry::Event(entry) = entry else {
                    panic!("{recorder:?}/{size}: a run entry in the events segment");
                };
                assert_eq!(entry.run_id, run_id, "{recorder:?}/{size}");
                let session = std::fs::read(crowd.store.join(entry.session_path.as_str())).unwrap();
                let span = entry.span.expect("a streamed event entry names its line");
                let start = span.offset as usize;
                let line = &session[start..start + span.length.get() as usize];
                match serde_json::from_slice(&line[..line.len() - 1]).unwrap() {
                    HistoryLine::Event(line) => {
                        assert_eq!((line.run_id, line.event.index), (run_id, i));
                    }
                    other => panic!("{recorder:?}/{size}: {other:?} is not event {i}"),
                }
            }

            // The run really was long-lived and appending while it was sampled:
            // many samples, spread over seconds, across which the segment kept
            // growing rather than taking every entry as the run closed.
            let series = measured.series;
            let life_ms = series.last().map_or(0, |sample| sample.at_ms);
            assert!(
                life_ms >= 3_000 && series.len() >= 25,
                "{recorder:?}/{size}: {} samples over {life_ms} ms",
                series.len()
            );
            let appending: Vec<_> = series
                .iter()
                .filter(|sample| {
                    sample.watched_len > segment_before
                        && sample.watched_len < appended.len() as u64
                })
                .copied()
                .collect();
            let growth_steps = appending
                .windows(2)
                .filter(|pair| pair[1].watched_len > pair[0].watched_len)
                .count();
            assert!(
                appending.len() >= 20 && growth_steps >= 15,
                "{recorder:?}/{size}: the segment was not appended to across the run's life: \
                 {series:?}"
            );

            // Its memory profile over that life: the resident size of the
            // first sample to see each further eighth of the turn's entries
            // in the segment.
            let turn_bytes = appended.len() as u64 - segment_before;
            let profile: Vec<u64> = (1..8u64)
                .map(|eighth| {
                    series
                        .iter()
                        .find(|sample| sample.watched_len - segment_before >= turn_bytes * eighth / 8)
                        .unwrap_or_else(|| {
                            panic!("{recorder:?}/{size}: no sample saw {eighth}/8 of the turn: {series:?}")
                        })
                        .rss_kib
                })
                .collect();
            lives.push((measured.rchar, profile, measured.max_rss_kib));
        }
        let (small_read, small_profile, small_peak) = &lives[0];
        let (large_read, large_profile, large_peak) = &lives[1];
        // The tenfold store — today's segments 3 MiB -> 31 MiB, the legacy
        // index 12 MiB -> 125 MiB, 200 -> 2000 other sessions' files — moves
        // none of it.
        assert!(
            *large_read <= small_read + 64 * 1024,
            "{recorder:?}: bytes read grew with the store: {small_read} -> {large_read}"
        );
        for (eighth, (small, large)) in small_profile.iter().zip(large_profile).enumerate() {
            assert!(
                *large <= small + 4 * 1024,
                "{recorder:?}: {}/8 through the turn, RSS was {small} KiB on the small store \
                 and {large} KiB on the tenfold one: {small_profile:?} vs {large_profile:?}",
                eighth + 1
            );
        }
        assert!(
            *large_peak <= small_peak + 4 * 1024,
            "{recorder:?}: peak RSS grew with the store: {small_peak} KiB -> {large_peak} KiB"
        );
    }
}

#[cfg(unix)]
fn flock_exclusive(path: &Path) -> std::fs::File {
    use std::os::unix::io::AsRawFd;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .unwrap();
    // SAFETY: a valid descriptor this test owns for the lock's lifetime.
    assert_eq!(unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) }, 0);
    file
}

#[cfg(unix)]
#[test]
fn recording_takes_no_lock_while_another_process_holds_every_lock_file() {
    // The legacy `.index.lock` and today's segments are held exclusively by
    // this test for the whole run: a writer that took either lock would wait
    // on it past the deadline.
    let scratch = ScratchDir::new("hindex-locks").unwrap();
    let store = scratch.join("store");
    let project = scratch.join("project");
    std::fs::create_dir_all(store.join(".index.d")).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    let day = today();
    let _held: Vec<_> = [
        store.join(".index.lock"),
        store.join(".index.d").join(format!("runs-{day}.ndjson")),
        store.join(".index.d").join(format!("events-{day}.ndjson")),
    ]
    .iter()
    .map(|path| flock_exclusive(path))
    .collect();
    for streaming in [false, true] {
        let mut command = oneharness();
        command
            .env("MOCK_STDOUT", CODEX_TELEMETRY)
            .args(["run", "--harness", "codex", "--prompt", "unlocked"])
            .args(["--bin", &format!("codex={}", mock_bin().display())])
            .args(["--history", "--history-dir", &store.display().to_string()])
            .args(["--cwd", &project.display().to_string(), "--bypass"])
            .args(["--format", "json"]);
        if streaming {
            command.arg("--stream");
        }
        let output = run_within(command, Duration::from_secs(30));
        assert!(output.status.success(), "{output:?}");
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains("warning"),
            "{output:?}"
        );
    }
    let runs = entries_of(&segment_files(&store)[&format!("runs-{day}.ndjson")]);
    assert_eq!(runs.len(), 2, "both runs recorded under the held locks");
    assert_eq!(
        std::fs::metadata(store.join(".index.lock")).unwrap().len(),
        0,
        "the legacy lock file is never written"
    );
}

#[test]
fn concurrent_writers_each_land_one_whole_entry_and_a_torn_tail_swallows_none() {
    let scratch = ScratchDir::new("hindex-concurrent").unwrap();
    let store = scratch.join("store");
    let project = scratch.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let day = today();
    let segment = store.join(".index.d").join(format!("runs-{day}.ndjson"));
    // An interrupted writer left half an entry, no newline.
    std::fs::create_dir_all(segment.parent().unwrap()).unwrap();
    std::fs::write(
        &segment,
        br#"{"schema_version":"1.0","kind":"run","history_id":"01"#,
    )
    .unwrap();
    let torn = std::fs::read(&segment).unwrap();

    let children: Vec<_> = (0..12)
        .map(|index| {
            oneharness()
                .env("MOCK_STDOUT", CODEX_TELEMETRY)
                .args(["run", "--harness", "codex", "--prompt", "concurrent"])
                .args(["--bin", &format!("codex={}", mock_bin().display())])
                .args(["--history", "--history-dir", &store.display().to_string()])
                .args(["--history-name", &format!("writer-{index}")])
                .args(["--cwd", &project.display().to_string(), "--bypass"])
                .args(["--format", "json"])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    let mut recorded = BTreeSet::new();
    for child in children {
        let output = child.wait_with_output().unwrap();
        let report = json(&output);
        let file = report["history_file"].as_str().unwrap();
        for record in history::read_session(Path::new(file)).unwrap() {
            recorded.insert(record.history_id);
        }
    }
    assert_eq!(recorded.len(), 12);
    let bytes = std::fs::read(&segment).unwrap();
    assert!(
        bytes.starts_with(&torn),
        "the torn bytes are never rewritten"
    );
    let indexed: Vec<HistoryId> = entries_of(&bytes)
        .into_iter()
        .filter_map(|entry| match entry {
            HistoryIndexEntry::Run(run) => Some(run.history_id),
            HistoryIndexEntry::Event(_) => None,
        })
        .collect();
    assert_eq!(indexed.len(), 12, "each writer's entry exactly once");
    assert_eq!(indexed.iter().collect::<BTreeSet<_>>().len(), 12);
    assert_eq!(indexed.into_iter().collect::<BTreeSet<_>>(), recorded);
    // Every line but the torn one is a whole entry.
    let lines: Vec<&[u8]> = bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .collect();
    assert_eq!(lines.len(), 13);
    assert_eq!(lines[0], torn.as_slice());
}

/// An entry with the fields that name its session (whose length follows the
/// clock and the pid, not the content) and its span (whose digits follow the
/// line's offset and length) blanked, serialized.
fn content_free_len(entry: &HistoryIndexEntry) -> usize {
    let mut value = serde_json::to_value(entry).unwrap();
    for field in ["session_path", "session", "offset", "length"] {
        if let Some(object) = value.as_object_mut() {
            object.remove(field);
        }
    }
    serde_json::to_string(&value).unwrap().len()
}

#[test]
fn an_entry_is_the_same_size_for_a_one_mebibyte_prompt_or_event_as_for_a_short_one() {
    let scratch = ScratchDir::new("hindex-size").unwrap();
    let project = scratch.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let huge = "x".repeat(1 << 20);
    let mut sizes = Vec::new();
    for (tag, prompt) in [("short", "short"), ("huge", huge.as_str())] {
        let store = scratch.join(tag);
        let prompt_file = scratch.join(format!("{tag}.prompt"));
        std::fs::write(&prompt_file, prompt).unwrap();
        let mut command = oneharness();
        command
            .env("MOCK_STDOUT", CODEX_TELEMETRY)
            .args(["run", "--harness", "codex"])
            .args(["--prompt-file", &prompt_file.display().to_string()])
            .args(["--bin", &format!("codex={}", mock_bin().display())])
            .args(["--history", "--history-dir", &store.display().to_string()])
            .args([
                "--history-name",
                "sized",
                "--cwd",
                &project.display().to_string(),
            ])
            .args(["--bypass", "--format", "json"]);
        let report = json(&run_within(command, Duration::from_secs(60)));
        let session = PathBuf::from(report["history_file"].as_str().unwrap());
        // An event with a 1 MiB output, through the library writer.
        let writer =
            HistoryWriter::open(&store, &project, "sized", HistoryLabels::default()).unwrap();
        writer
            .append_event(writer.begin_run(), "codex", event(0, prompt))
            .unwrap();
        let session_bytes = std::fs::read(&session).unwrap();
        let events_bytes = std::fs::read(writer.path()).unwrap();
        let mut run_len = None;
        let mut event_len = None;
        for (name, bytes) in segment_files(&store) {
            for entry in entries_of(&bytes) {
                // Each entry resolves to the line it indexes.
                let (span, file) = match &entry {
                    HistoryIndexEntry::Run(run) => (run.span.unwrap(), &session_bytes),
                    HistoryIndexEntry::Event(event)
                        if Path::new(event.session_path.as_str()).file_name()
                            == writer.path().file_name() =>
                    {
                        (event.span.unwrap(), &events_bytes)
                    }
                    HistoryIndexEntry::Event(event) => (event.span.unwrap(), &session_bytes),
                };
                let start = span.offset as usize;
                let line = &file[start..start + span.length.get() as usize];
                let parsed: HistoryLine = serde_json::from_slice(&line[..line.len() - 1])
                    .unwrap_or_else(|error| panic!("{name}: the span is not a line: {error}"));
                match (&entry, parsed) {
                    (HistoryIndexEntry::Run(run), HistoryLine::Run(line)) => {
                        assert_eq!(run.history_id, line.history_id);
                        if tag == "huge" {
                            assert!(line.prompt.len() >= 1 << 20);
                        }
                        run_len = Some(content_free_len(&entry));
                    }
                    (HistoryIndexEntry::Event(event), HistoryLine::Event(line)) => {
                        assert_eq!(
                            (event.run_id, event.event_index),
                            (line.run_id, line.event.index)
                        );
                        if line.event.output.as_deref() == Some(prompt) {
                            event_len = Some(content_free_len(&entry));
                        }
                    }
                    other => {
                        panic!("{name}: an entry pointing at the wrong kind of line: {other:?}")
                    }
                }
                assert!(
                    serde_json::to_vec(&entry).unwrap().len() < 1024,
                    "an entry carries no content: {entry:?}"
                );
            }
        }
        sizes.push((run_len.unwrap(), event_len.unwrap()));
    }
    assert_eq!(
        sizes[0], sizes[1],
        "short vs 1 MiB: (run entry, event entry)"
    );
}

struct DatedStore {
    _scratch: ScratchDir,
    store: PathBuf,
    project: PathBuf,
    runs: Vec<(HistoryId, &'static str)>,
}

/// Runs on two past UTC dates and today, plus a legacy index and event index
/// an older core left behind.
fn dated_store(tag: &str, seed: u64) -> DatedStore {
    let scratch = ScratchDir::new(&format!("hindex-dated-{tag}")).unwrap();
    let store = scratch.join("store");
    let project = scratch.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut runs = Vec::new();
    for (index, (date, secs)) in [
        ("2026-01-01", epoch_of("2026-01-01")),
        ("2026-01-02", epoch_of("2026-01-02")),
        ("today", now),
    ]
    .into_iter()
    .enumerate()
    {
        let id = id_at(secs, seed + index as u64 + 1);
        seed_run(&store, &project, &format!("dated-{index}"), id, "dated");
        runs.push((id, date));
    }
    std::fs::write(
        store.join(".index.jsonl"),
        "{\"session_path\":\"gone/old.jsonl\",\"record\":{\"not\":\"a record\"}}\n",
    )
    .unwrap();
    std::fs::write(
        store.join(".event-index.jsonl"),
        "{\"session_path\":\"gone/old.jsonl\"}\n",
    )
    .unwrap();
    DatedStore {
        _scratch: scratch,
        store,
        project,
        runs,
    }
}

fn listed_ids(output: &Output, _store: &Path) -> BTreeSet<HistoryId> {
    json(output)
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|session| {
            history::read_session(Path::new(session["path"].as_str().unwrap()))
                .unwrap()
                .into_iter()
                .map(|record| record.history_id)
        })
        .collect()
}

#[test]
fn past_runs_on_two_utc_dates_stay_found_and_nothing_rewrites_the_store() {
    let dated = dated_store("found", 0);
    let store = &dated.store;
    let before = snapshot(store);
    let segments = segment_files(store);
    assert!(segments.contains_key("runs-2026-01-01.ndjson"));
    assert!(segments.contains_key("runs-2026-01-02.ndjson"));

    for (id, date) in &dated.runs {
        let shown = json(&history_verb(store, &["show", &id.to_string()]));
        assert_eq!(shown[0]["history_id"], id.to_string(), "{date}");
        let all_time = json(&history_verb(
            store,
            &["show", &id.to_string(), "--all-time"],
        ));
        assert_eq!(all_time[0]["history_id"], id.to_string(), "{date}");
    }
    let every: BTreeSet<HistoryId> = dated.runs.iter().map(|(id, _)| *id).collect();
    let since = history_verb(store, &["list", "--all-projects", "--since", "2026-01-01"]);
    assert_eq!(listed_ids(&since, store), every);
    let since_second = history_verb(store, &["list", "--all-projects", "--since", "2026-01-02"]);
    assert_eq!(listed_ids(&since_second, store).len(), 2);
    let all_time = history_verb(store, &["list", "--all-projects", "--all-time"]);
    assert_eq!(listed_ids(&all_time, store), every);
    // The default window is the last 7 UTC days: today's run only.
    let recent = history_verb(store, &["list", "--all-projects"]);
    assert_eq!(listed_ids(&recent, store).len(), 1);
    // The library window agrees.
    assert_eq!(
        history::list_sessions(
            store,
            None,
            HistoryWindow::Since("2026-01-01".parse().unwrap())
        )
        .unwrap()
        .len(),
        3
    );
    assert_eq!(snapshot(store), before, "reading changed nothing");

    // migrate and reindex leave every segment and legacy index byte-identical
    // (reindex has nothing to add); clear --yes removes session files only.
    let legacy = |store: &Path| {
        [".index.jsonl", ".event-index.jsonl"].map(|name| std::fs::read(store.join(name)).unwrap())
    };
    let legacy_before = legacy(store);
    json(&history_verb(store, &["migrate"]));
    assert_eq!(segment_files(store), segments, "migrate");
    assert_eq!(legacy(store), legacy_before, "migrate");
    let reindexed = json(&history_verb(store, &["reindex"]));
    assert_eq!(reindexed["entries_added"], 0);
    assert_eq!(segment_files(store), segments, "reindex");
    assert_eq!(legacy(store), legacy_before, "reindex");
    let cleared = json(&history_verb(store, &["clear", "--all-projects", "--yes"]));
    assert_eq!(cleared["removed"], 3);
    assert_eq!(segment_files(store), segments, "clear --yes");
    assert_eq!(legacy(store), legacy_before, "clear --yes");
    assert!(store.join(".index.d").is_dir());
    // A cleared run's entry stays; its lookup skips the missing session.
    let (id, _) = dated.runs[0];
    assert_eq!(
        history_verb(store, &["show", &id.to_string()])
            .status
            .code(),
        Some(1)
    );
    let _ = &dated.project;
}

struct Watch {
    child: std::process::Child,
    lines: std::sync::mpsc::Receiver<Value>,
}

impl Watch {
    fn start(store: &Path, extra: &[&str]) -> Watch {
        use std::io::BufRead;
        let mut child = oneharness()
            .args(["history", "watch", "--all-projects", "--format", "jsonl"])
            .args(["--history-dir", &store.display().to_string()])
            .args(extra)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (send, lines) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if send.send(serde_json::from_str(&line).unwrap()).is_err() {
                    break;
                }
            }
        });
        Watch { child, lines }
    }

    /// Every envelope until a record with each of `wanted` has arrived.
    fn until_records(&self, wanted: &[HistoryId]) -> Vec<Value> {
        let mut missing: BTreeSet<String> = wanted.iter().map(ToString::to_string).collect();
        let mut seen = Vec::new();
        while !missing.is_empty() {
            let envelope = self
                .lines
                .recv_timeout(Duration::from_secs(20))
                .unwrap_or_else(|_| panic!("the watcher never emitted {missing:?}; saw {seen:?}"));
            if envelope["type"] == "record" {
                missing.remove(envelope["record"]["history_id"].as_str().unwrap());
            }
            seen.push(envelope);
        }
        seen
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.child.kill().ok();
        self.child.wait().ok();
    }
}

fn records_in(envelopes: &[Value]) -> Vec<String> {
    envelopes
        .iter()
        .filter(|envelope| envelope["type"] == "record")
        .map(|envelope| {
            envelope["record"]["history_id"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect()
}

fn event_runs_in(envelopes: &[Value]) -> BTreeSet<String> {
    envelopes
        .iter()
        .filter(|envelope| envelope["type"] == "event")
        .map(|envelope| envelope["line"]["run_id"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn a_watcher_follows_a_new_dates_segment_a_late_closing_line_and_the_legacy_index() {
    for (events, cursor) in [(false, false), (true, false), (false, true), (true, true)] {
        let tag = format!("watch-{events}-{cursor}");
        let scratch = ScratchDir::new(&format!("hindex-{tag}")).unwrap();
        let store = scratch.join("store");
        let project = scratch.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let tomorrow = now + 86_400;

        // A closed run the cursor names, and a run of today's still open.
        let before = id_at(now, 1);
        seed_run(&store, &project, "before", before, "before");
        let open_writer =
            HistoryWriter::open(&store, &project, "late-close", HistoryLabels::default()).unwrap();
        let late = id_at(now, 2);
        open_writer
            .append_event(late, "codex", event(0, "late"))
            .unwrap();
        // An older core's legacy index exists before the watcher opens.
        std::fs::write(store.join(".index.jsonl"), "").unwrap();

        let mut args = Vec::new();
        let cursor_text = before.to_string();
        if events {
            args.push("--events");
        }
        if cursor {
            args.extend(["--after", cursor_text.as_str()]);
        }
        // A closed run after the cursor: every variant emits it at open, which
        // is how this test knows the watcher has taken its starting offsets
        // before anything below is written.
        let ready = id_at(now, 6);
        seed_run(&store, &project, "ready", ready, "ready");
        let watch = Watch::start(&store, &args);
        let opened = watch.until_records(&[ready]);

        // A new UTC date's segment appears while it watches…
        let next_day = id_at(tomorrow, 3);
        seed_run(&store, &project, "tomorrow", next_day, "tomorrow");
        // …then today's open run closes, into today's (earlier-dated) segment.
        open_writer
            .append_streamed(
                late,
                PermissionMode::Default,
                None,
                "late",
                &finished_result("late"),
                &BTreeSet::from([0]),
            )
            .unwrap();
        // …then an older core appends a record to the legacy index.
        let legacy_session = seed_legacy_only(&store, &project, id_at(now, 4));
        let mut appended = std::fs::OpenOptions::new()
            .append(true)
            .open(store.join(".index.jsonl"))
            .unwrap();
        use std::io::Write;
        appended.write_all(legacy_session.as_bytes()).unwrap();
        // An older core's reconcile also re-appends runs this core recorded —
        // here the late-closed one, which the segments already delivered.
        appended
            .write_all(legacy_index_line(&store, open_writer.path(), late).as_bytes())
            .unwrap();
        // A final run marks the end of what the watcher must have emitted.
        let last = id_at(now, 5);
        seed_run(&store, &project, "last", last, "last");

        let mut wanted = vec![next_day, late, id_at(now, 4), last];
        if !cursor {
            wanted.push(before);
        }
        let already = records_in(&opened);
        let still: Vec<HistoryId> = wanted
            .iter()
            .copied()
            .filter(|id| !already.contains(&id.to_string()))
            .collect();
        let envelopes: Vec<Value> = opened
            .into_iter()
            .chain(watch.until_records(&still))
            .collect();
        wanted.push(ready);
        let mut records = records_in(&envelopes);
        let mut expected: Vec<String> = wanted.iter().map(ToString::to_string).collect();
        records.sort();
        expected.sort();
        assert_eq!(records, expected, "{tag}: each record exactly once");
        if events {
            let runs = event_runs_in(&envelopes);
            assert!(runs.contains(&next_day.to_string()), "{tag}: {runs:?}");
            assert!(runs.contains(&last.to_string()), "{tag}: {runs:?}");
        } else {
            assert!(event_runs_in(&envelopes).is_empty(), "{tag}");
        }
        // Nothing arrives twice, however long it keeps polling.
        std::thread::sleep(Duration::from_millis(400));
        let extra: Vec<Value> = watch.lines.try_iter().collect();
        assert!(records_in(&extra).is_empty(), "{tag}: duplicates {extra:?}");
    }
}

/// A session file an older core wrote, whose run is in no segment, and the
/// legacy index line that older core appends for it. Recorded in a store of
/// its own and copied in, so this store's segments never hear of it.
fn seed_legacy_only(store: &Path, project: &Path, id: HistoryId) -> String {
    let elsewhere = store.with_file_name("legacy-source");
    let recorded = seed_run(&elsewhere, project, &format!("legacy-{id}"), id, "legacy");
    let slug = recorded.parent().unwrap().file_name().unwrap();
    let session = store.join(slug).join(recorded.file_name().unwrap());
    std::fs::create_dir_all(session.parent().unwrap()).unwrap();
    std::fs::copy(&recorded, &session).unwrap();
    legacy_index_line(store, &session, id)
}

/// The line an older core's reconcile appends for a run: the session path and
/// the whole closing record.
fn legacy_index_line(store: &Path, session: &Path, id: HistoryId) -> String {
    let record = std::fs::read_to_string(session)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|line| line["type"] == "run" && line["history_id"] == id.to_string())
        .unwrap();
    let mut record = record;
    record.as_object_mut().unwrap().remove("type");
    let relative = session
        .strip_prefix(std::fs::canonicalize(store).unwrap())
        .or_else(|_| session.strip_prefix(store))
        .unwrap();
    format!(
        "{}\n",
        serde_json::json!({"session_path": relative.display().to_string(), "record": record})
    )
}

#[test]
fn a_watch_starts_from_since_or_from_all_time_instead_of_today() {
    let scratch = ScratchDir::new("hindex-watch-start").unwrap();
    let store = scratch.join("store");
    let project = scratch.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let old = id_at(now - 20 * 86_400, 1);
    let current = id_at(now, 2);
    seed_run(&store, &project, "old", old, "old");
    seed_run(&store, &project, "current", current, "current");
    // A run only the legacy index an older core kept names.
    let legacy = id_at(now - 30 * 86_400, 3);
    std::fs::write(
        store.join(".index.jsonl"),
        seed_legacy_only(&store, &project, legacy),
    )
    .unwrap();
    let since = today().add_days(-20).to_string();

    // --since: every segment from that date on, from its first entry; the
    // legacy index only from its size at open.
    let watch = Watch::start(&store, &["--since", &since, "--events"]);
    let mut seen = watch.until_records(&[old, current]);
    let later = id_at(now, 4);
    seed_run(&store, &project, "later", later, "later");
    seen.extend(watch.until_records(&[later]));
    std::thread::sleep(Duration::from_millis(400));
    seen.extend(watch.lines.try_iter());
    let mut records = records_in(&seen);
    records.sort();
    let mut expected = vec![old.to_string(), current.to_string(), later.to_string()];
    expected.sort();
    assert_eq!(records, expected, "--since");
    assert!(
        event_runs_in(&seen).contains(&old.to_string()),
        "--since --events"
    );
    drop(watch);

    // --all-time: every segment and the legacy index from its first byte.
    let watch = Watch::start(&store, &["--all-time"]);
    let mut seen = watch.until_records(&[old, current, later, legacy]);
    std::thread::sleep(Duration::from_millis(400));
    seen.extend(watch.lines.try_iter());
    let mut records = records_in(&seen);
    records.sort();
    let mut expected = vec![
        old.to_string(),
        current.to_string(),
        later.to_string(),
        legacy.to_string(),
    ];
    expected.sort();
    assert_eq!(records, expected, "--all-time, each once");
    drop(watch);

    // A start and a cursor are two answers to one question.
    let mut command = oneharness();
    command
        .args([
            "history",
            "watch",
            "--all-time",
            "--after",
            &current.to_string(),
        ])
        .args(["--history-dir", &store.display().to_string()]);
    let refused = run_within(command, Duration::from_secs(30));
    assert_eq!(refused.status.code(), Some(2), "{refused:?}");
}

#[test]
fn days_reads_the_last_n_utc_days_on_list_show_and_watch() {
    let scratch = ScratchDir::new("hindex-days").unwrap();
    let store = scratch.join("store");
    let project = scratch.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let old = id_at(now - 20 * 86_400, 1);
    let two = id_at(now - 2 * 86_400, 2);
    seed_run(&store, &project, "old", old, "old");
    seed_run(&store, &project, "two", two, "two");
    let listed = |days: &str| {
        let output = history_verb(&store, &["list", "--all-projects", "--days", days]);
        assert!(output.status.success(), "--days {days}: {output:?}");
        listed_ids(&output, &store)
    };
    // `--days N` is the last N UTC days, today included: 2 days ago is inside
    // three days and outside two, and 20 days ago is inside 21 alone.
    assert_eq!(listed("2"), BTreeSet::new());
    assert_eq!(listed("3"), BTreeSet::from([two]));
    assert_eq!(listed("20"), BTreeSet::from([two]));
    assert_eq!(listed("21"), BTreeSet::from([old, two]));

    let shown = |args: &[&str]| {
        let mut args = args.to_vec();
        args.push("--all-projects");
        let output = history_verb(&store, &args);
        match output.status.code() {
            Some(0) => json(&output)[0]["history_id"].as_str().map(str::to_string),
            Some(1) => None,
            _ => panic!("{args:?}: {output:?}"),
        }
    };
    assert_eq!(shown(&["show", "two", "--days", "2"]), None);
    assert_eq!(
        shown(&["show", "two", "--days", "3"]),
        Some(two.to_string())
    );
    assert_eq!(shown(&["show", "old", "--days", "20"]), None);
    assert_eq!(
        shown(&["show", "old", "--days", "21"]),
        Some(old.to_string())
    );
    assert_eq!(shown(&["show", "--last", "--days", "2"]), None);
    assert_eq!(
        shown(&["show", "--last", "--days", "3"]),
        Some(two.to_string())
    );

    // A watch starts at the beginning of the last N UTC days: what it emits
    // before a run recorded once it is open is exactly its opening read.
    let opening = |days: &str, counter: u64| {
        let watch = Watch::start(&store, &["--days", days]);
        let marker = id_at(now, counter);
        seed_run(&store, &project, "marker", marker, "marker");
        let seen = watch.until_records(&[marker]);
        let mut read: BTreeSet<String> = records_in(&seen).into_iter().collect();
        read.remove(&marker.to_string());
        (read, marker)
    };
    let (read, first_marker) = opening("1", 10);
    assert_eq!(read, BTreeSet::new(), "--days 1 is today alone");
    let (read, _) = opening("3", 11);
    assert_eq!(
        read,
        BTreeSet::from([two.to_string(), first_marker.to_string()]),
        "--days 3"
    );

    // One window at a time: a day count beside a date, all time or a cursor
    // is refused, and so is an empty day count.
    for refused in [
        vec!["list", "--days", "3", "--since", "2026-01-01"],
        vec!["list", "--days", "3", "--all-time"],
        vec!["show", "--last", "--days", "3", "--all-time"],
        vec!["list", "--days", "0"],
    ] {
        let output = history_verb(&store, &refused);
        assert_eq!(output.status.code(), Some(2), "{refused:?}: {output:?}");
    }
    let mut command = oneharness();
    command
        .args([
            "history",
            "watch",
            "--days",
            "3",
            "--after",
            &two.to_string(),
        ])
        .args(["--history-dir", &store.display().to_string()]);
    let refused = run_within(command, Duration::from_secs(30));
    assert_eq!(refused.status.code(), Some(2), "{refused:?}");
}

#[cfg(unix)]
#[test]
fn each_reader_opens_only_the_segments_its_window_names() {
    let scratch = ScratchDir::new("hindex-windows").unwrap();
    let store = scratch.join("store");
    let project = scratch.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let day = today();
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    // Readable runs: 20 days ago, 3 days ago, yesterday, today.
    let at = |days_ago: u64, counter: u64| id_at(now - days_ago * 86_400, counter);
    let old = at(20, 1);
    let recent = at(3, 2);
    let yesterday = at(1, 3);
    let current = at(0, 4);
    for (id, name) in [
        (old, "old"),
        (recent, "recent"),
        (yesterday, "yesterday"),
        (current, "current"),
    ] {
        seed_run(&store, &project, name, id, name);
    }
    let segments = store.join(".index.d");
    // Every segment no default reader may open is a FIFO: 10 and 30 days
    // back, and the 20-day-old run's own — restored only for --since/--all-time.
    let date = |days_ago: i64| day.add_days(-days_ago);
    let old_runs = segments.join(format!("runs-{}.ndjson", date(20)));
    let old_events = segments.join(format!("events-{}.ndjson", date(20)));
    let old_bytes = (
        std::fs::read(&old_runs).unwrap(),
        std::fs::read(&old_events).unwrap(),
    );
    std::fs::remove_file(&old_runs).unwrap();
    std::fs::remove_file(&old_events).unwrap();
    for path in [
        old_runs.clone(),
        old_events.clone(),
        segments.join(format!("runs-{}.ndjson", date(10))),
        segments.join(format!("events-{}.ndjson", date(30))),
    ] {
        make_fifo(&path);
    }
    // The legacy index and an unindexed session file are FIFOs too: a reader
    // that fell back to either, or walked the tree, would block on it.
    make_fifo(&store.join(".index.jsonl"));
    make_fifo(&store.join(".event-index.jsonl"));
    make_fifo(
        &store
            .join(project_slug(&project))
            .join("unindexed-20260101T000000Z-9.jsonl"),
    );

    // An id lookup: one runs segment and one session file.
    let shown = json(&history_verb(&store, &["show", &current.to_string()]));
    assert_eq!(shown[0]["history_id"], current.to_string());
    // A run of 3 days ago is found by its own date's segment too.
    let shown = json(&history_verb(&store, &["show", &recent.to_string()]));
    assert_eq!(shown[0]["history_id"], recent.to_string());
    // A pre-cutover id, in no segment: not found, naming the two remedies.
    let absent = id_at(now - 5 * 86_400, 99);
    let missing = history_verb(&store, &["show", &absent.to_string()]);
    assert_eq!(missing.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&missing.stderr);
    assert!(
        stderr.contains("history reindex") && stderr.contains("--all-time"),
        "{stderr}"
    );

    // list / show <name> / show --last: the last 7 UTC days.
    let listed = history_verb(&store, &["list", "--all-projects"]);
    let names: BTreeSet<String> = json(&listed)
        .as_array()
        .unwrap()
        .iter()
        .map(|session| session["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        names,
        BTreeSet::from(["current", "recent", "yesterday"].map(String::from))
    );
    assert_eq!(
        json(&history_verb(&store, &["show", "recent", "--all-projects"]))[0]["history_id"],
        recent.to_string()
    );
    assert_eq!(
        history_verb(&store, &["show", "old", "--all-projects"])
            .status
            .code(),
        Some(1),
        "a name outside the window is not listed"
    );
    assert_eq!(
        json(&history_verb(&store, &["show", "--last", "--all-projects"]))[0]["history_id"],
        current.to_string()
    );

    // watch with no --after: the current UTC day, and nothing earlier —
    // yesterday's segment only from its end, and the legacy index (which a
    // watch does tail, so it is a regular file here, holding an old run's
    // line) only from its size at open.
    for name in [".index.jsonl", ".event-index.jsonl"] {
        std::fs::remove_file(store.join(name)).unwrap();
    }
    let old_session = store
        .join(project_slug(&project))
        .read_dir()
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("old-")
        })
        .unwrap();
    std::fs::write(
        store.join(".index.jsonl"),
        legacy_index_line(&store, &old_session, old),
    )
    .unwrap();
    std::fs::write(store.join(".event-index.jsonl"), "").unwrap();
    let watch = Watch::start(&store, &[]);
    let emitted = watch.until_records(&[current]);
    std::thread::sleep(Duration::from_millis(400));
    let emitted: Vec<Value> = emitted.into_iter().chain(watch.lines.try_iter()).collect();
    assert_eq!(records_in(&emitted), vec![current.to_string()]);
    drop(watch);
    for name in [".index.jsonl", ".event-index.jsonl"] {
        std::fs::remove_file(store.join(name)).unwrap();
        make_fifo(&store.join(name));
    }

    // --since and --all-time read segments from a date on, or every one plus
    // the legacy files — never the session tree. Restore the old run's
    // segments and make the legacy files empty regular files for these.
    for path in [&old_runs, &old_events] {
        std::fs::remove_file(path).unwrap();
    }
    std::fs::write(&old_runs, &old_bytes.0).unwrap();
    std::fs::write(&old_events, &old_bytes.1).unwrap();
    let since = date(20).to_string();
    // The 10-day-old FIFO sits inside `--since`, so it has to go too.
    std::fs::remove_file(segments.join(format!("runs-{}.ndjson", date(10)))).unwrap();
    let from_20 = history_verb(&store, &["list", "--all-projects", "--since", &since]);
    assert_eq!(json(&from_20).as_array().unwrap().len(), 4);
    for name in [".index.jsonl", ".event-index.jsonl"] {
        std::fs::remove_file(store.join(name)).unwrap();
        std::fs::write(store.join(name), "").unwrap();
    }
    std::fs::remove_file(segments.join(format!("events-{}.ndjson", date(30)))).unwrap();
    let all_time = history_verb(&store, &["list", "--all-projects", "--all-time"]);
    assert_eq!(json(&all_time).as_array().unwrap().len(), 4);
    let shown = json(&history_verb(
        &store,
        &["show", &old.to_string(), "--all-time"],
    ));
    assert_eq!(shown[0]["history_id"], old.to_string());

    // An unreadable segment fails the read, naming its path — and nothing is
    // read in its place (the legacy index is a FIFO again).
    let unreadable = segments.join(format!("runs-{}.ndjson", date(3)));
    std::fs::remove_file(&unreadable).unwrap();
    std::fs::create_dir(&unreadable).unwrap();
    std::fs::remove_file(store.join(".index.jsonl")).unwrap();
    make_fifo(&store.join(".index.jsonl"));
    for args in [
        vec!["list", "--all-projects"],
        vec!["show", &recent.to_string() as &str],
    ] {
        let failed = history_verb(&store, &args);
        assert!(!failed.status.success(), "{args:?}: {failed:?}");
        assert!(
            failed.stdout.is_empty(),
            "{args:?}: nothing was read in its place"
        );
        let stderr = String::from_utf8_lossy(&failed.stderr);
        assert!(
            stderr.contains(&unreadable.display().to_string()),
            "{args:?}: {stderr}"
        );
    }
}

struct LegacyStore {
    _scratch: ScratchDir,
    store: PathBuf,
    project: PathBuf,
    pointer_file: PathBuf,
    runs: Vec<HistoryId>,
}

/// A store as released cores left it: session files in their (unchanged)
/// shape, a `.index.jsonl` of `{session_path, record}` lines — `padding`
/// entries of other runs first, so the lookup streams past all of them — and
/// no dated segment for any of its runs.
fn legacy_store(tag: &str, padding: usize) -> LegacyStore {
    let scratch = ScratchDir::new(&format!("hindex-legacy-{tag}")).unwrap();
    let store = scratch.join("store");
    let project = scratch.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let pointer_file = scratch.join("pointers.jsonl");
    let mut runs = Vec::new();
    let mut index = String::new();
    for n in 0..3u64 {
        let writer = HistoryWriter::open(
            &store,
            &project,
            &format!("legacy-{n}"),
            HistoryLabels::default(),
        )
        .unwrap()
        .with_pointer_file(Some(pointer_file.clone()));
        let id = writer.begin_harness_run(&"codex".parse().unwrap());
        writer
            .append_streamed(
                id,
                PermissionMode::Default,
                None,
                "legacy",
                &finished_result("legacy"),
                &BTreeSet::new(),
            )
            .unwrap();
        index.push_str(&legacy_index_line(&store, writer.path(), id));
        runs.push(id);
    }
    std::fs::remove_dir_all(store.join(".index.d")).unwrap();
    // Other runs' entries — each with a sizeable record, as released cores
    // copied the whole record, prompt included — then the three.
    let filler = index
        .lines()
        .next()
        .unwrap()
        .replace("\"legacy\"", &format!("\"{}\"", "p".repeat(4096)));
    let mut body = String::with_capacity(filler.len() * padding + index.len());
    for n in 0..padding {
        body.push_str(&filler.replace(
            &runs[0].to_string(),
            &id_at(1_700_000_000, n as u64 + 100).to_string(),
        ));
        body.push('\n');
    }
    body.push_str(&index);
    std::fs::write(store.join(".index.jsonl"), body).unwrap();
    std::fs::write(store.join(".event-index.jsonl"), "").unwrap();
    LegacyStore {
        _scratch: scratch,
        store,
        project,
        pointer_file,
        runs,
    }
}

#[test]
fn a_store_an_older_core_wrote_is_read_by_id_all_time_by_pointer_and_by_session_name() {
    let legacy = legacy_store("read", 50);
    let store = &legacy.store;
    let before = snapshot(store);
    for id in &legacy.runs {
        // Without --all-time: not found, naming both ways to reach it.
        let missing = history_verb(store, &["show", &id.to_string()]);
        assert_eq!(missing.status.code(), Some(1));
        let stderr = String::from_utf8_lossy(&missing.stderr);
        assert!(
            stderr.contains("history reindex") && stderr.contains("--all-time"),
            "{stderr}"
        );
        // With it: the record, streamed out of the legacy index.
        let shown = json(&history_verb(
            store,
            &["show", &id.to_string(), "--all-time"],
        ));
        assert_eq!(shown[0]["history_id"], id.to_string());
        let record = history::find_record_by_id_in(store, *id, HistoryWindow::AllTime).unwrap();
        assert_eq!(record.history_id, *id);
    }
    // Every pointer line's session reads by name, with no index entry.
    let pointers = history::read_pointers(&legacy.pointer_file).unwrap();
    assert_eq!(pointers.pointers.len(), 3);
    for pointer in &pointers.pointers {
        let records = history::read_session(Path::new(&pointer.history_file())).unwrap();
        assert_eq!(records[0].history_id, pointer.history_id());
        let shown = json(&history_verb(
            store,
            &[
                "show",
                pointer.history_session(),
                "--project",
                &legacy.project.display().to_string(),
            ],
        ));
        assert_eq!(shown[0]["history_id"], pointer.history_id().to_string());
    }
    assert!(!store.join(".index.d").exists(), "no read created an index");
    assert_eq!(
        snapshot(store),
        before,
        "no file was created, modified or removed"
    );
}

/// Paths a released core's reconcile indexed that the dated entries' stricter
/// `SessionPath` refuses: v0.19.1's `rebuild_index_locked` walked every
/// `<subdir>/*.jsonl` of the store, so a project at `/.index.d` (whose slug is
/// `.index.d`) and — where a file name may hold one — a hand-placed session
/// whose name holds `:` both reached `.index.jsonl`.
fn legacy_paths_released_cores_indexed() -> Vec<(&'static str, &'static str)> {
    let mut paths = vec![(".index.d", "legacy-20240101T000000Z-1")];
    if cfg!(unix) {
        paths.push(("proj", "copied:in-20240101T000000Z-2"));
    }
    paths
}

#[test]
fn an_all_time_lookup_reads_legacy_paths_a_session_path_refuses() {
    let scratch = ScratchDir::new("hindex-legacy-paths").unwrap();
    let store = scratch.join("store");
    let project = scratch.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let mut index = String::new();
    let mut runs = Vec::new();
    for (n, (slug, stem)) in legacy_paths_released_cores_indexed()
        .into_iter()
        .enumerate()
    {
        let id = id_at(1_704_067_200, n as u64 + 1);
        let recorded = seed_run(&scratch.join("source"), &project, "legacy", id, "legacy");
        let session = store.join(slug).join(format!("{stem}.jsonl"));
        std::fs::create_dir_all(session.parent().unwrap()).unwrap();
        std::fs::copy(&recorded, &session).unwrap();
        index.push_str(&legacy_index_line(&store, &session, id));
        runs.push((id, session));
    }
    std::fs::write(store.join(".index.jsonl"), &index).unwrap();
    std::fs::write(store.join(".event-index.jsonl"), "").unwrap();
    let before = snapshot(&store);
    for (id, session) in &runs {
        let shown = history_verb(&store, &["show", &id.to_string(), "--all-time"]);
        assert!(
            shown.status.success(),
            "{}: exit {:?}\nstderr: {}",
            session.display(),
            shown.status.code(),
            String::from_utf8_lossy(&shown.stderr)
        );
        assert_eq!(json(&shown)[0]["history_id"], id.to_string());
    }
    let listed = json(&history_verb(
        &store,
        &["list", "--all-projects", "--all-time"],
    ));
    let listed: BTreeSet<String> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|session| session["path"].as_str().unwrap().to_string())
        .collect();
    for (_, session) in &runs {
        let canonical = std::fs::canonicalize(session).unwrap();
        assert!(
            listed.contains(&session.display().to_string())
                || listed.contains(&canonical.display().to_string()),
            "{} missing from {listed:?}",
            session.display()
        );
    }
    assert_eq!(
        snapshot(&store),
        before,
        "no file was created, modified or removed"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn an_all_time_lookup_streams_the_legacy_index_in_bounded_memory() {
    let mut peaks = Vec::new();
    for (tag, padding) in [("small", 2_000), ("large", 20_000)] {
        let legacy = legacy_store(&format!("memory-{tag}"), padding);
        let before = snapshot(&legacy.store);
        let target = legacy.runs[2].to_string();
        let mut command = oneharness();
        command
            .args(["history", "show", &target, "--all-time"])
            .args(["--history-dir", &legacy.store.display().to_string()])
            .args(["--format", "json"]);
        let measured = measured::measure(command, Duration::from_secs(60));
        assert_eq!(json(&measured.output)[0]["history_id"], target);
        assert_eq!(snapshot(&legacy.store), before);
        assert!(
            measured.rchar
                > std::fs::metadata(legacy.store.join(".index.jsonl"))
                    .unwrap()
                    .len(),
            "the lookup read through the legacy index"
        );
        peaks.push(measured.max_rss_kib);
    }
    // 18 000 more 4 KiB entries — about 75 MiB more legacy index.
    assert!(
        peaks[1] <= peaks[0] + 4 * 1024,
        "peak RSS grew with the legacy index: {} KiB -> {} KiB",
        peaks[0],
        peaks[1]
    );
}

#[cfg(unix)]
#[test]
fn reindex_indexes_copied_in_sessions_appending_only_what_each_segment_lacks() {
    let source = dated_store("reindex-source", 100);
    let target = dated_store("reindex-target", 200);
    // The target's segments already hold entries for 2026-01-01, 2026-01-02
    // and today — the dates the copied runs land on.
    let target_slug = project_slug(&target.project);
    let copied_dir = target.store.join("copied-project");
    std::fs::create_dir_all(&copied_dir).unwrap();
    let source_slug = project_slug(&source.project);
    for entry in std::fs::read_dir(source.store.join(&source_slug)).unwrap() {
        let path = entry.unwrap().path();
        std::fs::copy(&path, copied_dir.join(path.file_name().unwrap())).unwrap();
    }
    // One copied-in file that cannot be read.
    let unreadable = copied_dir.join("unreadable-20260101T000000Z-7.jsonl");
    if is_root() {
        std::fs::create_dir(&unreadable).unwrap();
    } else {
        std::fs::write(&unreadable, "{}\n").unwrap();
        set_mode(&unreadable, 0o000);
    }
    let segments_before = segment_files(&target.store);
    let legacy_before = [".index.jsonl", ".event-index.jsonl"]
        .map(|name| std::fs::read(target.store.join(name)).unwrap());
    let sessions_before: BTreeMap<PathBuf, Vec<u8>> =
        [target.store.join(&target_slug), copied_dir.clone()]
            .iter()
            .flat_map(|dir| std::fs::read_dir(dir).unwrap())
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.is_file() && path != &unreadable)
            .map(|path| {
                let bytes = std::fs::read(&path).unwrap();
                (path, bytes)
            })
            .collect();
    for (id, _) in &source.runs {
        assert_eq!(
            history_verb(&target.store, &["show", &id.to_string()])
                .status
                .code(),
            Some(1)
        );
    }

    let report = json(&history_verb(&target.store, &["reindex"]));
    let after = segment_files(&target.store);
    // Each pre-existing segment begins with its old bytes, followed only by
    // the entries it lacked; the per-segment counts match what was appended.
    let mut appended_total = 0;
    for (name, bytes) in &after {
        let old = segments_before.get(name).cloned().unwrap_or_default();
        assert!(bytes.starts_with(&old), "{name} was rewritten");
        let added = entries_of(&bytes[old.len()..]);
        let reported = report["segments"]
            .as_array()
            .unwrap()
            .iter()
            .find(|segment| segment["segment"] == name.as_str())
            .map_or(0, |segment| segment["added"].as_u64().unwrap() as usize);
        assert_eq!(added.len(), reported, "{name}");
        appended_total += added.len();
        // No entry duplicated.
        let keys: Vec<String> = entries_of(bytes)
            .iter()
            .map(|entry| format!("{:?}", entry.key()))
            .collect();
        assert_eq!(
            keys.len(),
            keys.iter().collect::<BTreeSet<_>>().len(),
            "{name}"
        );
    }
    assert_eq!(report["entries_added"], appended_total);
    // Each copied run added a run entry and an event entry.
    assert_eq!(appended_total, 6);
    assert_eq!(
        report["unreadable"].as_array().unwrap().len(),
        1,
        "{report}"
    );
    assert_eq!(
        report["unreadable"][0]["path"],
        unreadable.display().to_string()
    );
    // Every copied run is found by id and listed by date.
    for (id, date) in &source.runs {
        let shown = json(&history_verb(&target.store, &["show", &id.to_string()]));
        assert_eq!(shown[0]["history_id"], id.to_string(), "{date}");
        let since = if *date == "today" {
            today().to_string()
        } else {
            date.to_string()
        };
        let listed = history_verb(
            &target.store,
            &["list", "--all-projects", "--since", &since],
        );
        assert!(listed_ids(&listed, &target.store).contains(id), "{date}");
    }
    // A second reindex adds nothing and leaves every segment byte-identical.
    let again = json(&history_verb(&target.store, &["reindex"]));
    assert_eq!(again["entries_added"], 0);
    assert_eq!(segment_files(&target.store), after);
    // No legacy index file or session file changed.
    assert_eq!(
        [".index.jsonl", ".event-index.jsonl"]
            .map(|name| std::fs::read(target.store.join(name)).unwrap()),
        legacy_before
    );
    for (path, bytes) in sessions_before {
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "{path:?}");
    }
    if !is_root() {
        set_mode(&unreadable, 0o644);
    }
}

#[cfg(target_os = "linux")]
#[test]
fn reindex_memory_does_not_grow_with_the_session_files_it_reads() {
    let mut peaks = Vec::new();
    for (tag, sessions) in [("small", 100u64), ("large", 1_000)] {
        let scratch = ScratchDir::new(&format!("hindex-reindex-mem-{tag}")).unwrap();
        let store = scratch.join("store");
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let project_dir = store.join("copied");
        std::fs::create_dir_all(&project_dir).unwrap();
        // Written directly in the session format: three events and a closing
        // line per file over three dates, then 1 MiB of hole — a whole-file read
        // would hold it.
        let template_scratch = ScratchDir::new(&format!("hindex-reindex-template-{tag}")).unwrap();
        let project = template_scratch.join("project");
        std::fs::create_dir_all(&project).unwrap();
        for n in 0..sessions {
            let id = id_at(now - (n % 3) * 86_400, n + 1);
            let writer = HistoryWriter::open(
                &template_scratch.join("store"),
                &project,
                &format!("mem-{n}"),
                HistoryLabels::default(),
            )
            .unwrap();
            for index in 0..3 {
                writer
                    .append_event(id, "codex", event(index, &"o".repeat(2048)))
                    .unwrap();
            }
            writer
                .append_streamed(
                    id,
                    PermissionMode::Default,
                    None,
                    "mem",
                    &finished_result("mem"),
                    &BTreeSet::from([0, 1, 2]),
                )
                .unwrap();
            let target = project_dir.join(format!("mem-{n}-20260101T000000Z-{n}.jsonl"));
            std::fs::rename(writer.path(), &target).unwrap();
            let len = std::fs::metadata(&target).unwrap().len();
            std::fs::OpenOptions::new()
                .write(true)
                .open(&target)
                .unwrap()
                .set_len(len + (1 << 20))
                .unwrap();
        }
        let mut command = oneharness();
        command
            .args([
                "history",
                "reindex",
                "--history-dir",
                &store.display().to_string(),
            ])
            .args(["--format", "json"]);
        let measured = measured::measure(command, Duration::from_secs(300));
        let report = json(&measured.output);
        assert_eq!(report["files_read"], sessions);
        assert_eq!(report["entries_added"], sessions * 4);
        peaks.push(measured.max_rss_kib);
    }
    assert!(
        peaks[1] <= peaks[0] + 8 * 1024,
        "reindex peak RSS grew with the store: {} KiB -> {} KiB",
        peaks[0],
        peaks[1]
    );
}

#[cfg(target_os = "linux")]
#[test]
fn reindex_memory_does_not_grow_with_the_sessions_one_utc_day_holds() {
    const EVENTS: usize = 10;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    // One session in the released session format, recorded through the
    // writer; every copy below is that file under a fresh id minted today, so
    // all of a store's entries land in today's two segments.
    let template_scratch = ScratchDir::new("hindex-reindex-day-template").unwrap();
    let project = template_scratch.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let template_id = id_at(now, 1);
    let writer = HistoryWriter::open(
        &template_scratch.join("store"),
        &project,
        "day",
        HistoryLabels::default(),
    )
    .unwrap();
    for index in 0..EVENTS {
        writer
            .append_event(template_id, "codex", event(index, "o"))
            .unwrap();
    }
    writer
        .append_streamed(
            template_id,
            PermissionMode::Default,
            None,
            "day",
            &finished_result("day"),
            &(0..EVENTS).collect(),
        )
        .unwrap();
    let template = std::fs::read_to_string(writer.path()).unwrap();
    let template_id = template_id.to_string();

    let mut peaks = Vec::new();
    for (tag, sessions) in [("small", 3_000u64), ("large", 30_000)] {
        let scratch = ScratchDir::new(&format!("hindex-reindex-day-{tag}")).unwrap();
        let store = scratch.join("store");
        let project_dir = store.join("copied");
        std::fs::create_dir_all(&project_dir).unwrap();
        for n in 0..sessions {
            let id = id_at(now, n + 2).to_string();
            std::fs::write(
                project_dir.join(format!("day-{n}-20260101T000000Z-{n}.jsonl")),
                template.replace(&template_id, &id),
            )
            .unwrap();
        }
        let mut command = oneharness();
        command
            .args([
                "history",
                "reindex",
                "--history-dir",
                &store.display().to_string(),
            ])
            .args(["--format", "json"]);
        let measured = measured::measure(command, Duration::from_secs(300));
        let report = json(&measured.output);
        assert_eq!(report["files_read"], sessions);
        assert_eq!(
            report["entries_added"],
            sessions * (EVENTS as u64 + 1),
            "{report}"
        );
        // Every entry went to one day: its runs and its events segment.
        let segments = report["segments"].as_array().unwrap();
        assert_eq!(segments.len(), 2, "{report}");
        let day = UtcDate::from_epoch_secs(now as i64);
        assert_eq!(segments[0]["segment"], format!("events-{day}.ndjson"));
        assert_eq!(segments[1]["segment"], format!("runs-{day}.ndjson"));
        peaks.push(measured.max_rss_kib);
    }
    assert!(
        peaks[1] <= peaks[0] + 8 * 1024,
        "reindex peak RSS grew with the sessions one day holds: {} KiB -> {} KiB",
        peaks[0],
        peaks[1]
    );
}
