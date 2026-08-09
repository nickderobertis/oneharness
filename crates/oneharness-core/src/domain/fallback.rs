//! Fallback run mode: drive the selected harnesses in **priority order**,
//! stopping at the first one that actually runs the task — success *or* real
//! failure — and falling through only the ones that could not run at all. The
//! point is graceful degradation across a set of harnesses a repo declares it
//! supports: whichever one the user has installed and authenticated runs, and a
//! genuine task failure (a non-zero result, a timeout) is *never* mistaken for
//! "try the next harness".
//!
//! This module is pure policy only: the [`RunMode`] knob (accepted on the CLI
//! and in config) and the classification of one harness's outcome as a *startup
//! failure* (fall through) versus a *real run* (stop here). The command layer
//! owns the sequential spawning; keeping the order and the stop/continue verdict
//! here makes both unit-testable against the mock harness.
//!
//! The verdict is a function of the finished [`RunResult`] alone — status,
//! classified `failure_kind`, and [`RunWork`] evidence — so it is the same
//! whether the chain was driven buffered or under `run --stream`. There is no
//! streaming-specific rule.

use serde::{Deserialize, Serialize};

use crate::domain::report::{RunResult, Status};
use crate::domain::signals::FailureKind;

/// How the selected harnesses are run. Accepted as a CLI value (`--run-mode`,
/// parsed in the `oneharness` binary) and a config-file value (`run_mode`, via
/// `Deserialize`); the CLI parsing lives in the binary so this core crate stays
/// free of `clap`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunMode {
    /// Run every selected harness at once, each an independent subprocess, and
    /// report them all. The default — the historical behavior.
    Parallel,
    /// Run the selected harnesses in priority order (the `--harness` / config
    /// order, else registry order under `--all`), stopping at the first that
    /// actually runs the task; fall through only the candidates that cannot run
    /// at all (see [`startup_failure_reason`]).
    Fallback,
}

impl RunMode {
    /// Every mode, for the CLI's possible-value list.
    pub const ALL: [RunMode; 2] = [RunMode::Parallel, RunMode::Fallback];

    /// The CLI/JSON token for this mode.
    pub fn as_str(self) -> &'static str {
        match self {
            RunMode::Parallel => "parallel",
            RunMode::Fallback => "fallback",
        }
    }

    /// Parse a CLI/config token into a mode; `None` for an unrecognized token.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "parallel" => Some(RunMode::Parallel),
            "fallback" => Some(RunMode::Fallback),
            _ => None,
        }
    }
}

/// Whether a candidate's normalized result carries **evidence it did the task's
/// work** — the first thing [`startup_failure_reason`] consults.
///
/// Two independent witnesses, either of which is decisive:
///
/// - **Tool events.** A recorded tool call is the harness acting on the task.
/// - **Usage accounting.** [`Usage::reports_billed_work`][billed] — the same definition
///   `signals::record_reports_work` classifies a raw harness record with, so the
///   two readings of "billed" are one contract with one implementation.
///
/// [billed]: crate::domain::signals::Usage::reports_billed_work
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunWork {
    /// The candidate ran the task, whatever its terminal record then said.
    Done,
    /// Nothing says the candidate got as far as doing work.
    None,
}

impl RunWork {
    /// Read the evidence off a finished result. A `spawn_error` nulls every
    /// signal by contract, so its evidence is always [`RunWork::None`].
    pub fn from_result(result: &RunResult) -> Self {
        let used_tools = result.events.as_ref().is_some_and(|e| !e.is_empty());
        if used_tools || result.usage.reports_billed_work() {
            RunWork::Done
        } else {
            RunWork::None
        }
    }
}

