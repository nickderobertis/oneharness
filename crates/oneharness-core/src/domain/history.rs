//! Standardized, cross-harness run history: the shape of one history record and
//! the pure helpers that name and time-stamp it. This is a pure module — it
//! builds the record and formats strings, but reads no clock or filesystem. The
//! actual streaming to disk, the `SystemTime` reads that mint the session id and
//! timestamps, and the reading/listing of history files all live in
//! `src/io/history.rs`.
//!
//! The history file is its own output contract, independent of the run report's:
//! it carries its own [`SCHEMA_VERSION`], and normalized signals only (never the
//! raw stdout/stderr — a consumer that needs the bytes re-runs, or reads the
//! harness's own transcript). Every field mirrors a [`crate::domain::report`]
//! signal, so a history record reads like a report result frozen in time.

use serde::Serialize;

use crate::domain::events::ActionEvent;
use crate::domain::mode::PermissionMode;
use crate::domain::report::{RunResult, Status};
use crate::domain::signals::Usage;

/// Bumped when the history record shape changes in a way a consumer must notice.
/// Independent of [`crate::domain::report::SCHEMA_VERSION`] — the history file and
/// the run report are separate contracts and version on their own cadence.
pub const SCHEMA_VERSION: &str = "0.1";

/// One harness run, normalized and frozen for the history log. Serialized as one
/// JSONL line per harness run, appended as the run finalizes. Carries only the
/// normalized cross-harness signals — no raw stdout/stderr.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HistoryRecord {
    pub schema_version: &'static str,
    /// The oneharness session id this run belongs to (the history file's stem).
    pub session: String,
    /// The human-meaningful session name (see [`session_name`]); repeated on
    /// every record so a reader can resolve a session by name from any line.
    pub name: String,
    /// The project directory the run operated in (the real path, not the
    /// on-disk slug), so the list view can show where a session ran.
    pub project: String,
    /// RFC3339 UTC instant the record was written (append time).
    pub timestamp: String,
    /// Canonical harness id (e.g. `claude-code`).
    pub harness: String,
    /// The effective top-level model for the run, if any.
    pub model: Option<String>,
    /// The prompt this harness run received (its own, on a batch run; else the
    /// run's single prompt).
    pub prompt: String,
    /// The normalized approval mode requested for the run.
    pub permission_mode: PermissionMode,
    pub status: Status,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<u128>,
    /// Best-effort final assistant text; `null` when extraction was impossible.
    pub text: Option<String>,
    /// How `text` was extracted; `null` when absent.
    pub text_source: Option<String>,
    /// Best-effort token/cost accounting (every field `null` when unreported).
    pub usage: Usage,
    /// The harness's own continuation id, when it exposed one; `null` otherwise.
    pub session_id: Option<String>,
    /// Best-effort normalized tool-call events; `null` when the harness exposes
    /// no machine-readable trace.
    pub events: Option<Vec<ActionEvent>>,
    /// Best-effort classified failure reason for a non-zero run; `null` when
    /// unclassified.
    pub failure_kind: Option<String>,
}

impl HistoryRecord {
    /// Freeze one [`RunResult`] into a history record. `session`/`name` identify
    /// the oneharness session; `timestamp` is the caller-supplied append instant
    /// (an I/O read, kept out of this pure function); `model` is the run's
    /// effective top-level model. `run_prompt` is the fallback prompt for an
    /// ordinary run — a batch result carries its own `prompt`, which wins.
    #[allow(clippy::too_many_arguments)]
    pub fn from_result(
        session: &str,
        name: &str,
        project: &str,
        timestamp: String,
        mode: PermissionMode,
        model: Option<&str>,
        run_prompt: &str,
        r: &RunResult,
    ) -> Self {
        HistoryRecord {
            schema_version: SCHEMA_VERSION,
            session: session.to_string(),
            name: name.to_string(),
            project: project.to_string(),
            timestamp,
            harness: r.harness.clone(),
            model: model.map(str::to_string),
            prompt: r.prompt.clone().unwrap_or_else(|| run_prompt.to_string()),
            permission_mode: mode,
            status: r.status,
            exit_code: r.exit_code,
            duration_ms: r.duration_ms,
            text: r.text.clone(),
            text_source: r.text_source.clone(),
            usage: r.usage.clone(),
            session_id: r.session_id.clone(),
            events: r.events.clone(),
            failure_kind: r.failure_kind.clone(),
        }
    }
}

