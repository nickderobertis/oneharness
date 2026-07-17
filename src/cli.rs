//! Command-line surface: all args, subcommands, and defaults in one place.

use std::path::PathBuf;

use clap::builder::{PossibleValuesParser, TypedValueParser};
use clap::{Args, Parser, Subcommand};

use oneharness_core::domain::batch::BatchStrategy;
use oneharness_core::domain::fallback::RunMode;
use oneharness_core::domain::mode::PermissionMode;
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

/// Parse `--mode` into the core [`PermissionMode`], keeping the possible-value
/// list (and its `--help` listing + validation error) in the binary so
/// `oneharness-core` need not depend on `clap`.
fn mode_parser() -> impl TypedValueParser<Value = PermissionMode> {
    PossibleValuesParser::new(PermissionMode::ALL.map(|m| m.as_str()))
        .map(|s| PermissionMode::parse(&s).expect("clap restricts to valid mode tokens"))
}

/// Parse `--batch-strategy` into the core [`BatchStrategy`], keeping the
/// possible-value list (and its `--help` listing + validation error) in the
/// binary so `oneharness-core` need not depend on `clap`.
fn batch_strategy_parser() -> impl TypedValueParser<Value = BatchStrategy> {
    PossibleValuesParser::new(BatchStrategy::ALL.map(|s| s.as_str()))
        .map(|s| BatchStrategy::parse(&s).expect("clap restricts to valid strategy tokens"))
}

/// Parse `--run-mode` into the core [`RunMode`], keeping the possible-value list
/// (and its `--help` listing + validation error) in the binary so
/// `oneharness-core` need not depend on `clap`.
fn run_mode_parser() -> impl TypedValueParser<Value = RunMode> {
    PossibleValuesParser::new(RunMode::ALL.map(|m| m.as_str()))
        .map(|s| RunMode::parse(&s).expect("clap restricts to valid run-mode tokens"))
}

/// How `oneharness history list`/`show` renders. Every other subcommand emits
/// JSON to stdout by design; the history views keep JSON as the default (the
/// programmatic contract) and add an opt-in human-readable `text` mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryFormat {
    Json,
    Text,
}

/// The continuous history stream's wire format. Kept separate from list/show so
/// those bounded commands cannot accidentally accept an unbounded format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryWatchFormat {
    Jsonl,
}

fn history_watch_format_parser() -> impl TypedValueParser<Value = HistoryWatchFormat> {
    PossibleValuesParser::new(["jsonl"]).map(|_| HistoryWatchFormat::Jsonl)
}

/// Parse `--format` into [`HistoryFormat`], keeping the possible-value list in
/// the binary (json is the default, applied on the field).
fn history_format_parser() -> impl TypedValueParser<Value = HistoryFormat> {
    PossibleValuesParser::new(["json", "text"]).map(|s| match s.as_str() {
        "text" => HistoryFormat::Text,
        _ => HistoryFormat::Json,
    })
}

const ABOUT: &str =
    "One CLI across many agentic coding harnesses. Emits JSON for programmatic consumers.";

const LONG_ABOUT: &str = "\
oneharness drives Claude Code, Codex, OpenCode, Goose, Qwen Code, Crush, Copilot
CLI, and Cursor through a single non-interactive interface, running them in
parallel and returning one stable JSON shape.

