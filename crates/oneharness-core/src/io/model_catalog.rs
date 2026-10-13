//! The shared Codex model catalog's I/O: probing the executable's version,
//! locking, refreshing the catalog file from one short-lived `codex app-server`,
//! and the failure fallback. The decisions are [`crate::domain::model_catalog`]'s.
//!
//! One catalog file per Codex home and Codex version lives at
//! `<state>/oneharness/codex-model-catalog/<key>.json`. A launch that finds it
//! younger than its interval uses it. One that finds it missing or stale takes
//! `<key>.lock`: the process holding it refreshes, and every other launch waits
//! for that one result, so a burst of launches costs one catalog fetch. The
//! refresh never calls the models endpoint itself — it starts Codex, which
//! fetches with its own credentials, and copies the cache Codex writes — so
//! oneharness never handles a ChatGPT credential. A failed refresh leaves
//! `<key>.failure.json`, and until its backoff ends every launch goes without a
//! fresh catalog rather than trying again.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use fs2::FileExt;

use crate::domain::harness::CODEX_ANALYTICS_OFF;
use crate::domain::model_catalog::{
    catalog_key, codex_version, is_fresh, read_codex_cache, CacheReading, FailureRecord,
    CODEX_MODELS_CACHE, MODEL_CATALOG_REFRESH_DEADLINE, MODEL_CATALOG_WAIT,
};
use crate::io::process::{resolve_program, Finish, Process};

/// How long `<bin> --version` may take before the launch goes without a
/// shared catalog.
const VERSION_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// How often a refresh re-reads Codex's cache, and a waiting launch retries
/// the lock.
const POLL: Duration = Duration::from_millis(25);

/// What `--print-command` shows in place of the catalog path: a dry run spawns
/// nothing, so it cannot ask the executable for the version the path is keyed
/// by.
pub const PLANNED_MODEL_CATALOG: &str = "<shared model catalog>";

/// One Codex launch the shared catalog is resolved for: the executable it
/// spawns and the environment it spawns it under, which is also what the
/// version probe and any refresh run under.
pub struct CatalogLaunch<'a> {
    pub bin: &'a str,
    pub cwd: Option<&'a Path>,
    /// Environment applied over the ambient one, last write winning.
    pub env: &'a [(String, String)],
    /// Variables removed after [`Self::env`].
    pub env_remove: &'a [String],
    /// The resolved `model_catalog_refresh_seconds`.
    pub refresh_interval: Duration,
}

/// The catalog a launch uses, and the one warning it prints when it goes
/// without a fresh one.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct CatalogChoice {
    /// The catalog file to pass as `model_catalog_json`; `None` leaves Codex to
    /// fetch its own.
    pub catalog: Option<PathBuf>,
    pub warning: Option<String>,
}

impl CatalogChoice {
    fn fresh(path: PathBuf) -> Self {
        Self {
            catalog: Some(path),
            warning: None,
        }
    }
}

/// The catalog this launch should read. Never fails a launch: anything that
/// keeps a fresh catalog out of reach is a fallback with a warning, and an
/// executable that does not report a Codex version, or a launch with no Codex
/// home to key on, simply goes without a shared catalog.
#[must_use]
pub fn resolve(launch: &CatalogLaunch<'_>) -> CatalogChoice {
    let Some(home) = codex_home(launch) else {
        return CatalogChoice::default();
    };
    let Some(version_output) = probe_version(launch) else {
        return CatalogChoice::default();
    };
    let Some(version) = codex_version(&version_output).map(str::to_string) else {
        return CatalogChoice::default();
    };
    let Some(root) = catalog_root() else {
        return CatalogChoice::default();
    };
    let files = CatalogFiles::new(
        &root,
        &catalog_key(&home.to_string_lossy(), &version_output),
    );
    if let Some(fresh) = files.fresh(launch.refresh_interval) {
        return CatalogChoice::fresh(fresh);
    }
    if let Some(choice) = files.backing_off() {
        return choice;
    }
    if let Err(error) = fs::create_dir_all(&root) {
        return files.fallback(&format!("cannot create {}: {error}", root.display()));
    }
    let lock = match OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&files.lock)
    {
        Ok(lock) => lock,
        Err(error) => {
            return files.fallback(&format!("cannot open {}: {error}", files.lock.display()))
        }
    };
    let waited_until = Instant::now() + MODEL_CATALOG_WAIT;
    loop {
        if lock.try_lock_exclusive().is_ok() {
            let choice = files.under_lock(launch, &home, &version);
            let _ = FileExt::unlock(&lock);
            return choice;
        }
        if Instant::now() >= waited_until {
            return files.waited_out();
        }
        std::thread::sleep(POLL);
    }
}

