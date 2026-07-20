//! The Rust-owned schema bundle consumed by language SDK generators.
//!
//! Keeping every shared contract root here gives all SDKs one generation
//! source. Language packages may add client behavior, but must not restate
//! these wire shapes by hand.

use schemars::{schema_for, Schema};
use serde::Serialize;

use crate::domain::history::{HistoryRecord, HistoryStreamEnvelope};
use crate::domain::report::{RunReport, RunStreamEnvelope};
use crate::domain::sdk::{
    schema_for_serialize, HistoryListOptions, HistoryLookup, HistoryWatchOptions, RunOptions,
};
use crate::io::history::SessionSummary;

/// All core schemas shared by oneharness SDKs.
#[derive(Debug, Serialize)]
pub struct SdkSchemaBundle {
    pub run_report: Schema,
    pub run_stream_envelope: Schema,
    pub run_options: Schema,
    pub history_lookup: Schema,
    pub history_list_options: Schema,
    pub history_watch_options: Schema,
    pub history_record: Schema,
    pub history_stream_envelope: Schema,
    pub history_records: Schema,
    pub history_list: Schema,
}

/// Generate the shared SDK schema roots from their Rust contract types.
pub fn bundle() -> SdkSchemaBundle {
    SdkSchemaBundle {
        run_report: schema_for_serialize::<RunReport>(),
        run_stream_envelope: schema_for_serialize::<RunStreamEnvelope>(),
        run_options: schema_for!(RunOptions),
        history_lookup: schema_for!(HistoryLookup),
        history_list_options: schema_for!(HistoryListOptions),
        history_watch_options: schema_for!(HistoryWatchOptions),
        history_record: history_schema(schema_for_serialize::<HistoryRecord>()),
        history_stream_envelope: history_schema(schema_for_serialize::<HistoryStreamEnvelope>()),
        history_records: history_schema(schema_for_serialize::<Vec<HistoryRecord>>()),
        history_list: schema_for_serialize::<Vec<SessionSummary>>(),
    }
}

fn history_schema(schema: Schema) -> Schema {
    let mut value = serde_json::to_value(schema).expect("Schema serializes");
    add_v03_condition(&mut value);
    Schema::try_from(value).expect("conditional history schema remains an object")
}