/// A filesystem-safe slug for a project directory, so history is partitioned by
/// project. Every character outside `[A-Za-z0-9._-]` becomes `-`, runs of `-`
/// collapse, and leading/trailing `-` are trimmed. An empty result falls back to
/// `project` so a path made entirely of separators still yields a directory.
pub fn project_slug(path: &str) -> String {
    let slug = collapse_dashes(path, |c| {
        c.is_ascii_alphanumeric() || matches!(c, '.' | '_')
    });
    if slug.is_empty() {
        "project".to_string()
    } else {
        slug
    }
}

/// The default human-meaningful session name: the first few words of the session's
/// first prompt, lowercased and joined with `-`, capped in length. Punctuation is
/// dropped; an empty or word-less prompt falls back to `session`. Deterministic and
/// pure, so the same prompt always names the same way.
pub fn session_name(first_prompt: &str) -> String {
    const MAX_WORDS: usize = 6;
    const MAX_LEN: usize = 48;

    let mut words: Vec<String> = Vec::new();
    for raw in first_prompt.split_whitespace() {
        let word: String = raw
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .map(|c| c.to_ascii_lowercase())
            .collect();
        if !word.is_empty() {
            words.push(word);
        }
        if words.len() == MAX_WORDS {
            break;
        }
    }
    if words.is_empty() {
        return "session".to_string();
    }
    let mut name = words.join("-");
    if name.len() > MAX_LEN {
        name.truncate(MAX_LEN);
        // Never end on a dangling dash after the truncation.
        name = name.trim_end_matches('-').to_string();
    }
    name
}

/// Sanitize an explicit `--history-name` into the same shape [`session_name`]
/// produces, so a user label and a derived name are interchangeable everywhere
/// (filenames, name lookup). An empty result falls back to `session`.
pub fn sanitize_name(name: &str) -> String {
    let slug = collapse_dashes(name, |c| c.is_ascii_alphanumeric());
    if slug.is_empty() {
        "session".to_string()
    } else {
        slug.to_ascii_lowercase()
    }
}

/// Replace every run of characters failing `keep` with a single `-`, trimming
/// leading/trailing dashes. Shared by [`project_slug`] and [`sanitize_name`].
fn collapse_dashes(input: &str, keep: impl Fn(char) -> bool) -> String {
    let mut out = String::with_capacity(input.len());
    let mut pending_dash = false;
    for c in input.chars() {
        if keep(c) {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(c);
        } else {
            pending_dash = true;
        }
    }
    out
}

/// The civil (proleptic Gregorian) date and time-of-day for a UNIX timestamp in
/// seconds (UTC). Uses Howard Hinnant's `civil_from_days` algorithm — exact for
/// all dates, no lookup tables — so history timestamps need no date library.
/// Returns `(year, month, day, hour, minute, second)`.
pub fn civil_from_epoch(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, mi, s) = (
        (rem / 3600) as u32,
        ((rem % 3600) / 60) as u32,
        (rem % 60) as u32,
    );

    // civil_from_days: days is the count since 1970-01-01.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d, h, mi, s)
}

