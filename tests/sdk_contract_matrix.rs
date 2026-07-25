use oneharness_core::domain::history::{HistoryLine, HistoryRecord, HistoryStreamEnvelope};
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
        "history_line" => serde_json::from_value::<HistoryLine>(case.value.clone()).is_ok(),
        "history_record" => serde_json::from_value::<HistoryRecord>(case.value.clone()).is_ok(),
        "history_stream_envelope" => {
            serde_json::from_str::<HistoryStreamEnvelope>(&case.value.to_string()).is_ok()
        }
        root => panic!("unknown SDK fixture root {root}"),
    }
}

fn matrix() -> Matrix {
    serde_json::from_str(include_str!("fixtures/sdk-contract-matrix.json"))
        .expect("shared SDK contract matrix must be valid JSON")
}

#[test]
fn rust_contracts_match_the_shared_sdk_acceptance_matrix() {
    let matrix = matrix();
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

/// The multibyte label case only tells the three languages apart while it is long
/// enough that a byte count and a UTF-16 code-unit count both overshoot the limit
/// the contract states in characters. Shortening it would leave every validator
/// agreeing for the wrong reason, so assert the probe is still a probe.
#[test]
fn the_multibyte_label_fixture_probes_every_length_unit() {
    const LABEL_VALUE_MAX: usize = 256;
    let matrix = matrix();
    let case = matrix
        .cases
        .iter()
        .find(|case| case.name == "run multibyte label value within the character limit")
        .expect("the multibyte label case must stay in the shared matrix");
    let value = case.value["historyLabels"]["graph"]
        .as_str()
        .expect("the multibyte label case carries a string value");

    assert!(
        value.chars().count() <= LABEL_VALUE_MAX,
        "the contract counts characters, so this case must be accepted"
    );
    assert!(
        value.len() > LABEL_VALUE_MAX,
        "must overshoot when miscounted in UTF-8 bytes, as Rust once did"
    );
    assert!(
        value.chars().map(char::len_utf16).sum::<usize>() > LABEL_VALUE_MAX,
        "must overshoot when miscounted in UTF-16 code units, as Zod's `.max()` does"
    );
}
