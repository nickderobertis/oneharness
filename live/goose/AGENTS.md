# AGENTS (live-goose)

The `live-goose` project: `just live-goose` (its `live` target) runs
`scripts/e2e-goose.sh` against the release binary installed through
`scripts/install.sh`. CI runs it from `.github/workflows/e2e-goose.yml` on Linux
for a pull request touching its paths; any platform on dispatch (the `os`
input). Paid and credential-gated, so it declares no gate target and neither
tier runs it. Its credentials and model knobs are declared in the script's
header. `live/AGENTS.md` and the root `AGENTS.md` still apply.
