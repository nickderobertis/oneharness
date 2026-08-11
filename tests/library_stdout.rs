//! Proof that the engine's run path writes nothing to the caller's stdout.
//!
//! Alone in its own test binary on purpose: the only way to ask "did that call
//! print anything?" of code in your own process is to redirect fd 1 across it,
//! and any *other* test finishing inside that window would have libtest's own
//! progress line land in the capture. One test per process makes the window
//! clean under `cargo test` as well as under nextest.
//!
//! Unix-only as a whole file (rather than per item), because redirecting a file
//! descriptor is: leaving the imports behind on Windows would only be an
//! unused-import warning, which this workspace treats as an error.
#![cfg(unix)]

use oneharness_core::domain::report::Status;
use oneharness_core::io::run::{run, RunControls};

#[path = "support/library_fixture.rs"]
mod fixture;
use fixture::request;

#[test]
fn a_library_caller_gets_the_report_back_without_the_engine_printing_it() {
    // The whole point of the library entry point: the report is a *value*, and
    // getting it costs the caller nothing on its own stdout — which a consumer
    // like onejudge cannot lend out, because its stdout is its own contract.
    // Proven by redirecting this process's fd 1 to a file across the call and
    // asserting the file stayed empty while the report came back in full.
    let stdout = concat!(
        r#"{"type":"text","part":{"type":"text","text":"working"}}"#,
        "\n",
        r#"{"type":"tool_use","part":{"type":"tool","tool":"bash","state":{"status":"completed","input":{"command":"echo hi"},"output":"hi"}}}"#,
        "\n",
    );
    let request = request("opencode", &[("MOCK_STDOUT", stdout)]);

    let captured = std::env::temp_dir().join(format!("oh-lib-stdout-{}.txt", std::process::id()));
    let _ = std::fs::remove_file(&captured);
    let redirect = StdoutRedirect::to(&captured);
    let outcome = run(&request, RunControls::default());
    drop(redirect);

    let outcome = outcome.expect("a hermetic mock run is not a usage error");
    assert_eq!(outcome.exit_code, 0, "{:?}", outcome.report.results[0]);
    assert!(!outcome.streamed, "a buffered run publishes nothing");
    assert!(outcome.failure_summary.is_none());
    let result = &outcome.report.results[0];
    assert_eq!(result.status, Status::Ok);
    assert_eq!(result.text.as_deref(), Some("working"));
    assert_eq!(result.events.as_ref().map(Vec::len), Some(1));
    // The report names the engine's own version when the caller supplies none.
    assert!(!outcome.report.oneharness_version.is_empty());

    let printed = std::fs::read_to_string(&captured).expect("the redirect file exists");
    assert!(
        printed.is_empty(),
        "the engine wrote to the caller's stdout: {printed:?}"
    );
    let _ = std::fs::remove_file(&captured);
}

/// This process's stdout, pointed at a file for as long as the guard lives.
///
/// The only way to ask "did that call print anything?" of code running in your
/// own process. Restored on drop, so a panicking assertion still leaves the test
/// harness able to report.
struct StdoutRedirect(std::os::fd::OwnedFd);

impl StdoutRedirect {
    fn to(path: &std::path::Path) -> Self {
        use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
        let file = std::fs::File::create(path).expect("create the redirect file");
        // SAFETY: fd 1 is open for the lifetime of the process, and the
        // descriptor `dup` returns is owned by the guard until it restores it.
        let saved = unsafe { libc::dup(libc::STDOUT_FILENO) };
        assert!(saved >= 0, "could not duplicate stdout");
        // SAFETY: `saved` is a fresh, valid, exclusively-owned descriptor.
        let saved = unsafe { OwnedFd::from_raw_fd(saved) };
        // SAFETY: both arguments are valid open descriptors.
        assert!(
            unsafe { libc::dup2(file.as_raw_fd(), libc::STDOUT_FILENO) } >= 0,
            "could not redirect stdout"
        );
        Self(saved)
    }
}

impl Drop for StdoutRedirect {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        // SAFETY: the saved descriptor is still owned and open here, and fd 1 is
        // a valid target.
        unsafe { libc::dup2(self.0.as_raw_fd(), libc::STDOUT_FILENO) };
    }
}
