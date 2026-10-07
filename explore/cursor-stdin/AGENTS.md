# AGENTS (explore-cursor-stdin)

The `explore-cursor-stdin` project: its `explore` target runs
`scripts/explore-cursor-stdin.sh` (extra arguments pass through: `bash
scripts/nx run explore-cursor-stdin:explore -- <args>`). CI runs it only on
dispatch, from `.github/workflows/explore-cursor-stdin.yml`. Informational,
never a gate. `explore/AGENTS.md` and the root `AGENTS.md` still apply.
