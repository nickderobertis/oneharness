# AGENTS (live-claude)

The `live-claude` project: `just live-claude` (its `live` target) runs
`scripts/e2e-claude.sh` against the release binary installed through
`scripts/install.sh`. CI runs it from `.github/workflows/e2e-claude.yml` on
ubuntu, macOS and Windows for a pull request touching its paths, and on
dispatch. Paid and credential-gated, so it declares no gate target and neither
tier runs it. Auth: CLAUDE_CODE_OAUTH_TOKEN (mint with `claude setup-token`) or
ANTHROPIC_API_KEY. `live/AGENTS.md` and the root `AGENTS.md` still apply.