/// Format a UNIX timestamp (seconds, UTC) as `YYYY-MM-DDThh:mm:ssZ` — the
/// human-and-machine-readable instant stored on each history record.
pub fn format_rfc3339(secs: i64) -> String {
    let (y, mo, d, h, mi, s) = civil_from_epoch(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// Format a UNIX timestamp (seconds, UTC) as `YYYYMMDDThhmmssZ` — colon-free so
/// it is safe in a filename on every platform (Windows forbids `:`). Used to make
/// the session id sortable by start time.
pub fn format_compact_utc(secs: i64) -> String {
    let (y, mo, d, h, mi, s) = civil_from_epoch(secs);
    format!("{y:04}{mo:02}{d:02}T{h:02}{mi:02}{s:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::report::OutputFormat;

    #[test]
    fn slug_sanitizes_and_collapses() {
        assert_eq!(
            project_slug("/home/user/My Project"),
            "home-user-My-Project"
        );
        assert_eq!(project_slug("/a//b/./c"), "a-b-.-c");
        assert_eq!(project_slug("C:\\Users\\me\\proj"), "C-Users-me-proj");
        // A path of only separators still yields a usable directory.
        assert_eq!(project_slug("///"), "project");
        assert_eq!(project_slug(""), "project");
    }

    #[test]
    fn session_name_takes_first_words_lowercased() {
        assert_eq!(
            session_name("Fix the login redirect bug please now urgently"),
            "fix-the-login-redirect-bug-please"
        );
        assert_eq!(
            session_name("Refactor!! the (parser)."),
            "refactor-the-parser"
        );
    }

    #[test]
    fn session_name_falls_back_when_empty_or_wordless() {
        assert_eq!(session_name(""), "session");
        assert_eq!(session_name("   \n\t  "), "session");
        assert_eq!(session_name("!!! ??? ..."), "session");
    }

    #[test]
    fn session_name_is_length_capped_without_trailing_dash() {
        let long = "aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii jjjj kkkk";
        let name = session_name(long);
        assert!(name.len() <= 48, "{name} too long");
        assert!(!name.ends_with('-'), "{name} ends with a dash");
    }

    #[test]
    fn sanitize_name_matches_derived_shape() {
        assert_eq!(sanitize_name("My Release v2!"), "my-release-v2");
        assert_eq!(sanitize_name("   "), "session");
        assert_eq!(sanitize_name("---"), "session");
    }

    #[test]
    fn civil_from_epoch_matches_known_instants() {
        assert_eq!(civil_from_epoch(0), (1970, 1, 1, 0, 0, 0));
        // 2026-07-07T13:14:15Z
        assert_eq!(civil_from_epoch(1_783_430_055), (2026, 7, 7, 13, 14, 15));
        // A leap day: 2024-02-29T23:59:59Z.
        assert_eq!(civil_from_epoch(1_709_251_199), (2024, 2, 29, 23, 59, 59));
        // Pre-epoch (negative) stays exact: 1969-12-31T23:59:59Z.
        assert_eq!(civil_from_epoch(-1), (1969, 12, 31, 23, 59, 59));
    }

    #[test]
    fn formatters_render_both_shapes() {
        assert_eq!(format_rfc3339(1_783_430_055), "2026-07-07T13:14:15Z");
        assert_eq!(format_compact_utc(1_783_430_055), "20260707T131415Z");
        // Colon-free compact form is safe as a filename component.
        assert!(!format_compact_utc(1_783_430_055).contains(':'));
    }

    fn result() -> RunResult {
        RunResult {
            harness: "claude-code".to_string(),
            bin: "claude".to_string(),
            available: true,
            status: Status::Ok,
            prompt: None,
            model: None,
            exit_code: Some(0),
            duration_ms: Some(42),
            command: vec!["claude".to_string()],
            output_format: OutputFormat::Json,
            text: Some("hello".to_string()),
            text_source: Some("json:result".to_string()),
            usage: Usage::default(),
            usage_source: None,
            session_id: Some("abc-123".to_string()),
            events: None,
            events_source: None,
            structured: None,
            schema_valid: None,
            schema_attempts: None,
            schema_error: None,
            failure_kind: None,
            failure_kind_source: None,
            stdout: "hello".to_string(),
            stderr: String::new(),
            error: None,
        }
    }

    #[test]
    fn from_result_maps_normalized_signals() {
        let r = result();
        let rec = HistoryRecord::from_result(
            "fix-bug-20260707T131415Z-9",
            "fix-bug",
            "/home/user/proj",
            "2026-07-07T13:14:15Z".to_string(),
            PermissionMode::Bypass,
            Some("sonnet"),
            "fix the bug",
            &r,
        );
        assert_eq!(rec.schema_version, SCHEMA_VERSION);
        assert_eq!(rec.session, "fix-bug-20260707T131415Z-9");
        assert_eq!(rec.name, "fix-bug");
        assert_eq!(rec.project, "/home/user/proj");
        assert_eq!(rec.harness, "claude-code");
        assert_eq!(rec.model.as_deref(), Some("sonnet"));
        assert_eq!(rec.session_id.as_deref(), Some("abc-123"));
        assert_eq!(rec.status, Status::Ok);
        // No per-result prompt → the run prompt is used.
        assert_eq!(rec.prompt, "fix the bug");
    }

    #[test]
    fn from_result_prefers_the_per_result_batch_prompt() {
        let mut r = result();
        r.prompt = Some("batch prompt 2".to_string());
        let rec = HistoryRecord::from_result(
            "s",
            "n",
            "/p",
            "t".to_string(),
            PermissionMode::Default,
            None,
            "run prompt",
            &r,
        );
        assert_eq!(rec.prompt, "batch prompt 2");
        assert_eq!(rec.model, None);
    }
}
