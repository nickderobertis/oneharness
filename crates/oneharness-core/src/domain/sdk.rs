//! Rust-owned schema metadata for the Node SDK's public input contract.
//!
//! The SDK generator reflects this type into both TypeScript and Zod. Keeping
//! the option names here means adding an SDK option cannot leave its static and
//! runtime contracts out of sync. This is metadata only: command orchestration
//! stays in the Node SDK and the CLI command layer.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};
use thiserror::Error;

use crate::domain::mode::PermissionMode;

/// Generate a schema for a value emitted by oneharness.
///
/// Schemars defaults to treating an [`Option`] field as omissible. oneharness's
/// output structs serialize every field, so this normalizes every object schema
/// to require its declared properties while retaining each option's nullable
/// value schema.
pub fn schema_for_serialize<T: ?Sized + JsonSchema>() -> Schema {
    let mut schema = SchemaGenerator::default().into_root_schema_for::<T>();
    if let Some(object) = schema.as_object_mut() {
        require_declared_properties(object);
    }
    schema
}

fn require_declared_properties(object: &mut Map<String, Value>) {
    if let Some(required) = object
        .get("properties")
        .and_then(Value::as_object)
        .map(|properties| properties.keys().cloned().map(Value::String).collect())
    {
        object.insert("required".to_string(), Value::Array(required));
    }

    for keyword in ["properties", "$defs"] {
        if let Some(children) = object.get_mut(keyword).and_then(Value::as_object_mut) {
            for child in children.values_mut() {
                visit_schema(child);
            }
        }
    }
    for keyword in ["oneOf", "anyOf", "allOf"] {
        if let Some(children) = object.get_mut(keyword).and_then(Value::as_array_mut) {
            for child in children {
                visit_schema(child);
            }
        }
    }
    for keyword in ["items", "additionalProperties"] {
        if let Some(child) = object.get_mut(keyword) {
            visit_schema(child);
        }
    }
}

fn visit_schema(schema: &mut Value) {
    if let Some(object) = schema.as_object_mut() {
        require_declared_properties(object);
    } else if let Some(items) = schema.as_array_mut() {
        for item in items {
            visit_schema(item);
        }
    }
}

/// The error returned when a value that must carry text is empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("must be a non-empty string")]
pub struct EmptyStringError;

/// A string that cannot be empty.
///
/// Deserialization rejects `""`, so an empty value is not representable past the
/// boundary and no caller-facing code has to re-check for one. The schema is the
/// inline `string` + `minLength: 1`, so the generated TypeScript stays `string`
/// and the generated Zod adds the runtime check.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct NonEmptyString(String);

impl NonEmptyString {
    /// Borrow the validated text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the wrapper and return the validated text.
    pub fn into_string(self) -> String {
        self.0
    }
}

impl TryFrom<String> for NonEmptyString {
    type Error = EmptyStringError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() {
            return Err(EmptyStringError);
        }
        Ok(Self(value))
    }
}

impl TryFrom<&str> for NonEmptyString {
    type Error = EmptyStringError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::try_from(value.to_string())
    }
}

impl fmt::Display for NonEmptyString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for NonEmptyString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::try_from(raw).map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for NonEmptyString {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("NonEmptyString")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        schemars::json_schema!({
            "type": "string",
            "minLength": 1,
        })
    }
}

/// The literal `true`.
///
/// This is what makes "select the most recent session" a value rather than a
/// flag to re-read: `last: false` does not select, so it is not a value of this
/// type and cannot satisfy the variant that requires it. The schema is the
/// inline `const: true`, so the generated TypeScript is the literal `true` and
/// the generated Zod is `z.literal(true)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct LiteralTrue;

impl Serialize for LiteralTrue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bool(true)
    }
}

impl<'de> Deserialize<'de> for LiteralTrue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        if bool::deserialize(deserializer)? {
            Ok(Self)
        } else {
            Err(serde::de::Error::custom("must be `true`"))
        }
    }
}

impl JsonSchema for LiteralTrue {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("LiteralTrue")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        schemars::json_schema!({
            "type": "boolean",
            "const": true,
        })
    }
}

