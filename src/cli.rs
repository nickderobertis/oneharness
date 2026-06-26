//! Command-line surface: all args, subcommands, and defaults in one place.

use std::path::PathBuf;

use clap::builder::{PossibleValuesParser, TypedValueParser};
use clap::{Args, Parser, Subcommand};

use oneharness_core::domain::report::OutputFormat;

/// Parse `--output-format` into the core [`OutputFormat`], keeping the
/// possible-value list (and its `--help` listing + validation error) here in
/// the binary so `oneharness-core` need not depend on `clap`.
fn output_format_parser() -> impl TypedValueParser<Value = OutputFormat> {
    PossibleValuesParser::new(["text", "json", "stream-json"]).map(|s| match s.as_str() {
        "text" => OutputFormat::Text,
        "json" => OutputFormat::Json,
        _ => OutputFormat::StreamJson,
    })
}

const ABOUT: &str =
    "One CLI across many agentic coding harnesses. Emits JSON for programmatic consumers.";

const LONG_ABOUT: &str = "\
oneharness drives Claude Code, Codex, OpenCode, Goose, Qwen Code, Crush, Copilot
CLI, and Cursor through a single non-interactive interface, running them in
parallel and returning one stable JSON shape.

All subcommands print JSON to stdout; diagnostics go to stderr. `run` requests
each harness's permission-bypass mode by default because headless agent runs hang
waiting for approval — pass --no-bypass to opt out.

Defaults come from layered config files: a user-level config.toml (the platform
config dir, or $ONEHARNESS_CONFIG) under a project-level oneharness.toml /
.oneharness.toml discovered upward from the working directory. Each field also
has an ONEHARNESS_<FIELD> environment override (e.g. ONEHARNESS_MODEL,
ONEHARNESS_TIMEOUT, ONEHARNESS_HARNESSES) that beats the files. Full precedence,
lowest first: built-in defaults < user file < project file < environment < CLI
flags. --no-config (or ONEHARNESS_NO_CONFIG=1) ignores files AND env overrides;
--config <path> loads exactly one file (the env overrides still apply on top).";

#[derive(Parser, Debug)]
#[command(name = "oneharness", version, about = ABOUT, long_about = LONG_ABOUT)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Run a prompt across one or more harnesses in parallel; emit a JSON report.
    ///
    /// Boxed because `RunArgs` is far larger than the other variants; keeping the
    /// enum small satisfies `clippy::large_enum_variant`.
    Run(Box<RunArgs>),
    /// List the supported harnesses as JSON.
    List(ListArgs),
    /// Probe which harnesses are installed (binary + version) as JSON.
    Detect(DetectArgs),
    /// Show the effective layered configuration as JSON: every field's value
    /// and which config file (or built-in default) it came from.
    Config(ConfigArgs),
    /// Merge the unified settings (permission rules, hooks, raw settings
    /// tables) into each harness's own project config file, so the policy
    /// also applies when the tools are used directly, without oneharness.
    /// Non-destructive: unrelated keys are preserved, lists are unioned.
    Sync(SyncArgs),
    /// Run the pre-tool gate for a harness: read that harness's hook event on
    /// stdin and, when it matches, emit the harness's native *deny* verdict on
    /// stdout (otherwise nothing — the call proceeds). This is what an installed
    /// `[[hooks]]` hook invokes, so it is the runtime proof a synced hook is
    /// honored. Always exits 0 (a gate never blocks on its own error).
    Gate(GateArgs),
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
    ///
    /// `allow_hyphen_values` so a prompt that begins with `-`/`--` (or YAML
    /// front matter's `---`) is taken as the value rather than parsed as a flag.
    #[arg(long, conflicts_with = "prompt_file", allow_hyphen_values = true)]
    pub prompt: Option<String>,

    /// Read the prompt from a file, or '-' for stdin.
    #[arg(long, value_name = "PATH")]
    pub prompt_file: Option<String>,

    /// Model passed to each harness that supports a model flag.
    #[arg(long)]
    pub model: Option<String>,

    /// System prompt passed to every harness. Delivered via the harness's native
    /// system flag where one exists (e.g. Claude Code's --append-system-prompt,
    /// Goose's --system); for harnesses without one it is prepended to the prompt
    /// so the instructions still reach the model. `allow_hyphen_values` so system
    /// text beginning with `-`/`--` (or `---` front matter) is read as the value.
    #[arg(long, value_name = "TEXT", allow_hyphen_values = true)]
    pub system: Option<String>,

    /// Continue a prior session: send the prompt as the next turn of <SESSION>.
    /// Single-harness only (a session belongs to one harness) and only for
    /// harnesses that support it (see `supports_resume` in `oneharness list`).
    #[arg(long, value_name = "SESSION", conflicts_with = "all")]
    pub resume: Option<String>,

    /// Override the output format requested from each harness (default: the
    /// per-harness default; see `oneharness list`). Affects both the emitted
    /// format flag and how `text` is extracted.
    #[arg(long, value_parser = output_format_parser())]
    pub output_format: Option<OutputFormat>,

    /// Constrain each harness's final answer to this JSON Schema file
    /// (structured output). The schema is delivered natively where the harness
    /// supports it (Claude Code's --json-schema) and via the prompt otherwise;
    /// the response is validated and, on failure, re-prompted up to
    /// --schema-max-retries times. Each result gains `structured`,
    /// `schema_valid`, `schema_attempts`, and `schema_error`.
    #[arg(long, value_name = "PATH")]
    pub schema: Option<PathBuf>,

    /// Max retries when a response fails schema validation (default 2; only with
    /// --schema). The harness is invoked at most 1 + N times per run.
    #[arg(long, value_name = "N")]
    pub schema_max_retries: Option<u32>,

    /// Write each harness's raw stdout/stderr to <DIR>/<harness>.stdout and
    /// <DIR>/<harness>.stderr (in addition to the JSON report on stdout).
    #[arg(long, value_name = "DIR")]
    pub output_dir: Option<PathBuf>,

    /// Per-harness timeout in seconds (default 120, or `timeout` from config).
    #[arg(long, value_name = "SECS")]
    pub timeout: Option<u64>,

    /// Working directory each harness process runs in.
    #[arg(long, value_name = "DIR")]
    pub cwd: Option<PathBuf>,

    /// Extra environment variable KEY=VALUE for each harness (repeatable).
    #[arg(long = "env", value_name = "KEY=VALUE")]
    pub env: Vec<String>,

    /// Do NOT request each harness's bypass/yolo mode (headless runs may hang).
    #[arg(long)]
    pub no_bypass: bool,

    /// Request bypass mode even when config sets `bypass = false`.
    #[arg(long, conflicts_with = "no_bypass")]
    pub bypass: bool,

    /// Load configuration from this file only (skip user/project discovery).
    #[arg(long, value_name = "PATH", conflicts_with = "no_config")]
    pub config: Option<PathBuf>,

    /// Ignore all configuration files (also via ONEHARNESS_NO_CONFIG=1).
    #[arg(long)]
    pub no_config: bool,

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
pub struct ConfigArgs {
    /// Discover the project config from this directory (mirrors `run --cwd`),
    /// so the output shows exactly what a run there would load.
    #[arg(long, value_name = "DIR")]
    pub cwd: Option<PathBuf>,

