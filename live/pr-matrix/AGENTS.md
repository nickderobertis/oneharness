# AGENTS (live PR-matrix suites)

Rules for the live suites `scripts/check-e2e-matrix.sh` holds to one shared
pull-request matrix contract: the eight per-harness suites under `harness/` and
`live-schema`. `live-control` and `live-variants` sit outside it.

<!-- llmlint: ignore-file[agents_md_durable_and_terse] Everything below this lead was moved verbatim from live/AGENTS.md (and before that the root AGENTS.md) under this change's instruction to move — not rewrite or trim — the text that governs these suites; only the connecting clause naming where a split-off rule now lives is new. The per-harness and per-platform detail is the original's; a durability pass over it is a change of its own. -->

- **Live e2e in CI — when it fires, and how to check ONE harness/platform.** The
  live workflows (`e2e-<id>.yml` + `e2e-schema.yml`) run on **`pull_request` and
  `workflow_dispatch` only — never `push: main`**. The on-main run was dropped
  as redundant paid model calls: the release-plz `release vX.Y.Z` PR re-runs the
  suite as the pre-release gate, so main is already covered by the last PR
  before a release. The **PR matrix is deliberately slim** — only **claude-code
  and codex run cross-platform** (ubuntu/macos/windows); every other harness
  (and the schema feature) runs **Linux-only** on PRs. Cross-platform coverage
  for the rest is **on demand**, not automatic. To check a single harness and/or
  platform, DO NOT push a commit — that re-runs the whole PR suite. Instead
  dispatch the one workflow with its `os` input (`all`, or a single
  `ubuntu-latest` / `macos-latest` / `windows-latest`; `default` = the PR
  matrix): e.g. `gh workflow run e2e-goose.yml -f os=windows-latest` (or the
  GitHub MCP `actions_run_trigger` with `workflow=e2e-goose.yml`, `inputs={os:
  windows-latest}`). GitHub Actions can't centralize the per-workflow dispatch
  options or matrix, so this contract is duplicated across the `e2e-*.yml` files
  by necessity; `scripts/check-e2e-matrix.sh` (in `ci-contracts`' `test`, so in
  `just check` and CI) is its **drift gate** — it holds the one canonical
  spelling of the contract and fails if any workflow diverges (no `push`
  trigger; claude/codex cross-platform, the rest Linux-only on PR).
