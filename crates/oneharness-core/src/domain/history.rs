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

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use thiserror::Error;
use uuid::{Uuid, Variant};

use crate::domain::events::{ActionEvent, ToolCallStatus};
use crate::domain::mode::PermissionMode;
use crate::domain::report::{RunResult, Status};
use crate::domain::signals::{FailureKind, Usage};

/// Bumped when the history record shape changes in a way a consumer must notice.
/// Independent of [`crate::domain::report::SCHEMA_VERSION`] — the history file and
/// the run report are separate contracts and version on their own cadence.
///
/// A record's `schema_version` is the **oldest reader that can understand it**,
/// not the version of the writer that produced it: a provider-measured run still
/// declares [`PREVIOUS_CURRENT_SCHEMA_VERSION`] because nothing in it needs a
/// newer reader. That is what lets an additive field ship without rewriting the
/// shape of every record — and what makes each version constant below the exact
/// gate for the one field it introduced.
pub const SCHEMA_VERSION: &str = "1.4";
/// v1.4 introduced the `cancelled` run status — a record a v1.3 reader would
/// refuse rather than misread, since its `status` enum has no such value. Spelled
/// literally, like every constant here: aliasing [`SCHEMA_VERSION`] would move
/// this "first version that understood it" on the next bump.
pub const FIRST_CANCELLED_SCHEMA_VERSION: &str = "1.4";
/// v1.3 introduced the normalized failure `error` text and invocation-bounds-only
/// timing, so both name the same version — and both must, because an older
/// reader refuses either shape rather than misreading it.
pub const FIRST_ERROR_SCHEMA_VERSION: &str = "1.3";
pub const FIRST_PARTIAL_TIMING_SCHEMA_VERSION: &str = "1.3";
/// v1.2 introduced stdout-observed tool timing (`observed_tool_ms`) and the
/// `timing_source` provenance it puts on events.
pub const OBSERVED_TIMING_SCHEMA_VERSION: &str = "1.2";
pub const PREVIOUS_CURRENT_SCHEMA_VERSION: &str = "1.1";
pub(crate) const FIRST_EVENT_SCHEMA_VERSION: &str = "1.0";

/// Every event-sourced history version this build reads, oldest first. Order is
/// the contract: a field introduced in version N is legible to N and everything
/// after it, which is what [`version_at_least`] answers.
pub(crate) const READABLE_SCHEMA_VERSIONS: [&str; 5] = [
    FIRST_EVENT_SCHEMA_VERSION,
    PREVIOUS_CURRENT_SCHEMA_VERSION,
    OBSERVED_TIMING_SCHEMA_VERSION,
    FIRST_ERROR_SCHEMA_VERSION,
    SCHEMA_VERSION,
];

fn version_rank(version: &str) -> Option<usize> {
    READABLE_SCHEMA_VERSIONS
        .iter()
        .position(|known| *known == version)
}

/// Every readable version at or after `minimum`, oldest first — the versions a
/// field introduced in `minimum` may legitimately appear at.
///
/// Public because the generated SDK schemas describe such a field by listing
/// exactly these versions. Deriving the list from [`READABLE_SCHEMA_VERSIONS`]
/// is what keeps a *new* version from silently narrowing an older field's
/// legality: a hand-written pair like `[introduced, current]` stops covering the
/// versions in between the moment `current` moves.
#[must_use]
pub fn versions_from(minimum: &str) -> Vec<&'static str> {
    READABLE_SCHEMA_VERSIONS
        .iter()
        .copied()
        .filter(|version| version_at_least(version, minimum))
        .collect()
}

/// Whether `version` is a readable version at or after `minimum`. An unreadable
/// version answers `false` rather than guessing an ordering for it.
fn version_at_least(version: &str, minimum: &str) -> bool {
    match (version_rank(version), version_rank(minimum)) {
        (Some(actual), Some(minimum)) => actual >= minimum,
        _ => false,
    }
}

/// The legacy record contract accepted by the migration reader.
pub const LEGACY_SCHEMA_VERSION: &str = "0.1";
const PREVIOUS_SCHEMA_VERSION: &str = "0.2";
const LEGACY_RECORD_SCHEMA_VERSION: &str = "0.3";

/// The whole-record versions whose events ended at `index`, before
/// [`LEGACY_RECORD_SCHEMA_VERSION`] (v0.3) added the lifecycle fields — see
/// [`LegacyActionEvent`]. Exported so the generated SDK schemas describe that
/// one legacy shape from this source rather than restating its versions.
pub const PRE_LIFECYCLE_RECORD_VERSIONS: [&str; 2] =
    [LEGACY_SCHEMA_VERSION, PREVIOUS_SCHEMA_VERSION];

/// One event-sourced history JSONL line.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HistoryLine {
    Event(HistoryEventLine),
    Run(HistoryRunRecord),
}

impl<'de> Deserialize<'de> for HistoryLine {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut value = Value::deserialize(deserializer)?;
        let line_type = value
            .as_object_mut()
            .and_then(|object| object.remove("type"))
            .and_then(|value| value.as_str().map(str::to_owned));
        match line_type.as_deref() {
            Some("event") => {
                let event = serde_json::from_value::<HistoryEventLine>(value)
                    .map_err(serde::de::Error::custom)?;
                if event.valid() {
                    Ok(Self::Event(event))
                } else {
                    Err(serde::de::Error::custom(
                        "invalid history schema v1.0 event line",
                    ))
                }
            }
            Some("run") => {
                let run = serde_json::from_value::<HistoryRunRecord>(value)
                    .map_err(serde::de::Error::custom)?;
                if run.valid() {
                    Ok(Self::Run(run))
                } else {
                    Err(serde::de::Error::custom(
                        "invalid history schema v1.0 run line",
                    ))
                }
            }
            _ => Err(serde::de::Error::custom("invalid history schema v1.0 line")),
        }
    }
}

/// One normalized action observed during a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HistoryEventLine {
    // llmlint: ignore[invalid_states_unrepresentable] Schema versions are strings throughout the report, session, and history contracts, and history migration must inspect several legacy string versions; `valid()` is the intentional wire-boundary check, so a bespoke current-version type would conflict with that established representation.
    pub schema_version: String,
    pub run_id: HistoryId,
    pub harness: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    // llmlint: ignore[invalid_states_unrepresentable] History v1.0 compatibility requires an optional string here; current writers derive it from a validated composed selector and materialization validates/normalizes the legacy tuple.
    pub variant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    // llmlint: ignore[invalid_states_unrepresentable] This field is optional specifically to read v1.0 event lines; current writers always derive it with base/variant from one composed id, and event-stream integration coverage asserts all three.
    pub harness_id: Option<String>,
    pub event: ActionEvent,
}

impl HistoryEventLine {
    pub(crate) fn valid(&self) -> bool {
        version_rank(&self.schema_version).is_some()
            && (self.event.timing_source.is_none()
                || version_at_least(&self.schema_version, OBSERVED_TIMING_SCHEMA_VERSION))
            && identity_fields_valid(
                &self.harness,
                self.variant.as_deref(),
                self.harness_id.as_deref(),
            )
    }
}

/// The terminal summary for one harness run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HistoryRunRecord {
    // llmlint: ignore[invalid_states_unrepresentable] Keep the established string version representation used by every serialized contract and required by legacy history migration; deserialization validates it before exposing a current record.
    pub schema_version: String,
    pub history_id: HistoryId,
    pub session: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "HistoryLabels::is_empty")]
    pub labels: HistoryLabels,
    pub project: String,
    pub timestamp: String,
    // llmlint: ignore[invalid_states_unrepresentable] These legacy-compatible wire fields must deserialize v1.0 records where the composed field is absent; `materialize` derives the composed identity and current writers copy the already-normalized run identity, with cross-contract tests pinning consistency.
    pub harness: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    // llmlint: ignore[invalid_states_unrepresentable] The v1.0 wire contract requires this optional string; current writers copy the validated run identity and deserialization/materialization validates the supported schema before exposure.
    pub variant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    // llmlint: ignore[invalid_states_unrepresentable] This remains optional to read v1.0 records; v1.1 writers always derive it with base/variant from one composed selector and round-trip tests pin consistency.
    pub harness_id: Option<String>,
    pub model: Option<String>,
    pub prompt: String,
    pub permission_mode: PermissionMode,
    pub status: Status,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_to_first_token_ms: Option<u128>,
    /// Union of tool intervals derived from stdout pipe-read observations.
    /// This is deliberately separate from provider-measured `tool_ms`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    // llmlint: ignore[invalid_states_unrepresentable] This additive published wire field must remain flat beside legacy provider timing fields; `valid()` rejects mixed states and constructors derive exactly one timing mode.
    pub observed_tool_ms: Option<u128>,
    pub text: Option<String>,
    pub text_source: Option<String>,
    pub usage: Usage,
    pub session_id: Option<String>,
    pub failure_kind: Option<FailureKind>,
    /// Normalized failure text for a run that did not succeed (see
    /// [`HistoryRecord::error`]). Omitted on the wire when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<FailureText>,
}