/// The three files of one catalog key.
struct CatalogFiles {
    catalog: PathBuf,
    lock: PathBuf,
    failure: PathBuf,
}

impl CatalogFiles {
    fn new(root: &Path, key: &str) -> Self {
        Self {
            catalog: root.join(format!("{key}.json")),
            lock: root.join(format!("{key}.lock")),
            failure: root.join(format!("{key}.failure.json")),
        }
    }

    /// The catalog file, when it exists and is younger than `interval`.
    fn fresh(&self, interval: Duration) -> Option<PathBuf> {
        let written = fs::metadata(&self.catalog).ok()?.modified().ok()?;
        // A file stamped in the future (a clock stepped back) is as fresh as
        // one written now.
        let age = SystemTime::now()
            .duration_since(written)
            .unwrap_or(Duration::ZERO);
        is_fresh(age, interval).then(|| self.catalog.clone())
    }

    /// The fallback while a recorded failure's backoff runs, if one does.
    fn backing_off(&self) -> Option<CatalogChoice> {
        let record: FailureRecord = serde_json::from_slice(&fs::read(&self.failure).ok()?).ok()?;
        record
            .backing_off(now_ms())
            .then(|| self.without_fresh(&record))
    }

    /// The launch's catalog with the lock held: another process may have
    /// refreshed (or failed to) while this one waited, so look again first.
    fn under_lock(&self, launch: &CatalogLaunch<'_>, home: &Path, version: &str) -> CatalogChoice {
        if let Some(fresh) = self.fresh(launch.refresh_interval) {
            return CatalogChoice::fresh(fresh);
        }
        if let Some(choice) = self.backing_off() {
            return choice;
        }
        match refresh(launch, home, version).and_then(|catalog| self.write(&catalog)) {
            Ok(()) => {
                let _ = fs::remove_file(&self.failure);
                CatalogChoice::fresh(self.catalog.clone())
            }
            Err(reason) => self.fallback(&reason),
        }
    }

    /// Record a failed refresh and fall back for this launch.
    fn fallback(&self, reason: &str) -> CatalogChoice {
        let record = FailureRecord {
            failed_at_ms: now_ms(),
            reason: reason.to_string(),
        };
        if let Ok(text) = serde_json::to_vec(&record) {
            let _ = write_atomically(&self.failure, &text);
        }
        self.without_fresh(&record)
    }

    /// The launch whose wait for another process's refresh ran out. That
    /// process records its own failure, so this one only falls back.
    fn waited_out(&self) -> CatalogChoice {
        let record = FailureRecord {
            failed_at_ms: now_ms(),
            reason: format!(
                "another launch's refresh gave no fresh catalog within {} s",
                MODEL_CATALOG_WAIT.as_secs()
            ),
        };
        self.without_fresh(&record)
    }

    /// The stale catalog when there is one, else none; with the warning.
    fn without_fresh(&self, record: &FailureRecord) -> CatalogChoice {
        let stale = self.catalog.is_file().then(|| self.catalog.clone());
        CatalogChoice {
            warning: Some(record.warning(stale.is_some())),
            catalog: stale,
        }
    }

    fn write(&self, catalog: &str) -> Result<(), String> {
        write_atomically(&self.catalog, catalog.as_bytes())
            .map_err(|error| format!("cannot write {}: {error}", self.catalog.display()))
    }
}

/// Write `bytes` to a sibling temp file and rename it over `path`, so a reader
/// never sees a half-written file.
fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    let written = File::create(&temp).and_then(|mut file| {
        file.write_all(bytes)?;
        file.sync_all()
    });
    match written.and_then(|()| fs::rename(&temp, path)) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&temp);
            Err(error)
        }
    }
}