All subcommands print JSON to stdout; diagnostics go to stderr. `run` uses the
`default` approval mode (each harness's normal posture, mapped to its cleanest
non-interactive variant) unless told otherwise — pass --mode
<read-only|plan|default|edit|auto|bypass> to choose another (or --bypass,
shorthand for --mode bypass, to approve everything).

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
    /// Scaffold a starter `oneharness.toml` (a commented fallback-mode chain) at
    /// PATH (default `oneharness.toml`). Refuses to overwrite an existing file
    /// unless --force. Prints the path written; this is the one subcommand that
    /// CREATES config rather than reading it, so it takes no --config/--no-config.
    Init(InitArgs),
    /// Run the pre-tool gate for a harness: read that harness's hook event on
    /// stdin and, when it matches, emit the harness's native *deny* verdict on
    /// stdout (otherwise nothing — the call proceeds). This is what an installed
    /// `[[hooks]]` hook invokes, so it is the runtime proof a synced hook is
    /// honored. Always exits 0 (a gate never blocks on its own error).
    Gate(GateArgs),
    /// Run the mock/spy responder for a harness: read its pre-tool hook event
    /// on stdin, append it to the spy log, and — when a `--rules` rule matches —
    /// emit the harness's native verdict on stdout: a *deny* (the model reads
    /// the message as tool feedback) or an *input rewrite* (the call runs with
    /// substituted arguments, e.g. a shell command swapped for a stub printing
    /// canned output). No match, or no rules, emits nothing (the call proceeds
    /// — spy-only). Installed the same way as `gate`, via a `[[hooks]]` entry;
    /// exits 0 on any post-startup fault (never blocks a call on its own
    /// error), while a bad ruleset or an action the harness cannot express is
    /// a loud usage error up front.
    Mock(MockArgs),
    /// View and manage the standardized run history recorded by `run --history`.
    /// The `list`/`show` views print JSON to stdout by default (the programmatic
    /// contract); pass `--format text` for a human-readable view.
    History(HistoryArgs),
}

#[derive(Args, Debug)]
pub struct HistoryArgs {
    #[command(subcommand)]
    pub command: HistoryCommand,
}

#[derive(Subcommand, Debug)]
pub enum HistoryCommand {
    /// List recorded sessions, newest first: id, name, project, start time,
    /// harnesses, and record count.
    List(HistoryListArgs),
    /// Print the normalized records of one session, resolved by id or name (name
    /// is non-unique — the newest match wins unless --all is given).
    Show(HistoryShowArgs),
    /// Follow normalized history records continuously. Existing records are
    /// emitted first (or only records after --after), then new records as they
    /// are indexed. Output is one tagged JSON envelope per line.
    Watch(HistoryWatchArgs),
    /// Delete recorded sessions. Reports what it WOULD remove and removes nothing
    /// unless --yes is given (so it is safe to run non-interactively first).
    Clear(HistoryClearArgs),
}

#[derive(Args, Debug)]
pub struct HistoryWatchArgs {
    /// Resume strictly after this history record UUID, without duplicating it.
    #[arg(long, value_name = "CURSOR")]
    pub after: Option<String>,

    /// Emit only records carrying this exact KEY=VALUE label (repeatable; all
    /// filters must match).
    #[arg(long = "label", value_name = "KEY=VALUE")]
    pub label: Vec<String>,

    /// Follow records for this project; defaults to the current directory.
    #[arg(long, value_name = "DIR")]
    pub project: Option<PathBuf>,

    /// Follow records across every project.
    #[arg(long, conflicts_with = "project")]
    pub all_projects: bool,

    /// History directory to follow (default: config `history_dir`,
    /// ONEHARNESS_HISTORY_DIR, else the platform state dir).
    #[arg(long, value_name = "DIR")]
    pub history_dir: Option<PathBuf>,

    /// Streaming output format. `jsonl` is currently the only format.
    #[arg(
        long,
        value_parser = history_watch_format_parser(),
        default_value = "jsonl"
    )]
    pub format: HistoryWatchFormat,

    /// Load configuration from this file only (skip user/project discovery).
    #[arg(long, value_name = "PATH", conflicts_with = "no_config")]
    pub config: Option<PathBuf>,

    /// Ignore all configuration files (also via ONEHARNESS_NO_CONFIG=1).
    #[arg(long)]
    pub no_config: bool,
}

#[derive(Args, Debug)]
pub struct HistoryListArgs {
    /// The project whose sessions to list (its history subdir); defaults to the
    /// current directory. Ignored when --all-projects is given.
    #[arg(long, value_name = "DIR")]
    pub project: Option<PathBuf>,

    /// List sessions across every project, not just one.
    #[arg(long, conflicts_with = "project")]
    pub all_projects: bool,

    /// History directory to read (default: config `history_dir`,
    /// ONEHARNESS_HISTORY_DIR, else the platform state dir).
    #[arg(long, value_name = "DIR")]
    pub history_dir: Option<PathBuf>,

    /// Output format: `json` (default) or `text` (human-readable).
    #[arg(long, value_parser = history_format_parser(), default_value = "json")]
    pub format: HistoryFormat,

