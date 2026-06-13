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

    #[error("no prompt provided: pass --prompt <text> or --prompt-file <path>")]
    NoPrompt,

    #[error("--resume needs exactly one harness (a session belongs to one harness), but {count} were selected: {selected}")]
    ResumeMultipleHarnesses { count: usize, selected: String },

    #[error("harness `{id}` does not support --resume. supported: {supported}")]
    ResumeUnsupported { id: String, supported: String },

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

    #[error("could not write harness config `{path}`: {source}")]
    HarnessConfigWrite {
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

    #[error("could not read config file `{path}`: {source}")]
    ConfigRead {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid config file `{path}`: {message}")]
    ConfigInvalid { path: String, message: String },

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

    #[error("failed to write JSON output: {0}")]
    Serialize(#[from] serde_json::Error),
}
