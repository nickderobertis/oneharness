use oneharness_core::domain::sdk::{
    HistoryListOptions, HistoryLookup, HistoryWatchOptions, RunOptions,
};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct Matrix {
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    root: String,
    accepted: bool,
    value: Value,
}

fn accepts(case: &Case) -> bool {
    match case.root.as_str() {
        "run_options" => serde_json::from_value::<RunOptions>(case.value.clone()).is_ok(),
        "history_lookup" => serde_json::from_value::<HistoryLookup>(case.value.clone()).is_ok(),
        "history_list_options" => {
            serde_json::from_value::<HistoryListOptions>(case.value.clone()).is_ok()
        }
        "history_watch_options" => {
            serde_json::from_value::<HistoryWatchOptions>(case.value.clone()).is_ok()
        }
        root => panic!("unknown SDK fixture root {root}"),
    }
}

#[test]
fn rust_contracts_match_the_shared_sdk_acceptance_matrix() {
    let matrix: Matrix = serde_json::from_str(include_str!("fixtures/sdk-contract-matrix.json"))
        .expect("shared SDK contract matrix must be valid JSON");
    assert!(!matrix.cases.is_empty(), "matrix must collect cases");
    for case in &matrix.cases {
        assert_eq!(
            accepts(case),
            case.accepted,
            "shared SDK contract case `{}` disagrees with Rust: {}",
            case.name,
            case.value
        );
    }
}
