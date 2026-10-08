//! The single generation source every language SDK reads. `scripts/generate.mjs`
//! and `scripts/generate.py` both run this example and generate their validators
//! from its JSON, so a contract stated in `oneharness-core` reaches Node and
//! Python without being restated by hand.
//!
//! It emits `oneharness_core::sdk_schema::bundle()` verbatim. The CLI's own
//! report roots used to be added here because `ListReport`/`DetectReport` lived
//! in this crate; they are `io::registry`/`io::detect` types now, so the bundle
//! carries them and this is a thin passthrough. It stays a separate example
//! because the SDK generators pin their generation source by path.

fn main() -> Result<(), serde_json::Error> {
    println!(
        "{}",
        serde_json::to_string_pretty(&oneharness_core::sdk_schema::bundle())?
    );
    Ok(())
}