    /// Load configuration from this file only (skip user/project discovery).
    #[arg(long, value_name = "PATH", conflicts_with = "no_config")]
    pub config: Option<PathBuf>,

    /// Ignore all configuration files (also via ONEHARNESS_NO_CONFIG=1).
    #[arg(long)]
    pub no_config: bool,

    /// Emit compact single-line JSON instead of pretty-printed (JSON format only).
    #[arg(long)]
    pub compact: bool,
}

#[derive(Args, Debug)]
pub struct HistoryShowArgs {
    /// The session id or name to show. Optional with --last (the newest session).
    #[arg(value_name = "SESSION", required_unless_present = "last")]
    pub session: Option<String>,

    /// Show the most recent session (in the selected project scope) instead of
    /// naming one.
    #[arg(long, conflicts_with = "session")]
    pub last: bool,

    /// When the name matches more than one session, show all of them (records
    /// concatenated, newest first) rather than just the newest.
    #[arg(long)]
    pub all: bool,

    /// The project scope for resolving the session; defaults to the current
    /// directory. Ignored when --all-projects is given.
    #[arg(long, value_name = "DIR")]
    pub project: Option<PathBuf>,

    /// Resolve the session across every project, not just one.
    #[arg(long, conflicts_with = "project")]
    pub all_projects: bool,

    /// History directory to read (default: config `history_dir`,
    /// ONEHARNESS_HISTORY_DIR, else the platform state dir).
    #[arg(long, value_name = "DIR")]
    pub history_dir: Option<PathBuf>,

    /// Output format: `json` (default) or `text` (human-readable).
    #[arg(long, value_parser = history_format_parser(), default_value = "json")]
    pub format: HistoryFormat,

    /// Load configuration from this file only (skip user/project discovery).
    #[arg(long, value_name = "PATH", conflicts_with = "no_config")]
    pub config: Option<PathBuf>,

    /// Ignore all configuration files (also via ONEHARNESS_NO_CONFIG=1).
    #[arg(long)]
    pub no_config: bool,

    /// Emit compact single-line JSON instead of pretty-printed (JSON format only).
    #[arg(long)]
    pub compact: bool,
}

#[derive(Args, Debug)]
pub struct HistoryClearArgs {
    /// The project whose sessions to delete; defaults to the current directory.
    /// Ignored when --all-projects is given.
    #[arg(long, value_name = "DIR")]
    pub project: Option<PathBuf>,

    /// Delete sessions across every project.
    #[arg(long, conflicts_with = "project")]
    pub all_projects: bool,

    /// Actually delete. Without it, `clear` only reports what it would remove.
    #[arg(long)]
    pub yes: bool,

    /// History directory to operate on (default: config `history_dir`,
    /// ONEHARNESS_HISTORY_DIR, else the platform state dir).
    #[arg(long, value_name = "DIR")]
    pub history_dir: Option<PathBuf>,

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

    /// The prompt to send. Repeatable: passing it more than once (or combining
    /// it with multiple --prompt-file) runs a **batch** — one harness fanned over
    /// each prompt, sharing the cacheable --system/model prefix (see
    /// --batch-strategy). Combined prompt order is every --prompt, then every
    /// --prompt-file.
    ///
    /// `allow_hyphen_values` so a prompt that begins with `-`/`--` (or YAML
    /// front matter's `---`) is taken as the value rather than parsed as a flag.
    #[arg(long, allow_hyphen_values = true)]
    pub prompt: Vec<String>,

    /// Read a prompt from a file, or '-' for stdin. Repeatable: each file is one
    /// whole prompt (the file is not split per line); '-' may appear only once.
    /// Combines with --prompt to form a batch (see --prompt / --batch-strategy).
    #[arg(long, value_name = "PATH")]
    pub prompt_file: Vec<String>,

    /// Model passed to each harness that supports a model flag. Repeatable: pass
    /// it more than once (or set config `models`) to **fan out over models**. In
    /// the default `parallel` run mode that runs the harness × model cross-product
    /// (each selected harness once per model); in `--run-mode fallback` the
    /// (harness, model) pairs are tried in priority order (harness-major,
    /// model-minor) and a per-model rejection (unknown model / rate limit) falls
    /// through to the next model. A CLI value overrides config `model`/`models`.
    /// More than one model is incompatible with a batch (multi-prompt) run and
    /// with --resume / --fork / --session / --stream.
    #[arg(long)]
    pub model: Vec<String>,

