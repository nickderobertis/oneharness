# AGENTS (live-copilot)

Rules for the `live-copilot` suite alone (`scripts/e2e-copilot.sh`,
`.github/workflows/e2e-copilot.yml`). Root `AGENTS.md`, `live/AGENTS.md`,
`live/pr-matrix/AGENTS.md` and `live/pr-matrix/harness/AGENTS.md` still apply.

- Its suite carries no `oh_hook_enforce` phase
  (`live/pr-matrix/harness/AGENTS.md`): Copilot's hooks were probe-REFUTED
  headlessly, zero events.
