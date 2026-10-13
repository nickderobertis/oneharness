# AGENTS (live-codex)

Rules for the `live-codex` suite alone (`scripts/e2e-codex.sh`,
`.github/workflows/e2e-codex.yml`).

- Its suite carries no `oh_hook_enforce` phase: Codex loads hooks only when the run opts
  in via `-c features.hooks=true --dangerously-bypass-hook-trust` —
  probe-verified, covered live by its mock phase.
- Its network budgets (`budgets.yaml`) read the recorder's `codex.jsonl`.
  `evidence/live-codex.jsonl` is the measured pass the
  thresholds were checked against and `evidence/live-codex-report.json` its
  calibration report, written by `network_budget.py --evidence-report --file
  evidence/live-codex.jsonl`; tools/network-telemetry's tests recompute it, so
  re-measuring means replacing both together — and moving the suite cap to the
  new total x1.2 rounded up to the next 0.5 MB, which those tests also hold.
  Lowering a cap to the evidence is that rule; raising one is not this file's
  to do.