impl HistoryRunRecord {
    pub(crate) fn valid(&self) -> bool {
        if version_rank(&self.schema_version).is_none() {
            return false;
        }
        if !identity_fields_valid(
            &self.harness,
            self.variant.as_deref(),
            self.harness_id.as_deref(),
        ) {
            return false;
        }
        if !status_version_valid(&self.schema_version, self.status)
            || !error_text_valid(
                &self.schema_version,
                self.error.as_ref(),
                self.status,
                self.failure_kind,
            )
        {
            return false;
        }
        if self.observed_tool_ms.is_some() {
            return version_at_least(&self.schema_version, OBSERVED_TIMING_SCHEMA_VERSION)
                && self.duration_ms.is_some()
                && self.started_at.is_none()
                && self.finished_at.is_none()
                && self.model_ms.is_none()
                && self.tool_ms.is_none()
                && self.time_to_first_token_ms.is_none();
        }
        match (
            self.started_at.as_deref(),
            self.finished_at.as_deref(),
            self.model_ms,
            self.tool_ms,
            self.time_to_first_token_ms,
        ) {
            (None, None, None, None, None) => true,
            (Some(started_at), finished_at, Some(model_ms), Some(tool_ms), _) => {
                !started_at.is_empty()
                    && self
                        .duration_ms
                        .is_some_and(|duration| model_ms.saturating_add(tool_ms) <= duration)
                    && (!requires_provider_finish(self.status) || finished_at.is_some())
            }
            // Invocation bounds with no split derived from them, and no provider
            // finish either: what a run cut short leaves, legible only on a run
            // that was cut short and only to a reader that knows the shape.
            (Some(started_at), None, None, None, time_to_first_token_ms) => {
                run_failed(self.status)
                    && version_at_least(&self.schema_version, FIRST_PARTIAL_TIMING_SCHEMA_VERSION)
                    && partial_trace_valid(started_at, time_to_first_token_ms, self.duration_ms)
            }
            _ => false,
        }
    }

    /// Split the terminal portion from the familiar materialized record.
    pub fn from_record(record: &HistoryRecord) -> Self {
        Self {
            schema_version: record.schema_version.clone(),
            history_id: record.history_id,
            session: record.session.clone(),
            name: record.name.clone(),
            labels: record.labels.clone(),
            project: record.project.clone(),
            timestamp: record.timestamp.clone(),
            harness: record.harness.clone(),
            variant: record.variant.clone(),
            harness_id: Some(record.harness_id.clone()),
            model: record.model.clone(),
            prompt: record.prompt.clone(),
            permission_mode: record.permission_mode,
            status: record.status,
            exit_code: record.exit_code,
            duration_ms: record.duration_ms,
            started_at: record.started_at.clone(),
            finished_at: record.finished_at.clone(),
            model_ms: record.model_ms,
            tool_ms: record.tool_ms,
            time_to_first_token_ms: record.time_to_first_token_ms,
            observed_tool_ms: record.observed_tool_ms,
            text: record.text.clone(),
            text_source: record.text_source.clone(),
            usage: record.usage.clone(),
            session_id: record.session_id.clone(),
            failure_kind: record.failure_kind,
            error: record.error.clone(),
        }
    }

    /// Rebuild the stable per-run presentation object from event-sourced lines.
    pub fn materialize(self, events: Vec<ActionEvent>) -> HistoryRecord {
        let harness_id = self
            .harness_id
            .clone()
            .unwrap_or_else(|| self.harness.clone());
        HistoryRecord {
            schema_version: self.schema_version,
            history_id: self.history_id,
            session: self.session,
            name: self.name,
            labels: self.labels,
            project: self.project,
            timestamp: self.timestamp,
            harness: self.harness,
            variant: self.variant,
            harness_id,
            model: self.model,
            prompt: self.prompt,
            permission_mode: self.permission_mode,
            status: self.status,
            exit_code: self.exit_code,
            duration_ms: self.duration_ms,
            started_at: self.started_at,
            finished_at: self.finished_at,
            model_ms: self.model_ms,
            tool_ms: self.tool_ms,
            time_to_first_token_ms: self.time_to_first_token_ms,
            observed_tool_ms: self.observed_tool_ms,
            text: self.text,
            text_source: self.text_source,
            usage: self.usage,
            session_id: self.session_id,
            events: (!events.is_empty()).then_some(events),
            failure_kind: self.failure_kind,
            error: self.error,
        }
    }
}

/// Label lengths are bounded in *characters* — Unicode code points. That is the
/// unit JSON Schema's `maxLength` measures in, and therefore the one unit both
/// the runtime checks below and every generated SDK validator can agree on; a
/// byte bound is not expressible in the schema the SDKs are generated from.
const LABEL_KEY_MAX: usize = 64;
const LABEL_VALUE_MAX: usize = 256;

/// Canonical hyphenated UUID text is always exactly this many characters. The
/// bound is load-bearing, not decoration: a regex `$` also matches *before* a
/// trailing newline in some engines the SDKs validate with (Python's `re`, which
/// drives the generated Python schemas), so pinning the length is what keeps
/// `"<uuid>\n"` from slipping past [`UUID_PATTERN`] there but not here.
const UUID_LEN: usize = 36;
/// Canonical hyphenated UUID text carrying the RFC 4122 variant (`[89abAB]`) and
/// a defined version (`[1-8]`) — exactly what [`HistoryId::from_str`] accepts.
const UUID_PATTERN: &str =
    "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$";
/// A label key begins with a letter or digit...
const LABEL_KEY_START_PATTERN: &str = "^[A-Za-z0-9]";
/// ...and carries nothing outside the permitted set. Stated as a *forbidden*
/// unanchored search rather than an anchored allow-list so the schema needs no
/// `$`, whose meaning differs across the regex engines the SDKs validate with
/// (see [`UUID_LEN`]).
const LABEL_KEY_FORBIDDEN_PATTERN: &str = "[^A-Za-z0-9._-]";
/// Every character Rust classifies as control (Unicode `Cc`): C0, DEL, *and* the
/// C1 block. Kept exactly in step with [`char::is_control`], which is what the
/// runtime checks, and forbidden by search for the same reason as
/// [`LABEL_KEY_FORBIDDEN_PATTERN`].
const LABEL_VALUE_FORBIDDEN_PATTERN: &str = "[\\u0000-\\u001f\\u007f-\\u009f]";

/// The error returned when text is not a canonical [`HistoryId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error(
    "must be a canonical hyphenated UUID with a version between 1 and 8 and the RFC 4122 variant"
)]
pub struct HistoryIdError;

/// A validated history-record identifier. New records are minted as UUIDv7 in
/// the I/O layer; legacy records are assigned deterministic UUIDv5 values while
/// being read, so the same v0.1 line always receives the same stable id.
///
/// Parsed text must be the canonical hyphenated spelling carrying the RFC 4122
/// variant and a defined version — both of which every minted id (v5 and v7)
/// satisfies, and both of which [`UUID_PATTERN`] promises consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct HistoryId(Uuid);

impl HistoryId {
    /// Wrap an already-generated UUID. The I/O layer uses this after its clock /
    /// randomness read; pure migration uses [`Self::legacy`].
    #[must_use]
    pub fn from_uuid(value: Uuid) -> Self {
        Self(value)
    }

    /// Deterministically identify one legacy record from a stable source key.
    #[must_use]
    pub fn legacy(stable_key: &[u8]) -> Self {
        Self(Uuid::new_v5(&Uuid::NAMESPACE_OID, stable_key))
    }

    /// The underlying UUID.
    #[must_use]
    pub fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl fmt::Display for HistoryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for HistoryId {
    type Err = HistoryIdError;

    /// Accept exactly what [`UUID_PATTERN`] promises. `Uuid::parse_str` is laxer
    /// on both counts: it also takes the simple, braced, and URN spellings, and
    /// it reads any variant or version bits — including the nil UUID. Comparing
    /// against the parsed value's own canonical rendering is what rejects the
    /// alternate spellings while still accepting upper-case hex.
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let uuid = Uuid::parse_str(value).map_err(|_| HistoryIdError)?;
        if uuid.hyphenated().to_string() != value.to_ascii_lowercase() {
            return Err(HistoryIdError);
        }
        if !matches!(uuid.get_variant(), Variant::RFC4122)
            || !(1..=8).contains(&uuid.get_version_num())
        {
            return Err(HistoryIdError);
        }
        Ok(Self(uuid))
    }
}

impl<'de> Deserialize<'de> for HistoryId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for HistoryId {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("HistoryId")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        schemars::json_schema!({
            "type": "string",
            "minLength": UUID_LEN,
            "maxLength": UUID_LEN,
            "pattern": UUID_PATTERN,
        })
    }
}

/// A validated, deterministically ordered label set attached to every record in
/// one history session. Keys are portable identifier-like strings; values are
/// non-empty strings without control characters, bounded in characters (see
/// [`LABEL_VALUE_MAX`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct HistoryLabels(BTreeMap<String, String>);

impl HistoryLabels {
    /// Validate and construct a label set at a config/env/CLI boundary.
    pub fn new(labels: BTreeMap<String, String>) -> Result<Self, String> {
        for (key, value) in &labels {
            validate_label(key, value)?;
        }
        Ok(Self(labels))
    }

    /// Borrow the ordered map.
    #[must_use]
    pub fn as_map(&self) -> &BTreeMap<String, String> {
        &self.0
    }

    /// Whether no labels are present.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Overlay higher-precedence labels on this set, key by key.
    pub fn extend(&mut self, higher: &Self) {
        self.0.extend(higher.0.clone());
    }

    /// Whether every filter pair appears with the same value.
    #[must_use]
    pub fn matches(&self, filters: &Self) -> bool {
        filters
            .0
            .iter()
            .all(|(key, value)| self.0.get(key) == Some(value))
    }
}

impl<'de> Deserialize<'de> for HistoryLabels {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let labels = BTreeMap::<String, String>::deserialize(deserializer)?;
        Self::new(labels).map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for HistoryLabels {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("HistoryLabels")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        schemars::json_schema!({
            "type": "object",
            "propertyNames": {
                "type": "string",
                "maxLength": LABEL_KEY_MAX,
                "pattern": LABEL_KEY_START_PATTERN,
                "not": { "pattern": LABEL_KEY_FORBIDDEN_PATTERN },
            },
            "additionalProperties": {
                "type": "string",
                "minLength": 1,
                "maxLength": LABEL_VALUE_MAX,
                "not": { "pattern": LABEL_VALUE_FORBIDDEN_PATTERN },
            },
        })
    }
}

