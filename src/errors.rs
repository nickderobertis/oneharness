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

    #[error("harness `{id}` cannot enforce `{setting}` through its headless invocation, so the rule would not apply — refusing to run rather than silently dropping it. harnesses that support it: {supported}. (scope the setting under [harness.<id>] in oneharness.toml, or narrow the selection)")]
    UnenforceableSetting {
        id: String,
        setting: &'static str,
        supported: String,
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
