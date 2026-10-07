# AGENTS (explore-events)

The `explore-events` project: its `explore` target runs
`scripts/explore-events.sh` (extra arguments pass through: `bash scripts/nx run
explore-events:explore -- <args>`). CI runs it only on dispatch, from
`.github/workflows/explore-events.yml`. Informational, never a gate.
`explore/AGENTS.md` and the root `AGENTS.md` still apply.
