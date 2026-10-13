# AGENTS (live suites)

Subtree rules for the `live-*` projects: the paid, credential-gated suites that
drive real harnesses. None declares a gate target, so neither tier ever runs one;
each runs from its own `.github/workflows/e2e-*.yml` (and `just live-<id>`).

- Also out of the core gate; the workflows are gated to the canonical repo and
  non-fork PRs. Auth comes from the `gh-secrets.json` manifest (Bitwarden secure
  notes → `.env` + GitHub Actions secrets via `just secrets-sync`); values never
  enter the repo and `.env` / `.gh-secrets-state.json` are gitignored.

## Network telemetry and the live-codex budgets

- **Every live launch is recorded, through one contract.** A suite's
  `oh_telemetry_setup` puts the measuring shim first on `PATH` for each harness
  executable; it and `scripts/e2e-lib.sh` are the only writers of
  `target/e2e-telemetry/network/<suite>.jsonl` (`start`, measurement and
  `expected` lines), and `live/pr-matrix/harness/codex/network_budget.py` its
  only reader. Change a line's shape on both sides together.
- **The figures are bytes at the launch's network-namespace boundary**
  (`netns-pasta`: pasta TAP counters over the whole process tree, detached
  descendants included) — payload plus per-frame headers, not host-NIC wire
  bytes. Say so wherever a figure is quoted.
- **Linux only, and it needs pasta plus unprivileged user namespaces**, not
  root: on Ubuntu 24.04, `sudo apt-get install -y passt` (the Linux legs of
  `e2e-codex.yml` and `ci.yml` run it first). The macOS and Windows legs, or a
  Linux host without them, record `"method": "unavailable"` and the budgets are
  unsupported there: the check runs on Linux alone, where an unmeasured launch
  fails every budget, naming it. The gate never checks real telemetry.
- **Turns are the budgets' evidence.** A catalog-scale regression shows only on
  a launch that runs long enough to finish an asynchronous fetch; one that exits
  within about a second (`--version`, a probe) cannot show it.
- **A Codex home that disables plugins hides an oneharness default regression**
  from the budgets, which see oneharness's defaults only through a home that
  does not override them. `e2e-variants.sh`'s opted-in launch (`plugins = true`)
  refuses such a home, naming its `config.toml`. The suite's own direct `codex`
  launches start with oneharness's defaults for the same reason.