fn add_v03_condition(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Array(values) => values.iter_mut().for_each(add_v03_condition),
        serde_json::Value::Object(object) => {
            let is_history = object
                .get("properties")
                .and_then(serde_json::Value::as_object)
                .is_some_and(|properties| {
                    properties.contains_key("history_id")
                        && properties.contains_key("schema_version")
                        && properties.contains_key("harness")
                });
            if is_history {
                let metadata = ["$schema", "$defs", "title", "description"]
                    .into_iter()
                    .filter_map(|key| object.remove(key).map(|value| (key.to_string(), value)))
                    .collect::<serde_json::Map<_, _>>();
                let base = serde_json::Value::Object(object.clone());
                let mut current = base.clone();
                current["properties"]["schema_version"] =
                    serde_json::json!({"const": "0.3", "type": "string"});
                let required = current["required"]
                    .as_array_mut()
                    .expect("history required array");
                for field in [
                    "started_at",
                    "finished_at",
                    "duration_ms",
                    "model_ms",
                    "tool_ms",
                ] {
                    if !required.iter().any(|value| value.as_str() == Some(field)) {
                        required.push(serde_json::Value::String(field.to_string()));
                    }
                }
                current["properties"]["started_at"] =
                    serde_json::json!({"type": "string", "minLength": 1});
                for field in ["duration_ms", "model_ms", "tool_ms"] {
                    current["properties"][field] =
                        serde_json::json!({"type": "integer", "minimum": 0});
                }
                let event_base = current["properties"]["events"]["items"].clone();
                let terminal_event = |status: &str, ended: bool| {
                    let mut properties = serde_json::json!({
                        "kind": {"const": "tool_call", "type": "string"},
                        "tool_call_id": {"type": "string", "minLength": 1},
                        "started_at": {"type": "string"},
                        "status": {"const": status, "type": "string"}
                    });
                    if ended {
                        properties["finished_at"] = serde_json::json!({"type": "string"});
                        properties["duration_ms"] =
                            serde_json::json!({"type": "integer", "minimum": 0});
                    }
                    let mut required =
                        serde_json::json!(["kind", "tool_call_id", "started_at", "status"]);
                    if ended {
                        required
                            .as_array_mut()
                            .expect("required fields are an array")
                            .extend([
                                serde_json::Value::String("finished_at".to_string()),
                                serde_json::Value::String("duration_ms".to_string()),
                            ]);
                    }
                    serde_json::json!({
                        "allOf": [event_base.clone(), {
                            "type": "object",
                            "properties": properties,
                            "required": required
                        }]
                    })
                };
                current["properties"]["events"]["items"] = serde_json::json!({
                    "oneOf": [
                        terminal_event("completed", true),
                        terminal_event("failed", true),
                        terminal_event("timeout", false),
                        terminal_event("interrupted", false),
                        {"allOf": [event_base, {
                            "type": "object",
                            "properties": {"kind": {"const": "tool_result", "type": "string"}}
                        }]}
                    ]
                });
                let mut legacy = base;
                legacy["properties"]["schema_version"] =
                    serde_json::json!({"enum": ["0.1", "0.2"], "type": "string"});
                if let Some(required) = legacy["required"].as_array_mut() {
                    required.retain(|value| {
                        !value.as_str().is_some_and(|field| {
                            matches!(
                                field,
                                "started_at"
                                    | "finished_at"
                                    | "model_ms"
                                    | "tool_ms"
                                    | "time_to_first_token_ms"
                            )
                        })
                    });
                }
                legacy["properties"]["events"] = serde_json::json!({
                    "type": ["array", "null"],
                    "items": {
                        "type": "object",
                        "properties": {
                            "kind": {"type": "string"},
                            "name": {"type": ["string", "null"]},
                            "input": true,
                            "output": {"type": ["string", "null"]},
                            "index": {"type": "integer", "minimum": 0}
                        },
                        "required": ["kind", "name", "input", "output", "index"]
                    }
                });
                object.clear();
                object.extend(metadata);
                object.insert("oneOf".to_string(), serde_json::json!([current, legacy]));
                return;
            }
            object.values_mut().for_each(add_v03_condition);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn current_record(event: serde_json::Value) -> serde_json::Value {
        json!({
            "schema_version": "0.3",
            "history_id": "0198f0d0-7b31-7000-8000-000000000001",
            "session": "session", "name": "name", "labels": {}, "project": "/tmp/project",
            "timestamp": "2026-07-19T00:00:00Z", "harness": "codex", "model": null,
            "prompt": "test", "permission_mode": "default", "status": "ok", "exit_code": 0,
            "duration_ms": 10, "started_at": "2026-07-19T00:00:00Z",
            "finished_at": "2026-07-19T00:00:00Z", "model_ms": 4, "tool_ms": 3,
            "time_to_first_token_ms": null, "text": "done", "text_source": "json",
            "usage": {"input_tokens": null, "output_tokens": null, "cache_read_tokens": null,
                      "cache_write_tokens": null, "cost_usd": null},
            "session_id": null, "events": [event], "failure_kind": null
        })
    }

    #[test]
    fn current_schema_rejects_incomplete_and_unfinished_terminal_tool_calls() {
        let schema = serde_json::to_value(bundle().history_record).unwrap();
        let validator = jsonschema::validator_for(&schema).unwrap();
        let base = json!({
            "kind": "tool_call", "name": "shell", "input": {}, "output": "ok", "index": 0,
            "tool_call_id": "call-1", "started_at": "2026-07-19T00:00:00Z",
            "finished_at": "2026-07-19T00:00:00Z", "duration_ms": 3, "status": "completed"
        });
        assert!(validator.is_valid(&current_record(base.clone())));
        for field in ["tool_call_id", "started_at", "status"] {
            let mut invalid = base.clone();
            invalid[field] = serde_json::Value::Null;
            assert!(!validator.is_valid(&current_record(invalid)), "{field}");
        }
        for field in ["finished_at", "duration_ms"] {
            let mut invalid = base.clone();
            invalid[field] = serde_json::Value::Null;
            assert!(!validator.is_valid(&current_record(invalid)), "{field}");
            let mut omitted = base.clone();
            omitted.as_object_mut().unwrap().remove(field);
            assert!(!validator.is_valid(&current_record(omitted)), "{field}");
        }
    }

    #[test]
    fn current_schema_rejects_null_required_timing_fields() {
        let schema = serde_json::to_value(bundle().history_record).unwrap();
        let validator = jsonschema::validator_for(&schema).unwrap();
        let event = json!({
            "kind": "tool_result", "name": null, "input": null, "output": "ok", "index": 0,
            "tool_call_id": null, "started_at": null, "finished_at": null,
            "duration_ms": null, "status": null
        });
        let valid = current_record(event);
        assert!(validator.is_valid(&valid));
        for field in ["started_at", "duration_ms", "model_ms", "tool_ms"] {
            let mut invalid = valid.clone();
            invalid[field] = serde_json::Value::Null;
            assert!(!validator.is_valid(&invalid), "{field}");
        }
    }
}
