# AGENTS (live-copilot)

The `live-copilot` project: `just live-copilot` (its `live` target) runs
`scripts/e2e-copilot.sh` against the release binary installed through
`scripts/install.sh`. CI runs it from `.github/workflows/e2e-copilot.yml` on
Linux for a pull request touching its paths; any platform on dispatch (the `os`
input). Paid and credential-gated, so it declares no gate target and neither
tier runs it. Auth: COPILOT_GITHUB_TOKEN (a fine-grained PAT with the "Copilot
Requests" permission), or an existing `copilot` login. `live/AGENTS.md` and the
root `AGENTS.md` still apply.