    /// Load configuration from this file only (skip user/project discovery).
    #[arg(long, value_name = "PATH", conflicts_with = "no_config")]
    pub config: Option<PathBuf>,

    /// Ignore all configuration files (also via ONEHARNESS_NO_CONFIG=1).
    #[arg(long)]
    pub no_config: bool,

    /// Emit compact single-line JSON instead of pretty-printed.
    #[arg(long)]
    pub compact: bool,
}

#[derive(Args, Debug)]
pub struct SyncArgs {
    /// Project directory whose harness config files are written (mirrors
    /// `run --cwd`; defaults to the current directory). Project config
    /// discovery also starts here.
    #[arg(long, value_name = "DIR")]
    pub cwd: Option<PathBuf>,

    /// Harness id(s) to sync (default: every harness that has something to
    /// sync). Repeatable, comma-separated.
    #[arg(long, value_delimiter = ',', value_name = "ID")]
    pub harness: Vec<String>,

    /// Check only: report what would change and exit 1 if anything is out of
    /// sync, writing nothing. For CI.
    #[arg(long)]
    pub check: bool,

    /// Install hooks into the user-global config location (resolved from $HOME /
    /// $XDG_CONFIG_HOME) instead of the project. Only `[[hooks]]` entries have a
    /// global mapping; permission rules and raw `settings` are project-scoped, so
    /// a config that sets them is a usage error under --global.
    #[arg(long)]
    pub global: bool,

    /// Load configuration from this file only (skip user/project discovery).
    #[arg(long, value_name = "PATH", conflicts_with = "no_config")]
    pub config: Option<PathBuf>,

    /// Ignore all configuration files (also via ONEHARNESS_NO_CONFIG=1).
    #[arg(long)]
    pub no_config: bool,

    /// Emit compact single-line JSON instead of pretty-printed.
    #[arg(long)]
    pub compact: bool,
}

#[derive(Args, Debug)]
pub struct GateArgs {
    /// Harness id whose hook protocol to speak (see `oneharness list`).
    #[arg(value_name = "ID")]
    pub harness: String,

    /// Block any tool call whose hook event (the JSON the harness pipes to
    /// stdin) contains this substring; everything else is allowed (empty
    /// stdout). Without it the gate allows everything — the inert default.
    #[arg(long, value_name = "SUBSTR")]
    pub deny_if_contains: Option<String>,

    /// Reason surfaced to the model when a call is blocked.
    #[arg(
        long,
        value_name = "TEXT",
        default_value = "blocked by oneharness gate"
    )]
    pub reason: String,
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

    /// Load configuration from this file only (skip user/project discovery).
    #[arg(long, value_name = "PATH", conflicts_with = "no_config")]
    pub config: Option<PathBuf>,

    /// Ignore all configuration files (also via ONEHARNESS_NO_CONFIG=1).
    #[arg(long)]
    pub no_config: bool,

    /// Exit non-zero if any probed harness is not installed.
    #[arg(long)]
    pub require_available: bool,

    /// Emit compact single-line JSON instead of pretty-printed.
    #[arg(long)]
    pub compact: bool,
}
