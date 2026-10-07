# AGENTS (live-codex)

The `live-codex` project: `just live-codex` (its `live` target) runs
`scripts/e2e-codex.sh` against the release binary installed through
`scripts/install.sh`. CI runs it from `.github/workflows/e2e-codex.yml` on
ubuntu, macOS and Windows for a pull request touching its paths, and on
dispatch. Paid and credential-gated, so it declares no gate target and neither
tier runs it. Auth: an existing codex login, else OPENAI_API_KEY.
`live/AGENTS.md` and the root `AGENTS.md` still apply.