/// Parse one `key=value` label spelling used by the CLI and environment.
pub fn parse_label(input: &str) -> Result<(String, String), String> {
    let Some((key, value)) = input.split_once('=') else {
        return Err(format!(
            "invalid history label `{input}`: expected KEY=VALUE"
        ));
    };
    validate_label(key, value)?;
    Ok((key.to_string(), value.to_string()))
}

/// Parse repeated `key=value` spellings into a validated set. Later duplicate
/// keys win, matching repeated CLI flags and layer precedence.
pub fn parse_labels<'a>(
    values: impl IntoIterator<Item = &'a str>,
) -> Result<HistoryLabels, String> {
    let mut labels = BTreeMap::new();
    for value in values {
        let (key, value) = parse_label(value)?;
        labels.insert(key, value);
    }
    HistoryLabels::new(labels)
}

fn validate_label(key: &str, value: &str) -> Result<(), String> {
    let valid_key = !key.is_empty()
        && key.chars().count() <= LABEL_KEY_MAX
        && key.chars().enumerate().all(|(index, c)| {
            c.is_ascii_alphanumeric() || (index > 0 && matches!(c, '.' | '_' | '-'))
        });
    if !valid_key {
        return Err(format!(
            "invalid history label key `{key}`: expected 1-{LABEL_KEY_MAX} ASCII letters, digits, `.`, `_`, or `-`, beginning with a letter or digit"
        ));
    }
    if value.is_empty() {
        return Err(format!(
            "invalid history label `{key}`: value must not be empty"
        ));
    }
    if value.chars().count() > LABEL_VALUE_MAX {
        return Err(format!(
            "invalid history label `{key}`: value exceeds {LABEL_VALUE_MAX} characters"
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(format!(
            "invalid history label `{key}`: value must not contain control characters"
        ));
    }
    Ok(())
}

/// One harness run, normalized and frozen for the history log. Serialized as one
/// JSONL line per harness run, appended as the run finalizes. Carries only the
/// normalized cross-harness signals — no raw stdout/stderr.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct HistoryRecord {
    pub schema_version: String,
    /// Globally unique, time-ordered record id. This is also the cursor accepted
    /// by `history watch --after` and the exact id accepted by history lookup.
    pub history_id: HistoryId,
    /// The oneharness session id this run belongs to (the history file's stem).
    pub session: String,
    /// The human-meaningful session name (see [`session_name`]); repeated on
    /// every record so a reader can resolve a session by name from any line.
    pub name: String,
    /// Caller-supplied metadata used to select related task-graph records.
    /// Omitted on the wire when empty for additive compatibility.
    #[serde(default, skip_serializing_if = "HistoryLabels::is_empty")]
    pub labels: HistoryLabels,
    /// The project directory the run operated in (the real path, not the
    /// on-disk slug), so the list view can show where a session ran.
    pub project: String,
    /// RFC3339 UTC instant the record was written (append time).
    pub timestamp: String,
    /// Canonical harness id (e.g. `claude-code`).
    pub harness: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    // llmlint: ignore[invalid_states_unrepresentable] Materialized records preserve the stable SDK string contract; the only constructors normalize a validated current/legacy wire identity before creating this value.
    pub variant: Option<String>,
    // llmlint: ignore[invalid_states_unrepresentable] The composed selector is a public serialized SDK field; materialization derives it from the validated wire tuple and never accepts caller-provided independent pieces.
    pub harness_id: String,
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
    /// UTC invocation bounds and monotonic time attribution. The provider/tool
    /// split is conservative when a transcript has tool calls but lacks native
    /// boundaries: the observed invocation interval is attributed to the union
    /// of those calls, never double-counted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_to_first_token_ms: Option<u128>,
    /// Union of tool intervals observed at the stdout pipe. Unlike `tool_ms`,
    /// this is not provider-measured and has no model-latency counterpart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    // llmlint: ignore[invalid_states_unrepresentable] The published history record is an additive flat JSON contract; constructors use the internal timing enum and deserialization rejects mixed provider/observed states.
    pub observed_tool_ms: Option<u128>,
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
    /// Best-effort classified failure reason (see [`FailureKind`]); `null` when
    /// unclassified.
    pub failure_kind: Option<FailureKind>,
    /// Best-effort normalized failure text for a run that did not succeed: the
    /// harness's own diagnostic as oneharness captured it on stderr, or
    /// oneharness's own message when it generated one (a spawn failure, a
    /// timeout, a binary that is not installed). This is the *only* place a
    /// record quotes the process's own bytes, and it is deliberately narrow —
    /// trimmed, bounded to [`ERROR_MAX`] characters, and written only for a run
    /// that failed. `failure_kind` says what class of failure it was; this says
    /// what the harness actually reported, which is what an operator reads when
    /// the class is unclassified. Never derived from stdout, so it can never
    /// stand in for provider output the run did not produce. Omitted on the wire
    /// when absent, and gated to [`FIRST_ERROR_SCHEMA_VERSION`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<FailureText>,
}

impl HistoryRecord {
    /// Decode a whole-record history line written before the v1.0 event-sourced
    /// contract. The stable source identity gives v0.1 records (which predate
    /// `history_id`) a repeatable UUID, making migration retries idempotent.
    pub fn from_legacy_value(
        value: Value,
        stable_identity: &str,
    ) -> Result<Self, serde_json::Error> {
        let wire: HistoryRecordWire = serde_json::from_value(value)?;
        let history_id = match wire.schema_version.as_str() {
            PREVIOUS_SCHEMA_VERSION | LEGACY_RECORD_SCHEMA_VERSION => wire
                .history_id
                .ok_or_else(|| invalid_history("legacy history record is missing `history_id`"))?,
            LEGACY_SCHEMA_VERSION => HistoryId::legacy(stable_identity.as_bytes()),
            version => {
                return Err(invalid_history(&format!(
                    "unsupported legacy history schema version `{version}`"
                )))
            }
        };
        let version = wire.schema_version.clone();
        let mut record = Self::from_wire(wire, history_id, SCHEMA_VERSION.to_string());
        if version == LEGACY_SCHEMA_VERSION {
            record.labels = HistoryLabels::default();
        }
        if version == LEGACY_RECORD_SCHEMA_VERSION && !record.complete() {
            return Err(invalid_history(
                "history schema v0.3 record has incomplete telemetry",
            ));
        }
        Ok(record)
    }

    /// Freeze one [`RunResult`] into a history record. `session`/`name` identify
    /// the oneharness session; `timestamp` is the caller-supplied append instant
    /// (an I/O read, kept out of this pure function); `model` is the run's
    /// effective top-level model. `run_prompt` is the fallback prompt for an
    /// ordinary run — a batch result carries its own `prompt`, which wins.
    // Every argument is a distinct caller-owned value, and the two that would
    // group naturally — the session id and name — are already threaded through
    // the I/O writer that owns them; a parameter struct here would only move the
    // same list one call up.
    #[allow(clippy::too_many_arguments)]
    pub fn from_result(
        history_id: HistoryId,
        session: &str,
        name: &str,
        labels: &HistoryLabels,
        project: &str,
        timestamp: String,
        mode: PermissionMode,
        model: Option<&str>,
        run_prompt: &str,
        r: &RunResult,
    ) -> Self {
        let telemetry = r.telemetry.as_ref();
        let measured = telemetry.and_then(|telemetry| match telemetry {
            crate::domain::report::ExecutionTelemetry::ProviderMeasured {
                started_at,
                finished_at,
                model_ms,
                tool_ms,
                time_to_first_token_ms,
            } => Some((
                started_at,
                finished_at,
                model_ms,
                tool_ms,
                time_to_first_token_ms,
            )),
            crate::domain::report::ExecutionTelemetry::StdoutObserved { .. }
            | crate::domain::report::ExecutionTelemetry::PartialInvocation { .. } => None,
        });
        let partial = telemetry.and_then(|telemetry| match telemetry {
            crate::domain::report::ExecutionTelemetry::PartialInvocation { started_at } => {
                Some(started_at)
            }
            _ => None,
        });
        let error = failure_text(r.status, r.error.as_deref(), &r.stderr);
        HistoryRecord {
            // The oldest reader that can understand this record: only the shape a
            // record actually carries forces its version forward.
            schema_version: if r.status == Status::Cancelled {
                FIRST_CANCELLED_SCHEMA_VERSION
            } else if error.is_some() {
                FIRST_ERROR_SCHEMA_VERSION
            } else if partial.is_some() {
                FIRST_PARTIAL_TIMING_SCHEMA_VERSION
            } else if matches!(
                telemetry,
                Some(crate::domain::report::ExecutionTelemetry::StdoutObserved { .. })
            ) {
                OBSERVED_TIMING_SCHEMA_VERSION
            } else {
                PREVIOUS_CURRENT_SCHEMA_VERSION
            }
            .to_string(),
            history_id,
            session: session.to_string(),
            name: name.to_string(),
            labels: labels.clone(),
            project: project.to_string(),
            timestamp: timestamp.clone(),
            harness: r.harness.clone(),
            variant: r.variant.clone(),
            harness_id: r.harness_id.clone(),
            model: model.map(str::to_string),
            prompt: r.prompt.clone().unwrap_or_else(|| run_prompt.to_string()),
            permission_mode: mode,
            status: r.status,
            exit_code: r.exit_code,
            duration_ms: r.duration_ms,
            started_at: measured
                .map(|timing| timing.0.as_str().to_string())
                .or_else(|| partial.map(|started_at| started_at.as_str().to_string())),
            finished_at: measured
                .and_then(|timing| timing.1.as_ref())
                .map(|finished_at| finished_at.as_str().to_string()),
            model_ms: measured.and_then(|timing| *timing.2),
            tool_ms: measured.and_then(|timing| *timing.3),
            time_to_first_token_ms: measured.and_then(|timing| *timing.4),
            observed_tool_ms: telemetry.and_then(|telemetry| match telemetry {
                crate::domain::report::ExecutionTelemetry::StdoutObserved { tool_ms } => {
                    Some(*tool_ms)
                }
                crate::domain::report::ExecutionTelemetry::ProviderMeasured { .. }
                | crate::domain::report::ExecutionTelemetry::PartialInvocation { .. } => None,
            }),
            text: r.text.clone(),
            text_source: r.text_source.clone(),
            usage: r.usage.clone(),
            session_id: r.session_id.clone(),
            events: r.events.clone(),
            failure_kind: r.failure_kind,
            error,
        }
    }

    pub(crate) fn complete(&self) -> bool {
        if !self.versioned_timing_valid()
            || !status_version_valid(&self.schema_version, self.status)
            || !error_text_valid(
                &self.schema_version,
                self.error.as_ref(),
                self.status,
                self.failure_kind,
            )
        {
            return false;
        }
        match self.timing_state() {
            // Measured telemetry is held to the same bar for every run: a
            // trace-capable harness must never write numbers that disagree with
            // the run's own wall clock or tool trace.
            Ok(HistoryTiming::Measured {
                started_at,
                finished_at,
                model_ms,
                tool_ms,
            }) => self.measured_trace_valid(started_at, finished_at, model_ms, tool_ms),
            // Absence is the honest representation twice over: for a harness
            // whose spec declares no provider/tool boundary trace, and for a run
            // that failed, which has no timing *because* it failed.
            Ok(HistoryTiming::Unavailable) => {
                run_failed(self.status)
                    || (self.harness_lacks_trace() && self.untimed_trace_valid())
            }
            // A measurement cut short belongs to a run that was cut short. On a
            // run that succeeded the same shape is corrupt data: the provider
            // answered, so the split it never wrote should have been there.
            Ok(HistoryTiming::Partial {
                started_at,
                time_to_first_token_ms,
            }) => {
                run_failed(self.status)
                    && version_at_least(&self.schema_version, FIRST_PARTIAL_TIMING_SCHEMA_VERSION)
                    && partial_trace_valid(started_at, time_to_first_token_ms, self.duration_ms)
            }
            // Anything else is incoherent rather than partial (see `timing_state`).
            Err(()) => false,
        }
    }

    /// Whether the harness this record names declares no provider/tool boundary
    /// trace, so it could not have derived timing however the run went.
    fn harness_lacks_trace(&self) -> bool {
        crate::domain::harness::all()
            .iter()
            .find(|spec| spec.id == self.harness)
            .is_some_and(|spec| spec.telemetry.is_none())
    }

    fn measured_trace_valid(
        &self,
        started_at: &str,
        finished_at: Option<&str>,
        model_ms: u128,
        tool_ms: u128,
    ) -> bool {
        let Some(duration) = self.duration_ms else {
            return false;
        };
        if started_at.is_empty() || model_ms.saturating_add(tool_ms) > duration {
            return false;
        }
        if requires_provider_finish(self.status) && finished_at.is_none() {
            return false;
        }
        self.events.as_ref().is_none_or(|events| {
            events
                .iter()
                .filter(|event| event.kind == "tool_call")
                .all(|event| {
                    let base = event
                        .tool_call_id
                        .as_deref()
                        .is_some_and(|id| !id.is_empty())
                        && event.started_at.is_some()
                        && event.status.is_some();
                    base && match event.status {
                        Some(ToolCallStatus::Completed | ToolCallStatus::Failed) => {
                            event.finished_at.is_some() && event.duration_ms.is_some()
                        }
                        Some(ToolCallStatus::Timeout | ToolCallStatus::Interrupted) => true,
                        None => false,
                    }
                })
        })
    }

    /// With no run-level timing, no event may claim any — otherwise a
    /// trace-capable harness silently writes half-measured data.
    fn untimed_trace_valid(&self) -> bool {
        if self.observed_tool_ms.is_some() {
            return true;
        }
        self.events.as_ref().is_none_or(|events| {
            events.iter().all(|event| {
                event.started_at.is_none()
                    && event.finished_at.is_none()
                    && event.duration_ms.is_none()
                    && event.status.is_none()
            })
        })
    }

    fn versioned_timing_valid(&self) -> bool {
        let reads_observed_timing =
            version_at_least(&self.schema_version, OBSERVED_TIMING_SCHEMA_VERSION);
        if !reads_observed_timing
            && self
                .events
                .as_ref()
                .is_some_and(|events| events.iter().any(|event| event.timing_source.is_some()))
        {
            return false;
        }
        match self.observed_tool_ms {
            None => true,
            Some(_) => {
                reads_observed_timing
                    && self.duration_ms.is_some()
                    && matches!(self.timing_state(), Ok(HistoryTiming::Unavailable))
            }
        }
    }

    fn timing_state(&self) -> Result<HistoryTiming<'_>, ()> {
        match (
            self.started_at.as_deref(),
            self.finished_at.as_deref(),
            self.model_ms,
            self.tool_ms,
            self.time_to_first_token_ms,
        ) {
            (None, None, None, None, None) => Ok(HistoryTiming::Unavailable),
            (Some(started_at), finished_at, Some(model_ms), Some(tool_ms), _) => {
                Ok(HistoryTiming::Measured {
                    started_at,
                    finished_at,
                    model_ms,
                    tool_ms,
                })
            }
            // A measurement that stopped at the invocation bounds — including
            // before any provider finish, which is dropped with the split it
            // belongs to. Every other combination is incoherent rather than
            // partial — a finish with no start or no split, a model total with no
            // tool total, a first-token offset with nothing to measure it from —
            // and stays refused.
            (Some(started_at), None, None, None, time_to_first_token_ms) => {
                Ok(HistoryTiming::Partial {
                    started_at,
                    time_to_first_token_ms,
                })
            }
            _ => Err(()),
        }
    }

    /// Deserialize the materialized view of a current event-sourced run.
    pub fn from_value(value: Value) -> Result<Self, serde_json::Error> {
        let wire: HistoryRecordWire = serde_json::from_value(value)?;
        if version_rank(&wire.schema_version).is_none() {
            return Err(serde_json::Error::io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "unsupported history schema version `{}`",
                    wire.schema_version
                ),
            )));
        }
        if !identity_fields_valid(
            &wire.harness,
            wire.variant.as_deref(),
            wire.harness_id.as_deref(),
        ) {
            return Err(invalid_history("inconsistent history harness identity"));
        }
        let history_id = wire.history_id.ok_or_else(|| {
            serde_json::Error::io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "history record is missing `history_id`",
            ))
        })?;
        let schema_version = wire.schema_version.clone();
        let record = Self::from_wire(wire, history_id, schema_version);
        if !record.complete() {
            return Err(invalid_history(
                "history schema v1.0 record has incomplete telemetry",
            ));
        }
        Ok(record)
    }

    fn from_wire(wire: HistoryRecordWire, history_id: HistoryId, schema_version: String) -> Self {
        Self {
            schema_version,
            history_id,
            session: wire.session,
            name: wire.name,
            labels: wire.labels,
            project: wire.project,
            timestamp: wire.timestamp.clone(),
            harness: wire.harness.clone(),
            variant: wire.variant,
            harness_id: wire.harness_id.unwrap_or_else(|| wire.harness.clone()),
            model: wire.model,
            prompt: wire.prompt,
            permission_mode: wire.permission_mode,
            status: wire.status,
            exit_code: wire.exit_code,
            duration_ms: wire.duration_ms,
            started_at: wire.started_at,
            finished_at: wire.finished_at,
            model_ms: wire.model_ms,
            tool_ms: wire.tool_ms,
            time_to_first_token_ms: wire.time_to_first_token_ms,
            observed_tool_ms: wire.observed_tool_ms,
            text: wire.text,
            text_source: wire.text_source,
            usage: wire.usage,
            session_id: wire.session_id,
            events: wire.events.map(|events| {
                events
                    .into_iter()
                    .map(LegacyActionEvent::into_current)
                    .collect()
            }),
            failure_kind: wire.failure_kind,
            error: wire.error,
        }
    }
}

