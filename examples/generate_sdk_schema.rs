//! The single generation source every language SDK reads: the core bundle plus
//! the CLI's own report roots. `scripts/generate.mjs` and `scripts/generate.py`
//! both run this example and generate their validators from its JSON, so a
//! contract stated here reaches Node and Python without being restated by hand.

use oneharness::commands::{detect::DetectReport, list::ListReport};
use oneharness_core::domain::sdk::schema_for_serialize;
use oneharness_core::sdk_schema::{self, SdkSchemaBundle};
use serde::Serialize;

#[derive(Serialize)]
struct SchemaBundle {
    #[serde(flatten)]
    core: SdkSchemaBundle,
    list_report: schemars::Schema,
    detect_report: schemars::Schema,
}

fn main() -> Result<(), serde_json::Error> {
    let bundle = SchemaBundle {
        core: sdk_schema::bundle(),
        list_report: schema_for_serialize::<ListReport>(),
        detect_report: schema_for_serialize::<DetectReport>(),
    };
    println!("{}", serde_json::to_string_pretty(&bundle)?);
    Ok(())
}