/// Options accepted by `OneHarness.run()` in the published Node SDK.
///
/// Unknown fields are rejected because the SDK cannot forward an option it does
/// not understand. This differs deliberately from output contracts, whose Zod
/// schemas preserve unknown fields for forward compatibility.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(rename = "RunOptions")]
pub struct RunOptions {
    /// The user message sent to the selected harnesses.
    pub prompt: NonEmptyString,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Vec<String>")]
    pub harnesses: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Vec<String>")]
    pub models: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub system: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub reasoning: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub resume: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub session: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub fork: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PermissionMode")]
    pub mode: Option<PermissionMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "u64")]
    pub timeout_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub events: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub history: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub history_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub history_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "BTreeMap<String, String>")]
    pub env: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "BTreeMap<String, String>")]
    pub bins: Option<BTreeMap<String, String>>,
}

/// The session selector accepted by `OneHarness.history()` in the published Node
/// SDK.
///
/// A lookup must select a session, so the contract is a union of the only two
/// ways to do that: ask for the most recent one ([`HistoryLookupByLast`]), or
/// name one ([`HistoryLookupBySession`]). A lookup that selects nothing — `{}`,
/// `{"session": ""}`, or `{"last": false}` — matches neither variant and fails
/// at the boundary, so `history()` never has to re-check for one.
///
/// The variants overlap on purpose, and the order is load-bearing: an untagged
/// enum takes the first match, so `Last` coming first is what gives `last: true`
/// priority over a name. `{"session": "x", "last": true}` satisfies *both*
/// variants and resolves to `Last`, selecting the most recent session rather
/// than `x`; `{"session": "x", "last": false}` fails `Last` and resolves to
/// `Session`. That is exactly the reading of the `if (last) … else if (session)`
/// rule this replaces, now decided by the union rather than re-derived after
/// parsing. Zod resolves its generated union in the same order, so the Node SDK
/// agrees with Rust by construction.
///
/// Because `Last` ignores the name it carries, that name is a plain `String`:
/// `{"session": "", "last": true}` stays valid and still selects the most recent
/// session, as it always has. Only a name that actually selects has to be
/// non-empty.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
#[schemars(rename = "HistoryLookup")]
pub enum HistoryLookup {
    /// Select the most recent session.
    Last(HistoryLookupByLast),
    /// Select the session named by `session`.
    Session(HistoryLookupBySession),
}

/// A [`HistoryLookup`] that selects the most recent session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(rename = "HistoryLookupByLast")]
pub struct HistoryLookupByLast {
    /// Select the most recent session. Only `true` selects, so this variant
    /// accepts no other value.
    pub last: LiteralTrue,
    /// A name may accompany `last: true` — it is what the caller would have
    /// looked up otherwise — but `last` takes priority, so it never selects.
    /// It is therefore unconstrained: an empty name is meaningless here rather
    /// than invalid, so `{"session": "", "last": true}` stays accepted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub session: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub all_projects: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub history_dir: Option<String>,
}

/// A [`HistoryLookup`] that names its session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(rename = "HistoryLookupBySession")]
pub struct HistoryLookupBySession {
    /// The oneharness-derived session name recorded by `run --history`. This is
    /// the name that selects, so it must be non-empty.
    pub session: NonEmptyString,
    /// Whether the most recent session was asked for instead. An ordinary
    /// `bool`, so a caller holding a `boolean` can pass it straight through.
    /// `true` here also satisfies [`HistoryLookup::Last`], which the union tries
    /// first — so a lookup that reaches this variant always meant the name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub last: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub all_projects: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub history_dir: Option<String>,
}

