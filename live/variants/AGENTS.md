# AGENTS (live-variants)

The `live-variants` project: `just live-variants` (its `live` target) runs
`scripts/e2e-variants.sh` against the release binary installed through
`scripts/install.sh`. CI runs it from `.github/workflows/e2e-variants.yml` for a
pull request touching its paths, and on dispatch. Paid and credential-gated, so
it declares no gate target and neither tier runs it. Its credentials and model
knobs are declared in the script's header. `live/AGENTS.md` and the root
`AGENTS.md` still apply.
