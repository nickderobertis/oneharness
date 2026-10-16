//! The schemas the Node SDK validates every verb's output against are the
//! engine's own: `sdk_schema::bundle()` generates each output root from the
//! Rust type with `domain::sdk::schema_for_serialize`, and the Node package
//! ships that bundle verbatim
//! (`npm/oneharness-sdk/src/generated/schemas.json`). Each root generated here
//! must therefore be the one the package carries — the hand-written schemas
//! (a report's `schema_version` const, a timestamp's description, a harness's
//! verbatim words) included, since a validator reading a wrong one would
//! reject every report the CLI emits.

use oneharness_core::domain::sdk::schema_for_serialize;
use serde_json::Value;

#[test]
fn every_report_schema_is_the_one_the_node_sdk_validates_against() {
    let shipped: Value = serde_json::from_str(include_str!(
        "../npm/oneharness-sdk/src/generated/schemas.json"
    ))
    .expect("the Node SDK's schemas are JSON");
    let generated = [
        (
            "usage_report",
            schema_for_serialize::<oneharness_core::domain::usage::UsageReport>(),
        ),
        (
            "run_report",
            schema_for_serialize::<oneharness_core::domain::report::RunReport>(),
        ),
        (
            "list_report",
            schema_for_serialize::<oneharness_core::io::registry::ListReport>(),
        ),
        (
            "detect_report",
            schema_for_serialize::<oneharness_core::io::detect::DetectReport>(),
        ),
        (
            "config_report",
            schema_for_serialize::<oneharness_core::domain::config::ConfigReport>(),
        ),
        (
            "sync_report",
            schema_for_serialize::<oneharness_core::io::sync::SyncReport>(),
        ),
        (
            "interrupt_response",
            schema_for_serialize::<oneharness_core::domain::control::ControlResponse>(),
        ),
        (
            "history_clear_report",
            schema_for_serialize::<oneharness_core::io::history::HistoryClearReport>(),
        ),
        (
            "history_migrate_report",
            schema_for_serialize::<oneharness_core::io::history::HistoryMigrateReport>(),
        ),
        (
            "history_reindex_report",
            schema_for_serialize::<oneharness_core::io::history::HistoryReindexReport>(),
        ),
        (
            "history_pointers",
            schema_for_serialize::<oneharness_core::io::history::HistoryPointers>(),
        ),
    ];
    for (root, schema) in generated {
        let schema = serde_json::to_value(schema).expect("a schema serializes");
        assert!(
            same_json(&schema, &shipped[root]),
            "the `{root}` schema generated from the engine is not the one the Node SDK ships; \
             if the change is intended, regenerate both SDKs (`just sdk-generate`, \
             `just python-sdk-generate`)"
        );
    }
}

/// Whether two JSON documents are equal as JSON, where a number is its value:
/// the Node generator writes a bound the engine spells `0.0` as `0`.
fn same_json(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same_json(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, a)| b.get(key).is_some_and(|b| same_json(a, b)))
        }
        (a, b) => a == b,
    }
}
