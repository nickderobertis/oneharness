//! The Rust-owned schema bundle consumed by language SDK generators.
//!
//! Keeping every shared contract root here gives all SDKs one generation
//! source. Language packages may add client behavior, but must not restate
//! these wire shapes by hand.

use schemars::{schema_for, Schema};
use serde::Serialize;

use crate::domain::history::{
    HistoryLine, HistoryRecord, HistoryStreamEnvelope, FIRST_EVENT_SCHEMA_VERSION,
    PREVIOUS_CURRENT_SCHEMA_VERSION, SCHEMA_VERSION as HISTORY_SCHEMA_VERSION,
};
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
    pub history_line: Schema,
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
        history_line: history_line_schema(schema_for_serialize::<HistoryLine>()),
        history_record: history_schema(schema_for_serialize::<HistoryRecord>()),
        history_stream_envelope: history_stream_schema(),
        history_records: history_schema(schema_for_serialize::<Vec<HistoryRecord>>()),
        history_list: schema_for_serialize::<Vec<SessionSummary>>(),
    }
}

fn history_stream_schema() -> Schema {
    // llmlint: ignore[no_panics_on_recoverable_errors] SDK schema generation is a build-time codegen boundary, and every sibling schema transformation in this module treats an impossible schemars-to-JSON failure as a generator invariant; returning a partial bundle would be less actionable than failing generation here.
    let mut value = serde_json::to_value(schema_for_serialize::<HistoryStreamEnvelope>())
        .expect("Schema serializes");
    // llmlint: ignore[no_panics_on_recoverable_errors] This is the same build-time schemars invariant as the envelope serialization above; callers cannot recover from a malformed generated contract.
    let history_line =
        serde_json::to_value(schema_for_serialize::<HistoryLine>()).expect("Schema serializes");
    let definitions = history_line["$defs"]
        .as_object()
        .expect("history line definitions");
    for name in ["ActionEvent", "HistoryEventLine"] {
        value["$defs"][name] = definitions[name].clone();
    }
    add_history_line_conditions(&mut value);
    add_v03_condition(&mut value);
    Schema::try_from(value).expect("history stream schema remains an object")
}

fn history_line_schema(schema: Schema) -> Schema {
    let mut value = serde_json::to_value(schema).expect("Schema serializes");
    let definitions = value["$defs"].clone();
    if let Some(variants) = value
        .get_mut("oneOf")
        .and_then(serde_json::Value::as_array_mut)
    {
        for variant in variants.iter_mut() {
            let Some(reference) = variant.get("$ref").and_then(serde_json::Value::as_str) else {
                continue;
            };
            let name = reference
                .rsplit('/')
                .next()
                .expect("schema reference has a name");
            let mut expanded = definitions[name].clone();
            let discriminator = variant["properties"].clone();
            expanded["properties"]
                .as_object_mut()
                .expect("object properties")
                .extend(
                    discriminator
                        .as_object()
                        .expect("discriminator properties")
                        .clone(),
                );
            expanded["required"]
                .as_array_mut()
                .expect("required array")
                .extend(
                    variant["required"]
                        .as_array()
                        .expect("required array")
                        .clone(),
                );
            *variant = expanded;
        }
    }
    value["$defs"]
        .as_object_mut()
        .expect("schema definitions")
        .remove("HistoryEventLine");
    value["$defs"]
        .as_object_mut()
        .expect("schema definitions")
        .remove("HistoryRunRecord");
    add_history_line_conditions(&mut value);
    if let Some(variants) = value
        .get_mut("oneOf")
        .and_then(serde_json::Value::as_array_mut)
    {
        for variant in variants {
            *variant = serde_json::json!({"allOf": [variant.clone()]});
        }
    }
    Schema::try_from(value).expect("conditional history line schema remains an object")
}

