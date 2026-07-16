//! The Rust-owned schema bundle consumed by language SDK generators.
//!
//! Keeping every shared contract root here gives all SDKs one generation
//! source. Language packages may add client behavior, but must not restate
//! these wire shapes by hand.

use schemars::{schema_for, Schema};
use serde::Serialize;

use crate::domain::history::{HistoryRecord, HistoryStreamEnvelope};
use crate::domain::report::{RunReport, RunStreamEnvelope};
use crate::domain::sdk::{schema_for_serialize, HistoryListOptions, HistoryLookup, RunOptions};
use crate::io::history::SessionSummary;

/// All core schemas shared by oneharness SDKs.
#[derive(Debug, Serialize)]
pub struct SdkSchemaBundle {
    pub run_report: Schema,
    pub run_stream_envelope: Schema,
    pub run_options: Schema,
    pub history_lookup: Schema,
    pub history_list_options: Schema,
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
        history_record: schema_for_serialize::<HistoryRecord>(),
        history_stream_envelope: schema_for_serialize::<HistoryStreamEnvelope>(),
        history_records: schema_for_serialize::<Vec<HistoryRecord>>(),
        history_list: schema_for_serialize::<Vec<SessionSummary>>(),
    }
}
