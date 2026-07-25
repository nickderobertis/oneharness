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
pub const SCHEMA_VERSION: &str = "1.1";
const PREVIOUS_CURRENT_SCHEMA_VERSION: &str = "1.0";

/// The legacy record contract accepted by the migration reader.
pub const LEGACY_SCHEMA_VERSION: &str = "0.1";
const PREVIOUS_SCHEMA_VERSION: &str = "0.2";
const LEGACY_RECORD_SCHEMA_VERSION: &str = "0.3";

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
        matches!(
            self.schema_version.as_str(),
            SCHEMA_VERSION | PREVIOUS_CURRENT_SCHEMA_VERSION
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
    pub text: Option<String>,
    pub text_source: Option<String>,
    pub usage: Usage,
    pub session_id: Option<String>,
    pub failure_kind: Option<FailureKind>,
}

impl HistoryRunRecord {
    pub(crate) fn valid(&self) -> bool {
        if !matches!(
            self.schema_version.as_str(),
            SCHEMA_VERSION | PREVIOUS_CURRENT_SCHEMA_VERSION
        ) {
            return false;
        }
        if !identity_fields_valid(
            &self.harness,
            self.variant.as_deref(),
            self.harness_id.as_deref(),
        ) {
            return false;
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
                    && (!matches!(self.status, Status::Ok | Status::Nonzero)
                        || finished_at.is_some())
            }
            _ => false,
        }
    }

    /// Split the terminal portion from the familiar materialized record.
    pub fn from_record(record: &HistoryRecord) -> Self {
        Self {
            schema_version: SCHEMA_VERSION.to_string(),
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
            text: record.text.clone(),
            text_source: record.text_source.clone(),
            usage: record.usage.clone(),
            session_id: record.session_id.clone(),
            failure_kind: record.failure_kind,
        }
    }

    /// Rebuild the stable per-run presentation object from event-sourced lines.
    pub fn materialize(self, events: Vec<ActionEvent>) -> HistoryRecord {
        let harness_id = self
            .harness_id
            .clone()
            .unwrap_or_else(|| self.harness.clone());
        HistoryRecord {
            schema_version: SCHEMA_VERSION.to_string(),
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
            text: self.text,
            text_source: self.text_source,
            usage: self.usage,
            session_id: self.session_id,
            events: (!events.is_empty()).then_some(events),
            failure_kind: self.failure_kind,
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
        HistoryRecord {
            schema_version: SCHEMA_VERSION.to_string(),
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
            started_at: telemetry.map(|telemetry| telemetry.started_at.clone()),
            finished_at: telemetry.and_then(|telemetry| telemetry.finished_at.clone()),
            model_ms: telemetry.and_then(|telemetry| telemetry.model_ms),
            tool_ms: telemetry.and_then(|telemetry| telemetry.tool_ms),
            time_to_first_token_ms: telemetry
                .and_then(|telemetry| telemetry.time_to_first_token_ms),
            text: r.text.clone(),
            text_source: r.text_source.clone(),
            usage: r.usage.clone(),
            session_id: r.session_id.clone(),
            events: r.events.clone(),
            failure_kind: r.failure_kind,
        }
    }

    pub(crate) fn complete(&self) -> bool {
        let timing_unavailable = matches!(self.timing_state(), Ok(HistoryTiming::Unavailable))
            && crate::domain::harness::all()
                .iter()
                .find(|spec| spec.id == self.harness)
                .is_some_and(|spec| spec.telemetry.is_none());
        self.valid_v03()
            && (!matches!(self.timing_state(), Ok(HistoryTiming::Unavailable))
                || timing_unavailable)
    }

    fn valid_v03(&self) -> bool {
        // A harness without a provider/tool boundary trace cannot derive timing.
        // Absence is the honest v1.0 representation; partial telemetry remains
        // invalid so a trace-capable harness cannot silently write corrupt data.
        let timing = match self.timing_state() {
            Ok(HistoryTiming::Unavailable) => {
                return self.events.as_ref().is_none_or(|events| {
                    events.iter().all(|event| {
                        event.started_at.is_none()
                            && event.finished_at.is_none()
                            && event.duration_ms.is_none()
                            && event.status.is_none()
                    })
                });
            }
            Ok(HistoryTiming::Measured {
                started_at,
                finished_at,
                model_ms,
                tool_ms,
            }) => (started_at, finished_at, model_ms, tool_ms),
            Err(()) => return false,
        };
        let Some(duration) = self.duration_ms else {
            return false;
        };
        if timing.0.is_empty() || timing.2.saturating_add(timing.3) > duration {
            return false;
        }
        if matches!(self.status, Status::Ok | Status::Nonzero) && timing.1.is_none() {
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
            _ => Err(()),
        }
    }

    /// Deserialize the materialized view of a current event-sourced run.
    pub fn from_value(value: Value) -> Result<Self, serde_json::Error> {
        let wire: HistoryRecordWire = serde_json::from_value(value)?;
        if !matches!(
            wire.schema_version.as_str(),
            SCHEMA_VERSION | PREVIOUS_CURRENT_SCHEMA_VERSION
        ) {
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
        let record = Self::from_wire(wire, history_id, SCHEMA_VERSION.to_string());
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
    Measured {
        started_at: &'a str,
        finished_at: Option<&'a str>,
        model_ms: u128,
        tool_ms: u128,
    },
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
    text: Option<String>,
    text_source: Option<String>,
    usage: Usage,
    session_id: Option<String>,
    events: Option<Vec<LegacyActionEvent>>,
    failure_kind: Option<FailureKind>,
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
            telemetry: Some(crate::domain::report::ExecutionTelemetry {
                started_at: "2026-07-07T13:14:14.958Z".to_string(),
                finished_at: Some("2026-07-07T13:14:15.000Z".to_string()),
                model_ms: Some(42),
                tool_ms: Some(0),
                time_to_first_token_ms: Some(10),
            }),
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
        assert_eq!(rec.schema_version, SCHEMA_VERSION);
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
}
