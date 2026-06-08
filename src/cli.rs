//! Command-line surface: all args, subcommands, and defaults in one place.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::domain::report::OutputFormat;

const ABOUT: &str =
    "One CLI across many agentic coding harnesses. Emits JSON for programmatic consumers.";

const LONG_ABOUT: &str = "\
oneharness drives Claude Code, Codex, OpenCode, Goose, Qwen Code, Crush, Copilot
CLI, and Cursor through a single non-interactive interface, running them in
parallel and returning one stable JSON shape.

All subcommands print JSON to stdout; diagnostics go to stderr. `run` requests
each harness's permission-bypass mode by default because headless agent runs hang
waiting for approval — pass --no-bypass to opt out.";

#[derive(Parser, Debug)]
#[command(name = "oneharness", version, about = ABOUT, long_about = LONG_ABOUT)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Run a prompt across one or more harnesses in parallel; emit a JSON report.
    Run(RunArgs),
    /// List the supported harnesses as JSON.
    List(ListArgs),
    /// Probe which harnesses are installed (binary + version) as JSON.
    Detect(DetectArgs),
}

#[derive(Args, Debug)]
pub struct RunArgs {
    /// Run against every supported harness.
    #[arg(long, conflicts_with = "harness")]
    pub all: bool,

    /// Harness id(s) to run (repeatable, comma-separated). See `oneharness list`.
    #[arg(long, value_delimiter = ',', value_name = "ID")]
    pub harness: Vec<String>,

    /// Harness id(s) to exclude when using --all (repeatable, comma-separated).
    #[arg(long, value_delimiter = ',', value_name = "ID")]
    pub exclude: Vec<String>,

    /// The prompt to send. Mutually exclusive with --prompt-file.
    #[arg(long, conflicts_with = "prompt_file")]
    pub prompt: Option<String>,

    /// Read the prompt from a file, or '-' for stdin.
    #[arg(long, value_name = "PATH")]
    pub prompt_file: Option<String>,

    /// Model passed to each harness that supports a model flag.
    #[arg(long)]
    pub model: Option<String>,

    /// Override the output format requested from each harness (default: the
    /// per-harness default; see `oneharness list`). Affects both the emitted
    /// format flag and how `text` is extracted.
    #[arg(long, value_enum)]
    pub output_format: Option<OutputFormat>,

    /// Write each harness's raw stdout/stderr to <DIR>/<harness>.stdout and
    /// <DIR>/<harness>.stderr (in addition to the JSON report on stdout).
    #[arg(long, value_name = "DIR")]
    pub output_dir: Option<PathBuf>,

    /// Per-harness timeout in seconds.
    #[arg(long, default_value_t = 120)]
    pub timeout: u64,

    /// Working directory each harness process runs in.
    #[arg(long, value_name = "DIR")]
    pub cwd: Option<PathBuf>,

    /// Extra environment variable KEY=VALUE for each harness (repeatable).
    #[arg(long = "env", value_name = "KEY=VALUE")]
    pub env: Vec<String>,

    /// Do NOT request each harness's bypass/yolo mode (headless runs may hang).
    #[arg(long)]
    pub no_bypass: bool,

    /// Maximum harnesses to run concurrently (default: all selected at once).
    #[arg(long, value_name = "N")]
    pub max_parallel: Option<usize>,

    /// Build and report each command without executing it (dry run).
    #[arg(long)]
    pub print_command: bool,

    /// Override a harness binary: --bin ID=PATH (repeatable). Also via
    /// ONEHARNESS_BIN_<ID> env vars.
    #[arg(long = "bin", value_name = "ID=PATH")]
    pub bin: Vec<String>,

    /// Treat a not-installed harness as a failure (non-zero exit).
    #[arg(long)]
    pub require_available: bool,

    /// Emit compact single-line JSON instead of pretty-printed.
    #[arg(long)]
    pub compact: bool,

    /// Extra arguments appended verbatim to each harness command, after `--`.
    /// Intended for single-harness runs (the flags differ per harness).
    #[arg(last = true, value_name = "HARNESS_ARG")]
    pub passthrough: Vec<String>,
}

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Emit compact single-line JSON instead of pretty-printed.
    #[arg(long)]
    pub compact: bool,
}

#[derive(Args, Debug)]
pub struct DetectArgs {
    /// Probe every supported harness (the default when none are named).
    #[arg(long, conflicts_with = "harness")]
    pub all: bool,

    /// Harness id(s) to probe (repeatable, comma-separated).
    #[arg(long, value_delimiter = ',', value_name = "ID")]
    pub harness: Vec<String>,

    /// Harness id(s) to exclude (repeatable, comma-separated).
    #[arg(long, value_delimiter = ',', value_name = "ID")]
    pub exclude: Vec<String>,

    /// Override a harness binary: --bin ID=PATH (repeatable).
    #[arg(long = "bin", value_name = "ID=PATH")]
    pub bin: Vec<String>,

    /// Exit non-zero if any probed harness is not installed.
    #[arg(long)]
    pub require_available: bool,

    /// Emit compact single-line JSON instead of pretty-printed.
    #[arg(long)]
    pub compact: bool,
}
