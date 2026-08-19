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

/// A directory under the host temp directory, owned by the value that made it
/// and **removed when that value is dropped** — including when the scope
/// unwinds, so a failing or panicking test cleans up exactly like a passing one.
///
/// It is also cleared on the way in, so a rerun starts from a known-empty
/// directory.
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
        let scratch = ScratchDir::new(&tag("prefix")).unwrap();
        assert!(scratch
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with(PREFIX));
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