    /// System prompt passed to every harness. Delivered via the harness's native
    /// system flag where one exists (e.g. Claude Code's --append-system-prompt,
    /// Goose's --system); for harnesses without one it is prepended to the prompt
    /// so the instructions still reach the model. `allow_hyphen_values` so system
    /// text beginning with `-`/`--` (or `---` front matter) is read as the value.
    #[arg(long, value_name = "TEXT", allow_hyphen_values = true)]
    pub system: Option<String>,

    /// Reasoning / thinking effort, forwarded verbatim to each harness that
    /// exposes it on the argv: Claude Code's --effort (low/medium/high/max/auto)
    /// and Codex's model_reasoning_effort (minimal/low/medium/high/xhigh). Pick a
    /// value your chosen model accepts — oneharness does not interpret it, so an
    /// unsupported value surfaces as that harness's own error, not a guess. A
    /// harness with no headless reasoning flag (every harness but claude-code and
    /// codex; the rest set effort via config only) is a loud usage error rather
    /// than a silent drop, so scope this per harness (config `[harness.<id>]
    /// reasoning`) when a selection mixes them. Overrides config `reasoning`.
    #[arg(long, value_name = "EFFORT")]
    pub reasoning: Option<String>,

    /// Read the system prompt from a file, or '-' for stdin — the file-based
    /// counterpart to --system (mirroring --prompt-file), for a system prompt too
    /// large to pass as an argv string: a big --system value can trip the OS
    /// single-argument limit and fail at spawn with `Argument list too long`
    /// (E2BIG) before any harness runs. Mutually exclusive with --system; config
    /// `system` remains the fallback for both. Front-matter/leading-`-` content is
    /// safe because it comes from the file, not argv. Only one input may read
    /// stdin, so '-' here cannot combine with `--prompt-file -`.
    #[arg(long, value_name = "PATH", conflicts_with = "system")]
    pub system_file: Option<String>,

    /// Continue a prior session: send the prompt as the next turn of <SESSION>.
    /// Single-harness only (a session belongs to one harness) and only for
    /// harnesses that support it (see `supports_resume` in `oneharness list`).
    #[arg(long, value_name = "SESSION", conflicts_with = "all")]
    pub resume: Option<String>,

    /// Fork the resumed session instead of appending to it: branch a new session
    /// from <SESSION> so the original (and its cached prefix) is untouched and can
    /// seed independent follow-ups. Requires --resume, and only for harnesses that
    /// support it (see `supports_fork` in `oneharness list`); others are a usage
    /// error rather than a silent linear resume.
    #[arg(long, requires = "resume")]
    pub fork: bool,

    /// Continue (or start) a named conversation by a stable, caller-owned handle.
    /// oneharness maps <NAME> to the harness's native session id in a small store,
    /// so you thread ONE name across turns instead of extracting and re-passing
    /// the id yourself: the first `--session <name>` run starts fresh and captures
    /// the id; later runs with the same name resume it. In the default parallel
    /// run mode it is single-harness; in `--run-mode fallback` it binds to the
    /// first session-capable harness in the priority chain (the one fallback
    /// settles on under stable availability). Only for harnesses that expose a
    /// session id headlessly (see `session_capable` in `oneharness list`); others
    /// are a loud usage error. The lower-level, per-invocation counterpart to
    /// --resume — mutually exclusive with --resume/--fork/--all and with a batch
    /// (multi-prompt) run.
    #[arg(long, value_name = "NAME", conflicts_with_all = ["resume", "fork", "all"])]
    pub session: Option<String>,

    /// Directory the `--session` store lives in (default: <platform state dir>/
    /// oneharness/sessions). Like --resume/--fork, a per-invocation knob with no
    /// config/env layer; mainly for isolating the store in tests and scripts.
    #[arg(long, value_name = "DIR")]
    pub session_dir: Option<PathBuf>,

