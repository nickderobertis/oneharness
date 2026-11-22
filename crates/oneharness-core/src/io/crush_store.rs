//! Reading a finished `crush run`'s reply back from crush's own session store,
//! for a run whose stdout crush cut short (see `domain::crush_store`).

use std::path::PathBuf;
use std::time::Duration;

use crate::domain::crush_store::{self, RunStore, SessionChoice, StoreMiss};
use crate::domain::report::{RunResult, Status};
use crate::io::runner::{self, Job};

/// Long enough for crush to open and migrate its store; it answers in well
/// under a second. A reader that hangs past it, or that cannot be spawned, is
/// a store read that failed:
/// `runner::run_job` ends it at this deadline as it ends any job, and the run
/// keeps stdout's text as for any unreadable store. No journey drives a hung
/// reader: proving the bound end to end would hold the suite for the whole
/// deadline, and the deadline itself is the runner's, which its own tests
/// prove.
const STORE_READ_TIMEOUT: Duration = Duration::from_secs(20);

/// What reading one crush run's store back needs: the spawn context of the run
/// itself, so `crush session` opens the same store, and when the run began.
#[derive(Debug, Clone)]
pub(crate) struct CrushStore {
    bin: String,
    cwd: Option<PathBuf>,
    env: Vec<(String, String)>,
    env_remove: Vec<String>,
    /// The run's store, or why its argv named none oneharness can read.
    store: Result<RunStore, StoreMiss>,
    since_ms: i64,
}

impl CrushStore {
    /// The store `job`, a `crush run` planned at `since_ms`, writes to.
    pub(crate) fn for_job(job: &Job, since_ms: u128) -> Option<Self> {
        let bin = job.argv.first()?.clone();
        Some(Self {
            bin,
            cwd: job.cwd.clone(),
            env: job.env.clone(),
            env_remove: job.env_remove.clone(),
            store: crush_store::run_store(&job.argv),
            since_ms: i64::try_from(since_ms).unwrap_or(i64::MAX),
        })
    }

    /// Run one `crush session` read; its stdout, or why it gave none.
    fn read(&self, argv: Vec<String>) -> Result<String, StoreMiss> {
        let shown = argv[1..].join(" ");
        let capture = runner::run_job(&Job {
            argv,
            cwd: self.cwd.clone(),
            env: self.env.clone(),
            env_remove: self.env_remove.clone(),
            timeout: STORE_READ_TIMEOUT,
            stdin: None,
        });
        if capture.status == Status::Ok {
            return Ok(capture.stdout);
        }
        // A runner error is the deadline or a failed spawn (see
        // `STORE_READ_TIMEOUT` for why no journey drives either); otherwise
        // crush's own stderr says why, or, when it said nothing, its exit code.
        let why = capture
            .error
            .or_else(|| {
                let stderr = capture.stderr.trim();
                (!stderr.is_empty()).then(|| stderr.to_string())
            })
            .unwrap_or_else(|| match capture.exit_code {
                Some(code) => format!("exited {code} without a word"),
                None => "ended without an exit code".to_string(),
            });
        Err(StoreMiss::Unreadable(format!("`crush {shown}`: {why}")))
    }

    /// The reply of this run's turn, as the store holds it.
    fn stored_reply(&self) -> Result<String, StoreMiss> {
        let store = self.store.as_ref().map_err(Clone::clone)?;
        let session = match &store.session {
            SessionChoice::Named(session) => session.clone(),
            SessionChoice::Continued => {
                return Err(StoreMiss::Unidentified(
                    "`--continue` names no session".to_string(),
                ))
            }
            SessionChoice::Created => crush_store::created_session(
                &self.read(crush_store::list_argv(&self.bin, store))?,
                self.since_ms,
            )?,
        };
        crush_store::turn_reply(&self.read(crush_store::show_argv(&self.bin, &session, store))?)
    }
}

/// Replace a finished crush run's stdout `text` with the store's reply when
/// stdout carried only its start, and record in `text_source` where the text
/// came from. Returns whether `text` changed.
///
/// Only an `ok` run whose text is stdout's is read back: a failed run's reply
/// is not the answer, and a controlled run's text already came from crush's
/// server. When the store cannot be read, `text` stays stdout's and
/// `text_source` says why; a run that printed nothing and whose store is
/// unreadable keeps its `null` text and source.
pub(crate) fn recover(result: &mut RunResult, store: &CrushStore) -> bool {
    if result.status != Status::Ok || !matches!(result.text_source.as_deref(), None | Some("raw")) {
        return false;
    }
    let printed = result.text.clone().unwrap_or_default();
    match store.stored_reply() {
        Ok(stored) => match crush_store::recovered_reply(&printed, &stored) {
            Some(reply) => {
                result.text = Some(reply);
                result.text_source = Some(crush_store::RECOVERED_SOURCE.to_string());
                true
            }
            None => false,
        },
        Err(miss) => {
            let source = match &miss {
                StoreMiss::Unreadable(why) => {
                    eprintln!("oneharness: crush session store unavailable: {why}");
                    crush_store::UNAVAILABLE_SOURCE
                }
                StoreMiss::Unidentified(why) => {
                    eprintln!("oneharness: crush run's session not identified: {why}");
                    crush_store::UNIDENTIFIED_SOURCE
                }
            };
            if result.text.is_some() {
                result.text_source = Some(source.to_string());
            }
            false
        }
    }
}
