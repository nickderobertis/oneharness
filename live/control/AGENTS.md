# AGENTS (live-control)

The `live-control` project: `just live-control` (its `live` target) runs
`scripts/e2e-control.sh` against the release binary installed through
`scripts/install.sh`. CI runs it from `.github/workflows/e2e-control.yml` on
Linux for a pull request touching the control feature's own sources, on ubuntu
and macOS on the daily schedule, and on dispatch. Paid and credential-gated, so
it declares no gate target and neither tier runs it. Its credentials and model
knobs are declared in the script's header. `live/AGENTS.md` and the root
`AGENTS.md` still apply.
