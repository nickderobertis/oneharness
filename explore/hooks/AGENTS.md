# AGENTS (explore-hooks)

The `explore-hooks` project: its `explore` target runs
`scripts/explore-hooks.sh` (extra arguments pass through: `bash scripts/nx run
explore-hooks:explore -- <args>`). CI runs it only on dispatch, from
`.github/workflows/explore-hooks.yml`. Informational, never a gate.
`explore/AGENTS.md` and the root `AGENTS.md` still apply.
