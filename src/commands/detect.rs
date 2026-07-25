//! `oneharness detect` — probe which harnesses are installed (binary + version).

use schemars::JsonSchema;
use serde::Serialize;

use crate::cli::DetectArgs;
use crate::commands::{dedupe_exact_ids, print_json, select_specs};
use oneharness_core::errors::OneharnessError;
use oneharness_core::io::config as config_io;
use oneharness_core::io::detect::{self, BinOverrides};

#[derive(JsonSchema, Serialize)]
pub struct DetectInfo {
    // llmlint: ignore[invalid_states_unrepresentable] This JSON boundary mirrors the CLI selector string; selection and variant lookup validate it before construction, with integration coverage for valid and invalid composed ids.
    id: String,
    bin: String,
    available: bool,
    path: Option<String>,
    version: Option<String>,
}

#[derive(JsonSchema, Serialize)]
pub struct DetectReport {
    schema_version: &'static str,
    detected: Vec<DetectInfo>,
}

pub fn run(args: &DetectArgs) -> Result<i32, OneharnessError> {
    // Detection defaults to every harness; an explicit selection narrows it.
    let all = args.all || (args.harness.is_empty() && args.exclude.is_empty());
    let specs = select_specs(all, &args.harness, &args.exclude)?;
    let selected_ids = dedupe_exact_ids(&args.harness);
    // Configured binaries apply to probing too, so `detect` reports the same
    // binary `run` would invoke. Discovery starts from the current directory
    // (detect has no --cwd).
    let project_start = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let loaded = config_io::load(args.config.as_deref(), args.no_config, &project_start)?;
    for id in args.harness.iter().chain(&args.exclude) {
        if let Some((base, variant)) = id.split_once(':') {
            if loaded.config.variant_for(id).is_none() {
                return Err(OneharnessError::UnknownHarnessVariant {
                    id: id.clone(),
                    base: base.to_string(),
                    variant: variant.to_string(),
                });
            }
        }
    }
    let mut config_bins: std::collections::HashMap<String, String> = loaded
        .config
        .harness
        .iter()
        .filter_map(|(id, h)| h.bin.clone().map(|bin| (id.clone(), bin)))
        .collect();
    for (base, harness) in &loaded.config.harness {
        for name in harness.variant.keys() {
            let id = format!("{base}:{name}");
            if let Some(bin) = loaded.config.bin_for(&id) {
                config_bins.insert(id, bin.to_string());
            }
        }
    }
    let overrides = BinOverrides::parse(&args.bin)?.with_config_bins(config_bins);

    let detected: Vec<DetectInfo> = specs
        .iter()
        .enumerate()
        .map(|(index, spec)| {
            let id = selected_ids
                .get(index)
                .map_or(spec.id.to_string(), Clone::clone);
            let resolved = detect::resolve_named(spec, &id, &overrides);
            let version = if resolved.available {
                detect::probe_version(&resolved.bin)
            } else {
                None
            };
            DetectInfo {
                id,
                bin: resolved.bin,
                available: resolved.available,
                path: resolved.path.map(|p| p.display().to_string()),
                version,
            }
        })
        .collect();

    let any_missing = detected.iter().any(|d| !d.available);

    let report = DetectReport {
        schema_version: oneharness_core::domain::report::SCHEMA_VERSION,
        detected,
    };
    print_json(&report, args.compact)?;

    if args.require_available && any_missing {
        eprintln!("oneharness: one or more requested harnesses are not installed");
        return Ok(1);
    }
    Ok(0)
}