/// Why a harness **could not run the task at all** — the conditions that make a
/// fallback run fall through to the next candidate. Returns a short reason token
/// when the outcome is a startup failure, or `None` when the harness *did* run
/// (so a fallback run stops there).
///
/// [`RunWork::Done`] short-circuits every reason below: falling through a
/// candidate that already worked burns the next one's quota re-running what
/// happened. This is the "work done, not error text" rule the Claude
/// session-limit classifier applies (issue #1211), lifted to the whole verdict so
/// it also covers the surfaces with no accounting of their own to gate on (a
/// generic `401` scanned out of a transcript, Codex's `turn.failed` usage limit).
///
/// With no work evidence the reasons are:
///
/// - [`Status::Skipped`] → `"not-installed"` (the binary was not on PATH).
/// - [`Status::SpawnError`] → `"spawn-error"` (resolved but could not execute).
/// - [`Status::Nonzero`], [`Status::Ok`] or [`Status::Skipped`] with `failure_kind == "auth"` →
///   `"auth"` (rejected before doing any work — bad/absent credentials). A
///   classified `auth` outranks the skip reason: a candidate the command layer
///   declined to run because the identity it selects is unprovisioned (an
///   `env_from` home directory that is not on disk) reads as the credential
///   problem it is, not as a missing binary. [`Status::Skipped`] is listed only
///   for `auth`, because that refusal is the only one decided before a spawn;
///   every other kind is read out of a process that ran.
/// - [`Status::Nonzero`] or [`Status::Ok`] with `failure_kind == "quota"` → `"quota"` (the account
///   has no credit/quota to do work — a provisioning problem like `auth`).
/// - [`Status::Nonzero`] or [`Status::Ok`] with `failure_kind == "session_not_found"` →
///   `"session-not-found"` (the run asked to continue a session this identity's
///   store has never seen, so it refused before doing anything). It belongs with
///   `auth` and `quota` for the same reason: the *task* is fine and the next
///   candidate can still do it. Leaving it unclassified is what stranded a chain
///   of five identities on the one that minted the token — the token is scoped to
///   a single identity's session namespace, so every other candidate's resume is
///   guaranteed to fail this way, and reading that as a real task failure stopped
///   the chain at candidate one with four authenticated identities untried.
///
/// Everything else is a **real run**, so `None`: a clean [`Status::Ok`]; a
/// [`Status::Timeout`] (a genuine, if slow, run — falling through it would let a
/// long real run masquerade as a setup problem); a [`Status::Cancelled`] one
/// (nothing about the candidate failed, and falling through would spawn the very
/// next harness the caller just cancelled); a plain non-zero task failure;
/// and — by default — a non-zero classified `rate_limit` (a transient condition
/// of a *working, authenticated* harness) or `model_not_found` (a configuration
/// mistake the user should see, not silently route around). A [`Status::Planned`]
/// dry-run row is not a run at all and is `None` too (the fallback driver never
/// executes under `--print-command`).
///
/// `model_fallback` widens the fall-through set for a run that is **trying
/// several models in priority order** (`--model` given more than once, or config
/// `models`). There, a per-model rejection *is* the signal to try the next
/// candidate: `model_not_found` (this model is unavailable — reason
/// `"model-not-found"`) and `rate_limit` (this model can't serve the request
/// right now — reason `"rate-limit"`) both fall through, since the point of a
/// model list is graceful degradation across models exactly as the harness list
/// degrades across harnesses. With a single model (`model_fallback == false`) the
/// historical rule stands: both stop the chain.
pub fn startup_failure_reason(
    status: Status,
    failure_kind: Option<FailureKind>,
    model_fallback: bool,
    work: RunWork,
) -> Option<&'static str> {
    if work == RunWork::Done {
        return None;
    }
    match (status, failure_kind) {
        (_, Some(FailureKind::Auth))
            if matches!(status, Status::Ok | Status::Nonzero | Status::Skipped) =>
        {
            Some("auth")
        }
        (_, Some(FailureKind::Quota)) if matches!(status, Status::Ok | Status::Nonzero) => {
            Some("quota")
        }
        (_, Some(FailureKind::SessionNotFound))
            if matches!(status, Status::Ok | Status::Nonzero) =>
        {
            Some("session-not-found")
        }
        (Status::Skipped, _) => Some("not-installed"),
        (Status::SpawnError, _) => Some("spawn-error"),
        (Status::Nonzero, _) => match failure_kind {
            // Only when a model list is being tried: an unusable/over-limit model
            // means "try the next model", not "stop with a real failure".
            Some(FailureKind::ModelNotFound) if model_fallback => Some("model-not-found"),
            Some(FailureKind::RateLimit) if model_fallback => Some("rate-limit"),
            _ => None,
        },
        (Status::Ok | Status::Timeout | Status::Cancelled | Status::Planned, _) => None,
    }
}

