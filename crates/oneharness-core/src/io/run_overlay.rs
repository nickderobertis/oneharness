//! The per-run overlay a run-level allowlist reaches a harness through.
//!
//! Some harnesses read their permission rules only from their own home
//! directory: Codex honours an exec-policy rule only from `$CODEX_HOME/rules/`
//! (`spike-command-grant`). Writing the run's rule there would grant every
//! session of that user; writing it nowhere would grant nothing. So a granted
//! run gets a directory of its own under oneharness's per-user state directory
//! ([`overlay_root`]) — outside the project and every harness home — and the
//! harness is pointed at a home built inside it whose entries link back to the
//! real home's. Not under the temp dir: codex-cli 0.159.2 refuses to create its
//! PATH helpers (`apply_patch`, its sandbox launcher) for a `CODEX_HOME` there,
//! which an ungranted run of the same home has.
//!
//! What makes the overlay the run's alone:
//!
//! - **It is removed when the run ends** — on success, failure, timeout, and an
//!   interrupt the run's signal handler turns into a cancellation — because a
//!   [`RunOverlay`] removes its directory when dropped.
//! - **An orphan is reclaimed.** A run killed before it could clean up (SIGKILL)
//!   leaves its directory behind with its owner lock released; the next
//!   `oneharness run` finds it with [`reclaim_orphans`], stops every process
//!   that still carries its [`MARKER_ENV`], returns what the harness recorded to
//!   the real home, and removes it. A live run's lock is held, so a run beside
//!   it is never touched.
//! - **What the harness records lands in the real home.** Every entry is a
//!   symlink, so sessions, logs, the login (`auth.json`, which Codex resolves
//!   through the link and renames over its target) and every other file are
//!   written in place. A file the harness creates or replaces at the home's top
//!   level is moved into the real home when the run ends (`CodexHome::sweep`).
//!
//! The one exception is Codex's thread index (`state_<n>.sqlite`). Its rows
//! store each rollout's absolute path, which Codex builds from `CODEX_HOME`
//! without resolving links, and codex-cli 0.159.2 refuses to resume a thread
//! whose stored path no longer exists ("no rollout found for thread id"). So
//! the overlay keeps its own index, seeded from one Codex builds over an empty
//! home (already backfilled, so it never rescans the real `sessions/` tree),
//! and after the turn [`RunOverlay::repair_threads`] asks a Codex app-server on the real
//! home to read each thread the run touched: finding no row, Codex locates the
//! rollout under the real `sessions/` and writes the row with its real path.

use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use fs2::FileExt;
use serde_json::{json, Value};
use wait_timeout::ChildExt;

use crate::domain::dialogue;
use crate::domain::grant::RUN_RULES_FILE;
use crate::io::marked_processes::{self, ProcessMarker};
use crate::io::scratch::{ScratchDir, PREFIX};

/// The environment variable every process an overlay's run starts carries,
/// whose value is the overlay directory's name: how [`reclaim_orphans`] finds
/// a killed run's harness, which may have outlived it.
pub const MARKER_ENV: &str = "ONEHARNESS_RUN_OVERLAY";

/// What `--print-command` shows in place of the overlay directory a run would
/// create: a dry run creates nothing on disk.
pub const PLANNED_OVERLAY: &str = "<run-overlay>";

/// The scratch tag every overlay directory's name opens with, after
/// [`PREFIX`].
const TAG: &str = "run-overlay-";

/// The path under oneharness's per-user state directory every overlay is made
/// in, one component per entry.
const ROOT_COMPONENTS: [&str; 2] = ["oneharness", "run-overlays"];

/// The file inside an overlay its owning run holds an exclusive lock on for
/// the run's lifetime.
const OWNER_LOCK: &str = "owner.lock";

/// The file recording the real Codex home an overlay's home links into, read
/// back when an orphan is reclaimed.
const REAL_HOME_FILE: &str = "real-codex-home";

/// Where a granted Codex's `CODEX_HOME` sits inside its overlay. A run builds
/// it here and reclaiming an orphan sweeps it from here, possibly in a later
/// oneharness, so both must name the same place.
const CODEX_HOME_DIR: &str = "codex-home";

