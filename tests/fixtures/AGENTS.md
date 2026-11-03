# AGENTS (harness-captures)

Subtree rules for the captured harness output — the external CLIs' own wire
formats every parser here is held to — and two acceptance matrices, which are
contracts rather than captures: the SDK's (`sdk-contract-matrix.json`) and the
UTC instants `scripts/e2e-lib.sh` and `UtcInstant` must agree on
(`utc-instant-matrix.json`).

- **A capture comes from a real run, never from imagination.** Record it from the
  CLI itself (the `explore-*` probes dump live output for exactly this) and keep
  it byte for byte; a hand-written shape proves only what its author believed.
  `opencode-run-tool-steps.jsonl` is the one re-emitted capture: a real opencode
  1.18.5 run's parts, each kept as its session store recorded it, wrapped in the
  `{type, timestamp, sessionID, part}` envelope `opencode_run.jsonl` shows
  `run --format json` streaming (user-prompt and `patch` parts omitted, as that
  stream omits them).
- **Read a capture through a graph edge, never an input.** This is a contract
  project (`type:contract`): a project whose code or tests read a file here
  declares `harness-captures` in its `implicitDependencies`, so a changed capture
  re-runs exactly its readers and the boundary check sees the dependency.