fn identity_fields_valid(harness: &str, variant: Option<&str>, harness_id: Option<&str>) -> bool {
    let expected = match variant {
        Some(variant)
            if variant
                .parse::<crate::domain::config::VariantName>()
                .is_ok() =>
        {
            format!("{harness}:{variant}")
        }
        Some(_) => return false,
        None => harness.to_string(),
    };
    harness_id.is_none_or(|harness_id| harness_id == expected)
}

fn invalid_history(message: &str) -> serde_json::Error {
    serde_json::Error::io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        message,
    ))
}

/// Validated availability states for v1.0 timing telemetry. Keeping the
/// cross-field wire representation behind this conversion prevents producers
/// and readers from treating a partial set of optional fields as meaningful.
enum HistoryTiming<'a> {
    Unavailable,
    /// The invocation bounds the runner observed directly, with no provider/tool
    /// split derived from them — what a run cut short leaves behind. Only the
    /// bounds: a split read out of a transcript that never finished is not a
    /// measurement, so it is dropped rather than reported as one. Introduced in
    /// [`FIRST_PARTIAL_TIMING_SCHEMA_VERSION`], which every record carrying it
    /// declares.
    Partial {
        started_at: &'a str,
        time_to_first_token_ms: Option<u128>,
    },
    Measured {
        started_at: &'a str,
        finished_at: Option<&'a str>,
        model_ms: u128,
        tool_ms: u128,
    },
}

/// Whether an invocation-bounds-only measurement agrees with the run's own wall
/// clock. Shared by the record and the run line so the file a writer produces and
/// the record a reader materializes are held to one rule.
fn partial_trace_valid(
    started_at: &str,
    time_to_first_token_ms: Option<u128>,
    duration_ms: Option<u128>,
) -> bool {
    !started_at.is_empty()
        && duration_ms.is_some_and(|duration| {
            time_to_first_token_ms.is_none_or(|to_first_token| to_first_token <= duration)
        })
}

