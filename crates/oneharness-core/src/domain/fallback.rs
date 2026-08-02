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

use serde::{Deserialize, Serialize};

use crate::domain::report::Status;
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

/// Why a harness **could not run the task at all** — the conditions that make a
/// fallback run fall through to the next candidate. Returns a short reason token
/// when the outcome is a startup failure, or `None` when the harness *did* run
/// (so a fallback run stops there):
///
/// - [`Status::Skipped`] → `"not-installed"` (the binary was not on PATH).
/// - [`Status::SpawnError`] → `"spawn-error"` (resolved but could not execute).
/// - [`Status::Nonzero`] or [`Status::Ok`] with `failure_kind == "auth"` → `"auth"` (rejected
///   before doing any work — bad/absent credentials).
/// - [`Status::Nonzero`] or [`Status::Ok`] with `failure_kind == "quota"` → `"quota"` (the account
///   has no credit/quota to do work — a provisioning problem like `auth`).
///
/// Everything else is a **real run**, so `None`: a clean [`Status::Ok`]; a
/// [`Status::Timeout`] (a genuine, if slow, run — falling through it would let a
/// long real run masquerade as a setup problem); a plain non-zero task failure;
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
) -> Option<&'static str> {
    match (status, failure_kind) {
        (_, Some(FailureKind::Auth)) if matches!(status, Status::Ok | Status::Nonzero) => {
            Some("auth")
        }
        (_, Some(FailureKind::Quota)) if matches!(status, Status::Ok | Status::Nonzero) => {
            Some("quota")
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
        (Status::Ok | Status::Timeout | Status::Planned, _) => None,
    }
}

/// Whether `status`/`failure_kind` mean the harness could not run the task at
/// all, so a fallback run should try the next candidate. The boolean face of
/// [`startup_failure_reason`].
pub fn is_startup_failure(
    status: Status,
    failure_kind: Option<FailureKind>,
    model_fallback: bool,
) -> bool {
    startup_failure_reason(status, failure_kind, model_fallback).is_some()
}

