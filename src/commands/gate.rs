//! `oneharness gate <id>` — the runtime pre-tool gate for a harness.
//!
//! An installed `[[hooks]]` hook (see `oneharness sync`) invokes this: the
//! harness pipes its pre-tool event JSON to stdin, and this reads it, decides
//! whether to block, and — only on a block — writes that harness's native deny
//! verdict to stdout. A non-block writes nothing (the universal fall-through),
//! and the process always exits 0 so a gate never blocks a call on its own
//! error. The per-harness verdict shapes are pure data in
//! `oneharness_core::domain::gate`; this is the thin stdin/stdout wrapper.
//!
//! The decision is intentionally a substring match (`--deny-if-contains`): this
//! gate exists to *prove a synced hook is honored* end to end (the e2e drives a
//! real harness through it), not to be a policy engine.

use std::io::Read;

use crate::cli::GateArgs;
use oneharness_core::domain::gate;
use oneharness_core::domain::harness;
use oneharness_core::errors::OneharnessError;

pub fn run(args: &GateArgs) -> Result<i32, OneharnessError> {
    let spec = harness::by_id(&args.harness).ok_or_else(|| OneharnessError::UnknownHarness {
        id: args.harness.clone(),
        valid: harness::valid_ids(),
    })?;
    let shape = spec
        .gate_deny
        .ok_or_else(|| OneharnessError::GateUnsupported { id: spec.id.into() })?;

    // Read the whole event. A read failure is a fail-open no-op (exit 0, no
    // output) — never block the call on our own I/O error.
    let mut event = String::new();
    if std::io::stdin().read_to_string(&mut event).is_err() {
        return Ok(0);
    }

    if let Some(needle) = args.deny_if_contains.as_deref() {
        if gate::should_deny(&event, needle) {
            // The verdict is the JSON alone; the exit code stays 0.
            println!("{}", gate::render_deny(shape, &args.reason));
        }
    }
    Ok(0)
}