/// Whether `status`/`failure_kind` mean the harness could not run the task at
/// all, so a fallback run should try the next candidate. The boolean face of
/// [`startup_failure_reason`].
pub fn is_startup_failure(
    status: Status,
    failure_kind: Option<FailureKind>,
    model_fallback: bool,
    work: RunWork,
) -> bool {
    startup_failure_reason(status, failure_kind, model_fallback, work).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::signals::Usage;

    #[test]
    fn mode_token_round_trips_for_every_variant() {
        for m in RunMode::ALL {
            assert_eq!(RunMode::parse(m.as_str()), Some(m));
        }
        assert_eq!(RunMode::parse("nope"), None);
        // The serialized form matches the CLI token (kebab-case).
        assert_eq!(
            serde_json::to_string(&RunMode::Fallback).unwrap(),
            "\"fallback\""
        );
        assert_eq!(
            serde_json::to_string(&RunMode::Parallel).unwrap(),
            "\"parallel\""
        );
    }

    #[test]
    fn startup_failures_fall_through() {
        // Structural "could not run": not installed, or resolved-but-unspawnable.
        assert_eq!(
            startup_failure_reason(Status::Skipped, None, false, RunWork::None),
            Some("not-installed")
        );
        assert_eq!(
            startup_failure_reason(Status::SpawnError, None, false, RunWork::None),
            Some("spawn-error")
        );
        // Provisioning "could not run": rejected before any work.
        assert_eq!(
            startup_failure_reason(
                Status::Nonzero,
                Some(FailureKind::Auth),
                false,
                RunWork::None
            ),
            Some("auth")
        );
        assert_eq!(
            startup_failure_reason(
                Status::Nonzero,
                Some(FailureKind::Quota),
                false,
                RunWork::None
            ),
            Some("quota")
        );
        // A candidate that was not run *because* its identity is unprovisioned
        // carries the credential reason, not the missing-binary one.
        assert_eq!(
            startup_failure_reason(
                Status::Skipped,
                Some(FailureKind::Auth),
                false,
                RunWork::None
            ),
            Some("auth")
        );
        for status in [Status::Skipped, Status::SpawnError] {
            assert!(is_startup_failure(status, None, false, RunWork::None));
        }
        assert!(is_startup_failure(
            Status::Nonzero,
            Some(FailureKind::Auth),
            false,
            RunWork::None
        ));
        assert!(is_startup_failure(
            Status::Nonzero,
            Some(FailureKind::Quota),
            false,
            RunWork::None
        ));
        // A resume the harness could not resolve: this identity does not hold the
        // session, but the task is untouched, so the next candidate gets it.
        assert_eq!(
            startup_failure_reason(
                Status::Nonzero,
                Some(FailureKind::SessionNotFound),
                false,
                RunWork::None
            ),
            Some("session-not-found")
        );
        assert!(is_startup_failure(
            Status::Nonzero,
            Some(FailureKind::SessionNotFound),
            false,
            RunWork::None
        ));
        // Some harnesses report the same refusal on a zero exit.
        assert_eq!(
            startup_failure_reason(
                Status::Ok,
                Some(FailureKind::SessionNotFound),
                false,
                RunWork::None
            ),
            Some("session-not-found")
        );
    }

    #[test]
    fn a_real_run_does_not_fall_through() {
        // A clean run, a slow run that timed out, and a dry-run row all stay.
        assert_eq!(
            startup_failure_reason(Status::Ok, None, false, RunWork::None),
            None
        );
        assert_eq!(
            startup_failure_reason(Status::Timeout, None, false, RunWork::None),
            None
        );
        assert_eq!(
            startup_failure_reason(Status::Planned, None, false, RunWork::None),
            None
        );
        assert!(!is_startup_failure(Status::Ok, None, false, RunWork::None));
        assert!(!is_startup_failure(
            Status::Timeout,
            None,
            false,
            RunWork::None
        ));
    }

    #[test]
    fn a_cancelled_candidate_does_not_fall_through() {
        // Nothing about the candidate failed — the caller stopped it. Falling
        // through would spawn the very next harness the cancellation was meant
        // to prevent, which is the opposite of what was asked for.
        assert_eq!(
            startup_failure_reason(Status::Cancelled, None, false, RunWork::None),
            None
        );
        assert!(!is_startup_failure(
            Status::Cancelled,
            None,
            false,
            RunWork::None
        ));
    }

    #[test]
    fn a_real_failure_does_not_fall_through() {
        // A plain non-zero task failure (no classified reason) is a real run.
        assert_eq!(
            startup_failure_reason(Status::Nonzero, None, false, RunWork::None),
            None
        );
        assert!(!is_startup_failure(
            Status::Nonzero,
            None,
            false,
            RunWork::None
        ));
        // Transient / configuration / did-run reasons are NOT setup failures for a
        // single-model run: falling through a 429 would mask a working harness's
        // real hiccup, an unknown model is a config mistake the user sees, and a
        // deferred-tool dead-end is a harness that *ran* (so the chain stops there).
        for kind in [
            FailureKind::RateLimit,
            FailureKind::ModelNotFound,
            FailureKind::ToolDeferred,
        ] {
            assert_eq!(
                startup_failure_reason(Status::Nonzero, Some(kind), false, RunWork::None),
                None,
                "failure_kind {kind:?} must not fall through without a model list"
            );
            assert!(!is_startup_failure(
                Status::Nonzero,
                Some(kind),
                false,
                RunWork::None
            ));
        }
    }

    #[test]
    fn model_errors_fall_through_only_with_a_model_list() {
        // Trying several models in order: an unusable or over-limit model is
        // "try the next model", so both fall through with their own reason.
        assert_eq!(
            startup_failure_reason(
                Status::Nonzero,
                Some(FailureKind::ModelNotFound),
                true,
                RunWork::None
            ),
            Some("model-not-found")
        );
        assert_eq!(
            startup_failure_reason(
                Status::Nonzero,
                Some(FailureKind::RateLimit),
                true,
                RunWork::None
            ),
            Some("rate-limit")
        );
        assert!(is_startup_failure(
            Status::Nonzero,
            Some(FailureKind::ModelNotFound),
            true,
            RunWork::None
        ));
        assert!(is_startup_failure(
            Status::Nonzero,
            Some(FailureKind::RateLimit),
            true,
            RunWork::None
        ));
        // A plain task failure still stops the chain even with a model list — it
        // is a real run, not a per-model provisioning problem. A deferred-tool
        // dead-end likewise ran, so it stops the chain too.
        assert_eq!(
            startup_failure_reason(Status::Nonzero, None, true, RunWork::None),
            None
        );
        assert_eq!(
            startup_failure_reason(
                Status::Nonzero,
                Some(FailureKind::ToolDeferred),
                true,
                RunWork::None
            ),
            None
        );
        // The structural/provisioning reasons are unchanged by the model flag.
        assert_eq!(
            startup_failure_reason(Status::Skipped, None, true, RunWork::None),
            Some("not-installed")
        );
        assert_eq!(
            startup_failure_reason(
                Status::Nonzero,
                Some(FailureKind::Auth),
                true,
                RunWork::None
            ),
            Some("auth")
        );
    }

    #[test]
    fn a_candidate_that_did_work_never_falls_through() {
        // Every pair below falls through without work evidence, so each one shows
        // the short circuit overriding a reason that would otherwise apply.
        for (status, kind, model_fallback) in [
            (Status::Nonzero, Some(FailureKind::Auth), false),
            (Status::Nonzero, Some(FailureKind::Quota), false),
            (Status::Ok, Some(FailureKind::Quota), false),
            (Status::Nonzero, Some(FailureKind::SessionNotFound), false),
            (Status::Nonzero, Some(FailureKind::RateLimit), true),
            (Status::Nonzero, Some(FailureKind::ModelNotFound), true),
            (Status::Skipped, None, false),
            (Status::SpawnError, None, false),
        ] {
            assert!(
                startup_failure_reason(status, kind, model_fallback, RunWork::None).is_some(),
                "{status:?}/{kind:?} must fall through with no work evidence"
            );
            assert_eq!(
                startup_failure_reason(status, kind, model_fallback, RunWork::Done),
                None,
                "{status:?}/{kind:?} did work, so it must not fall through"
            );
            assert!(!is_startup_failure(
                status,
                kind,
                model_fallback,
                RunWork::Done
            ));
        }
    }

    #[test]
    fn work_evidence_reads_tool_events_and_billed_usage() {
        let mut result = zero_work_result();
        assert_eq!(RunWork::from_result(&result), RunWork::None);
        // ...and so is a result with no accounting at all (a bare limit line on
        // stderr carries none), the shared `Usage::reports_billed_work` rule.
        result.usage = Usage::default();
        assert_eq!(RunWork::from_result(&result), RunWork::None);
        result.events = Some(Vec::new());
        assert_eq!(RunWork::from_result(&result), RunWork::None);

        // Any single billed count is decisive.
        for billed in [
            Usage {
                input_tokens: Some(1),
                ..Usage::default()
            },
            Usage {
                output_tokens: Some(1),
                ..Usage::default()
            },
            Usage {
                cache_read_tokens: Some(1),
                ..Usage::default()
            },
            Usage {
                cache_write_tokens: Some(1),
                ..Usage::default()
            },
            Usage {
                cost_usd: Some(0.01),
                ..Usage::default()
            },
        ] {
            let mut worked = zero_work_result();
            worked.usage = billed;
            assert_eq!(RunWork::from_result(&worked), RunWork::Done);
        }

        // A tool call alone is decisive too, with no usage reported at all.
        let mut used_tools = zero_work_result();
        used_tools.events = Some(vec![tool_call()]);
        assert_eq!(RunWork::from_result(&used_tools), RunWork::Done);
    }

    /// A result shaped like the real zero-work Claude session-limit rejection.
    fn zero_work_result() -> RunResult {
        RunResult {
            harness: "claude-code".to_string(),
            variant: None,
            harness_id: "claude-code".to_string(),
            bin: "claude".to_string(),
            available: true,
            status: Status::Nonzero,
            prompt: None,
            model: None,
            exit_code: Some(1),
            duration_ms: Some(401),
            telemetry: None,
            command: vec!["claude".to_string()],
            output_format: crate::domain::report::OutputFormat::Json,
            text: Some("You've hit your session limit".to_string()),
            text_source: Some("json:result".to_string()),
            usage: Usage {
                input_tokens: Some(0),
                output_tokens: Some(0),
                cache_read_tokens: Some(0),
                cache_write_tokens: Some(0),
                cost_usd: Some(0.0),
            },
            usage_source: Some("json".to_string()),
            session_id: None,
            events: None,
            events_source: None,
            structured: None,
            schema_valid: None,
            schema_attempts: None,
            schema_error: None,
            failure_kind: Some(FailureKind::Quota),
            failure_kind_source: Some("stdout".to_string()),
            stdout: String::new(),
            stderr: String::new(),
            error: None,
        }
    }

    fn tool_call() -> crate::domain::events::ActionEvent {
        crate::domain::events::ActionEvent {
            kind: "tool_call".to_string(),
            name: Some("Bash".to_string()),
            input: None,
            output: None,
            index: 0,
            tool_call_id: None,
            started_at: None,
            finished_at: None,
            duration_ms: None,
            status: None,
            timing_source: None,
        }
    }
}
