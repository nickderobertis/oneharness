use oneharness::commands::{detect::DetectReport, list::ListReport};
use oneharness_core::domain::{
    history::HistoryRecord,
    report::RunReport,
    sdk::{schema_for_serialize, HistoryListOptions, RunOptions},
};
use oneharness_core::io::history::SessionSummary;
use schemars::{schema_for, JsonSchema};
use serde::Serialize;

#[derive(JsonSchema, Serialize)]
struct SchemaBundle {
    run_report: schemars::Schema,
    run_options: schemars::Schema,
    history_list_options: schemars::Schema,
    history_record: schemars::Schema,
    history_records: schemars::Schema,
    history_list: schemars::Schema,
    list_report: schemars::Schema,
    detect_report: schemars::Schema,
}

fn main() -> Result<(), serde_json::Error> {
    let bundle = SchemaBundle {
        run_report: schema_for_serialize::<RunReport>(),
        run_options: schema_for!(RunOptions),
        history_list_options: schema_for!(HistoryListOptions),
        history_record: schema_for_serialize::<HistoryRecord>(),
        history_records: schema_for_serialize::<Vec<HistoryRecord>>(),
        history_list: schema_for_serialize::<Vec<SessionSummary>>(),
        list_report: schema_for_serialize::<ListReport>(),
        detect_report: schema_for_serialize::<DetectReport>(),
    };
    println!("{}", serde_json::to_string_pretty(&bundle)?);
    Ok(())
}
