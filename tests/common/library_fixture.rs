//! Shared fixture for the library-API integration tests (`library.rs`,
//! `library_stdout.rs`), which are two test binaries so the one that redirects
//! this process's stdout is the only test in its own process.
//!
//! `dead_code` is allowed because each binary uses the subset it needs; the
//! alternative is duplicating the same wiring in both.
#![allow(dead_code)]

use std::path::PathBuf;

use oneharness_core::io::run::RunRequest;

/// The mock harness is built beside the main binary when the `mock-harness`
/// feature is enabled (which `just test` / `just check` and CI do).
pub fn mock_bin() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_BIN_EXE_oneharness"));
    path.set_file_name(format!(
        "oneharness-mock-harness{}",
        std::env::consts::EXE_SUFFIX
    ));
    path
}

pub fn bin_override(id: &str) -> String {
    format!("{id}={}", mock_bin().display())
}

/// A harness child this test will have killed must not leave a truncated
/// coverage profile in the target directory, where `just coverage` would collect
/// it and fail the whole merge — nor in bare system temp, where nothing removes
/// it. Same redirect the CLI suite applies.
#[path = "../profile-scratch/profile_scratch.rs"]
mod profile_scratch;
// Each binary uses the subset it needs, as with `dead_code` above.
#[allow(unused_imports)]
pub use profile_scratch::{profile_redirect, ProfileScratch, PROFILE_PATTERN};

/// A hermetic single-harness request against the mock fixture. `no_config` keeps
/// the developer's real config files and `ONEHARNESS_*` overrides out of the
/// assertion, exactly as `ONEHARNESS_NO_CONFIG=1` does for the CLI suite. The
/// mock's coverage profile goes into `profiles`, which the test holds until the
/// run has ended.
pub fn request(profiles: &ProfileScratch, id: &str, env: &[(&str, &str)]) -> RunRequest {
    let mut child_env: Vec<String> = env
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect();
    child_env.push(profiles.as_str().to_string());
    RunRequest {
        harness: vec![id.to_string()],
        prompt: vec!["hi".to_string()],
        bin: vec![bin_override(id)],
        env: child_env,
        no_config: true,
        timeout: Some(60),
        ..RunRequest::default()
    }
}

/// Five opencode tool parts — five `tool_call` events when streamed one line at
/// a time.
pub fn tool_part_lines(count: usize) -> String {
    (0..count)
        .map(|i| {
            format!(
                r#"{{"type":"tool_use","part":{{"type":"tool","tool":"bash","state":{{"input":{{"command":"step {i}"}}}}}}}}"#
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
