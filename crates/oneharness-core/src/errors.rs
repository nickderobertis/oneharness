//! Typed errors surfaced only at the application boundary (see `dispatch`).
//!
//! These cover usage and configuration faults — the kind that should abort the
//! process. A harness's own behavior (missing binary, non-zero exit, hang) is
//! never an error here: it is recorded as data in the JSON report.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum OneharnessError {
    #[error("no harness selected: pass --all or --harness <id>, or set `all`/`harnesses` in oneharness.toml (see `oneharness list`)")]
    NoSelection,

    #[error("unknown harness id `{id}`. valid ids: {valid}")]
    UnknownHarness { id: String, valid: String },

    #[error(
        "unknown harness variant `{id}`; declare it under `[harness.{base}.variant.{variant}]`"
    )]
    UnknownHarnessVariant {
        // llmlint: ignore[invalid_states_unrepresentable] Error payloads intentionally preserve exact untrusted text that failed validation; VariantName cannot represent the malformed input the diagnostic must quote.
        id: String,
        base: String,
        variant: String,
    },

    #[error("could not read variant environment file `{path}`: {source}")]
    VariantEnvFile {
        path: String,
        source: std::io::Error,
    },

    #[error("variant environment file `{path}` must be a regular file readable only by its owner (mode 0600 or stricter)")]
    VariantEnvFilePermissions { path: String },

    #[error("invalid line {line} in variant environment file `{path}`; expected KEY=VALUE")]
    VariantEnvFileLine { path: String, line: usize },

    #[error("variant environment indirection `{name}` is not set in the parent process")]
    VariantEnvSourceMissing { name: String },

    #[error("harness variants `{first}` and `{second}` declare conflicting sync settings for the shared `{base}` config file")]
    VariantSyncConflict {
        base: String,
        first: String,
        second: String,
    },

    #[error("--mock-harness `{id}` must name a selected harness")]
    MockHarnessNotSelected { id: String },

    #[error("could not locate the running oneharness executable for --mock-harness: {0}")]
    MockHarnessExecutable(std::io::Error),

    #[error("no prompt provided: pass --prompt <text> or --prompt-file <path>")]
    NoPrompt,

    #[error("--prompt-file - (stdin) can be given only once, but it was passed {count} times")]
    MultipleStdinPrompts { count: usize },

    #[error("stdin ('-') can be read only once, but {count} inputs request it (across --prompt-file and --system-file)")]
    MultipleStdinConsumers { count: usize },

    #[error("a batch run (more than one prompt) needs exactly one harness (a shared cache prefix is per harness/model), but {count} were selected: {selected}. Select one with --harness <id>")]
    BatchMultipleHarnesses { count: usize, selected: String },

    #[error("a batch run (more than one prompt) cannot be combined with --resume/--fork (those continue a single session with a single prompt)")]
    BatchResume,

    #[error("`--run-mode fallback` is incompatible with {with} ({why})")]
    FallbackConflict {
        with: &'static str,
        why: &'static str,
    },

    #[error("a multi-model run (more than one --model / config `models`) is incompatible with {with} ({why})")]
    MultiModelConflict {
        with: &'static str,
        why: &'static str,
    },

    #[error("--resume needs exactly one harness (a session belongs to one harness), but {count} were selected: {selected}")]
    ResumeMultipleHarnesses { count: usize, selected: String },

    #[error("harness `{id}` does not support --resume. supported: {supported}")]
    ResumeUnsupported { id: String, supported: String },

    #[error("harness `{id}` does not support --fork (it resumes linearly, appending in place). supported: {supported}")]
    ForkUnsupported { id: String, supported: String },

    #[error("--session needs exactly one harness in the parallel run mode (a named session belongs to one harness), but {count} were selected: {selected}. Use --run-mode fallback to bind a named session to a priority chain (it anchors to the first session-capable harness).")]
    SessionMultipleHarnesses { count: usize, selected: String },

    #[error("--session cannot be combined with a batch run (more than one prompt): a named session is one continued conversation, not a fan-out")]
    SessionBatch,

    #[error("harness `{id}` does not support --session: it exposes no session id headlessly, so a named handle cannot be mapped to it. supported: {supported}")]
    SessionUnsupported { id: String, supported: String },

    #[error("harness `{id}` cannot capture --session under explicitly selected output format `{format}`: its session id is emitted only in {supported}. Remove --output-format/config `output_format` to let oneharness select a session-bearing format")]
    SessionOutputFormat {
        id: String,
        format: String,
        supported: String,
    },

    #[error("session `{name}` was created on harness `{was}`, so it cannot be continued on `{now}` (a named session is bound to one harness; use a different --session name)")]
    SessionHarnessConflict {
        name: String,
        was: String,
        now: String,
    },

    #[error("cannot resolve a session store directory (no --session-dir and no platform state dir): set --session-dir <DIR>")]
    SessionNoStore,

    #[error("harness `{id}` does not support `--mode {mode}`. supported modes: {supported}")]
    ModeUnsupported {
        id: String,
        mode: String,
        supported: String,
    },

    #[error("harness `{id}` cannot take a reasoning/effort setting headlessly: it exposes no reasoning flag (effort is provider/model config there). harnesses that can: {supported}")]
    ReasoningUnsupported { id: String, supported: String },

    #[error("harness `{id}` delivers reasoning effort through the model id (`--model 'MODEL[effort=…]'`), so `--reasoning` needs a model — set --model or `[harness.{id}] model`")]
    ReasoningNeedsModel { id: String },

    #[error("could not read harness config `{path}`: {source}")]
    HarnessConfigRead {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("cannot sync into `{path}`: {message} (oneharness only rewrites files it can parse, so it never destroys content it does not understand)")]
    HarnessConfigUnmergeable { path: String, message: String },

    #[error("harness `{id}` has no hook mapping, so oneharness cannot install a hook into it")]
    HookUnsupported { id: String },

    #[error("harness `{id}` has no user-global hook location, so oneharness cannot install a global hook into it")]
    HookGlobalUnsupported { id: String },

    #[error("cannot resolve the user-global hook directory for `{id}`: {var} is not set")]
    HookGlobalDirMissing { id: String, var: &'static str },

    #[error("`oneharness sync --global` installs hooks only, but permission rules or `settings` are configured for `{id}` — those are project-scoped (sync them without --global, or remove them)")]
    GlobalSyncOnlyHooks { id: String },

    #[error(
        "harness `{id}` has no pre-tool gate, so `oneharness gate {id}` cannot emit a verdict"
    )]
    GateUnsupported { id: String },

    #[error("could not read mock rules file `{path}`: {source}")]
    MockRulesFile {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid mock rules file `{path}`: {message}")]
    MockRulesInvalid { path: String, message: String },

    #[error("harness `{id}` cannot express the mock action `{action}`, so `oneharness mock {id}` refuses this ruleset (see `mock_rewrite`/`supports_mock_deny` in `oneharness list`)")]
    MockActionUnsupported { id: String, action: &'static str },

    #[error(
        "harness `{id}` cannot take a one-shot mock hook (`--mock-rules`/`--spy-file`): {reason}"
    )]
    MockDeliveryUnsupported { id: String, reason: &'static str },

    #[error("cannot embed `{path}` into a hook command: it contains whitespace (hook commands are tokenized on spaces by some harnesses; use space-free paths)")]
    MockPathWhitespace { path: String },

    #[error("could not wire the one-shot mock hook: {message}")]
    MockSetup { message: String },

    #[error("could not write harness config `{path}`: {source}")]
    HarnessConfigWrite {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("{path} already exists; pass --force to overwrite it (nothing was written)")]
    InitFileExists { path: String },

    #[error("could not write starter config `{path}`: {source}")]
    InitWrite {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("could not read prompt file `{path}`: {source}")]
    PromptFile {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("could not read system prompt file `{path}`: {source}")]
    SystemFile {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("could not read schema file `{path}`: {source}")]
    SchemaFile {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid --schema: {0}")]
    Schema(String),

    #[error("invalid --stream: {0}")]
    StreamInvalid(String),

    #[error("could not read config file `{path}`: {source}")]
    ConfigRead {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid config file `{path}`: {message}")]
    ConfigInvalid { path: String, message: String },

    #[error("invalid environment-variable config override: {0}")]
    EnvConfigInvalid(String),

    #[error("invalid --bin override `{0}`: expected the form ID=PATH")]
    BadBinOverride(String),

    #[error("invalid --env `{0}`: expected the form KEY=VALUE")]
    BadEnv(String),

    #[error("could not write output to `{path}`: {source}")]
    OutputDir {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("could not access history under `{path}`: {source}")]
    HistoryIo {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("no history directory: pass --history-dir, set `history_dir` in config, or ONEHARNESS_HISTORY_DIR (a default under the platform state dir could not be resolved)")]
    HistoryNoDir,

    #[error("harness `{id}` needs output format `{required}` for history telemetry, but `{selected}` was selected; remove the explicit format or incompatible native schema")]
    HistoryTelemetryFormat {
        id: String,
        required: String,
        selected: String,
    },

    #[error("history record `{id}` was not found")]
    HistoryNotFound { id: String },

    #[error("invalid history label: {0}")]
    HistoryLabelInvalid(String),

    #[error("invalid history cursor `{value}`: expected a UUID")]
    HistoryCursorInvalid { value: String },

    #[error("failed to write JSON output: {0}")]
    Serialize(#[from] serde_json::Error),
}
