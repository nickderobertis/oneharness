//! `oneharness detect` — probe which harnesses are installed (binary + version).

use serde::Serialize;

use crate::cli::DetectArgs;
use crate::commands::{print_json, select_specs};
use crate::errors::OneharnessError;
use crate::io::detect::{self, BinOverrides};

#[derive(Serialize)]
struct DetectInfo {
    id: &'static str,
    bin: String,
    available: bool,
    path: Option<String>,
    version: Option<String>,
}

#[derive(Serialize)]
struct DetectReport {
    schema_version: &'static str,
    detected: Vec<DetectInfo>,
}

pub fn run(args: &DetectArgs) -> Result<i32, OneharnessError> {
    // Detection defaults to every harness; an explicit selection narrows it.
    let all = args.all || (args.harness.is_empty() && args.exclude.is_empty());
    let specs = select_specs(all, &args.harness, &args.exclude)?;
    let overrides = BinOverrides::parse(&args.bin)?;

    let detected: Vec<DetectInfo> = specs
        .iter()
        .map(|spec| {
            let resolved = detect::resolve(spec, &overrides);
            let version = if resolved.available {
                detect::probe_version(&resolved.bin)
            } else {
                None
            };
            DetectInfo {
                id: spec.id,
                bin: resolved.bin,
                available: resolved.available,
                path: resolved.path.map(|p| p.display().to_string()),
                version,
            }
        })
        .collect();

    let any_missing = detected.iter().any(|d| !d.available);

    let report = DetectReport {
        schema_version: crate::domain::report::SCHEMA_VERSION,
        detected,
    };
    print_json(&report, args.compact)?;

    if args.require_available && any_missing {
        eprintln!("oneharness: one or more requested harnesses are not installed");
        return Ok(1);
    }
    Ok(0)
}