fn add_history_line_conditions(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Array(values) => values.iter_mut().for_each(add_history_line_conditions),
        serde_json::Value::Object(object) => {
            let Some(properties) = object
                .get("properties")
                .and_then(serde_json::Value::as_object)
            else {
                object.values_mut().for_each(add_history_line_conditions);
                return;
            };
            if properties.contains_key("run_id") && properties.contains_key("event") {
                let mut current = serde_json::Value::Object(object.clone());
                current["properties"]["schema_version"] = serde_json::json!({
                    "const": HISTORY_SCHEMA_VERSION,
                    "type": "string"
                });
                let mut previous = serde_json::Value::Object(object.clone());
                previous["properties"]["schema_version"] = serde_json::json!({
                    "enum": [
                        FIRST_EVENT_SCHEMA_VERSION,
                        PREVIOUS_CURRENT_SCHEMA_VERSION
                    ],
                    "type": "string"
                });
                forbid_action_event_timing_source(&mut previous["properties"]["event"]);
                object.clear();
                object.insert(
                    "oneOf".to_string(),
                    serde_json::json!([
                        {"allOf": [current]},
                        {"allOf": [previous]}
                    ]),
                );
                return;
            }
            if properties.contains_key("history_id")
                && properties.contains_key("schema_version")
                && properties.contains_key("harness")
                && !properties.contains_key("events")
            {
                let base = serde_json::Value::Object(object.clone());
                let mut measured = base.clone();
                measured["properties"]["schema_version"] = current_history_versions_schema();
                for field in ["started_at", "duration_ms", "model_ms", "tool_ms"] {
                    let required = measured["required"].as_array_mut().expect("required array");
                    if !required.iter().any(|value| value.as_str() == Some(field)) {
                        required.push(serde_json::Value::String(field.to_string()));
                    }
                }
                measured["properties"]["started_at"] =
                    serde_json::json!({"type": "string", "minLength": 1});
                for field in ["duration_ms", "model_ms", "tool_ms"] {
                    measured["properties"][field] =
                        serde_json::json!({"type": "integer", "minimum": 0});
                }
                measured["properties"]["observed_tool_ms"] = serde_json::Value::Bool(false);
                let mut terminal = measured.clone();
                terminal["properties"]["status"] =
                    serde_json::json!({"enum": ["ok", "nonzero"], "type": "string"});
                terminal["properties"]["finished_at"] = serde_json::json!({"type": "string"});
                measured["properties"]["status"] = serde_json::json!({
                    "enum": ["timeout", "spawn-error", "skipped", "planned"],
                    "type": "string"
                });
                let mut unavailable = base;
                unavailable["properties"]["schema_version"] = current_history_versions_schema();
                unavailable["properties"]["finished_at"] = serde_json::json!({"type": "null"});
                for field in [
                    "started_at",
                    "model_ms",
                    "tool_ms",
                    "time_to_first_token_ms",
                    "observed_tool_ms",
                ] {
                    unavailable["properties"][field] = serde_json::Value::Bool(false);
                    unavailable["required"]
                        .as_array_mut()
                        .expect("required array")
                        .retain(|value| value.as_str() != Some(field));
                }
                let mut observed = unavailable.clone();
                observed["properties"]["schema_version"] = serde_json::json!({
                    "const": HISTORY_SCHEMA_VERSION,
                    "type": "string"
                });
                observed["properties"]["observed_tool_ms"] =
                    serde_json::json!({"type": "integer", "minimum": 0});
                observed["properties"]["duration_ms"] =
                    serde_json::json!({"type": "integer", "minimum": 0});
                for field in ["duration_ms", "observed_tool_ms"] {
                    let required = observed["required"].as_array_mut().expect("required array");
                    if !required.iter().any(|value| value.as_str() == Some(field)) {
                        required.push(serde_json::Value::String(field.to_string()));
                    }
                }
                object.clear();
                object.insert(
                    "oneOf".to_string(),
                    serde_json::json!([
                        {"allOf": [terminal]},
                        {"allOf": [measured]},
                        {"allOf": [observed]},
                        {"allOf": [unavailable]}
                    ]),
                );
                return;
            }
            object.values_mut().for_each(add_history_line_conditions);
        }
        _ => {}
    }
}

fn history_schema(schema: Schema) -> Schema {
    let mut value = serde_json::to_value(schema).expect("Schema serializes");
    add_v03_condition(&mut value);
    Schema::try_from(value).expect("conditional history schema remains an object")
}

