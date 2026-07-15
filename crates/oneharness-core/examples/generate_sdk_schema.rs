use oneharness_core::domain::{history::HistoryRecord, report::RunReport};
use schemars::{schema_for, JsonSchema};
use serde::Serialize;

#[derive(JsonSchema, Serialize)]
struct SchemaBundle {
    run_report: schemars::Schema,
    history_record: schemars::Schema,
}

fn main() {
    let bundle = SchemaBundle {
        run_report: schema_for!(RunReport),
        history_record: schema_for!(HistoryRecord),
    };
    println!("{}", serde_json::to_string_pretty(&bundle).expect("schema serializes"));
}
