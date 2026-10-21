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
        let path = root.join(checked_name(tag)?);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)?;
        // Held before it is marked, so a marker that cannot be written still
        // gives the directory back.
        let scratch = ScratchDir { path };
        if let Some(token) = std::env::var_os(RUN_ENV).filter(|token| !token.is_empty()) {
            std::fs::write(scratch.join(RUN_MARKER), token.as_encoded_bytes())?;
        }
        Ok(scratch)
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
        let _ = std::fs::remove_dir_all(&self.path);
    }
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
