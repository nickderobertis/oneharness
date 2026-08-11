//! `oneharness detect` — probe which harnesses are installed (binary + version).
//!
//! The sweep is a library call ([`oneharness_core::io::detect::detect`]), so a
//! Rust consumer reads the same [`DetectReport`] without spawning anything; this
//! is the shell that prints it and maps `--require-available` to an exit code.

use crate::cli::DetectArgs;
use crate::commands::print_json;
use oneharness_core::errors::OneharnessError;
use oneharness_core::io::detect::{self, DetectRequest};

// Re-exported so the schema generator and existing consumers keep one import
// path for the CLI's output contract, wherever the type is defined.
pub use oneharness_core::io::detect::{DetectInfo, DetectReport};

pub fn run(args: &DetectArgs) -> Result<i32, OneharnessError> {
    let report = detect::detect(&DetectRequest {
        all: args.all,
        harness: args.harness.clone(),
        exclude: args.exclude.clone(),
        bin: args.bin.clone(),
        config: args.config.clone(),
        no_config: args.no_config,
        cwd: None,
    })?;
    let any_missing = report.any_missing();
    print_json(&report, args.compact)?;

    if args.require_available && any_missing {
        eprintln!("oneharness: one or more requested harnesses are not installed");
        return Ok(1);
    }
    Ok(0)
}
