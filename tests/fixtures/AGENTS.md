# AGENTS (harness-captures)

Subtree rules for the captured harness output — the external CLIs' own wire
formats every parser here is held to — and two acceptance matrices, which are
contracts rather than captures: the SDK's (`sdk-contract-matrix.json`) and the
UTC instants `scripts/e2e-lib.sh` and `UtcInstant` must agree on
(`utc-instant-matrix.json`).

- **A capture comes from a real run, never from imagination.** Record it from the
  CLI itself (the `explore-*` probes dump live output for exactly this) and keep
  it byte for byte; a hand-written shape proves only what its author believed.
- **Read a capture through a graph edge, never an input.** This is a contract
  project (`type:contract`): a project whose code or tests read a file here
  declares `harness-captures` in its `implicitDependencies`, so a changed capture
  re-runs exactly its readers and the boundary check sees the dependency.
- **`crush-http-api.json` is crush's server contract per supported release**:
  each claim cites crush's source at the release's tagged commit
  (`path:line` + the line verbatim), and `observed` is that release's real
  server answering. A new crush release that changes the posture routes or
  fields gets a NEW entry with both, never an edit to an older one
  (`the_crush_http_api_fixture_is_backed_by_its_pinned_source_and_covers_the_pinned_release`
  in `crates/oneharness-core/src/domain/http.rs` holds the pin to it).
