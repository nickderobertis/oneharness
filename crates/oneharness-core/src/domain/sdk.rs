//! Rust-owned schema metadata for the Node SDK's public input contract.
//!
//! The SDK generator reflects this type into both TypeScript and Zod. Keeping
//! the option names here means adding an SDK option cannot leave its static and
//! runtime contracts out of sync. This is metadata only: command orchestration
//! stays in the Node SDK and the CLI command layer.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::domain::mode::PermissionMode;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
