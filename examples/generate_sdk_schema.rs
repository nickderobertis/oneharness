use oneharness::commands::{detect::DetectReport, list::ListReport};
use oneharness_core::domain::{history::HistoryRecord, report::RunReport};
use schemars::{schema_for, JsonSchema};
use serde::Serialize;

#[derive(JsonSchema, Serialize)]
struct SchemaBundle {
    run_report: schemars::Schema,
    history_record: schemars::Schema,
    list_report: schemars::Schema,
    detect_report: schemars::Schema,
}

fn main() -> Result<(), serde_json::Error> {
    let bundle = SchemaBundle {
        run_report: schema_for!(RunReport),
        history_record: schema_for!(HistoryRecord),
        list_report: schema_for!(ListReport),
        detect_report: schema_for!(DetectReport),
    };
    println!("{}", serde_json::to_string_pretty(&bundle)?);
    Ok(())
}
