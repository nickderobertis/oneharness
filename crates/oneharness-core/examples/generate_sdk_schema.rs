use oneharness_core::domain::{history::HistoryRecord, report::RunReport, sdk::RunOptions};
use oneharness_core::io::history::SessionSummary;
use schemars::{schema_for, JsonSchema};
use serde::Serialize;

#[derive(JsonSchema, Serialize)]
struct SchemaBundle {
    run_report: schemars::Schema,
    run_options: schemars::Schema,
    history_record: schemars::Schema,
    history_records: schemars::Schema,
    history_list: schemars::Schema,
}

fn main() -> Result<(), serde_json::Error> {
    let bundle = SchemaBundle {
        run_report: schema_for!(RunReport),
        run_options: schema_for!(RunOptions),
        history_record: schema_for!(HistoryRecord),
        history_records: schema_for!(Vec<HistoryRecord>),
        history_list: schema_for!(Vec<SessionSummary>),
    };
    println!("{}", serde_json::to_string_pretty(&bundle)?);
    Ok(())
}
