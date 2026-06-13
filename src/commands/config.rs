//! `oneharness config` — show the effective layered configuration as JSON,
//! with each value attributed to the config file (or built-in default) it came
//! from. This is the debugging surface for the layering: when a run behaves
//! unexpectedly, this shows exactly which file shaped which setting.

use crate::cli::ConfigArgs;
use crate::commands::print_json;
use crate::domain::config as domain_config;
use crate::errors::OneharnessError;
use crate::io::config as config_io;

pub fn run(args: &ConfigArgs) -> Result<i32, OneharnessError> {
    // Mirror `run`'s discovery exactly (--cwd, else the current directory) so
    // the report shows what a run from that directory would actually load.
    let project_start = match &args.cwd {
        Some(dir) => dir.clone(),
        None => std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
    };
    let layers = config_io::load_layers(args.config.as_deref(), args.no_config, &project_start)?;
    let report = domain_config::explain(&layers);
    print_json(&report, args.compact)?;
    Ok(0)
}
