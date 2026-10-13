# AGENTS (network-breakdown)

Subtree rules for `scripts/codex-network-breakdown.py` and its hermetic test.

- **The figures it prints are the README's.** The README's *What a Codex launch
  downloads* table quotes runs of this script against real Codex; re-measure
  with the invocation stated there when Codex or a launch default changes, and
  update the table and its before-and-after totals together.
- **Fixtures are recorded, then scrubbed.** `recorded-measurement.json` and
  `recorded-codex-trace.log` come from a real `--measurement-out` / `--trace-out`
  run, trimmed to the connection and request lines and stripped of account,
  installation and session identifiers; `expected-paths.json` is the
  attribution a reviewer checked against that trace.