/// Whether a record with this status must carry a provider `finished_at` to be a
/// complete measurement: the run reached its own end, so a measured trace that
/// omits the finish is corrupt rather than cut short.
///
/// Public for the same reason [`run_failed`] is: the generated SDK schemas split
/// their measured-timing branches by this predicate, so the rule lives once and
/// is applied to the enum's own variants rather than being restated as a
/// hand-kept status list that a new variant would silently fall out of.
#[must_use]
pub fn requires_provider_finish(status: Status) -> bool {
    matches!(status, Status::Ok | Status::Nonzero)
}

/// Whether the run this record describes did not succeed. Such a run may have no
/// timing *because* it failed — it never reached the boundary a provider trace is
/// measured between — so absent telemetry is its honest representation, exactly
/// as it is for a harness that declares no trace at all. A run that succeeded
/// keeps the full strictness, so a trace-capable harness still cannot silently
/// write corrupt data for work that actually happened.
///
/// The test is the status alone. Extracted `text` is deliberately NOT consulted:
/// a failed run's transcript can still yield partial assistant text (a reasoning
/// line, a half-finished answer), and reading that as "the provider succeeded"
/// would refuse the record for the very runs — cut short mid-turn — this exists
/// to keep. `status` is the run's own verdict on whether it worked.
///
/// Public because the generated SDK schemas split their status branches by this
/// same predicate: one rule, applied to the enum's own serialized variants,
/// rather than two hand-kept lists.
pub fn run_failed(status: Status) -> bool {
    matches!(
        status,
        Status::Nonzero
            | Status::Timeout
            | Status::SpawnError
            | Status::Skipped
            | Status::Cancelled
    )
}

/// The longest normalized failure text a record carries, in *characters* — the
/// unit [`crate::domain::history`] bounds every string in, because it is the one
/// unit `maxLength` expresses and therefore the one every generated SDK
/// validator can agree on. A CLI's error message is a line or two; the bound is
/// what keeps a runaway stderr dump out of a history line, and the marker is
/// what tells a reader the text was cut.
pub const ERROR_MAX: usize = 2048;
const ERROR_TRUNCATION_MARKER: char = '\u{2026}';

/// The error returned when text cannot be a [`FailureText`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("must be non-empty and at most {ERROR_MAX} characters")]
pub struct FailureTextError;

/// A record's failure message: non-empty, and bounded to [`ERROR_MAX`]
/// characters. That pair is the whole invariant — exactly what the generated
/// schema states, so reading a record can never widen what writing one promises.
/// Wrapping it is what keeps an empty or runaway value out of a record entirely,
/// the same way [`HistoryId`] and [`HistoryLabels`] keep their own invariants
/// unrepresentable rather than re-checked at each use.
///
/// Trimming and the truncation marker belong to [`Self::normalized`], the
/// constructor writers use — not to the type. A record read back from disk
/// preserves the text it was written with rather than being quietly rewritten.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct FailureText(String);

impl FailureText {
    /// Trim `text` and bound it to [`ERROR_MAX`] characters, marking the cut so
    /// a reader can tell truncated output from a message that simply ended.
    /// `None` when nothing is left to report.
    #[must_use]
    pub fn normalized(text: &str) -> Option<Self> {
        let source = text.trim();
        if source.is_empty() {
            return None;
        }
        let mut bounded: String = source.chars().take(ERROR_MAX).collect();
        if source.chars().nth(ERROR_MAX).is_some() {
            bounded.truncate(
                bounded
                    .char_indices()
                    .nth(ERROR_MAX - 1)
                    .map_or(bounded.len(), |(offset, _)| offset),
            );
            bounded.push(ERROR_TRUNCATION_MARKER);
        }
        Some(Self(bounded))
    }

    /// The message text — non-empty and within [`ERROR_MAX`] by construction, so
    /// a caller renders or stores it without re-checking either.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FailureText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for FailureText {
    type Err = FailureTextError;

    /// Accept exactly what the schema promises a reader, and nothing more.
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.is_empty() || value.chars().count() > ERROR_MAX {
            return Err(FailureTextError);
        }
        Ok(Self(value.to_string()))
    }
}

impl<'de> Deserialize<'de> for FailureText {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for FailureText {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("FailureText")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        schemars::json_schema!({
            "type": "string",
            "minLength": 1,
            "maxLength": ERROR_MAX,
        })
    }
}

/// The failure text for one run, normalized by [`FailureText::normalized`]:
/// oneharness's own diagnostic when it generated one (a spawn failure, a
/// timeout, a binary that is not installed), else the harness's captured stderr
/// — but only for a run that failed, so a successful run's stderr chatter never
/// lands in history.
fn failure_text(status: Status, error: Option<&str>, stderr: &str) -> Option<FailureText> {
    match error.map(str::trim) {
        Some(error) if !error.is_empty() => FailureText::normalized(error),
        _ if run_failed(status) => FailureText::normalized(stderr),
        _ => None,
    }
}

/// Whether the run this record describes reported a failure *in words* — by its
/// own exit status, or as the one clean exit that still has something to say.
///
/// That exception is [`FailureKind::ToolDeferred`]: a deferred-tool dead-end
/// exits 0 having done no work, and oneharness's own message about it is exactly
/// the failure text a record should carry. It is the only clean exit that gets
/// one — a provider failure classified off a clean run's own output leaves the
/// harness's words in its transcript, not in a diagnostic — so no other kind
/// justifies failure text on a `status: ok` record.
///
/// Deliberately broader than [`run_failed`], which gates *timing* and must stay
/// keyed on the status alone: a clean exit claims the run worked, so its
/// telemetry is still held to the full bar.
fn reported_failure(status: Status, failure_kind: Option<FailureKind>) -> bool {
    run_failed(status) || failure_kind == Some(FailureKind::ToolDeferred)
}

/// Whether `status` is a value a reader at `schema_version` has. A record's
/// version is the promise "you can read this"; a status introduced later would
/// break that promise, so it is refused rather than read back at a version that
/// never had it. Stated here because the generated SDK schemas gate the same
/// value the same way — one rule, two validators.
fn status_version_valid(schema_version: &str, status: Status) -> bool {
    status != Status::Cancelled || version_at_least(schema_version, FIRST_CANCELLED_SCHEMA_VERSION)
}

/// Whether a record's failure text agrees with the rest of the record: the field
/// arrived in [`FIRST_ERROR_SCHEMA_VERSION`], so an older record carrying it was
/// not written by any oneharness, and it is a *failure* signal, so a run that
/// reported no failure has nothing to put in it. (Emptiness and length are not
/// checked here — [`FailureText`] makes those unrepresentable, and the generated
/// schema states them.)
fn error_text_valid(
    schema_version: &str,
    error: Option<&FailureText>,
    status: Status,
    failure_kind: Option<FailureKind>,
) -> bool {
    error.is_none()
        || (version_at_least(schema_version, FIRST_ERROR_SCHEMA_VERSION)
            && reported_failure(status, failure_kind))
}

impl<'de> Deserialize<'de> for HistoryRecord {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        Self::from_value(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Deserialize)]
struct HistoryRecordWire {
    schema_version: String,
    history_id: Option<HistoryId>,
    session: String,
    name: String,
    #[serde(default)]
    labels: HistoryLabels,
    project: String,
    timestamp: String,
    harness: String,
    #[serde(default)]
    // llmlint: ignore[invalid_states_unrepresentable] This private compatibility wire type must deserialize legacy optional strings before schema-aware normalization; it is never exposed directly.
    variant: Option<String>,
    #[serde(default)]
    // llmlint: ignore[invalid_states_unrepresentable] This private optional field exists only for v1.0 compatibility and is normalized into the required materialized composed id.
    harness_id: Option<String>,
    model: Option<String>,
    prompt: String,
    permission_mode: PermissionMode,
    status: Status,
    exit_code: Option<i32>,
    duration_ms: Option<u128>,
    #[serde(default)]
    started_at: Option<String>,
    #[serde(default)]
    finished_at: Option<String>,
    #[serde(default)]
    model_ms: Option<u128>,
    #[serde(default)]
    tool_ms: Option<u128>,
    #[serde(default)]
    time_to_first_token_ms: Option<u128>,
    #[serde(default)]
    observed_tool_ms: Option<u128>,
    text: Option<String>,
    text_source: Option<String>,
    usage: Usage,
    session_id: Option<String>,
    events: Option<Vec<LegacyActionEvent>>,
    failure_kind: Option<FailureKind>,
    #[serde(default)]
    error: Option<FailureText>,
}

/// Superset reader for action events across the legacy contracts. Versions 0.1
/// and 0.2 ended at `index`; 0.3 added the remaining lifecycle fields.
#[derive(Deserialize)]
struct LegacyActionEvent {
    kind: String,
    name: Option<String>,
    input: Option<Value>,
    output: Option<String>,
    index: usize,
    #[serde(default)]
    tool_call_id: Option<String>,
    #[serde(default)]
    started_at: Option<String>,
    #[serde(default)]
    finished_at: Option<String>,
    #[serde(default)]
    duration_ms: Option<u128>,
    #[serde(default)]
    status: Option<ToolCallStatus>,
    #[serde(default)]
    timing_source: Option<crate::domain::events::TimingSource>,
}

impl LegacyActionEvent {
    fn into_current(self) -> ActionEvent {
        ActionEvent {
            kind: self.kind,
            name: self.name,
            input: self.input,
            output: self.output,
            index: self.index,
            tool_call_id: self.tool_call_id,
            started_at: self.started_at,
            finished_at: self.finished_at,
            duration_ms: self.duration_ms,
            status: self.status,
            timing_source: self.timing_source,
        }
    }
}

