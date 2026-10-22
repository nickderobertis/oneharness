//! A temp directory that removes itself.

use std::io;
use std::ops::Deref;
use std::path::{Path, PathBuf};

/// The prefix every scratch directory's name carries.
///
/// Minted here rather than spelled by each caller, so the leak gate that sweeps
/// for abandoned scratch space (`scripts/check-temp-leaks.sh`) reads one source
/// and no caller can drift out of its reach.
pub const PREFIX: &str = "oneharness-";

/// The variable the leak gate hands every process of its run, naming that run.
///
/// Read, like [`PREFIX`], by `scripts/check-temp-leaks.sh` from this line, and
/// held in step with the Node and Python helpers' spellings by
/// `scripts/check-scratch-prefixes.sh`.
const RUN_ENV: &str = "OH_TEMP_LEAKS_RUN";

/// The file each scratch directory made under the leak gate carries, holding
/// that run's token: how the gate tells another live run's scratch from its own
/// without reading the maker's environment, which Windows does not expose.
const RUN_MARKER: &str = ".oh-temp-leaks-run";

/// A directory under the host temp directory, owned by the value that made it
/// and **removed when that value is dropped** — including when the scope
/// unwinds, so a failing or panicking test cleans up exactly like a passing one.
///
/// It is also cleared on the way in, so a rerun starts from a known-empty
/// directory — known-empty but for the leak gate's run marker, when one runs.
///
/// Public, and always compiled, because there is no one compilation unit that
/// could hold it otherwise: the engine's own unit tests, the binary crate's unit
/// tests, and the integration-test binaries are three separate builds, and a
/// `#[cfg(test)]` item reaches only the first.
#[derive(Debug)]
pub struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    /// A scratch directory for `tag` directly under [`std::env::temp_dir`].
    ///
    /// # Errors
    ///
    /// [`io::ErrorKind::InvalidInput`] for a `tag` carrying a path separator,
    /// and whatever creating the directory failed with otherwise.
    pub fn new(tag: &str) -> io::Result<ScratchDir> {
        ScratchDir::under(&std::env::temp_dir(), tag)
    }

    /// The same, under an explicit `root` — for a caller whose path is also an
    /// address it has to budget (a unix socket under `/tmp`), rather than just a
    /// place to put files. Pair it with [`ScratchDir::name`], which is what that
    /// caller measures before asking for the directory.
    ///
    /// # Errors
    ///
    /// As [`ScratchDir::new`].
    pub fn under(root: &Path, tag: &str) -> io::Result<ScratchDir> {
        let token = std::env::var_os(RUN_ENV).filter(|token| !token.is_empty());
        ScratchDir::make(root, tag, token.as_deref())
    }

    /// [`ScratchDir::under`], with the leak gate's run token passed rather than
    /// read, so a test can name a run without setting the process environment
    /// every other test reads.
    ///
    /// A marked directory is made under a name the gate does not sweep, marked
    /// there, and renamed into place. A sibling run's gate sweeping the same temp
    /// dir therefore never sees it unmarked. An unmarked directory would fall
    /// back to its maker's environment, which Windows cannot read, so the gate
    /// would count it as that run's own leak.
    fn make(root: &Path, tag: &str, token: Option<&std::ffi::OsStr>) -> io::Result<ScratchDir> {
        let name = checked_name(tag)?;
        let path = root.join(&name);
        retire(&path);
        let Some(token) = token else {
            std::fs::create_dir_all(&path)?;
            return Ok(ScratchDir { path });
        };
        let staged = root.join(format!(".{name}.new"));
        let _ = std::fs::remove_dir_all(&staged);
        std::fs::create_dir_all(&staged)?;
        let placed = std::fs::write(staged.join(RUN_MARKER), token.as_encoded_bytes())
            .and_then(|()| std::fs::rename(&staged, &path));
        if let Err(error) = placed {
            let _ = std::fs::remove_dir_all(&staged);
            return Err(error);
        }
        Ok(ScratchDir { path })
    }

    /// The directory name `tag` resolves to: [`PREFIX`], the tag, and the
    /// process that asked — so two processes running the same test do not share
    /// scratch space. A caller needing more separation than that (one thread per
    /// case, say) spells it into the tag.
    #[must_use]
    pub fn name(tag: &str) -> String {
        format!("{PREFIX}{tag}-{}", std::process::id())
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// [`ScratchDir::name`], refusing a tag that would make the result more than one
/// path component.
///
/// A separator is the whole attack surface here, and it matters because the
/// directory is recursively **removed**: `../..` as a tag would delete a
/// directory the caller never named. Nothing else can escape, because the name
/// is built by prepending [`PREFIX`] — so it is never absolute, never `.` and
/// never `..`, whatever the tag says.
fn checked_name(tag: &str) -> io::Result<String> {
    if tag.contains(['/', '\\']) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "a scratch tag names one directory, so it cannot contain a path separator: {tag:?}"
            ),
        ));
    }
    Ok(ScratchDir::name(tag))
}