/// What a fallback chain does with a candidate that was driven under `--stream`.
///
/// The extra input over [`startup_failure_reason`] is `streamed`: whether this
/// candidate already wrote a normalized event to the consumer's stdout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamVerdict {
    /// The candidate could not run the task at all and streamed nothing, so the
    /// chain moves on. The reason token is [`startup_failure_reason`]'s.
    FallThrough(&'static str),
    /// The candidate ran the task (well or badly): the chain stops here.
    Ran,
    /// The candidate streamed events — proof it ran — so the chain stops here
    /// even though its terminal classification alone (the carried reason token)
    /// would have fallen through. See [`stream_verdict`].
    Committed(&'static str),
}

/// The fallback verdict for one candidate driven under `--stream`.
///
/// A streaming run publishes each normalized event to the consumer the instant
/// it is observed, and a consumer acts on what it reads. The chain must
/// therefore never emit events from a candidate it goes on to discard: the
/// consumer would act on a harness whose output is not the answer, then see a
/// second harness's events arrive on the same stdout.
///
/// The reconciling rule is **an emitted event commits the candidate**. Events
/// are tool calls and their observations, so writing one is direct evidence
/// that the harness *ran the task* — which is exactly the condition under which
/// fallback stops. Everything that falls through does so *without* running: not
/// installed and never spawned, unspawnable, or rejected by the provider before
/// doing any work (`auth`, and a `quota` rejection that the classifier already
/// gates on zero work done). None of those produce a tool event, so
/// [`StreamVerdict::Committed`] cannot fire for them and a fallen-through
/// candidate is guaranteed to have published nothing.
///
/// [`StreamVerdict::Committed`] is the residue: a candidate that made tool calls
/// and *then* hit a classification the terminal record alone reads as a startup
/// failure (a provider surface with no work accounting to gate on, such as
/// Codex's `turn.failed` usage limit). Observing the run beats re-reading its
/// final record — the harness demonstrably ran — so the chain stops there, and
/// the command layer says so on stderr. The result keeps its honest
/// `failure_kind`, so the caller still sees why the run failed.
pub fn stream_verdict(
    streamed: bool,
    status: Status,
    failure_kind: Option<FailureKind>,
    model_fallback: bool,
) -> StreamVerdict {
    match startup_failure_reason(status, failure_kind, model_fallback) {
        Some(reason) if streamed => StreamVerdict::Committed(reason),
        Some(reason) => StreamVerdict::FallThrough(reason),
        None => StreamVerdict::Ran,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            startup_failure_reason(Status::Skipped, None, false),
            Some("not-installed")
        );
        assert_eq!(
            startup_failure_reason(Status::SpawnError, None, false),
            Some("spawn-error")
        );
        // Provisioning "could not run": rejected before any work.
        assert_eq!(
            startup_failure_reason(Status::Nonzero, Some(FailureKind::Auth), false),
            Some("auth")
        );
        assert_eq!(
            startup_failure_reason(Status::Nonzero, Some(FailureKind::Quota), false),
            Some("quota")
        );
        for status in [Status::Skipped, Status::SpawnError] {
            assert!(is_startup_failure(status, None, false));
        }
        assert!(is_startup_failure(
            Status::Nonzero,
            Some(FailureKind::Auth),
            false
        ));
        assert!(is_startup_failure(
            Status::Nonzero,
            Some(FailureKind::Quota),
            false
        ));
    }

    #[test]
    fn a_real_run_does_not_fall_through() {
        // A clean run, a slow run that timed out, and a dry-run row all stay.
        assert_eq!(startup_failure_reason(Status::Ok, None, false), None);
        assert_eq!(startup_failure_reason(Status::Timeout, None, false), None);
        assert_eq!(startup_failure_reason(Status::Planned, None, false), None);
        assert!(!is_startup_failure(Status::Ok, None, false));
        assert!(!is_startup_failure(Status::Timeout, None, false));
    }

    #[test]
    fn a_real_failure_does_not_fall_through() {
        // A plain non-zero task failure (no classified reason) is a real run.
        assert_eq!(startup_failure_reason(Status::Nonzero, None, false), None);
        assert!(!is_startup_failure(Status::Nonzero, None, false));
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
                startup_failure_reason(Status::Nonzero, Some(kind), false),
                None,
                "failure_kind {kind:?} must not fall through without a model list"
            );
            assert!(!is_startup_failure(Status::Nonzero, Some(kind), false));
        }
    }

    #[test]
    fn a_candidate_that_streamed_nothing_keeps_the_plain_verdict() {
        // With no event published, the streaming verdict is exactly the
        // classification — the chain behaves as it always has.
        assert_eq!(
            stream_verdict(false, Status::Skipped, None, false),
            StreamVerdict::FallThrough("not-installed")
        );
        assert_eq!(
            stream_verdict(false, Status::Nonzero, Some(FailureKind::Quota), false),
            StreamVerdict::FallThrough("quota")
        );
        assert_eq!(
            stream_verdict(false, Status::Nonzero, Some(FailureKind::Auth), false),
            StreamVerdict::FallThrough("auth")
        );
        assert_eq!(
            stream_verdict(false, Status::Ok, None, false),
            StreamVerdict::Ran
        );
        assert_eq!(
            stream_verdict(false, Status::Timeout, None, false),
            StreamVerdict::Ran
        );
        assert_eq!(
            stream_verdict(false, Status::Nonzero, None, false),
            StreamVerdict::Ran
        );
    }

    #[test]
    fn a_streamed_candidate_is_committed_and_never_falls_through() {
        // Publishing an event is evidence the harness ran the task, so the chain
        // stops there — a fallen-through candidate can never have published one.
        for (status, kind) in [
            (Status::Nonzero, Some(FailureKind::Auth)),
            (Status::Nonzero, Some(FailureKind::Quota)),
            (Status::Ok, Some(FailureKind::Quota)),
        ] {
            let reason = startup_failure_reason(status, kind, false).expect("a startup failure");
            assert_eq!(
                stream_verdict(true, status, kind, false),
                StreamVerdict::Committed(reason)
            );
        }
        // A candidate that never spawned cannot have streamed, so the combination
        // is unreachable; the rule still reports it as committed rather than
        // silently discarding published events.
        assert_eq!(
            stream_verdict(true, Status::SpawnError, None, false),
            StreamVerdict::Committed("spawn-error")
        );
        // A real run is `Ran` whether or not it published events.
        assert_eq!(
            stream_verdict(true, Status::Ok, None, false),
            StreamVerdict::Ran
        );
        assert_eq!(
            stream_verdict(true, Status::Nonzero, Some(FailureKind::RateLimit), false),
            StreamVerdict::Ran
        );
    }

    #[test]
    fn model_errors_fall_through_only_with_a_model_list() {
        // Trying several models in order: an unusable or over-limit model is
        // "try the next model", so both fall through with their own reason.
        assert_eq!(
            startup_failure_reason(Status::Nonzero, Some(FailureKind::ModelNotFound), true),
            Some("model-not-found")
        );
        assert_eq!(
            startup_failure_reason(Status::Nonzero, Some(FailureKind::RateLimit), true),
            Some("rate-limit")
        );
        assert!(is_startup_failure(
            Status::Nonzero,
            Some(FailureKind::ModelNotFound),
            true
        ));
        assert!(is_startup_failure(
            Status::Nonzero,
            Some(FailureKind::RateLimit),
            true
        ));
        // A plain task failure still stops the chain even with a model list — it
        // is a real run, not a per-model provisioning problem. A deferred-tool
        // dead-end likewise ran, so it stops the chain too.
        assert_eq!(startup_failure_reason(Status::Nonzero, None, true), None);
        assert_eq!(
            startup_failure_reason(Status::Nonzero, Some(FailureKind::ToolDeferred), true),
            None
        );
        // The structural/provisioning reasons are unchanged by the model flag.
        assert_eq!(
            startup_failure_reason(Status::Skipped, None, true),
            Some("not-installed")
        );
        assert_eq!(
            startup_failure_reason(Status::Nonzero, Some(FailureKind::Auth), true),
            Some("auth")
        );
    }
}
