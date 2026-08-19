//! A temp directory that removes itself.

use std::ops::Deref;
use std::path::{Path, PathBuf};

/// A directory under the host temp directory, owned by the value that made it
/// and **removed when that value is dropped** — including when the scope
/// unwinds, so a failing or panicking test cleans up exactly like a passing one.
///
/// Every scratch directory in this workspace's suites goes through here. The
/// shape it replaces created the directory and cleared it on the way *in*, which
/// meant each tagged helper in each process left one behind for good: a single
/// host accumulated 108,234 of them and filled its root filesystem, taking every
/// program on it down with the suite. Clearing on entry is kept — a rerun still
/// starts from a known-empty directory — and the removal is what is new.
///
/// It is public, and always compiled, because there is no one compilation unit
/// that could hold it otherwise: the engine's own unit tests, the binary crate's
/// unit tests, and the integration-test binaries are three separate builds, and
/// a `#[cfg(test)]` item reaches only the first of them.
#[derive(Debug)]
pub struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    /// A scratch directory named `name` directly under [`std::env::temp_dir`].
    ///
    /// `name` is the whole directory name rather than a stem this decorates,
    /// because a scratch directory is shared with other processes and other
    /// threads: what makes it private is the caller's own tag plus whatever
    /// process/thread identity that caller already spells into it, and inventing
    /// a second scheme here would only make the two disagree.
    ///
    /// # Panics
    ///
    /// If the directory cannot be created. A test that cannot get scratch space
    /// has nothing to assert, so this is loud rather than deferred to the first
    /// confusing write failure inside it.
    #[must_use]
    pub fn new(name: &str) -> ScratchDir {
        ScratchDir::under(&std::env::temp_dir(), name)
    }

    /// The same, under an explicit `root` — for a caller whose path is also an
    /// address it has to budget (a unix socket under `/tmp`), rather than just a
    /// place to put files.
    ///
    /// # Panics
    ///
    /// If the directory cannot be created, as [`ScratchDir::new`] does.
    #[must_use]
    pub fn under(root: &Path, name: &str) -> ScratchDir {
        let path = root.join(name);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)
            .unwrap_or_else(|err| panic!("could not create the scratch directory {path:?}: {err}"));
        ScratchDir { path }
    }

    /// The directory itself.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
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

    #[test]
    fn the_directory_exists_while_it_is_held_and_is_gone_after() {
        let path = {
            let scratch = ScratchDir::new(&format!(
                "oneharness-scratch-drop-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
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
        let name = format!(
            "oneharness-scratch-panic-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        );
        let path = std::env::temp_dir().join(&name);
        let panicked = std::panic::catch_unwind(|| {
            let scratch = ScratchDir::new(&name);
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
        let name = format!(
            "oneharness-scratch-reuse-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        );
        let stale = ScratchDir::new(&name);
        std::fs::write(stale.join("stale.txt"), "from a previous run").unwrap();
        let fresh = ScratchDir::new(&name);
        assert!(!fresh.join("stale.txt").exists());
        assert!(fresh.path().is_dir());
    }

    #[test]
    fn an_explicit_root_is_honoured() {
        let outer = ScratchDir::new(&format!(
            "oneharness-scratch-root-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let inner = ScratchDir::under(outer.path(), "nested");
        assert_eq!(inner.path(), outer.join("nested"));
        assert!(inner.path().is_dir());
        // `AsRef<Path>` and `Deref` both address the same directory, so a caller
        // passes the guard itself wherever a path is wanted.
        assert!(std::fs::metadata(&inner).unwrap().is_dir());
    }
}