/// Start the refresh helper — `codex app-server` with plugins and analytics
/// off and no catalog flag, under the launch's environment — send it
/// `initialize` and nothing else (no thread, so no session-start fetch races
/// it), and wait for Codex to write a catalog fetched after the start into its
/// cache. Returns the catalog file's contents, or why there are none.
fn refresh(launch: &CatalogLaunch<'_>, home: &Path, version: &str) -> Result<String, String> {
    let started_ms = now_ms();
    let deadline = Instant::now() + MODEL_CATALOG_REFRESH_DEADLINE;
    let mut command = launch_command(launch, &refresh_helper_args());
    command.stdin(Stdio::piped());
    let mut helper = Process::spawn(command)
        .map_err(|error| format!("cannot start `{} app-server`: {error}", launch.bin))?;
    let mut stdin = helper.take_stdin();
    if let Some(stdin) = stdin.as_mut() {
        let _ = writeln!(stdin, "{}", initialize_request());
        let _ = stdin.flush();
    }
    let cache = home.join(CODEX_MODELS_CACHE);
    let read_cache = || match fs::read_to_string(&cache) {
        Ok(text) => read_codex_cache(&text, started_ms, version),
        Err(_) => CacheReading::NotYet,
    };
    let outcome = loop {
        if let CacheReading::Fresh(catalog) = read_cache() {
            break Ok(catalog);
        }
        let exited = helper.wait_until(Instant::now()).ok().flatten();
        if let Some(status) = exited {
            // One last look: Codex may have written the cache on its way out.
            break match read_cache() {
                CacheReading::Fresh(catalog) => Ok(catalog),
                CacheReading::NotYet => Err(format!(
                    "the refresh `{} app-server` exited ({status}) before writing a fresh catalog",
                    launch.bin
                )),
            };
        }
        if Instant::now() >= deadline {
            break Err(format!(
                "Codex wrote no fresh catalog to {} within {} s",
                cache.display(),
                MODEL_CATALOG_REFRESH_DEADLINE.as_secs()
            ));
        }
        std::thread::sleep(POLL);
    };
    drop(stdin);
    helper.finish(Finish::Terminate);
    outcome
}

/// The refresh helper's arguments: plugins, apps and analytics off whatever
/// the config says (the helper loads no plugin, calls no connector and reports
/// nothing), and no catalog flag, so Codex fetches the catalog this refresh is
/// for.
#[must_use]
pub fn refresh_helper_args() -> Vec<String> {
    [
        "app-server",
        "--disable",
        "plugins",
        "--disable",
        "apps",
        "-c",
        CODEX_ANALYTICS_OFF,
        "--stdio",
    ]
    .map(str::to_string)
    .to_vec()
}

/// The JSON-RPC `initialize` the refresh helper is sent, the same request the
/// usage probe opens with.
fn initialize_request() -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "clientInfo": {
                "name": "oneharness",
                "title": null,
                "version": env!("CARGO_PKG_VERSION"),
            },
            "capabilities": null,
        },
    })
    .to_string()
}

/// `<bin> <args>` under the launch's working directory and environment, with
/// every pipe set up for [`Process::spawn`].
fn launch_command(launch: &CatalogLaunch<'_>, args: &[String]) -> Command {
    let mut command = Command::new(resolve_program(launch.bin));
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cwd) = launch.cwd {
        command.current_dir(cwd);
    }
    for (key, value) in launch.env {
        command.env(key, value);
    }
    for key in launch.env_remove {
        command.env_remove(key);
    }
    command
}

/// `<bin> --version`'s output, under the launch's environment; `None` when it
/// cannot be run, fails, or does not answer in time.
fn probe_version(launch: &CatalogLaunch<'_>) -> Option<String> {
    let process = Process::spawn(launch_command(launch, &["--version".to_string()])).ok()?;
    let mut process = process;
    let status = process
        .wait_until(Instant::now() + VERSION_PROBE_TIMEOUT)
        .ok()
        .flatten();
    let finished = process.finish(if status.is_some() {
        Finish::Exited
    } else {
        Finish::Terminate
    });
    status
        .filter(std::process::ExitStatus::success)
        .map(|_| finished.stdout)
}

/// The Codex home the launch reads: its `CODEX_HOME` (the launch's own
/// environment over the ambient one), else Codex's default `~/.codex`;
/// canonicalized when it exists, so two spellings of one home share a catalog.
fn codex_home(launch: &CatalogLaunch<'_>) -> Option<PathBuf> {
    let var = |name: &str| -> Option<String> {
        if launch.env_remove.iter().any(|key| key == name) {
            return None;
        }
        launch
            .env
            .iter()
            .rev()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
            .or_else(|| std::env::var(name).ok())
            .filter(|value| !value.is_empty())
    };
    let home = var("CODEX_HOME").map(PathBuf::from).or_else(|| {
        let user = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
        var(user).map(|home| PathBuf::from(home).join(".codex"))
    })?;
    Some(fs::canonicalize(&home).unwrap_or(home))
}

/// `<state>/oneharness/codex-model-catalog`.
fn catalog_root() -> Option<PathBuf> {
    crate::io::server_pool::state_dir()
        .map(|dir| dir.join("oneharness").join("codex-model-catalog"))
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
        })
}