/// How long an overlay with no owner lock yet may exist before it counts as
/// an orphan (a run killed between creating the directory and locking it).
const UNLOCKED_GRACE: Duration = Duration::from_secs(60);

/// How long one helper app-server exchange may take before it is abandoned:
/// a real Codex answers it in about 0.1-0.3 s, so a helper this slow is
/// stuck, and the turn it serves must not wait on it longer.
const HELPER_DEADLINE: Duration = Duration::from_secs(5);

/// One granted run's overlay directory: locked while it lives, removed when
/// dropped.
#[derive(Debug)]
pub struct RunOverlay {
    // Field order is drop order: the lock is released (and its file handle
    // closed, which Windows needs) before the directory is removed.
    _lock: fs::File,
    codex: Option<CodexHome>,
    dir: ScratchDir,
}

impl RunOverlay {
    /// Create and lock a fresh overlay directory.
    ///
    /// # Errors
    ///
    /// Creating the directory or its lock file failed.
    pub fn create() -> io::Result<RunOverlay> {
        RunOverlay::create_in(&overlay_root())
    }

    /// [`RunOverlay::create`], under an explicit root.
    ///
    /// # Errors
    ///
    /// As [`RunOverlay::create`].
    pub fn create_in(root: &Path) -> io::Result<RunOverlay> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let tag = format!("{TAG}{}{nanos:x}", NEXT.fetch_add(1, Ordering::Relaxed));
        create_private_dir(root)?;
        let dir = ScratchDir::under(root, &tag)?;
        let lock = fs::File::create(dir.join(OWNER_LOCK))?;
        lock.lock_exclusive()?;
        Ok(RunOverlay {
            _lock: lock,
            codex: None,
            dir,
        })
    }

    /// The overlay directory.
    #[must_use]
    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// The `(name, value)` every process of this run carries.
    #[must_use]
    pub fn marker(&self) -> (String, String) {
        (MARKER_ENV.to_string(), dir_name(self.dir.path()))
    }

    /// Build the Codex home this run is pointed at, linking every entry of
    /// `real` but its `rules/` and thread index, with `rules/` linking each of
    /// the real home's rule files beside one file holding `rules_text`.
    ///
    /// # Errors
    ///
    /// Reading the real home or creating a link failed.
    pub fn codex_home(&mut self, real: &Path, rules_text: &str) -> io::Result<PathBuf> {
        let home = self.dir.join(CODEX_HOME_DIR);
        fs::create_dir_all(&home)?;
        if let Some(real) = real.to_str() {
            fs::write(self.dir.join(REAL_HOME_FILE), real)?;
        }
        for entry in fs::read_dir(real)? {
            let name = entry?.file_name();
            let text = name.to_string_lossy();
            if text == "rules" || is_thread_index(&text) {
                continue;
            }
            symlink(&real.join(&name), &home.join(&name))?;
        }
        let rules = home.join("rules");
        fs::create_dir_all(&rules)?;
        // A home with no `rules/` has no user rule to carry. Any other failure
        // to read it, or a user file under the run's own rules file name,
        // stops the overlay: building one without every user rule file would
        // drop their `forbidden` rules while adding the run's allows.
        match fs::read_dir(real.join("rules")) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
            Ok(entries) => {
                for entry in entries {
                    let name = entry?.file_name();
                    let user = real.join("rules").join(&name);
                    if name == RUN_RULES_FILE {
                        return Err(io::Error::new(
                            io::ErrorKind::AlreadyExists,
                            format!(
                                "the Codex home's own `{}` has the name of the run's rules \
                                 file, so the overlay cannot carry it beside the run's rules",
                                user.display()
                            ),
                        ));
                    }
                    symlink(&user, &rules.join(&name))?;
                }
            }
        }
        fs::write(rules.join(RUN_RULES_FILE), rules_text)?;
        self.codex = Some(CodexHome {
            home: home.clone(),
            real: real.to_path_buf(),
        });
        Ok(home)
    }

    /// Seed the overlay home's thread index with one `bin` builds over an
    /// empty home, so the run never backfills the real `sessions/` tree. Only
    /// when the real home keeps an index. Every way a seed can fail — the
    /// template directory, the helper's exchange, reading or copying what it
    /// built — takes one path: a warning on stderr, no partial index in the
    /// home, and Codex indexing what it finds, slower but correct. The
    /// journey `a_helper_that_refuses_answers_badly_or_never_answers_leaves_the_turn_whole`
    /// drives that path.
    pub fn seed_thread_index(&self, launch: &HelperLaunch<'_>) {
        let Some(codex) = self.codex.as_ref().filter(|codex| codex.indexed()) else {
            return;
        };
        let template = self.dir.join("thread-index-seed");
        let seeded = self.build_seed(launch, &template, &codex.home);
        let _ = fs::remove_dir_all(&template);
        if let Err(error) = seeded {
            eprintln!(
                "oneharness: warning: could not build the run overlay's Codex thread index \
                 ({error}); Codex will index the home itself"
            );
        }
    }

    fn build_seed(
        &self,
        launch: &HelperLaunch<'_>,
        template: &Path,
        home: &Path,
    ) -> Result<(), String> {
        fs::create_dir_all(template).map_err(|error| error.to_string())?;
        exchange(
            launch,
            template,
            self.marker(),
            &[("thread/list", json!({}))],
        )?;
        copy_thread_index(template, home).map_err(|error| error.to_string())
    }

    /// After a granted Codex turn has ended, have a Codex app-server on the
    /// real home read each thread the run touched, so the real thread index
    /// holds it under its real rollout path. Best-effort: a thread Codex has no
    /// row for is still found by resume, which writes the row itself.
    pub fn repair_threads(&self, launch: &HelperLaunch<'_>, threads: &[String]) {
        let Some(codex) = self.codex.as_ref().filter(|codex| codex.indexed()) else {
            return;
        };
        if threads.is_empty() {
            return;
        }
        let requests: Vec<(&str, Value)> = threads
            .iter()
            .map(|thread| ("thread/read", json!({ "threadId": thread })))
            .collect();
        if let Err(error) = exchange(launch, &codex.real, self.marker(), &requests) {
            eprintln!(
                "oneharness: warning: could not record the granted run's Codex thread in the \
                 real home's index ({error}); resuming it still finds it"
            );
        }
    }

    /// Return what the harness created or replaced at the overlay home's top
    /// level (and in its `rules/`) to the real home now, while the overlay
    /// stays. Dropping the overlay sweeps again, which finds nothing new.
    pub fn sweep_now(&self) {
        if let Some(codex) = &self.codex {
            codex.sweep();
        }
    }

    /// Return what the harness recorded at the home's top level to the real
    /// home, then remove the overlay. Dropping the overlay does the same, but
    /// leaves the shared root behind.
    pub fn finish(self) {
        self.retire();
    }
}

