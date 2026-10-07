# AGENTS (live-claude)

Rules for the `live-claude` suite alone (`scripts/e2e-claude.sh`,
`.github/workflows/e2e-claude.yml`). Root `AGENTS.md`, `live/AGENTS.md`,
`live/pr-matrix/AGENTS.md` and `live/pr-matrix/harness/AGENTS.md` still apply.

- Its suite carries the `oh_cache_assert` phase
  (`live/pr-matrix/harness/AGENTS.md`), since the harness reports provider
  prompt-cache counts in its usage (today only Claude Code and OpenCode — see
  `extract_usage` in `domain::signals` and the README `usage` support matrix).
