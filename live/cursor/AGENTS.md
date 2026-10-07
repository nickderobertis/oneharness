# AGENTS (live-cursor)

The `live-cursor` project: `just live-cursor` (its `live` target) runs
`scripts/e2e-cursor.sh` against the release binary installed through
`scripts/install.sh`. CI runs it from `.github/workflows/e2e-cursor.yml` on
Linux for a pull request touching its paths; any platform on dispatch (the `os`
input). Paid and credential-gated, so it declares no gate target and neither
tier runs it. Auth: CURSOR_API_KEY (generate at
https://cursor.com/dashboard/api). `live/AGENTS.md` and the root `AGENTS.md`
still apply.
