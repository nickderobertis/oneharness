# AGENTS (live-qwen)

Rules for the `live-qwen` suite alone (`scripts/e2e-qwen.sh`,
`.github/workflows/e2e-qwen.yml`).

- Its suite's `oh_hook_enforce` phase (`live/pr-matrix/harness/AGENTS.md`) takes
  the `global` scope argument (use `global` scope for a harness, like Qwen, that
  only fires user-scoped hooks headlessly).