impl Deref for ScratchDir {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.path
    }
}

impl AsRef<Path> for ScratchDir {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl Drop for ScratchDir {
    /// Best-effort: a directory that cannot be removed is not worth failing a
    /// run that has already produced its verdict, and on Windows a child process
    /// still holding a handle inside it would otherwise turn cleanup into a
    /// flake.
    fn drop(&mut self) {
        retire(&self.path);
    }
}

/// Remove a scratch directory without ever leaving it visible to the leak gate
/// unmarked: a sibling run's gate counts an unmarked directory as its own leak
/// on Windows, where it cannot read the maker's environment.
///
/// The directory is first renamed off the swept name, and only then emptied.
/// A marked directory that cannot be renamed (Windows refuses while a handle is
/// open inside) is left whole and marked, a leak its own run's gate still
/// reports. An unmarked one has no ownership to lose and is removed in place.
/// Its marker is removed first, so whatever a failed removal leaves is the same
/// in every order the filesystem lists entries. That remainder gets its
/// original marker back while still hidden, then returns to the swept name,
/// where its run's gate reports it.
fn retire(path: &Path) {
    let Some(name) = path.file_name() else {
        return;
    };
    let marker = std::fs::read(path.join(RUN_MARKER)).ok();
    let mut gone_name = std::ffi::OsString::from(".");
    gone_name.push(name);
    gone_name.push(".gone");
    let gone = path.with_file_name(gone_name);
    let _ = std::fs::remove_dir_all(&gone);
    if std::fs::rename(path, &gone).is_err() {
        if marker.is_none() {
            let _ = std::fs::remove_dir_all(path);
        }
        return;
    }
    let _ = std::fs::remove_file(gone.join(RUN_MARKER));
    if std::fs::remove_dir_all(&gone).is_ok() {
        return;
    }
    if let Some(marker) = marker {
        let _ = std::fs::write(gone.join(RUN_MARKER), marker);
    }
    let _ = std::fs::rename(&gone, path);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(what: &str) -> String {
        format!("scratch-{what}-{:?}", std::thread::current().id())
    }

    #[test]
    fn the_directory_exists_while_it_is_held_and_is_gone_after() {
        let path = {
            let scratch = ScratchDir::new(&tag("drop")).unwrap();
            std::fs::write(scratch.join("inside.txt"), "content").unwrap();
            assert!(scratch.path().is_dir());
            scratch.path().to_path_buf()
        };
        assert!(
            !path.exists(),
            "the scratch directory outlived its guard: {path:?}"
        );
    }

    #[test]
    fn a_panicking_scope_still_removes_its_directory() {
        // The case the old shape could never cover: a failing test unwinds, and
        // cleanup that lives at the end of the test body never runs.
        let tag = tag("panic");
        let path = std::env::temp_dir().join(ScratchDir::name(&tag));
        let panicked = std::panic::catch_unwind(|| {
            let scratch = ScratchDir::new(&tag).unwrap();
            std::fs::write(scratch.join("inside.txt"), "content").unwrap();
            panic!("the test this stands in for failed");
        });
        assert!(panicked.is_err(), "the scope must have unwound");
        assert!(
            !path.exists(),
            "an unwinding scope left its scratch directory behind: {path:?}"
        );
    }

    #[test]
    fn an_existing_directory_is_cleared_on_the_way_in() {
        // A rerun (or a crashed predecessor) must not leak state into the next
        // one, which is the one property of the old shape worth keeping.
        let tag = tag("reuse");
        let stale = ScratchDir::new(&tag).unwrap();
        std::fs::write(stale.join("stale.txt"), "from a previous run").unwrap();
        let fresh = ScratchDir::new(&tag).unwrap();
        assert!(!fresh.join("stale.txt").exists());
        assert!(fresh.path().is_dir());
    }

    #[test]
    fn the_name_carries_the_prefix_the_leak_gate_sweeps_for() {
        let name = ScratchDir::name("anything");
        assert!(name.starts_with(PREFIX), "{name}");
        // The gate also reads the maker's pid off the end of the name, to leave
        // another checkout's live run out of its verdict.
        assert!(
            name.ends_with(&format!("-{}", std::process::id())),
            "{name}"
        );
        let scratch = ScratchDir::new(&tag("prefix")).unwrap();
        assert!(scratch
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with(PREFIX));
    }

    #[test]
    fn a_directory_made_under_the_leak_gate_names_its_run() {
        // Every suite runs under `scripts/check-temp-leaks.sh`, which reads this
        // marker to tell the run's own scratch from a sibling's; outside it there
        // is no run to name, and nothing is written.
        let scratch = ScratchDir::new(&tag("marker")).unwrap();
        let marker = scratch.join(RUN_MARKER);
        match std::env::var_os(RUN_ENV).filter(|token| !token.is_empty()) {
            Some(token) => assert_eq!(
                std::fs::read(&marker).unwrap(),
                token.as_encoded_bytes(),
                "{marker:?}"
            ),
            None => assert!(!marker.exists(), "{marker:?}"),
        }
    }

    #[test]
    fn a_sibling_gate_never_sees_a_scratch_directory_without_its_marker() {
        // The gate on a sibling project sweeps the same temp dir while this run
        // makes and removes its scratch. On Windows it cannot read this
        // process's environment, so any swept directory it finds unmarked counts
        // as its own leak. That failed node-sdk:e2e on node-sdk:test's
        // directories. So a separate observer lists the root throughout, and
        // every swept name it sees must hold this run's marker while it exists.
        use std::sync::atomic::{AtomicBool, Ordering};
        const CYCLES: usize = 2000;
        let root = ScratchDir::new(&tag("observed")).unwrap();
        let token = std::ffi::OsStr::new("4242.rust-probe");
        let done = AtomicBool::new(false);
        let (seen, unmarked) = std::thread::scope(|scope| {
            let observer = scope.spawn(|| {
                let (mut seen, mut unmarked) = (0usize, Vec::new());
                while !done.load(Ordering::Acquire) {
                    for entry in std::fs::read_dir(root.path()).unwrap().flatten() {
                        if !entry.file_name().to_string_lossy().starts_with(PREFIX) {
                            continue;
                        }
                        seen += 1;
                        let marker = std::fs::read(entry.path().join(RUN_MARKER));
                        // Each cycle's name is its own, so a directory still
                        // there after a failed read is the one that was read.
                        if marker.as_deref().ok() != Some(token.as_encoded_bytes())
                            && entry.path().exists()
                        {
                            unmarked.push(entry.path());
                        }
                    }
                }
                (seen, unmarked)
            });
            for cycle in 0..CYCLES {
                drop(
                    ScratchDir::make(root.path(), &format!("churn-{cycle}"), Some(token)).unwrap(),
                );
            }
            done.store(true, Ordering::Release);
            observer.join().unwrap()
        });
        assert!(
            seen > 0,
            "the observer never overlapped a scratch directory"
        );
        assert!(
            unmarked.is_empty(),
            "{} of {seen} sightings were unmarked, as {:?}",
            unmarked.len(),
            unmarked.first()
        );
        // A directory that really is left behind is still this run's leak: it
        // stands under the swept name, marked for this run.
        let leaked = ScratchDir::make(root.path(), "leaked", Some(token)).unwrap();
        assert!(leaked
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(PREFIX));
        assert_eq!(
            std::fs::read(leaked.join(RUN_MARKER)).unwrap(),
            token.as_encoded_bytes()
        );
        assert_eq!(
            entries(root.path()),
            [leaked.file_name().unwrap()],
            "staged or retired names were left behind"
        );
    }

    /// Whether this user can be refused a write by a directory's permissions;
    /// a privileged one cannot, and then there is no failure to drive.
    #[cfg(unix)]
    fn permissions_bind(probe: &Path) -> bool {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir(probe).unwrap();
        std::fs::set_permissions(probe, std::fs::Permissions::from_mode(0o500)).unwrap();
        let bound = std::fs::write(probe.join("probe"), "").is_err();
        std::fs::set_permissions(probe, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::remove_dir_all(probe).unwrap();
        bound
    }

    /// What `root` holds besides the gate's marker for this run, if any.
    fn entries(root: &Path) -> Vec<std::ffi::OsString> {
        let mut names: Vec<_> = std::fs::read_dir(root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| name != RUN_MARKER)
            .collect();
        names.sort();
        names
    }

    #[cfg(unix)]
    #[test]
    fn a_partly_removed_scratch_directory_returns_to_the_sweep_with_its_marker() {
        // Removal renames a directory off the sweep and removes its marker
        // before anything else. If the rest cannot be removed, the remainder must
        // not stay hidden, nor come back unmarked. It must return still owned by
        // its run, which is what makes it that run's leak in the gate.
        use std::os::unix::fs::PermissionsExt;
        let root = ScratchDir::new(&tag("unremovable")).unwrap();
        if !permissions_bind(&root.join("probe")) {
            return;
        }
        let token = std::ffi::OsStr::new("4242.rust-unremovable");
        let stuck = ScratchDir::make(root.path(), "stuck", Some(token)).unwrap();
        let path = stuck.path().to_path_buf();
        let locked = path.join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::write(locked.join("file"), "held").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500)).unwrap();
        drop(stuck);
        let left = entries(root.path());
        let marker = std::fs::read(path.join(RUN_MARKER));
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(left, [path.file_name().unwrap()]);
        assert_eq!(marker.unwrap(), token.as_encoded_bytes());
    }