impl Drop for RunOverlay {
    fn drop(&mut self) {
        if let Some(codex) = &self.codex {
            codex.sweep();
        }
    }
}

impl RunOverlay {
    /// Remove the overlay now, and with it the shared root if no other run
    /// holds one there. The `ScratchDir` field would remove the directory
    /// anyway as it drops; the root can only follow once it has.
    fn retire(self) {
        let root = self.dir.path().parent().map(Path::to_path_buf);
        drop(self);
        if let Some(root) = root {
            remove_root_if_empty(&root);
        }
    }
}

/// How a helper Codex app-server is launched: the run's own executable and
/// launch settings, its environment, and its working directory.
#[derive(Debug, Clone)]
pub struct HelperLaunch<'a> {
    pub bin: &'a str,
    /// The argv after the executable (`app-server` and its launch settings).
    pub args: &'a [String],
    pub env: &'a [(String, String)],
    pub env_remove: &'a [String],
    pub cwd: Option<&'a Path>,
}

#[derive(Debug)]
struct CodexHome {
    home: PathBuf,
    real: PathBuf,
}

impl CodexHome {
    /// Whether the real home keeps a thread index. One that does not has
    /// nothing to seed from or repair: Codex indexes it on its next run, as it
    /// would after an ungranted one.
    fn indexed(&self) -> bool {
        fs::read_dir(&self.real).is_ok_and(|entries| {
            entries
                .flatten()
                .any(|entry| is_thread_index(&entry.file_name().to_string_lossy()))
        })
    }

