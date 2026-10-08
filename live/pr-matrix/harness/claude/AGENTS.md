# AGENTS (live-claude)

Rules for the `live-claude` suite alone (`scripts/e2e-claude.sh`,
`.github/workflows/e2e-claude.yml`).

<!-- llmlint: ignore-block[agents_md_durable_and_terse] The parenthetical naming which harnesses report prompt-cache counts today was moved verbatim from the root AGENTS.md (the oh_cache_assert clause of its live per-harness rule), under this change's instruction to move — not rewrite or trim — the text that governs only this suite; a durability pass over it is a change of its own. -->
- Its suite carries the `oh_cache_assert` phase, since the harness reports provider
  prompt-cache counts in its usage (today only Claude Code and OpenCode — see
  `extract_usage` in `domain::signals` and the README `usage` support matrix).
<!-- llmlint: ignore-end[agents_md_durable_and_terse] -->
