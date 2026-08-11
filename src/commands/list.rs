//! `oneharness list` — describe the supported harnesses as JSON.
//!
//! The description itself is a library call ([`oneharness_core::io::registry::list`]),
//! so a Rust consumer reads the same [`ListReport`] without spawning anything;
//! this is the shell that prints it.

use crate::cli::ListArgs;
use crate::commands::print_json;
use oneharness_core::errors::OneharnessError;
use oneharness_core::io::registry::{self, ListRequest};

// Re-exported so the schema generator and existing consumers keep one import
// path for the CLI's output contract, wherever the type is defined.
pub use oneharness_core::io::registry::{HarnessInfo, ListReport, ModeInfo, VariantInfo};

pub fn run(args: &ListArgs) -> Result<i32, OneharnessError> {
    let report = registry::list(&ListRequest::default())?;
    print_json(&report, args.compact)?;
    Ok(0)
}