    /// Move every entry Codex created or replaced (a regular file or
    /// directory where a link was, or a new name) into the real home. The
    /// overlay's own thread index and run rules stay behind.
    fn sweep(&self) {
        sweep_dir(&self.home, &self.real, &|name| {
            name == "rules" || is_thread_index(name)
        });
        sweep_dir(
            &self.home.join("rules"),
            &self.real.join("rules"),
            &|name| name == RUN_RULES_FILE,
        );
    }
}

fn sweep_dir(from: &Path, to: &Path, keep: &dyn Fn(&str) -> bool) {
    let Ok(entries) = fs::read_dir(from) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if keep(&name.to_string_lossy()) {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        if let Err(error) =
            fs::create_dir_all(to).and_then(|()| move_into(&entry.path(), &to.join(&name)))
        {
            eprintln!(
                "oneharness: warning: could not return `{}` from the run overlay to `{}`: {error}",
                entry.path().display(),
                to.display()
            );
        }
    }
}

/// Move `from` to `to`, replacing a file there and merging into a directory
/// there, across filesystems when a rename cannot.
fn move_into(from: &Path, to: &Path) -> io::Result<()> {
    let to_meta = fs::symlink_metadata(to).ok();
    if from.is_dir() {
        if to_meta.as_ref().is_some_and(fs::Metadata::is_dir) {
            for entry in fs::read_dir(from)? {
                let entry = entry?;
                move_into(&entry.path(), &to.join(entry.file_name()))?;
            }
            return fs::remove_dir_all(from);
        }
    } else if to_meta.as_ref().is_some_and(fs::Metadata::is_dir) {
        return Err(io::Error::other("a directory is in the way"));
    }
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    if from.is_dir() {
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            move_into(&entry.path(), &to.join(entry.file_name()))?;
        }
        fs::remove_dir_all(from)
    } else {
        fs::copy(from, to)?;
        fs::remove_file(from)
    }
}

/// Whether `name` is part of Codex's thread index (`state_<n>.sqlite` and its
/// SQLite companions), which an overlay keeps for itself.
#[must_use]
pub fn is_thread_index(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("state_") else {
        return false;
    };
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 {
        return false;
    }
    matches!(
        &rest[digits..],
        ".sqlite" | ".sqlite-wal" | ".sqlite-shm" | ".sqlite-journal"
    )
}

/// Where every run's overlay directory is made: oneharness's per-user state
/// directory (`$XDG_STATE_HOME`, else `~/.local/state`; `%LOCALAPPDATA%` on
/// Windows) under `oneharness/run-overlays`, else the host temp dir when the
/// platform names none.
#[must_use]
pub fn overlay_root() -> PathBuf {
    crate::io::server_pool::state_dir().map_or_else(std::env::temp_dir, |dir| {
        ROOT_COMPONENTS
            .iter()
            .fold(dir, |path, part| path.join(part))
    })
}

fn create_private_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Reclaim every overlay whose run is gone.
///
/// Called as a run starts. Returns the overlays it removed.
pub fn reclaim_orphans() -> Vec<PathBuf> {
    let root = overlay_root();
    let reclaimed = reclaim_under(&root);
    remove_root_if_empty(&root);
    reclaimed
}

/// The shared root is the last thing a run's overlay leaves; once no run
/// holds one it goes too. Best-effort: a sibling run that has just made its
/// overlay keeps it (removing a non-empty directory fails).
fn remove_root_if_empty(root: &Path) {
    if root != std::env::temp_dir() {
        let _ = fs::remove_dir(root);
    }
}

fn reclaim_under(root: &Path) -> Vec<PathBuf> {
    let prefix = format!("{PREFIX}{TAG}");
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut reclaimed = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(&prefix) || !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let dir = entry.path();
        let lock = match fs::OpenOptions::new()
            .write(true)
            .open(dir.join(OWNER_LOCK))
        {
            Ok(lock) => lock,
            // No lock file: a run killed between making its directory and
            // locking it. Proven by `only_an_overlay_whose_owner_is_gone_is_
            // reclaimed` rather than end to end, because that window is the
            // few instructions inside `RunOverlay::create`, which no signal a
            // journey sends through the CLI can be aimed at.
            Err(_) => {
                let young = entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .is_none_or(|age| age < UNLOCKED_GRACE);
                if young {
                    continue;
                }
                reclaim(&dir, &name);
                if !dir.exists() {
                    reclaimed.push(dir);
                }
                continue;
            }
        };
        if lock.try_lock_exclusive().is_err() {
            continue;
        }
        reclaim(&dir, &name);
        drop(lock);
        let _ = fs::remove_dir_all(&dir);
        if !dir.exists() {
            reclaimed.push(dir);
        }
    }
    reclaimed
}

