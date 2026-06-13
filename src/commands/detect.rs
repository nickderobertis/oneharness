//! `oneharness detect` — probe which harnesses are installed (binary + version).

use serde::Serialize;

use crate::cli::DetectArgs;
use crate::commands::{print_json, select_specs};
use crate::errors::OneharnessError;
use crate::io::config as config_io;
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
    // Configured binaries apply to probing too, so `detect` reports the same
    // binary `run` would invoke. Discovery starts from the current directory
    // (detect has no --cwd).
    let project_start = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let loaded = config_io::load(args.config.as_deref(), args.no_config, &project_start)?;
    let config_bins: std::collections::HashMap<String, String> = loaded
        .config
        .harness
        .iter()
        .filter_map(|(id, h)| h.bin.clone().map(|bin| (id.clone(), bin)))
        .collect();
    let overrides = BinOverrides::parse(&args.bin)?.with_config_bins(config_bins);

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
