//! Pure logic for the uniform session handle (`run --session <name>`).
//!
//! oneharness lets a caller thread ONE stable name across turns instead of
//! extracting each harness's native session id and re-passing it. This module is
//! the pure half: the record persisted per named conversation, the
//! create-vs-continue decision given what the store already holds, and the
//! harness-binding check. All disk/clock I/O lives in [`crate::io::session`].
//!
//! The handle is supported only for harnesses that emit a native session id
//! headlessly (the `extract_session` sources — see
//! [`crate::domain::harness::HarnessSpec::session_capable`]); for the rest the
//! command layer refuses `--session` loudly rather than silently starting fresh.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The on-disk session record's schema version, independent of the report's and
/// the history's — the session store shape versions on its own cadence.
pub const SCHEMA_VERSION: &str = "0.1";

/// One named conversation's persisted state: the harness it runs on and that
/// harness's native token to resume it with. Written after a run captures a
/// session id, read on the next run to continue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRecord {
    /// The store shape version ([`SCHEMA_VERSION`]).
    pub schema_version: String,
    /// The caller's stable handle (`--session <name>`, sanitized for a path).
    pub name: String,
    /// The project the session belongs to (its display path), so the same name
    /// in different projects never collides.
    pub project: String,
    /// The harness id this session runs on. A session is bound to the harness
    /// that created it; resuming it on another is a usage error.
    pub harness: String,
    /// The harness's native continuation token (its emitted session id) — what
    /// the next run resumes with.
    pub token: String,
    /// RFC 3339 timestamp the session was first created.
    pub created: String,
    /// RFC 3339 timestamp of the most recent run that touched it.
    pub updated: String,
}

/// Whether a run starts a new named session or continues an existing one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SessionPhase {
    /// No stored token yet — the harness runs fresh and its emitted session id is
    /// captured for next time.
    Create,
    /// A stored token exists — the run resumes it.
    Continue,
}

impl SessionPhase {
    /// The lowercase token used in the JSON report and text output.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            SessionPhase::Create => "create",
            SessionPhase::Continue => "continue",
        }
    }
}

/// How to build this run's argv for a `--session` request, given what the store
/// holds: the phase, and the token to resume with (`Some` iff continuing).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPlan {
    pub phase: SessionPhase,
    /// The native token to resume with — fed to the harness's verified `--resume`
    /// mapping. `Some` exactly when `phase == Continue`.
    pub resume_token: Option<String>,
}

impl SessionPlan {
    /// Decide the plan from the existing record (if any). A stored record
    /// continues its token; no record starts fresh.
    #[must_use]
    pub fn decide(existing: Option<&SessionRecord>) -> SessionPlan {
        match existing {
            Some(record) => SessionPlan {
                phase: SessionPhase::Continue,
                resume_token: Some(record.token.clone()),
            },
            None => SessionPlan {
                phase: SessionPhase::Create,
                resume_token: None,
            },
        }
    }
}

/// The harness a prior record is bound to when it differs from `harness` — a
/// usage error, since a named session cannot migrate between harnesses. `None`
/// when there is no record or it already belongs to `harness`.
#[must_use]
pub fn harness_conflict<'a>(existing: Option<&'a SessionRecord>, harness: &str) -> Option<&'a str> {
    existing
        .filter(|record| record.harness != harness)
        .map(|record| record.harness.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(token: &str, harness: &str) -> SessionRecord {
        SessionRecord {
            schema_version: SCHEMA_VERSION.to_string(),
            name: "greet".to_string(),
            project: "/p".to_string(),
            harness: harness.to_string(),
            token: token.to_string(),
            created: "2026-07-10T00:00:00Z".to_string(),
            updated: "2026-07-10T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn decide_creates_without_a_record() {
        let plan = SessionPlan::decide(None);
        assert_eq!(plan.phase, SessionPhase::Create);
        assert_eq!(plan.resume_token, None);
    }

    #[test]
    fn decide_continues_a_stored_token() {
        let existing = record("sess-1", "claude-code");
        let plan = SessionPlan::decide(Some(&existing));
        assert_eq!(plan.phase, SessionPhase::Continue);
        assert_eq!(plan.resume_token.as_deref(), Some("sess-1"));
    }

    #[test]
    fn phase_strings_are_stable() {
        assert_eq!(SessionPhase::Create.as_str(), "create");
        assert_eq!(SessionPhase::Continue.as_str(), "continue");
    }

    #[test]
    fn harness_conflict_flags_a_mismatch_only() {
        let claude = record("s", "claude-code");
        assert_eq!(harness_conflict(None, "claude-code"), None);
        assert_eq!(harness_conflict(Some(&claude), "claude-code"), None);
        assert_eq!(
            harness_conflict(Some(&claude), "codex"),
            Some("claude-code")
        );
    }

    #[test]
    fn record_round_trips_through_json() {
        let original = record("sess-9", "opencode");
        let text = serde_json::to_string(&original).unwrap();
        let parsed: SessionRecord = serde_json::from_str(&text).unwrap();
        assert_eq!(original, parsed);
    }
}