    #[cfg(unix)]
    #[test]
    fn a_marked_scratch_directory_that_cannot_be_renamed_is_left_whole() {
        // Removing it in place would take its marker first and leave it visible
        // unmarked, the race this module exists to close. So it stays exactly as
        // it was: under its swept name, owned by its run.
        use std::os::unix::fs::PermissionsExt;
        let outer = ScratchDir::new(&tag("unrenamable")).unwrap();
        if !permissions_bind(&outer.join("probe")) {
            return;
        }
        let root = outer.join("root");
        std::fs::create_dir(&root).unwrap();
        let token = std::ffi::OsStr::new("4242.rust-unrenamable");
        let stuck = ScratchDir::make(&root, "stuck", Some(token)).unwrap();
        let path = stuck.path().to_path_buf();
        std::fs::write(path.join("inside.txt"), "content").unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o500)).unwrap();
        drop(stuck);
        let left = entries(&root);
        let marker = std::fs::read(path.join(RUN_MARKER));
        let inside = path.join("inside.txt").exists();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(left, [path.file_name().unwrap()]);
        assert_eq!(marker.unwrap(), token.as_encoded_bytes());
        assert!(inside, "the directory was emptied in place");
    }

    #[cfg(unix)]
    #[test]
    fn an_unmarked_scratch_directory_that_cannot_be_renamed_is_removed_in_place() {
        // With no marker there is no ownership to lose, so removal goes on.
        use std::os::unix::fs::PermissionsExt;
        let outer = ScratchDir::new(&tag("unmarked-unrenamable")).unwrap();
        if !permissions_bind(&outer.join("probe")) {
            return;
        }
        let root = outer.join("root");
        std::fs::create_dir(&root).unwrap();
        let stuck = ScratchDir::make(&root, "stuck", None).unwrap();
        let path = stuck.path().to_path_buf();
        std::fs::write(path.join("inside.txt"), "content").unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o500)).unwrap();
        drop(stuck);
        let inside = path.join("inside.txt").exists();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(!inside, "an unmarked directory was not emptied");
    }

    #[cfg(unix)]
    #[test]
    fn a_marker_write_failure_returns_the_error_and_removes_the_directory() {
        const CHILD: &str = "OH_SCRATCH_WRITE_FAILURE_CHILD";
        if std::env::var_os(CHILD).is_some() {
            let tag = "marker-write-failure";
            let path = std::env::temp_dir().join(ScratchDir::name(tag));
            let error = ScratchDir::new(tag).expect_err("the file-size limit rejects the marker");
            assert_eq!(error.raw_os_error(), Some(27), "expected EFBIG: {error}");
            assert!(
                !path.exists(),
                "failed creation left scratch behind: {path:?}"
            );
            // The marker is written while the directory is staged off the sweep,
            // where a leftover would be invisible to the gate.
            let staged = std::env::temp_dir().join(format!(".{}.new", ScratchDir::name(tag)));
            assert!(
                !staged.exists(),
                "failed creation left staging behind: {staged:?}"
            );
            return;
        }

        // A real OS write failure, confined to a subprocess so neither the run
        // token nor the file-size limit can affect concurrently running tests.
        // Ignoring SIGXFSZ makes write return EFBIG instead of killing the child.
        let output = std::process::Command::new("bash")
            .args([
                "-c",
                "ulimit -f 0; trap '' XFSZ; exec \"$1\" --exact io::scratch::tests::a_marker_write_failure_returns_the_error_and_removes_the_directory --nocapture",
                "scratch-write-failure",
            ])
            .arg(std::env::current_exe().unwrap())
            .env(CHILD, "1")
            .env(RUN_ENV, "marker-write-failure-run")
            .env("LLVM_PROFILE_FILE", "/dev/null")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "child failed: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    }

    #[test]
    fn a_tag_cannot_escape_the_directory_it_names() {
        // The directory is recursively removed, so a tag that walked out of it
        // would delete something the caller never named. Refused, not sanitized.
        for escape in ["../..", "nested/inside", "back\\slash"] {
            let err = ScratchDir::new(escape).expect_err(escape);
            assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{escape}");
        }
        // The prefix is what makes everything else unrepresentable: a tag of
        // `..` still names a directory inside the root.
        let dots = ScratchDir::new("..").unwrap();
        assert_eq!(dots.parent().unwrap(), std::env::temp_dir());
    }

    #[test]
    fn an_explicit_root_is_honoured() {
        let outer = ScratchDir::new(&tag("root")).unwrap();
        let inner = ScratchDir::under(outer.path(), "nested").unwrap();
        assert_eq!(inner.path(), outer.join(ScratchDir::name("nested")));
        assert!(inner.path().is_dir());
        // `AsRef<Path>` and `Deref` both address the same directory, so a caller
        // passes the guard itself wherever a path is wanted.
        assert!(std::fs::metadata(&inner).unwrap().is_dir());
    }

    #[test]
    fn an_unusable_root_is_an_error_rather_than_a_panic() {
        let outer = ScratchDir::new(&tag("unusable")).unwrap();
        let file = outer.join("a-file");
        std::fs::write(&file, "not a directory").unwrap();
        assert!(ScratchDir::under(&file, "under-a-file").is_err());
    }
}
