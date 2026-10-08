# AGENTS (e2e-support)

Subtree rules for what the live suites share — `scripts/e2e-lib.sh` and the
provider doubles — and the hermetic tests that hold its helpers, and the probes'
capability tables, to the registry without a credential or a model call. The suites
themselves are governed by `live/AGENTS.md`.

- **Deterministic, so in the gate** — unlike the `live-*` projects it serves. A
  change to a live helper is proven here first, by a hermetic test against this
  checkout's built binary and mock harness; the paid suite that uses it runs
  only from its own workflow.
