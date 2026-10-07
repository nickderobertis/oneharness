# AGENTS (live-crush)

The `live-crush` project: `just live-crush` (its `live` target) runs
`scripts/e2e-crush.sh` against the release binary installed through
`scripts/install.sh`. CI runs it from `.github/workflows/e2e-crush.yml` on Linux
for a pull request touching its paths; any platform on dispatch (the `os`
input). Paid and credential-gated, so it declares no gate target and neither
tier runs it. Auth: a provider key (ANTHROPIC_API_KEY or OPENAI_API_KEY).
`live/AGENTS.md` and the root `AGENTS.md` still apply.