/// Options accepted by `OneHarness.historyList()` in the published Node SDK.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(rename = "HistoryListOptions")]
pub struct HistoryListOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub all_projects: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub history_dir: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(JsonSchema, Serialize)]
    struct OutputFixture {
        value: Option<String>,
    }

    #[test]
    fn output_schema_requires_serialized_option_keys_and_allows_null() {
        let schema = schema_for_serialize::<OutputFixture>();
        let value = schema.as_value();
        assert_eq!(value["required"], serde_json::json!(["value"]));
        assert_eq!(
            value["properties"]["value"]["type"],
            serde_json::json!(["string", "null"])
        );
    }

    fn non_empty(value: &str) -> NonEmptyString {
        NonEmptyString::try_from(value).expect("fixture is non-empty")
    }

    #[test]
    fn non_empty_string_rejects_an_empty_value_at_every_entry_point() {
        assert_eq!(
            NonEmptyString::try_from(String::new()),
            Err(EmptyStringError)
        );
        assert_eq!(NonEmptyString::try_from(""), Err(EmptyStringError));

        let error = serde_json::from_value::<NonEmptyString>(serde_json::json!(""))
            .expect_err("empty string should fail");
        assert!(error.to_string().contains("must be a non-empty string"));
    }

    #[test]
    fn non_empty_string_is_transparent_over_its_value() {
        let value = non_empty("node-session");
        assert_eq!(value.as_str(), "node-session");
        assert_eq!(value.to_string(), "node-session");
        assert_eq!(
            serde_json::to_value(&value).expect("serialize"),
            serde_json::json!("node-session")
        );
        assert_eq!(
            serde_json::from_value::<NonEmptyString>(serde_json::json!("node-session"))
                .expect("deserialize"),
            value
        );
        assert_eq!(value.into_string(), "node-session");
    }

    #[test]
    fn non_empty_string_schema_is_an_inline_string_with_a_minimum_length() {
        let schema = schemars::schema_for!(NonEmptyString);
        let value = schema.as_value();
        assert_eq!(value["type"], "string");
        assert_eq!(value["minLength"], 1);
    }

    #[test]
    fn literal_true_rejects_false_and_schemas_as_an_inline_const() {
        let error = serde_json::from_value::<LiteralTrue>(serde_json::json!(false))
            .expect_err("false is not `true`");
        assert!(error.to_string().contains("must be `true`"));

        assert_eq!(
            serde_json::from_value::<LiteralTrue>(serde_json::json!(true)).expect("true is valid"),
            LiteralTrue
        );
        assert_eq!(
            serde_json::to_value(LiteralTrue).expect("serialize"),
            serde_json::json!(true)
        );

        let schema = schemars::schema_for!(LiteralTrue);
        let value = schema.as_value();
        assert_eq!(value["type"], "boolean");
        assert_eq!(value["const"], true);
    }

    #[test]
    fn run_options_reject_an_empty_prompt() {
        let error = serde_json::from_value::<RunOptions>(serde_json::json!({ "prompt": "" }))
            .expect_err("empty prompt should fail");
        assert!(error.to_string().contains("must be a non-empty string"));
    }

    #[test]
    fn run_options_schema_requires_a_non_empty_prompt() {
        let schema = schemars::schema_for!(RunOptions);
        let value = schema.as_value();
        assert_eq!(value["required"], serde_json::json!(["prompt"]));
        assert_eq!(value["properties"]["prompt"]["type"], "string");
        assert_eq!(value["properties"]["prompt"]["minLength"], 1);
    }

    #[test]
    fn optional_fields_round_trip_and_are_omitted_when_absent() {
        let options = RunOptions {
            prompt: non_empty("inspect the repository"),
            harnesses: Some(vec!["codex".to_string()]),
            models: None,
            system: None,
            reasoning: None,
            resume: None,
            session: None,
            fork: None,
            mode: Some(PermissionMode::ReadOnly),
            cwd: None,
            timeout_seconds: Some(30),
            events: Some(true),
            history: None,
            history_name: None,
            history_dir: None,
            env: None,
            bins: None,
        };

        let value = serde_json::to_value(&options).expect("serialize SDK options");
        assert_eq!(value["timeoutSeconds"], 30);
        assert_eq!(value["mode"], "read-only");
        assert!(value.get("models").is_none());
        assert!(value.get("historyDir").is_none());
        assert_eq!(
            serde_json::from_value::<RunOptions>(value).expect("deserialize SDK options"),
            options
        );
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let error = serde_json::from_value::<RunOptions>(serde_json::json!({
            "prompt": "hello",
            "harneses": ["codex"]
        }))
        .expect_err("misspelled option should fail");
        assert!(error.to_string().contains("unknown field `harneses`"));
    }

    #[test]
    fn history_lookup_optional_fields_round_trip_and_are_omitted_when_absent() {
        let lookup = HistoryLookup::Session(HistoryLookupBySession {
            session: non_empty("node-session"),
            last: None,
            project: None,
            all_projects: Some(true),
            history_dir: Some("/tmp/oneharness-history".to_string()),
        });

        let value = serde_json::to_value(&lookup).expect("serialize history lookup");
        assert_eq!(value["session"], "node-session");
        assert_eq!(value["allProjects"], true);
        assert_eq!(value["historyDir"], "/tmp/oneharness-history");
        assert!(value.get("last").is_none());
        assert!(value.get("project").is_none());
        assert_eq!(
            serde_json::from_value::<HistoryLookup>(value).expect("deserialize history lookup"),
            lookup
        );
    }

    #[test]
    fn history_lookup_by_last_round_trips_without_a_session() {
        let lookup = HistoryLookup::Last(HistoryLookupByLast {
            last: LiteralTrue,
            session: None,
            project: Some("oneharness".to_string()),
            all_projects: None,
            history_dir: None,
        });

        let value = serde_json::to_value(&lookup).expect("serialize history lookup");
        assert_eq!(value["last"], true);
        assert_eq!(value["project"], "oneharness");
        assert!(value.get("session").is_none());
        assert_eq!(
            serde_json::from_value::<HistoryLookup>(value).expect("deserialize history lookup"),
            lookup
        );
    }

    #[test]
    fn history_lookup_requires_a_selector() {
        for selectorless in [
            serde_json::json!({}),
            serde_json::json!({ "session": "" }),
            serde_json::json!({ "last": false }),
            serde_json::json!({ "session": "", "last": false }),
            serde_json::json!({ "project": "oneharness" }),
        ] {
            serde_json::from_value::<HistoryLookup>(selectorless.clone())
                .expect_err(&format!("{selectorless} selects no session"));
        }
    }

    #[test]
    fn history_lookup_accepts_every_way_to_select_a_session() {
        assert_eq!(
            serde_json::from_value::<HistoryLookup>(serde_json::json!({ "session": "x" }))
                .expect("a named session selects"),
            HistoryLookup::Session(HistoryLookupBySession {
                session: non_empty("x"),
                last: None,
                project: None,
                all_projects: None,
                history_dir: None,
            })
        );
        assert_eq!(
            serde_json::from_value::<HistoryLookup>(serde_json::json!({ "last": true }))
                .expect("the last session selects"),
            HistoryLookup::Last(HistoryLookupByLast {
                last: LiteralTrue,
                session: None,
                project: None,
                all_projects: None,
                history_dir: None,
            })
        );
    }

    #[test]
    fn history_lookup_gives_last_priority_over_a_name() {
        // `{session, last: true}` satisfies both variants. `Last` is declared
        // first, so it wins and the name rides along unselected — the priority
        // the SDK has always given `last`.
        assert_eq!(
            serde_json::from_value::<HistoryLookup>(
                serde_json::json!({ "session": "x", "last": true })
            )
            .expect("last selects even when a name is present"),
            HistoryLookup::Last(HistoryLookupByLast {
                last: LiteralTrue,
                session: Some("x".to_string()),
                project: None,
                all_projects: None,
                history_dir: None,
            })
        );
        // Dropping `last` to `false` fails the `Last` variant, so the same
        // caller asks for the named session instead.
        assert_eq!(
            serde_json::from_value::<HistoryLookup>(
                serde_json::json!({ "session": "x", "last": false })
            )
            .expect("an explicit `last: false` selects the name"),
            HistoryLookup::Session(HistoryLookupBySession {
                session: non_empty("x"),
                last: Some(false),
                project: None,
                all_projects: None,
                history_dir: None,
            })
        );
    }

    #[test]
    fn history_lookup_ignores_an_empty_name_that_does_not_select() {
        // `last: true` selects, so the name beside it is inert — an empty one is
        // meaningless rather than invalid, and stays accepted.
        assert_eq!(
            serde_json::from_value::<HistoryLookup>(
                serde_json::json!({ "session": "", "last": true })
            )
            .expect("an unselected name is unconstrained"),
            HistoryLookup::Last(HistoryLookupByLast {
                last: LiteralTrue,
                session: Some(String::new()),
                project: None,
                all_projects: None,
                history_dir: None,
            })
        );
    }

    #[test]
    fn history_lookup_rejects_unknown_fields() {
        let error = serde_json::from_value::<HistoryLookup>(serde_json::json!({
            "session": "node-session",
            "sesion": "node-session"
        }))
        .expect_err("misspelled selector should fail");
        assert!(
            error
                .to_string()
                .contains("did not match any variant of untagged enum HistoryLookup"),
            "unexpected error: {error}"
        );
    }

    /// Resolve one `anyOf` member of a schema document, following the local
    /// `$ref` schemars emits for a named variant.
    fn union_variant(document: &Value, index: usize) -> Value {
        let variant = document["anyOf"]
            .as_array()
            .expect("a union of variants")
            .get(index)
            .unwrap_or_else(|| panic!("variant {index} is missing"))
            .clone();
        let Some(reference) = variant.get("$ref").and_then(Value::as_str) else {
            return variant;
        };
        let name = reference
            .strip_prefix("#/$defs/")
            .unwrap_or_else(|| panic!("unexpected non-local reference {reference}"));
        document["$defs"][name].clone()
    }

    #[test]
    fn history_lookup_schema_is_a_union_that_requires_a_selector() {
        let schema = schemars::schema_for!(HistoryLookup);
        let document = schema.as_value();
        assert_eq!(
            document["anyOf"].as_array().map(Vec::len),
            Some(2),
            "a lookup selects a session in exactly two ways"
        );

        // `Last` is first, so a consumer matching the union in order gives
        // `last: true` the priority it has always had over a name. The name it
        // carries never selects, so it stays unconstrained.
        let by_last = union_variant(document, 0);
        assert_eq!(by_last["additionalProperties"], serde_json::json!(false));
        assert_eq!(by_last["required"], serde_json::json!(["last"]));
        assert_eq!(by_last["properties"]["last"]["const"], true);
        assert_eq!(by_last["properties"]["session"]["type"], "string");
        assert!(by_last["properties"]["session"].get("minLength").is_none());

        // Only the name that selects is constrained, and `last` stays an
        // ordinary boolean so a caller holding one can pass it through.
        let by_session = union_variant(document, 1);
        assert_eq!(by_session["additionalProperties"], serde_json::json!(false));
        assert_eq!(by_session["required"], serde_json::json!(["session"]));
        assert_eq!(by_session["properties"]["session"]["type"], "string");
        assert_eq!(by_session["properties"]["session"]["minLength"], 1);
        assert_eq!(by_session["properties"]["last"]["type"], "boolean");

        // Both ways to select carry the same non-selecting fields.
        for variant in [&by_session, &by_last] {
            for field in ["project", "allProjects", "historyDir"] {
                assert!(
                    variant["properties"].get(field).is_some(),
                    "{field} should be accepted by every variant"
                );
            }
        }
    }

    #[test]
    fn history_list_optional_fields_round_trip_and_are_omitted_when_absent() {
        let options = HistoryListOptions {
            project: None,
            all_projects: Some(true),
            history_dir: Some("/tmp/oneharness-history".to_string()),
        };

        let value = serde_json::to_value(&options).expect("serialize history list options");
        assert_eq!(value["allProjects"], true);
        assert_eq!(value["historyDir"], "/tmp/oneharness-history");
        assert!(value.get("project").is_none());
        assert_eq!(
            serde_json::from_value::<HistoryListOptions>(value)
                .expect("deserialize history list options"),
            options
        );
    }

    #[test]
    fn history_list_options_default_to_every_field_absent() {
        assert_eq!(
            serde_json::from_value::<HistoryListOptions>(serde_json::json!({}))
                .expect("empty options are valid"),
            HistoryListOptions {
                project: None,
                all_projects: None,
                history_dir: None,
            }
        );
    }

    #[test]
    fn history_list_options_reject_unknown_fields() {
        let error = serde_json::from_value::<HistoryListOptions>(serde_json::json!({
            "allProject": true
        }))
        .expect_err("misspelled option should fail");
        assert!(error.to_string().contains("unknown field `allProject`"));
    }
}
