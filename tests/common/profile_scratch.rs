//! Where a mock harness child's coverage profile goes: into scratch space a
//! fixture owns, never the target directory and never bare system temp.
//!
//! A harness child torn down after the TERM grace (which a loaded parallel
//! suite reaches often enough to be a race, not a property of one test) never
//! finishes its profile, and one truncated `.profraw` in the target directory
//! fails the whole `llvm-profdata merge` (#1400). So every suite that drives the
//! mock sends the child's profile elsewhere with `--env` (which reaches only the
//! harness process, so the binary under test keeps its own coverage). That
//! elsewhere is a [`ScratchDir`], removed when its owner drops — on a panic as
//! well — so the discarded profiles never outlive the fixture (#1431). A
//! forgotten owner leaves a prefixed directory, which
//! `scripts/check-temp-leaks.sh` refuses. Harmless when nothing is instrumented.
//!
//! One file shared by every suite that redirects: the in-process tier and the
//! history index (`tests/`), the binary journeys (`crates/oneharness-e2e`) and
//! the history compatibility test (`crates/history-compat`).
//!
//! `dead_code` is allowed because each binary uses the subset it needs.
#![allow(dead_code)]

use std::ops::Deref;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use oneharness_core::io::scratch::ScratchDir;

/// The profile's file name inside the owning directory; `%p` is the child's pid.
pub const PROFILE_PATTERN: &str = "killed-mock-%p.profraw";

/// The `LLVM_PROFILE_FILE=…` assignment sending a child's profile into `dir`,
/// which the caller owns and outlives the child.
pub fn profile_redirect(dir: &Path) -> String {
    format!("LLVM_PROFILE_FILE={}", dir.join(PROFILE_PATTERN).display())
}

/// A scratch directory of its own plus the redirect into it, for a helper with
/// no scratch to reuse. Hold it until the child it was handed to has ended; it
/// derefs to the assignment, so `&profiles` goes wherever an `--env` value does.
#[derive(Debug)]
pub struct ProfileScratch {
    dir: ScratchDir,
    redirect: String,
}

impl ProfileScratch {
    pub fn new() -> ProfileScratch {
        // Each guard its own directory: `ScratchDir` keys a tag by process only,
        // and clears an existing directory on the way in, so two tests in one
        // process sharing a tag would remove each other's.
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let tag = format!("killed-mock-{}", NEXT.fetch_add(1, Ordering::Relaxed));
        let dir = ScratchDir::new(&tag).expect("a scratch directory for mock profiles");
        let redirect = profile_redirect(&dir);
        ProfileScratch { dir, redirect }
    }

    pub fn dir(&self) -> &Path {
        self.dir.path()
    }

    pub fn as_str(&self) -> &str {
        &self.redirect
    }
}

impl Default for ProfileScratch {
    fn default() -> ProfileScratch {
        ProfileScratch::new()
    }
}

impl Deref for ProfileScratch {
    type Target = str;

    fn deref(&self) -> &str {
        &self.redirect
    }
}
