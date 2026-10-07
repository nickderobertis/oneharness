# AGENTS (live-schema)

The `live-schema` project: `just live-schema` (its `live` target) runs
`scripts/e2e-schema.sh` against the release binary installed through
`scripts/install.sh`. CI runs it from `.github/workflows/e2e-schema.yml` on
Linux for a pull request touching its paths; ubuntu or macOS on dispatch (never
Windows: the `.cmd` shim mangles its quote-heavy argv). Paid and
credential-gated, so it declares no gate target and neither tier runs it. Its
credentials and model knobs are declared in the script's header.
`live/AGENTS.md` and the root `AGENTS.md` still apply.