    /// Override the output format requested from each harness (default: the
    /// per-harness default; see `oneharness list`). Affects both the emitted
    /// format flag and how `text` is extracted.
    #[arg(long, value_parser = output_format_parser())]
    pub output_format: Option<OutputFormat>,

    /// Surface each harness's normalized tool-call `events`. Selects the
    /// harness's events-capable output format when its default carries no tool
    /// transcript (e.g. Claude Code → stream-json), unless --output-format is set
    /// explicitly. Harnesses whose default already carries a transcript (OpenCode,
    /// Cursor) report `events` regardless; this flag just makes the upgrade
    /// explicit and covers the rest.
    #[arg(long)]
    pub events: bool,

    /// Stream normalized events to stdout as they occur (single harness), then a
    /// final result line — instead of one report at the end. Implies --events'
    /// format selection. Lets a consumer short-circuit (close stdin / signal) the
    /// moment it observes a disallowed action. Mutually exclusive with a
    /// multi-harness selection, --schema, and batch prompts.
    #[arg(long)]
    pub stream: bool,

    /// Mock/spy the selected harnesses' tool calls FOR THIS RUN ONLY: a JSON
    /// ruleset (same format as `oneharness mock --rules`) whose matching rules
    /// deny a call or rewrite its input. Delivery is ephemeral and layers on
    /// top of any existing workspace config: Claude Code takes the hook on the
    /// argv (--settings, zero file mutation); the other harnesses get a
    /// project-scope install via the non-destructive merge, snapshotted and
    /// restored after the run (Codex's hook-engine opt-in flags are appended
    /// automatically). A selected harness whose hooks cannot fire this way
    /// (qwen, copilot), or a rule action it cannot express, is a loud usage
    /// error before anything spawns.
    #[arg(long, value_name = "PATH", conflicts_with = "print_command")]
    pub mock_rules: Option<PathBuf>,

    /// Append one JSONL record per observed tool call (the raw hook event plus
    /// the action taken) to this file — the spy channel, recording the
    /// original call even when a rewrite substituted its input. Works with or
    /// without --mock-rules (alone it is a pure observer: every call allowed
    /// through, all of them recorded). Same ephemeral delivery as
    /// --mock-rules.
    #[arg(long, value_name = "PATH", conflicts_with = "print_command")]
    pub spy_file: Option<PathBuf>,

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

    /// Approval mode requested from each harness: read-only, plan, default, edit,
    /// auto, or bypass (default: default). Each harness maps it to its native
    /// mechanism; `oneharness list` shows which modes each supports. A mode a
    /// selected harness can't express is refused up front; one that may block on
    /// a prompt headlessly is warned about and run, with --timeout as the
    /// backstop. Supersedes --bypass / --no-bypass.
    #[arg(long, value_parser = mode_parser(), conflicts_with_all = ["bypass", "no_bypass"])]
    pub mode: Option<PermissionMode>,

    /// Do NOT request each harness's bypass/yolo mode. Shorthand for `--mode
    /// default`.
    #[arg(long)]
    pub no_bypass: bool,

    /// Request bypass mode even when config sets `bypass`/`mode`. Shorthand for
    /// `--mode bypass`.
    #[arg(long, conflicts_with = "no_bypass")]
    pub bypass: bool,

    /// Silence the warning that the chosen mode may block on an approval prompt
    /// for a selected harness (the run proceeds either way). Use when allow-rules
    /// have been synced so the prompt never fires.
    #[arg(long)]
    pub permit_prompts: bool,

    /// Load configuration from this file only (skip user/project discovery).
    #[arg(long, value_name = "PATH", conflicts_with = "no_config")]
    pub config: Option<PathBuf>,

    /// Ignore all configuration files (also via ONEHARNESS_NO_CONFIG=1).
    #[arg(long)]
    pub no_config: bool,

    /// Maximum harnesses (or, in a batch run, prompts) to run concurrently
    /// (default: all at once).
    #[arg(long, value_name = "N")]
    pub max_parallel: Option<usize>,

