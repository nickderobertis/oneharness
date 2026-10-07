# AGENTS (harness-captures)

Subtree rules for the captured harness output — the external CLIs' own wire
formats every parser here is held to — and the SDK acceptance matrix
(`sdk-contract-matrix.json`). Root `AGENTS.md` still applies.

- **A capture comes from a real run, never from imagination.** Record it from the
  CLI itself (the `explore-*` probes dump live output for exactly this) and keep
  it byte for byte; a hand-written shape proves only what its author believed.
- **Read a capture through a graph edge, never an input.** This is a contract
  project (`type:contract`): a project whose code or tests read a file here
  declares `harness-captures` in its `implicitDependencies`, so a changed capture
  re-runs exactly its readers and the boundary check sees the dependency.
