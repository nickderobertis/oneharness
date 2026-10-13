# AGENTS (live suites)

Subtree rules for the `live-*` projects: the paid, credential-gated suites that
drive real harnesses. None declares a gate target, so neither tier ever runs one;
each runs from its own `.github/workflows/e2e-*.yml` (and `just live-<id>`).

- Also out of the core gate; the workflows are gated to the canonical repo and
  non-fork PRs. Auth comes from the `gh-secrets.json` manifest (Bitwarden secure
  notes → `.env` + GitHub Actions secrets via `just secrets-sync`); values never
  enter the repo and `.env` / `.gh-secrets-state.json` are gitignored.

## Network telemetry and the live-codex budgets

- **Every live launch is measured.** Each suite's `oh_telemetry_setup` puts a
  shim (`scripts/e2e-netshim.sh`) first on `PATH` for every harness executable,
  resolved beforehand; the shim refuses to wrap itself. It appends a `start`
  line (`suite`, `phase` — the `OH_E2E_PHASE` its helper exported — `harness`,
  `kind`, `launch_id`, `argv`) and one measurement line to
  `target/e2e-telemetry/network/<suite>.jsonl` (`OH_E2E_TELEMETRY_DIR`
  overrides), and each helper adds an `expected` line naming the Codex launch
  kind its phase must produce. Those lines are the contract
  `live/pr-matrix/harness/codex/network_budget.py` reads; the shim and
  `scripts/e2e-lib.sh` are their only writers.
- **The method** (`scripts/e2e-netmeasure.py`, `"method": "netns-pasta"`): the
  launch runs in a fresh unprivileged user + network + mount + PID namespace
  whose only interface is a pasta TAP device, and its bytes are that
  interface's `/proc/net/dev` deltas over the whole process tree — detached and
  double-forked descendants inherit the namespace, which lasts until the last
  of them exits (a deadline, `OH_E2E_NET_DEADLINE`, sends SIGTERM, SIGKILL 5 s
  later). DNS goes to the upstream resolvers bind-mounted inside. The figures
  are **bytes at the namespace boundary** — payload plus per-frame headers —
  not host-NIC wire bytes. It needs pasta on `PATH` and unprivileged user
  namespaces, not root: on Ubuntu 24.04 `sudo apt-get install -y passt` (its
  AppArmor profile permits pasta's namespaces), which the Linux legs of
  `e2e-codex.yml` and `ci.yml` run first.
- **Linux only.** Where the method cannot run — the macOS and Windows legs, or
  a Linux host without pasta or user namespaces — the launch still runs and its
  line is `"method": "unavailable"` with a `reason` and no bytes. The budgets
  are unsupported there: `scripts/e2e-network-budget.sh` checks nothing off
  Linux, and on Linux an unmeasured launch fails every budget, naming it.
- **The budgets** (`live/pr-matrix/harness/codex/budgets.yaml`): bytes any one
  Codex launch received, any usage probe received, and the whole pass
  received. `just live-codex` runs `onebudgetspec check` over them after its
  suite; the gate never does (`live-*` declare no gate target).
- **Turns are the evidence.** A catalog-scale regression (a default that
  downloads tens of MB per launch) shows on a launch that runs long enough to
  finish an asynchronous fetch; a launch that exits within about a second
  (`--version`, a probe) cannot show it. So the suite's exec turns, its
  controlled app-server turn and its usage probe are what the per-launch budget
  stands on.
- **A Codex home that disables plugins hides a regression.** The budgets see
  oneharness's defaults only through a Codex home that does not override them;
  one whose `config.toml` sets `[features] plugins = false` would keep an
  oneharness plugins-default regression off the record. `e2e-variants.sh`'s
  opted-in launch (`codex:plugins`, `plugins = true`) refuses such a home,
  naming the file, and asserts its own catalog request and plugin load.
