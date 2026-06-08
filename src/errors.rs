//! Typed errors surfaced only at the application boundary (see `dispatch`).
//!
//! These cover usage and configuration faults — the kind that should abort the
//! process. A harness's own behavior (missing binary, non-zero exit, hang) is
//! never an error here: it is recorded as data in the JSON report.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum OneharnessError {
    #[error("no harness selected: pass --all or --harness <id> (see `oneharness list`)")]
    NoSelection,

    #[error("unknown harness id `{id}`. valid ids: {valid}")]
    UnknownHarness { id: String, valid: String },

    #[error("no prompt provided: pass --prompt <text> or --prompt-file <path>")]
    NoPrompt,

    #[error("could not read prompt file `{path}`: {source}")]
    PromptFile {
        path: String,
        #[source]
        source: std::io::Error,
    },

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
