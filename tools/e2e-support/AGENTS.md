# AGENTS (e2e-support)

Subtree rules for what the live suites share — `scripts/e2e-lib.sh` and the
provider doubles — and the hermetic tests that hold its helpers, and the probes'
capability tables, to the registry without a credential or a model call. Root
`AGENTS.md` still applies; `live/AGENTS.md` governs the suites themselves.

- **Deterministic, so in the gate** — unlike the `live-*` projects it serves. A
  change to a live helper is proven here first (`check-usage-enforce.sh`,
  `check-control-enforce.sh`, `check-copilot-login-probe.sh`,
  `check-codex-usage-schema*.sh`, `check-control-probe*.sh`,
  `e2e-variants-test.sh`), against this checkout's built binary and mock
  harness; the paid suite that uses it runs only from its own workflow.
