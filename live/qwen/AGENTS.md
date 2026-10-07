# AGENTS (live-qwen)

The `live-qwen` project: `just live-qwen` (its `live` target) runs
`scripts/e2e-qwen.sh` against the release binary installed through
`scripts/install.sh`. CI runs it from `.github/workflows/e2e-qwen.yml` on Linux
for a pull request touching its paths; any platform on dispatch (the `os`
input). Paid and credential-gated, so it declares no gate target and neither
tier runs it. Its credentials and model knobs are declared in the script's
header. `live/AGENTS.md` and the root `AGENTS.md` still apply.