    /// How a **batch** run (more than one prompt) schedules its calls (no effect on
    /// a single-prompt run). `speed` (the DEFAULT) fires all prompts at once for
    /// minimum wall-clock. `min-tokens` reduces tokens by warming the shared
    /// --system as a session on the first prompt and forking it for the rest, so
    /// the fan-out reuses the cached prefix — but only on a harness whose fork
    /// reuses the cache (today Claude Code only; see `fork_reuses_cache` in
    /// `oneharness list`). On any other harness `min-tokens` only orders the calls
    /// (no saving, with a stderr warning), so `speed` is the safe default.
    #[arg(long, value_parser = batch_strategy_parser(), value_name = "STRATEGY")]
    pub batch_strategy: Option<BatchStrategy>,

    /// How the selected harnesses are run (also `run_mode` in config /
    /// ONEHARNESS_RUN_MODE). `parallel` (the DEFAULT) runs them all at once and
    /// reports each. `fallback` runs them in **priority order** (the --harness /
    /// config order, else registry order under --all) and stops at the first that
    /// actually runs the task, falling through only harnesses that cannot run at
    /// all — not installed, unspawnable, or rejected before doing any work (auth /
    /// no-credit quota). A real task failure or a timeout does NOT fall through
    /// (it would mask a real failure). Incompatible with a batch run and with
    /// --resume / --fork / --session / --stream. Every listed harness is validated
    /// up front, so a flag unsupported by ANY candidate is a usage error even if
    /// that harness is never reached — keeping the command valid for the whole set.
    #[arg(long, value_parser = run_mode_parser(), value_name = "MODE")]
    pub run_mode: Option<RunMode>,

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

    /// Record a normalized, cross-harness history of this run to disk (opt-in;
    /// off by default). Also settable via `history` in config or ONEHARNESS_HISTORY
    /// — most naturally in a user-level config.toml, to turn it on everywhere.
    /// Never written under --print-command. See `oneharness history` to view it.
    #[arg(long, conflicts_with = "no_history")]
    pub history: bool,

    /// Do NOT record history for this run, overriding `history` in config or
    /// ONEHARNESS_HISTORY.
    #[arg(long)]
    pub no_history: bool,

    /// Directory history is written to (default: <platform state dir>/oneharness/
    /// history; also `history_dir` in config or ONEHARNESS_HISTORY_DIR).
    #[arg(long, value_name = "DIR")]
    pub history_dir: Option<PathBuf>,

    /// Human-meaningful label for this session, shown by `oneharness history list`
    /// and resolvable by `oneharness history show`. Defaults to a slug of the
    /// first prompt.
    #[arg(long, value_name = "NAME")]
    pub history_name: Option<String>,

    /// Attach a validated KEY=VALUE label to every history record (repeatable).
    /// Labels from the CLI override the same keys from
    /// ONEHARNESS_HISTORY_LABELS and config `history_labels`.
    #[arg(long = "history-label", value_name = "KEY=VALUE")]
    pub history_label: Vec<String>,

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
pub struct InitArgs {
    /// Where to write the starter config (default: `oneharness.toml` in the
    /// current directory). An explicit path lets a tool scaffold differently
    /// named configs, e.g. `oneharness init oneharness.judge.toml`.
    #[arg(value_name = "PATH", default_value = "oneharness.toml")]
    pub path: PathBuf,

    /// Overwrite PATH if it already exists. Without it, an existing file is a
    /// loud error (nothing is written) so a scaffold never clobbers real config.
    #[arg(long)]
    pub force: bool,
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
pub struct MockArgs {
    /// Harness id whose hook protocol to speak (see `oneharness list`).
    #[arg(value_name = "ID")]
    pub harness: String,

    /// JSON ruleset deciding which tool calls to intercept and how (first
    /// matching rule wins): {"rules":[{"match":{"tool":…,"event_contains":…},
    /// "action":{"deny":{"message":…}} | {"rewrite":{"input":{…}}}}]}. Without
    /// it every call is allowed through and only spied on.
    #[arg(long, value_name = "PATH")]
    pub rules: Option<PathBuf>,

    /// Append one JSONL record per observed hook event (the raw event plus the
    /// action taken) to this file — the spy channel, recording the *original*
    /// tool call even when a rewrite substituted its input. Also settable via
    /// ONEHARNESS_SPY_FILE (the flag wins); absent means no spy log.
    #[arg(long, value_name = "PATH")]
    pub spy_file: Option<PathBuf>,
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
