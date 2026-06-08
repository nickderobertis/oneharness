//! `oneharness list` — describe the supported harnesses as JSON.

use serde::Serialize;

use crate::cli::ListArgs;
use crate::commands::print_json;
use crate::domain::harness::{self, BuildCtx};
use crate::domain::report::OutputFormat;
use crate::errors::OneharnessError;

#[derive(Serialize)]
struct HarnessInfo {
    id: &'static str,
    display: &'static str,
    default_bin: &'static str,
    install_hint: &'static str,
    output_format: OutputFormat,
    /// The argv oneharness would build, with placeholders, so the adapter's
    /// shape is visible without running anything.
    example_command: Vec<String>,
}

#[derive(Serialize)]
struct ListReport {
    schema_version: &'static str,
    harnesses: Vec<HarnessInfo>,
}

pub fn run(args: &ListArgs) -> Result<i32, OneharnessError> {
    let harnesses = harness::all()
        .iter()
        .map(|spec| {
            let ctx = BuildCtx {
                bin: spec.default_bin,
                prompt: "<PROMPT>",
                model: None,
                bypass: true,
            };
            HarnessInfo {
                id: spec.id,
                display: spec.display,
                default_bin: spec.default_bin,
                install_hint: spec.install_hint,
                output_format: spec.output_format,
                example_command: (spec.build_argv)(&ctx),
            }
        })
        .collect();

    let report = ListReport {
        schema_version: crate::domain::report::SCHEMA_VERSION,
        harnesses,
    };
    print_json(&report, args.compact)?;
    Ok(0)
}