fn reclaim(dir: &Path, name: &str) {
    let marker = ProcessMarker {
        name: MARKER_ENV.to_string(),
        value: name.to_string(),
    };
    let _ = marked_processes::stop_marked(&marker, Duration::from_secs(2));
    if let Some(real) = recorded_real_home(dir) {
        CodexHome {
            home: dir.join(CODEX_HOME_DIR),
            real,
        }
        .sweep();
    }
    let _ = fs::remove_dir_all(dir);
}

/// The real home an orphan recorded, only when the record is one a sweep may
/// write into: an absolute path to a directory that every link of the
/// orphan's home points into. Anything else — a corrupt or foreign record —
/// returns nothing to anywhere, and the orphan is only removed.
///
/// Proven by the unit test beside it rather than end to end: a corrupt record
/// arises only when something outside oneharness rewrites a file inside its
/// private state directory, which no interface of oneharness can produce, so a
/// journey would have to forge the same file this test writes.
fn recorded_real_home(dir: &Path) -> Option<PathBuf> {
    let recorded = PathBuf::from(fs::read_to_string(dir.join(REAL_HOME_FILE)).ok()?);
    if !recorded.is_absolute() {
        return None;
    }
    // Containment on resolved paths: a `..` or a further link in a target
    // cannot pass for the home lexically and resolve outside it.
    let real = fs::canonicalize(&recorded)
        .ok()
        .filter(|real| real.is_dir())?;
    let mut links = 0;
    for entry in fs::read_dir(dir.join(CODEX_HOME_DIR)).ok()?.flatten() {
        if entry.file_type().is_ok_and(|kind| kind.is_symlink()) {
            let target = fs::canonicalize(fs::read_link(entry.path()).ok()?).ok()?;
            if !target.starts_with(&real) {
                return None;
            }
            links += 1;
        }
    }
    (links > 0).then_some(recorded)
}

fn dir_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(unix)]
fn symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(not(unix))]
fn symlink(_target: &Path, _link: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "a run overlay links into the real home, which this platform cannot do without privilege",
    ))
}

/// Whether this platform can build an overlay home of links.
#[must_use]
pub fn links_supported() -> bool {
    cfg!(unix)
}

/// Copy every thread-index file at `template`'s top level into `home`. A copy
/// that fails part-way removes every destination it reached — the one whose
/// copy failed included, which `fs::copy` may already have created or
/// truncated — so `home` holds the whole index or none of it. The unit tests
/// beside it prove that, since no journey can make one copy within the
/// overlay fail and not the others.
fn copy_thread_index(template: &Path, home: &Path) -> io::Result<()> {
    let mut copied = Vec::new();
    let result = (|| {
        let mut names = Vec::new();
        for entry in fs::read_dir(template)? {
            let name = entry?.file_name();
            if is_thread_index(&name.to_string_lossy()) {
                names.push(name);
            }
        }
        // In name order, the database before its companions.
        names.sort();
        for name in names {
            let to = home.join(&name);
            // Registered before the copy, which can fail after creating it.
            copied.push(to.clone());
            fs::copy(template.join(&name), &to)?;
        }
        Ok(())
    })();
    if result.is_err() {
        for path in &copied {
            let _ = fs::remove_file(path);
        }
    }
    result
}

