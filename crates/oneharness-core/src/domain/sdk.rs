//! Rust-owned schema metadata for the Node SDK's public input contract.
//!
//! The SDK generator reflects this type into both TypeScript and Zod. Keeping
//! the option names here means adding an SDK option cannot leave its static and
//! runtime contracts out of sync. This is metadata only: command orchestration
//! stays in the Node SDK and the CLI command layer.

use std::collections::BTreeMap;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

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
    #[schemars(length(min = 1))]
    pub prompt: String,
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

    #[test]
    fn optional_fields_round_trip_and_are_omitted_when_absent() {
        let options = RunOptions {
            prompt: "inspect the repository".to_string(),
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
