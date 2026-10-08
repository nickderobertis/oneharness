//! The fixture executable answers the way the suites that spawn it rely on:
//! it writes `MOCK_STDOUT`/`MOCK_STDERR` verbatim and exits `MOCK_EXIT`.
//!
//! Being an integration test is also what makes `cargo test`/`nextest` build
//! this crate's binary when a consumer's test target selects the package
//! beside its own (`-p oneharness-mock-harness`).

use std::process::Command;

#[test]
fn the_fixture_writes_its_scripted_streams_and_exit_code() {
    let output = Command::new(env!("CARGO_BIN_EXE_oneharness-mock-harness"))
        .env("MOCK_STDOUT", "scripted stdout")
        .env("MOCK_STDERR", "scripted stderr")
        .env("MOCK_EXIT", "3")
        .output()
        .expect("spawn the mock harness");
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "scripted stdout");
    assert_eq!(String::from_utf8_lossy(&output.stderr), "scripted stderr");
}

#[test]
fn the_fixture_echoes_the_environment_variable_it_is_asked_about() {
    let output = Command::new(env!("CARGO_BIN_EXE_oneharness-mock-harness"))
        .env("MOCK_ECHO_ENV", "FIXTURE_PROBE")
        .env("FIXTURE_PROBE", "inherited value")
        .output()
        .expect("spawn the mock harness");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim_end(),
        "FIXTURE_PROBE=inherited value"
    );
}