fn current_history_versions_schema() -> serde_json::Value {
    serde_json::json!({
        "enum": [
            FIRST_EVENT_SCHEMA_VERSION,
            PREVIOUS_CURRENT_SCHEMA_VERSION,
            HISTORY_SCHEMA_VERSION
        ],
        "type": "string"
    })
}

fn set_history_identity_version(
    schema: &mut serde_json::Value,
    version: &str,
    identity_required: bool,
) {
    schema["properties"]["schema_version"] =
        serde_json::json!({"const": version, "type": "string"});
    let required = schema["required"]
        .as_array_mut()
        .expect("history required array");
    required.retain(|value| value.as_str() != Some("harness_id"));
    if identity_required {
        required.push(serde_json::Value::String("harness_id".to_string()));
    }
}

fn forbid_action_event_timing_source(schema: &mut serde_json::Value) {
    let event = schema.take();
    *schema = serde_json::json!({
        "allOf": [
            event,
            {
                "type": "object",
                "properties": {"timing_source": false}
            }
        ]
    });
}

fn forbid_record_event_timing_source(schema: &mut serde_json::Value) {
    forbid_action_event_timing_source(&mut schema["properties"]["events"]["items"]);
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
                set_history_identity_version(&mut current, HISTORY_SCHEMA_VERSION, true);
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
                current["properties"]["observed_tool_ms"] = serde_json::Value::Bool(false);
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
                let mut previous_current = current.clone();
                set_history_identity_version(
                    &mut previous_current,
                    PREVIOUS_CURRENT_SCHEMA_VERSION,
                    false,
                );
                forbid_record_event_timing_source(&mut previous_current);
                let mut first_current = current.clone();
                set_history_identity_version(&mut first_current, FIRST_EVENT_SCHEMA_VERSION, false);
                forbid_record_event_timing_source(&mut first_current);
                let mut unavailable = base.clone();
                set_history_identity_version(&mut unavailable, HISTORY_SCHEMA_VERSION, true);
                unavailable["properties"]["finished_at"] = serde_json::json!({"type": "null"});
                if let Some(required) = unavailable["required"].as_array_mut() {
                    required.retain(|value| {
                        !value.as_str().is_some_and(|field| {
                            matches!(
                                field,
                                "started_at" | "model_ms" | "tool_ms" | "time_to_first_token_ms"
                            )
                        })
                    });
                }
                for field in [
                    "started_at",
                    "model_ms",
                    "tool_ms",
                    "time_to_first_token_ms",
                    "observed_tool_ms",
                ] {
                    unavailable["properties"][field] = serde_json::Value::Bool(false);
                }
                unavailable["properties"]["events"]["items"] = serde_json::json!({
                    "type": "object",
                    "properties": {
                        "kind": {"type": "string"},
                        "name": {"type": ["string", "null"]},
                        "input": true,
                        "output": {"type": ["string", "null"]},
                        "index": {"type": "integer", "minimum": 0},
                        "tool_call_id": {"type": ["string", "null"]},
                        "started_at": {"type": "null"},
                        "finished_at": {"type": "null"},
                        "duration_ms": {"type": "null"},
                        "status": {"type": "null"}
                    },
                    "required": ["kind", "name", "input", "output", "index"]
                });
                let mut previous_unavailable = unavailable.clone();
                set_history_identity_version(
                    &mut previous_unavailable,
                    PREVIOUS_CURRENT_SCHEMA_VERSION,
                    false,
                );
                forbid_record_event_timing_source(&mut previous_unavailable);
                let mut first_unavailable = unavailable.clone();
                set_history_identity_version(
                    &mut first_unavailable,
                    FIRST_EVENT_SCHEMA_VERSION,
                    false,
                );
                forbid_record_event_timing_source(&mut first_unavailable);
                let mut observed = base.clone();
                set_history_identity_version(&mut observed, HISTORY_SCHEMA_VERSION, true);
                observed["properties"]["finished_at"] = serde_json::json!({"type": "null"});
                for field in [
                    "started_at",
                    "model_ms",
                    "tool_ms",
                    "time_to_first_token_ms",
                ] {
                    observed["properties"][field] = serde_json::Value::Bool(false);
                    observed["required"]
                        .as_array_mut()
                        .expect("required array")
                        .retain(|value| value.as_str() != Some(field));
                }
                for field in ["duration_ms", "observed_tool_ms"] {
                    let required = observed["required"].as_array_mut().expect("required array");
                    if !required.iter().any(|value| value.as_str() == Some(field)) {
                        required.push(serde_json::Value::String(field.to_string()));
                    }
                }
                observed["properties"]["duration_ms"] =
                    serde_json::json!({"type": "integer", "minimum": 0});
                observed["properties"]["observed_tool_ms"] =
                    serde_json::json!({"type": "integer", "minimum": 0});
                let mut legacy = base;
                legacy["properties"]["schema_version"] =
                    serde_json::json!({"enum": ["0.1", "0.2"], "type": "string"});
                forbid_record_event_timing_source(&mut legacy);
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
                object.insert(
                    "oneOf".to_string(),
                    serde_json::json!([
                        current,
                        previous_current,
                        first_current,
                        unavailable,
                        observed,
                        previous_unavailable,
                        first_unavailable,
                        legacy
                    ]),
                );
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
            "schema_version": "1.0",
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

    #[test]
    fn current_schema_accepts_unavailable_timing_but_rejects_partial_timing() {
        let schema = serde_json::to_value(bundle().history_record).unwrap();
        let validator = jsonschema::validator_for(&schema).unwrap();
        let event = json!({
            "kind": "tool_result", "name": null, "input": null, "output": "ok", "index": 0,
            "tool_call_id": null, "started_at": null, "finished_at": null,
            "duration_ms": null, "status": null
        });
        let mut unavailable = current_record(event);
        unavailable["finished_at"] = serde_json::Value::Null;
        for field in [
            "started_at",
            "model_ms",
            "tool_ms",
            "time_to_first_token_ms",
        ] {
            unavailable.as_object_mut().unwrap().remove(field);
        }
        assert!(validator.is_valid(&unavailable));

        let mut omitted_event_timing = unavailable.clone();
        for field in ["started_at", "finished_at", "duration_ms", "status"] {
            omitted_event_timing["events"][0]
                .as_object_mut()
                .unwrap()
                .remove(field);
        }
        assert!(validator.is_valid(&omitted_event_timing));

        unavailable["model_ms"] = json!(1);
        assert!(!validator.is_valid(&unavailable));

        unavailable.as_object_mut().unwrap().remove("model_ms");
        unavailable["events"][0]["started_at"] = json!("2026-07-19T00:00:00Z");
        assert!(!validator.is_valid(&unavailable));
    }

    #[test]
    fn current_schema_distinguishes_stdout_observed_from_provider_timing() {
        let schema = serde_json::to_value(bundle().history_record).unwrap();
        let validator = jsonschema::validator_for(&schema).unwrap();
        let event = json!({
            "kind": "tool_call", "name": "Bash", "input": {}, "output": "ok", "index": 0,
            "tool_call_id": "call-1", "started_at": "2026-07-19T00:00:00Z",
            "finished_at": "2026-07-19T00:00:00Z", "duration_ms": 3,
            "status": "completed", "timing_source": "stdout_observed"
        });
        let mut previous = current_record(event.clone());
        previous["schema_version"] = json!("1.1");
        assert!(!validator.is_valid(&previous));

        let mut observed = current_record(event);
        observed["schema_version"] = json!("1.2");
        observed["harness_id"] = json!("codex");
        observed["finished_at"] = serde_json::Value::Null;
        for field in [
            "started_at",
            "model_ms",
            "tool_ms",
            "time_to_first_token_ms",
        ] {
            observed.as_object_mut().unwrap().remove(field);
        }
        observed["observed_tool_ms"] = json!(3);
        assert!(validator.is_valid(&observed));

        observed["model_ms"] = json!(1);
        assert!(!validator.is_valid(&observed));
        observed.as_object_mut().unwrap().remove("model_ms");
        observed["events"][0]["timing_source"] = json!("estimated");
        assert!(!validator.is_valid(&observed));
    }
}