/// One line emitted by `history watch --format jsonl`. The tagged envelope lets
/// SDKs distinguish the stream from other NDJSON surfaces while the record's
/// `history_id` remains the resumable cursor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HistoryStreamEnvelope {
    /// A newly observed history record.
    Record { record: HistoryRecord },
    /// A normalized action event observed before its run closes.
    Event { line: HistoryEventLine },
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

/// Format a UNIX millisecond instant as RFC3339 UTC while retaining the
/// precision needed to distinguish short model/tool intervals.
pub fn format_rfc3339_millis(millis: u128) -> String {
    let secs = (millis / 1_000).min(i64::MAX as u128) as i64;
    let fraction = millis % 1_000;
    let base = format_rfc3339(secs);
    format!("{}.{fraction:03}Z", base.trim_end_matches('Z'))
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
            variant: None,
            harness_id: "claude-code".to_string(),
            bin: "claude".to_string(),
            available: true,
            status: Status::Ok,
            prompt: None,
            model: None,
            exit_code: Some(0),
            duration_ms: Some(42),
            telemetry: Some(
                crate::domain::report::ExecutionTelemetry::ProviderMeasured {
                    started_at: "2026-07-07T13:14:14.958Z".parse().unwrap(),
                    finished_at: Some("2026-07-07T13:14:15.000Z".parse().unwrap()),
                    model_ms: Some(42),
                    tool_ms: Some(0),
                    time_to_first_token_ms: Some(10),
                },
            ),
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
            HistoryId::legacy(b"record-1"),
            "fix-bug-20260707T131415Z-9",
            "fix-bug",
            &parse_labels(["graph=deploy"]).unwrap(),
            "/home/user/proj",
            "2026-07-07T13:14:15Z".to_string(),
            PermissionMode::Bypass,
            Some("sonnet"),
            "fix the bug",
            &r,
        );
        assert_eq!(rec.schema_version, PREVIOUS_CURRENT_SCHEMA_VERSION);
        assert_eq!(rec.session, "fix-bug-20260707T131415Z-9");
        assert_eq!(rec.name, "fix-bug");
        assert_eq!(rec.labels.as_map().get("graph").unwrap(), "deploy");
        assert_eq!(rec.project, "/home/user/proj");
        assert_eq!(rec.harness, "claude-code");
        assert_eq!(rec.model.as_deref(), Some("sonnet"));
        assert_eq!(rec.session_id.as_deref(), Some("abc-123"));
        assert_eq!(rec.status, Status::Ok);
        // No per-result prompt → the run prompt is used.
        assert_eq!(rec.prompt, "fix the bug");
    }

    #[test]
    fn codex_provider_record_matches_the_pre_observed_timing_bytes() {
        let mut r = result();
        r.harness = "codex".to_string();
        r.harness_id = "codex".to_string();
        r.bin = "codex".to_string();
        let rec = HistoryRecord::from_result(
            HistoryId::legacy(b"codex-provider-golden"),
            "measure-20260707T131415Z-9",
            "measure",
            &parse_labels(["graph=deploy"]).unwrap(),
            "/home/user/proj",
            "2026-07-07T13:14:15Z".to_string(),
            PermissionMode::Bypass,
            Some("gpt-5"),
            "measure the run",
            &r,
        );

        let actual = serde_json::to_string(&rec).unwrap();
        let expected = include_str!("../../../../tests/fixtures/history-codex-v11.json").trim_end();
        assert_eq!(actual.as_bytes(), expected.as_bytes());
    }

    #[test]
    fn from_result_prefers_the_per_result_batch_prompt() {
        let mut r = result();
        r.prompt = Some("batch prompt 2".to_string());
        let rec = HistoryRecord::from_result(
            HistoryId::legacy(b"record-2"),
            "s",
            "n",
            &HistoryLabels::default(),
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

    #[test]
    fn labels_validate_and_later_values_win() {
        let labels = parse_labels(["graph=release", "task=build", "task=test"]).unwrap();
        assert_eq!(labels.as_map().get("graph").unwrap(), "release");
        assert_eq!(labels.as_map().get("task").unwrap(), "test");

        for invalid in ["missing-equals", "=value", "bad/key=value", "key="] {
            assert!(parse_label(invalid).is_err(), "{invalid} should fail");
        }
        assert!(parse_label("key=line\nbreak").is_err());
        assert!(parse_label(&format!("key={}", "x".repeat(LABEL_VALUE_MAX + 1))).is_err());
    }

    #[test]
    fn label_lengths_are_bounded_in_characters_not_bytes() {
        // The bound is stated in characters because that is the unit the JSON
        // Schema the SDKs are generated from can express. An astral character is
        // four UTF-8 bytes, so a byte bound would reject this at a quarter of the
        // documented limit.
        let astral = "🚀".repeat(LABEL_VALUE_MAX);
        assert_eq!(astral.len(), LABEL_VALUE_MAX * 4);
        assert!(parse_label(&format!("key={astral}")).is_ok());

        let over = "🚀".repeat(LABEL_VALUE_MAX + 1);
        let error = parse_label(&format!("key={over}")).expect_err("one character too many");
        assert!(error.contains("exceeds 256 characters"), "{error}");

        assert!(parse_label(&format!("{}=value", "k".repeat(LABEL_KEY_MAX))).is_ok());
        assert!(parse_label(&format!("{}=value", "k".repeat(LABEL_KEY_MAX + 1))).is_err());
    }

    #[test]
    fn label_values_reject_every_character_rust_calls_control() {
        // Including the C1 block, which the schema's allow-list once let through
        // while this runtime check rejected it.
        for control in ['\u{0}', '\u{1f}', '\u{7f}', '\u{80}', '\u{85}', '\u{9f}'] {
            assert!(control.is_control(), "{control:?} must be a control char");
            assert!(
                parse_label(&format!("key=a{control}b")).is_err(),
                "{control:?} must be rejected"
            );
        }
        // A lone trailing newline is the spelling a `$`-anchored schema pattern
        // would miss under a regex engine whose `$` also matches before one.
        assert!(parse_label("key=value\n").is_err());
        // Neighbours of the control ranges stay valid.
        assert!(parse_label("key=caf\u{e9} \u{7e}\u{a0}").is_ok());
    }

    #[test]
    fn history_id_accepts_only_canonical_hyphenated_text() {
        let canonical = "01931b0f-4a3c-7cde-bf12-3456789abcde";
        assert_eq!(
            canonical.parse::<HistoryId>().unwrap().to_string(),
            canonical
        );
        // Upper-case hex names the same id; every other spelling `Uuid::parse_str`
        // would take is not the canonical text the schema promises.
        assert_eq!(
            canonical
                .to_ascii_uppercase()
                .parse::<HistoryId>()
                .unwrap()
                .to_string(),
            canonical
        );
        for rejected in [
            "01931b0f4a3c7cdebf123456789abcde",       // simple, unhyphenated
            "{01931b0f-4a3c-7cde-bf12-3456789abcde}", // braced
            "urn:uuid:01931b0f-4a3c-7cde-bf12-3456789abcde", // URN
            "01931b0f-4a3c-7cde-bf12-3456789abcde\n", // trailing newline
            "01931b0f-4a3c-7cde-0f12-3456789abcde",   // NCS variant
            "01931b0f-4a3c-7cde-cf12-3456789abcde",   // Microsoft variant
            "01931b0f-4a3c-0cde-bf12-3456789abcde",   // undefined version
            "01931b0f-4a3c-9cde-bf12-3456789abcde",   // undefined version
            "00000000-0000-0000-0000-000000000000",   // nil
            "not-a-uuid",
        ] {
            assert!(
                rejected.parse::<HistoryId>().is_err(),
                "{rejected} must be rejected"
            );
        }
    }

    #[test]
    fn schemas_state_exactly_what_the_runtime_enforces() {
        // These are hand-written, so nothing but a test keeps them in step with
        // `validate_label` and `HistoryId::from_str` above. Every SDK's validator
        // is generated from them.
        let labels = schemars::schema_for!(HistoryLabels);
        let labels = labels.as_value();
        let value = &labels["additionalProperties"];
        assert_eq!(value["minLength"], 1);
        assert_eq!(value["maxLength"], LABEL_VALUE_MAX);
        assert_eq!(value["not"]["pattern"], LABEL_VALUE_FORBIDDEN_PATTERN);
        // An anchored allow-list would let a trailing newline through a regex
        // engine whose `$` matches before one, so there must be no `pattern`.
        assert!(value.get("pattern").is_none(), "{value}");

        let key = &labels["propertyNames"];
        assert_eq!(key["maxLength"], LABEL_KEY_MAX);
        assert_eq!(key["pattern"], LABEL_KEY_START_PATTERN);
        assert_eq!(key["not"]["pattern"], LABEL_KEY_FORBIDDEN_PATTERN);

        let id = schemars::schema_for!(HistoryId);
        let id = id.as_value();
        assert_eq!(id["pattern"], UUID_PATTERN);
        assert_eq!(id["minLength"], UUID_LEN);
        assert_eq!(id["maxLength"], UUID_LEN);

        let failure = schemars::schema_for!(FailureText);
        let failure = failure.as_value();
        assert_eq!(failure["minLength"], 1);
        assert_eq!(failure["maxLength"], ERROR_MAX);
        // The bound is a character count in both places: an astral-heavy message
        // at the limit must read back, or an SDK would refuse what the CLI wrote.
        let astral = "\u{1F600}".repeat(ERROR_MAX);
        assert_eq!(
            FailureText::normalized(&astral).map(|text| text.as_str().chars().count()),
            Some(ERROR_MAX)
        );
        assert!(astral.parse::<FailureText>().is_ok());
    }

    #[test]
    fn minted_history_ids_round_trip_through_the_public_contract() {
        // A migrated legacy record is the one id minted in this pure layer, so the
        // text it renders must be text the same contract accepts back.
        let id = HistoryId::legacy(b"stable-key");
        assert_eq!(id.as_uuid().get_version_num(), 5);
        assert_eq!(id.to_string().parse::<HistoryId>().unwrap(), id);
    }

    #[test]
    fn current_record_requires_a_valid_id_but_tolerates_additive_fields() {
        let current = HistoryRecord::from_result(
            HistoryId::legacy(b"current"),
            "session",
            "name",
            &HistoryLabels::default(),
            "/project",
            "2026-01-01T00:00:00Z".to_string(),
            PermissionMode::Default,
            None,
            "prompt",
            &result(),
        );
        let mut value = serde_json::to_value(&current).unwrap();
        value["future_output_field"] = serde_json::json!({ "accepted": true });
        assert_eq!(
            serde_json::from_value::<HistoryRecord>(value).unwrap(),
            current
        );
        let mut inconsistent = serde_json::to_value(&current).unwrap();
        inconsistent["harness_id"] = Value::String("codex:work".to_string());
        assert!(serde_json::from_value::<HistoryRecord>(inconsistent).is_err());
        let mut malformed_variant = serde_json::to_value(&current).unwrap();
        malformed_variant["variant"] = Value::String("bad.name".to_string());
        malformed_variant["harness_id"] = Value::String("claude-code:bad.name".to_string());
        assert!(serde_json::from_value::<HistoryRecord>(malformed_variant).is_err());

        let mut invalid_v03 = serde_json::to_value(&current).unwrap();
        invalid_v03.as_object_mut().unwrap().remove("model_ms");
        assert!(serde_json::from_value::<HistoryRecord>(invalid_v03).is_err());

        let mut unavailable_v03 = serde_json::to_value(&current).unwrap();
        for field in [
            "started_at",
            "finished_at",
            "model_ms",
            "tool_ms",
            "time_to_first_token_ms",
        ] {
            unavailable_v03.as_object_mut().unwrap().remove(field);
        }
        assert!(serde_json::from_value::<HistoryRecord>(unavailable_v03).is_ok());

        let mut invalid_unavailable = serde_json::to_value(&current).unwrap();
        for field in [
            "started_at",
            "model_ms",
            "tool_ms",
            "time_to_first_token_ms",
        ] {
            invalid_unavailable.as_object_mut().unwrap().remove(field);
        }
        invalid_unavailable["finished_at"] = Value::Null;
        invalid_unavailable["events"] = serde_json::json!([{
            "kind": "tool_call", "name": "shell", "input": {}, "output": null, "index": 0,
            "tool_call_id": "call-1", "started_at": "2026-01-01T00:00:00Z",
            "finished_at": null, "duration_ms": null, "status": null
        }]);
        assert!(serde_json::from_value::<HistoryRecord>(invalid_unavailable).is_err());

        let mut previous = serde_json::to_value(&current).unwrap();
        previous["schema_version"] = Value::String("0.2".to_string());
        for field in ["started_at", "finished_at", "model_ms", "tool_ms"] {
            previous.as_object_mut().unwrap().remove(field);
        }
        assert!(serde_json::from_value::<HistoryRecord>(previous).is_err());

        let mut missing = serde_json::to_value(&current).unwrap();
        missing.as_object_mut().unwrap().remove("history_id");
        assert!(serde_json::from_value::<HistoryRecord>(missing).is_err());
        let mut malformed = serde_json::to_value(current).unwrap();
        malformed["history_id"] = Value::String("not-a-uuid".to_string());
        assert!(serde_json::from_value::<HistoryRecord>(malformed).is_err());

        let mut unsupported = serde_json::to_value(HistoryRecord::from_result(
            HistoryId::legacy(b"future"),
            "session",
            "name",
            &HistoryLabels::default(),
            "/project",
            "2026-01-01T00:00:00Z".to_string(),
            PermissionMode::Default,
            None,
            "prompt",
            &result(),
        ))
        .unwrap();
        unsupported["schema_version"] = Value::String("9.9".to_string());
        assert!(serde_json::from_value::<HistoryRecord>(unsupported).is_err());
    }

    /// A codex run — a harness that *does* declare a provider trace, so timing is
    /// normally mandatory — killed before it produced any answer.
    fn failed_traced_result() -> RunResult {
        RunResult {
            harness: "codex".to_string(),
            harness_id: "codex".to_string(),
            bin: "codex".to_string(),
            status: Status::Nonzero,
            exit_code: Some(1),
            telemetry: None,
            text: None,
            text_source: None,
            session_id: None,
            stdout: String::new(),
            stderr: "Error: insufficient_quota".to_string(),
            failure_kind: Some(FailureKind::Quota),
            ..result()
        }
    }

    fn record_of(r: &RunResult) -> HistoryRecord {
        HistoryRecord::from_result(
            HistoryId::legacy(b"failure"),
            "session",
            "name",
            &HistoryLabels::default(),
            "/project",
            "2026-01-01T00:00:00Z".to_string(),
            PermissionMode::Default,
            None,
            "prompt",
            r,
        )
    }

    #[test]
    fn a_cancelled_run_records_at_the_version_that_first_understood_it() {
        // `cancelled` is a status value no v1.3 reader has, so a record carrying
        // it must declare v1.4 — otherwise an older consumer would read the
        // version as a promise it can parse the record, and then choke on it.
        let cancelled = record_of(&RunResult {
            status: Status::Cancelled,
            exit_code: None,
            error: Some("`codex` was cancelled".to_string()),
            ..failed_traced_result()
        });
        assert_eq!(cancelled.status, Status::Cancelled);
        assert_eq!(cancelled.schema_version, FIRST_CANCELLED_SCHEMA_VERSION);
        assert!(run_failed(cancelled.status));
        assert!(cancelled.complete());
        // Round-trips: the writer's shape is the reader's shape.
        let wire = serde_json::to_value(&cancelled).unwrap();
        assert_eq!(wire["status"], "cancelled");
        assert_eq!(
            serde_json::from_value::<HistoryRecord>(wire).unwrap(),
            cancelled
        );
    }

    #[test]
    fn a_failed_run_records_with_whatever_telemetry_its_failure_left() {
        let failed = record_of(&failed_traced_result());
        // Absent timing: the run never reached the boundary a trace is measured
        // between, so absence is the honest reading — not a reason to drop it.
        assert!(failed.complete());
        assert_eq!(failed.failure_kind, Some(FailureKind::Quota));
        // The harness's own stderr is what an operator reads when the classified
        // kind is not enough, so it rides along — and forces v1.3.
        assert_eq!(
            failed.error.as_ref().map(FailureText::as_str),
            Some("Error: insufficient_quota")
        );
        assert_eq!(failed.schema_version, FIRST_ERROR_SCHEMA_VERSION);

        // Events the failure interrupted keep their observed boundaries; the
        // "no run timing means no event timing" rule is a success-run rule.
        let mut interrupted = serde_json::to_value(&failed).unwrap();
        interrupted["events"] = serde_json::json!([{
            "kind": "tool_call", "name": "shell", "input": {}, "output": null, "index": 0,
            "tool_call_id": "call-1", "started_at": "2026-01-01T00:00:00Z",
            "finished_at": null, "duration_ms": null, "status": "interrupted"
        }]);
        assert!(serde_json::from_value::<HistoryRecord>(interrupted).is_ok());

        // The run line written to the JSONL file reads back by the same rule.
        let line =
            serde_json::to_value(HistoryLine::Run(HistoryRunRecord::from_record(&failed))).unwrap();
        assert_eq!(
            line["error"],
            Value::String("Error: insufficient_quota".to_string())
        );
        assert!(serde_json::from_value::<HistoryLine>(line).is_ok());
    }

    /// The carve-out reads the run's own verdict, never its extracted text. A
    /// turn cut short can still leave partial assistant text behind, and treating
    /// that as "the provider succeeded" would refuse exactly the records this
    /// exists to keep.
    #[test]
    fn a_failure_that_left_partial_text_is_still_recordable() {
        let partial_answer = record_of(&RunResult {
            text: Some("I was still thinking when".to_string()),
            text_source: Some("json:codex-agent-message".to_string()),
            ..failed_traced_result()
        });
        assert!(partial_answer.complete());
        assert_eq!(
            partial_answer.text.as_deref(),
            Some("I was still thinking when")
        );
    }

    #[test]
    fn a_run_that_succeeded_still_needs_complete_telemetry() {
        // A clean exit is the run's own claim that it worked, so a trace-capable
        // harness that reported no timing for it is drift, not an honest failure.
        let worked = record_of(&RunResult {
            status: Status::Ok,
            exit_code: Some(0),
            stderr: String::new(),
            failure_kind: None,
            text: Some("the answer is 42".to_string()),
            ..failed_traced_result()
        });
        assert!(!worked.complete());
        assert!(worked.error.is_none());
        // ... and one whose answer was never extracted is no different.
        let silent = record_of(&RunResult {
            status: Status::Ok,
            exit_code: Some(0),
            stderr: String::new(),
            failure_kind: None,
            ..failed_traced_result()
        });
        assert!(!silent.complete());
    }

    /// A run cut short leaves the invocation bounds the runner watched directly,
    /// with no provider/tool split derived from a transcript that never finished.
    /// That partial measurement is legible on a failed run and corrupt on one
    /// that succeeded — where the provider answered, so the split it never wrote
    /// should have been there.
    #[test]
    fn invocation_bounds_without_a_split_belong_to_a_run_that_was_cut_short() {
        let mut partial = serde_json::to_value(record_of(&failed_traced_result())).unwrap();
        partial["started_at"] = Value::String("2026-01-01T00:00:00Z".to_string());
        let read = serde_json::from_value::<HistoryRecord>(partial.clone()).unwrap();
        assert_eq!(read.started_at.as_deref(), Some("2026-01-01T00:00:00Z"));
        assert!(read.model_ms.is_none() && read.tool_ms.is_none());
        // A first-token offset within the run's duration rides along...
        let mut with_offset = partial.clone();
        with_offset["time_to_first_token_ms"] = Value::from(7);
        assert!(serde_json::from_value::<HistoryRecord>(with_offset).is_ok());
        // ...but not one the run was never long enough to contain.
        let mut impossible = partial.clone();
        impossible["time_to_first_token_ms"] = Value::from(u64::from(u32::MAX));
        assert!(serde_json::from_value::<HistoryRecord>(impossible).is_err());

        // The written run line reads back by the same rule.
        let mut line =
            serde_json::to_value(HistoryLine::Run(HistoryRunRecord::from_record(&read))).unwrap();
        assert!(serde_json::from_value::<HistoryLine>(line.clone()).is_ok());

        // The same shape on a run that succeeded is refused, record and line alike.
        for succeeded in ["ok", "planned"] {
            let mut worked = partial.clone();
            worked["status"] = Value::String(succeeded.to_string());
            worked["failure_kind"] = Value::Null;
            worked.as_object_mut().unwrap().remove("error");
            assert!(
                serde_json::from_value::<HistoryRecord>(worked).is_err(),
                "{succeeded}"
            );
        }
        line["status"] = Value::String("ok".to_string());
        line["failure_kind"] = Value::Null;
        line.as_object_mut().unwrap().remove("error");
        assert!(serde_json::from_value::<HistoryLine>(line).is_err());
    }

    /// A measurement is either whole, stopped at the bounds, or absent. Anything
    /// else is incoherent rather than partial — a total with no counterpart, an
    /// offset with nothing to measure it from — and no status makes it legible.
    #[test]
    fn an_incoherent_half_measurement_is_refused_whatever_the_run_did() {
        let base = serde_json::to_value(record_of(&failed_traced_result())).unwrap();
        let incoherent = [
            (
                "model total with no tool total",
                vec![
                    (
                        "started_at",
                        Value::String("2026-01-01T00:00:00Z".to_string()),
                    ),
                    ("model_ms", Value::from(1)),
                ],
            ),
            (
                "tool total with no model total",
                vec![
                    (
                        "started_at",
                        Value::String("2026-01-01T00:00:00Z".to_string()),
                    ),
                    ("tool_ms", Value::from(1)),
                ],
            ),
            (
                "first-token offset with no start",
                vec![("time_to_first_token_ms", Value::from(1))],
            ),
            (
                "finish with no start",
                vec![(
                    "finished_at",
                    Value::String("2026-01-01T00:00:01Z".to_string()),
                )],
            ),
        ];
        for (tag, fields) in incoherent {
            for status in ["nonzero", "timeout", "ok"] {
                let mut record = base.clone();
                record["status"] = Value::String(status.to_string());
                for (field, value) in &fields {
                    record[*field] = value.clone();
                }
                assert!(
                    serde_json::from_value::<HistoryRecord>(record).is_err(),
                    "{tag} on {status}"
                );
            }
        }
    }

    #[test]
    fn failure_text_is_normalized_bounded_and_never_taken_from_a_run_that_worked() {
        let text = |status, error, stderr| {
            failure_text(status, error, stderr).map(|text| text.as_str().to_string())
        };
        // oneharness's own diagnostic wins over the child's stderr: on a spawn
        // failure or a timeout it is the only account of what happened.
        assert_eq!(
            text(Status::SpawnError, Some("  could not spawn  "), "noise").as_deref(),
            Some("could not spawn")
        );
        // A failed run with no diagnostic of its own falls back to stderr.
        assert_eq!(
            text(Status::Nonzero, None, "\n401 Unauthorized\n").as_deref(),
            Some("401 Unauthorized")
        );
        // A run that worked never contributes its stderr chatter.
        assert_eq!(text(Status::Ok, None, "warning: deprecated"), None);
        // Nothing to say stays absent rather than becoming an empty string.
        assert_eq!(text(Status::Nonzero, None, "   \n"), None);
        // A runaway stderr is cut to the bound, with the cut marked.
        let flood = "x".repeat(ERROR_MAX * 2);
        let bounded = text(Status::Timeout, None, &flood).unwrap();
        assert_eq!(bounded.chars().count(), ERROR_MAX);
        assert!(bounded.ends_with(ERROR_TRUNCATION_MARKER));
        // Exactly at the bound, nothing is marked.
        let exact = "y".repeat(ERROR_MAX);
        assert_eq!(
            text(Status::Timeout, None, &exact).as_deref(),
            Some(&*exact)
        );
        // An invalid value is not constructible, so a record cannot carry one.
        assert_eq!("".parse::<FailureText>(), Err(FailureTextError));
        assert_eq!(
            "z".repeat(ERROR_MAX + 1).parse::<FailureText>(),
            Err(FailureTextError)
        );
    }

    #[test]
    fn failure_text_is_gated_to_the_version_that_introduced_it() {
        let failed = record_of(&failed_traced_result());
        let mut older = serde_json::to_value(&failed).unwrap();
        for version in [
            FIRST_EVENT_SCHEMA_VERSION,
            PREVIOUS_CURRENT_SCHEMA_VERSION,
            OBSERVED_TIMING_SCHEMA_VERSION,
        ] {
            older["schema_version"] = Value::String(version.to_string());
            assert!(
                serde_json::from_value::<HistoryRecord>(older.clone()).is_err(),
                "{version} predates the error field"
            );
        }
        // An empty or over-long value is not what the contract promises either.
        let mut empty = serde_json::to_value(&failed).unwrap();
        empty["error"] = Value::String(String::new());
        assert!(serde_json::from_value::<HistoryRecord>(empty).is_err());
        let mut long = serde_json::to_value(&failed).unwrap();
        long["error"] = Value::String("z".repeat(ERROR_MAX + 1));
        assert!(serde_json::from_value::<HistoryRecord>(long).is_err());
    }

    /// `error` is a failure signal, so the record cannot claim one for a run that
    /// reported no failure — but a clean exit that *did* report one (the
    /// deferred-tool dead-end: exit 0, no work done) is exactly the case the
    /// field exists for, and must not be caught by the same rule.
    #[test]
    fn failure_text_belongs_only_to_a_run_that_reported_a_failure() {
        let failed = record_of(&failed_traced_result());
        let mut clean_exit = serde_json::to_value(&failed).unwrap();
        clean_exit["status"] = Value::String("ok".to_string());
        clean_exit["exit_code"] = Value::from(0);
        clean_exit["started_at"] = Value::String("2026-01-01T00:00:00Z".to_string());
        clean_exit["finished_at"] = Value::String("2026-01-01T00:00:01Z".to_string());
        clean_exit["model_ms"] = Value::from(1);
        clean_exit["tool_ms"] = Value::from(0);
        clean_exit["duration_ms"] = Value::from(42);

        let mut unreported = clean_exit.clone();
        unreported["failure_kind"] = Value::Null;
        assert!(serde_json::from_value::<HistoryRecord>(unreported).is_err());

        let deferred = record_of(&RunResult {
            status: Status::Ok,
            exit_code: Some(0),
            stderr: String::new(),
            failure_kind: Some(FailureKind::ToolDeferred),
            error: Some("claude-code deferred a builtin tool call".to_string()),
            ..result()
        });
        assert_eq!(
            deferred.error.as_ref().map(FailureText::as_str),
            Some("claude-code deferred a builtin tool call")
        );
        assert!(deferred.complete());

        // ...and it is the *only* clean exit that gets one. A provider failure
        // classified off a clean run's own output left the harness's words in its
        // transcript, not in a diagnostic, so no other kind justifies the field.
        for other in [
            FailureKind::Auth,
            FailureKind::Quota,
            FailureKind::RateLimit,
            FailureKind::ModelNotFound,
        ] {
            let mut mismatched = clean_exit.clone();
            mismatched["failure_kind"] = serde_json::to_value(other).unwrap();
            assert!(
                serde_json::from_value::<HistoryRecord>(mismatched).is_err(),
                "{other:?} on a clean exit"
            );
        }
    }

    #[test]
    fn readable_versions_are_ordered_so_a_field_gate_can_ask_for_a_minimum() {
        assert!(version_at_least(SCHEMA_VERSION, FIRST_EVENT_SCHEMA_VERSION));
        assert!(version_at_least(
            OBSERVED_TIMING_SCHEMA_VERSION,
            OBSERVED_TIMING_SCHEMA_VERSION
        ));
        assert!(!version_at_least(
            PREVIOUS_CURRENT_SCHEMA_VERSION,
            OBSERVED_TIMING_SCHEMA_VERSION
        ));
        // An unreadable version never satisfies a minimum by accident.
        assert!(!version_at_least("9.9", FIRST_EVENT_SCHEMA_VERSION));
        assert!(!version_at_least(SCHEMA_VERSION, "9.9"));
    }

    #[test]
    fn pre_v1_2_record_rejects_event_timing_provenance() {
        let current = HistoryRecord::from_result(
            HistoryId::legacy(b"pre-v1.2-event-timing"),
            "session",
            "name",
            &HistoryLabels::default(),
            "/project",
            "2026-01-01T00:00:00Z".to_string(),
            PermissionMode::Default,
            None,
            "prompt",
            &result(),
        );
        let mut value = serde_json::to_value(current).unwrap();
        value["events"] = serde_json::json!([{
            "kind": "tool_call",
            "name": "Bash",
            "input": {"command": "true"},
            "output": "",
            "index": 0,
            "tool_call_id": "tool-1",
            "started_at": "2026-01-01T00:00:00.000Z",
            "finished_at": "2026-01-01T00:00:00.001Z",
            "duration_ms": 1,
            "status": "completed",
            "timing_source": "stdout_observed"
        }]);

        assert!(serde_json::from_value::<HistoryRecord>(value.clone()).is_err());
        value["schema_version"] = Value::String(SCHEMA_VERSION.to_string());
        assert!(serde_json::from_value::<HistoryRecord>(value).is_ok());
    }
}
