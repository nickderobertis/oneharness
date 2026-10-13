# AGENTS (live-codex)

Rules for the `live-codex` suite alone (`scripts/e2e-codex.sh`,
`.github/workflows/e2e-codex.yml`).

- Its suite carries no `oh_hook_enforce` phase: Codex loads hooks only when the run opts
  in via `-c features.hooks=true --dangerously-bypass-hook-trust` —
  probe-verified, covered live by its mock phase.
- Its network budgets (`budgets.yaml`) are calibrated against the measured
  pass in `evidence/`: re-measure, and the evidence, its report and the suite
  cap move together (tools/network-telemetry's `Evidence` test holds how).
  Lowering a cap to the evidence is routine; raising one is a decision for
  whoever owns the budgets, not a fix for a red run.