/// Start a Codex app-server on `home`, initialize it, send `requests` in turn
/// and wait for each reply, then close its stdin and wait for it to exit.
fn exchange(
    launch: &HelperLaunch<'_>,
    home: &Path,
    marker: (String, String),
    requests: &[(&str, Value)],
) -> Result<(), String> {
    let mut command = Command::new(launch.bin);
    command
        .args(launch.args)
        .envs(launch.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .env("CODEX_HOME", home)
        .env(&marker.0, &marker.1)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for name in launch.env_remove {
        if name != "CODEX_HOME" {
            command.env_remove(name);
        }
    }
    if let Some(cwd) = launch.cwd {
        command.current_dir(cwd);
    }
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let mut stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let (tx, rx) = mpsc::channel::<Value>();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if let Ok(value) = serde_json::from_str::<Value>(&line) {
                if tx.send(value).is_err() {
                    break;
                }
            }
        }
    });
    let deadline = Instant::now() + HELPER_DEADLINE;
    let send = |stdin: &mut std::process::ChildStdin,
                id: u64,
                method: &str,
                frame: String|
     -> Result<(), String> {
        stdin
            .write_all(frame.as_bytes())
            .map_err(|e| e.to_string())?;
        stdin.flush().map_err(|e| e.to_string())?;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let reply = rx
                .recv_timeout(left)
                .map_err(|_| format!("no reply to `{method}`"))?;
            if reply.get("id").and_then(Value::as_u64) == Some(id) {
                return match (reply.get("error"), reply.get("result")) {
                    (Some(error), _) => Err(format!("`{method}` failed: {error}")),
                    (None, Some(result)) if result.is_object() => Ok(()),
                    (None, other) => Err(format!(
                        "`{method}` answered with no result object: {}",
                        other.map_or_else(|| "nothing".to_string(), Value::to_string)
                    )),
                };
            }
        }
    };
    let mut outcome = send(&mut stdin, 0, "initialize", dialogue::codex_initialize(0));
    if outcome.is_ok() {
        let _ = stdin.write_all(dialogue::codex_initialized().as_bytes());
        for (id, (method, params)) in (1..).zip(requests) {
            let frame = dialogue::request(id, method, params.clone());
            if let Err(error) = send(&mut stdin, id, method, frame) {
                outcome = Err(error);
                break;
            }
        }
    }
    drop(stdin);
    let waited = child.wait_timeout(Duration::from_secs(5)).ok().flatten();
    if waited.is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The live-control suite finds overlays (and their processes) by restating
    /// this module's marker, directory prefix and root path; it runs only in
    /// the paid live tier, so this gated test is what catches the two drifting.
    #[test]
    fn the_live_suites_overlay_detector_restates_the_engines_layout() {
        let suite = include_str!("../../../../live/control/run_allowlist_live.py");
        let root = ROOT_COMPONENTS.map(|part| format!("\"{part}\"")).join(", ");
        for line in [
            format!("OVERLAY_MARKER = \"{}\"", MARKER_ENV),
            format!("OVERLAY_PREFIX = \"{}{}\"", PREFIX, TAG),
            format!("OVERLAY_ROOT = ({root})"),
        ] {
            assert!(
                suite.lines().any(|l| l == line),
                "live/control/run_allowlist_live.py no longer has `{line}`: update its overlay detector to the engine's layout"
            );
        }
    }

    #[test]
    fn the_thread_index_is_the_state_database_and_its_companions_only() {
        for name in [
            "state_5.sqlite",
            "state_5.sqlite-wal",
            "state_5.sqlite-shm",
            "state_12.sqlite-journal",
        ] {
            assert!(is_thread_index(name), "{name}");
        }
        for name in [
            "state.sqlite",
            "state_x.sqlite",
            "state_5.sqlite.bak",
            "thread_history_1.sqlite",
            "logs_2.sqlite",
            "sessions",
        ] {
            assert!(!is_thread_index(name), "{name}");
        }
    }

    #[test]
    fn a_thread_index_copy_that_fails_part_way_leaves_no_partial_index() {
        let scratch = ScratchDir::new("seed-copy").unwrap();
        let (template, home) = (scratch.join("template"), scratch.join("home"));
        fs::create_dir_all(&template).unwrap();
        fs::create_dir_all(&home).unwrap();
        fs::write(template.join("state_5.sqlite"), "db").unwrap();
        fs::write(template.join("state_5.sqlite-shm"), "shm").unwrap();
        fs::write(template.join("logs_2.sqlite"), "not the index").unwrap();
        copy_thread_index(&template, &home).unwrap();
        let mut names: Vec<String> = fs::read_dir(&home)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["state_5.sqlite", "state_5.sqlite-shm"]);
        // The last companion in name order cannot be copied (a directory), so
        // the database and `-shm` were copied first: none is left behind.
        let home = scratch.join("home-2");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(template.join("state_5.sqlite-wal")).unwrap();
        assert!(copy_thread_index(&template, &home).is_err());
        assert_eq!(fs::read_dir(&home).unwrap().count(), 0);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_thread_index_copy_that_fails_after_opening_its_destination_removes_it() {
        // The `-wal` destination is a link to `/dev/full`: `fs::copy` opens it
        // for writing, then its first write fails with ENOSPC — a copy that
        // reached its destination and failed, as on a full disk.
        let scratch = ScratchDir::new("seed-copy-enospc").unwrap();
        let (template, home) = (scratch.join("template"), scratch.join("home"));
        fs::create_dir_all(&template).unwrap();
        fs::create_dir_all(&home).unwrap();
        fs::write(template.join("state_5.sqlite"), "db").unwrap();
        fs::write(template.join("state_5.sqlite-wal"), "wal").unwrap();
        std::os::unix::fs::symlink("/dev/full", home.join("state_5.sqlite-wal")).unwrap();
        let error = copy_thread_index(&template, &home).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(28), "{error}"); // ENOSPC
        let left: Vec<_> = fs::read_dir(&home)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert!(left.is_empty(), "{left:?}");
    }

    #[cfg(unix)]
    #[test]
    fn an_overlay_home_links_the_real_home_and_returns_what_codex_wrote() {
        let real = ScratchDir::new("run-overlay-test-real").unwrap();
        fs::create_dir_all(real.join("sessions")).unwrap();
        fs::write(real.join("auth.json"), "{}").unwrap();
        fs::write(real.join("config.toml"), "a = 1\n").unwrap();
        fs::write(real.join("state_5.sqlite"), "real index").unwrap();
        fs::create_dir_all(real.join("rules")).unwrap();
        fs::write(real.join("rules").join("user.rules"), "# user\n").unwrap();

        let root = ScratchDir::new("run-overlay-test-overlays").unwrap();
        let mut overlay = RunOverlay::create_in(&root.join("run-overlays")).unwrap();
        let dir = overlay.path().to_path_buf();
        let home = overlay.codex_home(real.path(), "# run\n").unwrap();
        assert!(fs::symlink_metadata(home.join("auth.json"))
            .unwrap()
            .is_symlink());
        assert!(fs::symlink_metadata(home.join("sessions"))
            .unwrap()
            .is_symlink());
        assert!(!home.join("state_5.sqlite").exists());
        assert!(fs::symlink_metadata(home.join("rules").join("user.rules"))
            .unwrap()
            .is_symlink());
        assert_eq!(
            fs::read_to_string(home.join("rules").join(RUN_RULES_FILE)).unwrap(),
            "# run\n"
        );
        // What Codex would do during a turn: write through a link, replace a
        // linked file, create a new top-level file and its own index.
        fs::write(home.join("sessions").join("rollout.jsonl"), "r").unwrap();
        fs::remove_file(home.join("config.toml")).unwrap();
        fs::write(home.join("config.toml"), "a = 2\n").unwrap();
        fs::write(home.join("session_index.jsonl"), "i").unwrap();
        fs::write(home.join("state_5.sqlite"), "overlay index").unwrap();
        // A directory Codex created, and one it replaced a link with while
        // the real home gained the same name: the first moves, the second
        // merges into the real one.
        fs::create_dir_all(home.join("created").join("deep")).unwrap();
        fs::write(home.join("created").join("deep").join("f"), "c").unwrap();
        fs::create_dir_all(real.join("both")).unwrap();
        fs::write(real.join("both").join("kept"), "k").unwrap();
        fs::create_dir_all(home.join("both")).unwrap();
        fs::write(home.join("both").join("added"), "a").unwrap();

        overlay.finish();
        assert!(!dir.exists(), "the overlay is removed");
        assert!(
            !root.join("run-overlays").exists(),
            "an emptied root goes with it"
        );
        assert_eq!(
            fs::read_to_string(real.join("sessions").join("rollout.jsonl")).unwrap(),
            "r"
        );
        assert_eq!(
            fs::read_to_string(real.join("config.toml")).unwrap(),
            "a = 2\n"
        );
        assert_eq!(
            fs::read_to_string(real.join("session_index.jsonl")).unwrap(),
            "i"
        );
        assert_eq!(
            fs::read_to_string(real.join("state_5.sqlite")).unwrap(),
            "real index"
        );
        assert!(!real.join("rules").join(RUN_RULES_FILE).exists());
        assert_eq!(fs::read_to_string(real.join("auth.json")).unwrap(), "{}");
        assert_eq!(
            fs::read_to_string(real.join("created/deep/f")).unwrap(),
            "c"
        );
        assert_eq!(fs::read_to_string(real.join("both/kept")).unwrap(), "k");
        assert_eq!(fs::read_to_string(real.join("both/added")).unwrap(), "a");
    }

    #[cfg(unix)]
    #[test]
    fn an_orphans_recorded_home_is_trusted_only_when_its_links_agree() {
        let root = ScratchDir::new("run-overlay-test-recorded").unwrap();
        let real = root.join("real");
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("auth.json"), "{}").unwrap();
        let orphan = root.join("orphan");
        fs::create_dir_all(orphan.join(CODEX_HOME_DIR)).unwrap();
        symlink(
            &real.join("auth.json"),
            &orphan.join(CODEX_HOME_DIR).join("auth.json"),
        )
        .unwrap();
        let record = |text: &str| fs::write(orphan.join(REAL_HOME_FILE), text).unwrap();
        record(real.to_str().unwrap());
        assert_eq!(recorded_real_home(&orphan), Some(real.clone()));
        record("relative/home");
        assert_eq!(recorded_real_home(&orphan), None);
        record(root.join("missing").to_str().unwrap());
        assert_eq!(recorded_real_home(&orphan), None);
        let elsewhere = root.join("elsewhere");
        fs::create_dir_all(&elsewhere).unwrap();
        record(elsewhere.to_str().unwrap());
        assert_eq!(
            recorded_real_home(&orphan),
            None,
            "its links point elsewhere"
        );
        // A target that names the home lexically but resolves out of it.
        let sneaky = orphan.join(CODEX_HOME_DIR).join("sneaky");
        symlink(&real.join("..").join("elsewhere"), &sneaky).unwrap();
        record(real.to_str().unwrap());
        assert_eq!(
            recorded_real_home(&orphan),
            None,
            "a `..` target escapes the home"
        );
    }

    #[cfg(unix)]
    #[test]
    fn only_an_overlay_whose_owner_is_gone_is_reclaimed() {
        let root = ScratchDir::new("run-overlay-test-root").unwrap();
        let live = root.join(format!("{PREFIX}{TAG}live"));
        fs::create_dir_all(&live).unwrap();
        let held = fs::File::create(live.join(OWNER_LOCK)).unwrap();
        held.lock_exclusive().unwrap();
        let dead = root.join(format!("{PREFIX}{TAG}dead"));
        fs::create_dir_all(dead.join(CODEX_HOME_DIR)).unwrap();
        fs::File::create(dead.join(OWNER_LOCK)).unwrap();
        let fresh = root.join(format!("{PREFIX}{TAG}fresh"));
        fs::create_dir_all(&fresh).unwrap();
        let other = root.join(format!("{PREFIX}something-else"));
        fs::create_dir_all(&other).unwrap();
        // A run killed between making its directory and locking it: no lock,
        // and older than the grace a run takes to lock one.
        let stale = root.join(format!("{PREFIX}{TAG}stale"));
        fs::create_dir_all(&stale).unwrap();
        fs::File::open(&stale)
            .unwrap()
            .set_modified(SystemTime::now() - UNLOCKED_GRACE - Duration::from_secs(5))
            .unwrap();

        let mut reclaimed = reclaim_under(root.path());
        reclaimed.sort();
        assert_eq!(reclaimed, [dead.clone(), stale.clone()]);
        assert!(!dead.exists() && !stale.exists());
        assert!(live.exists() && fresh.exists() && other.exists());
        drop(held);
    }
}
