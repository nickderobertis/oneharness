# AGENTS (live-codex)

Rules for the `live-codex` suite alone (`scripts/e2e-codex.sh`,
`.github/workflows/e2e-codex.yml`). Root `AGENTS.md`, `live/AGENTS.md`,
`live/pr-matrix/AGENTS.md` and `live/pr-matrix/harness/AGENTS.md` still apply.

- Its suite carries no `oh_hook_enforce` phase
  (`live/pr-matrix/harness/AGENTS.md`): Codex loads hooks only when the run opts
  in via `-c features.hooks=true --dangerously-bypass-hook-trust` —
  probe-verified, covered live by its mock phase.
