# AGENTS (harness-captures)

Subtree rules for the captured harness output — the external CLIs' own wire
formats every parser here is held to — and the SDK acceptance matrix
(`sdk-contract-matrix.json`). Root `AGENTS.md` still applies.

- **A capture comes from a real run, never from imagination.** Record it from the
  CLI itself (the `explore-*` probes dump live output for exactly this) and keep
  it byte for byte; a hand-written shape proves only what its author believed.
- **It is a contract project** (`type:contract`): the engine, the shipped mock
  responder, the e2e journeys, the compatibility test, the SDK contract and the
  live helpers' checks depend on it by graph edge, so a changed capture re-runs
  exactly them.
