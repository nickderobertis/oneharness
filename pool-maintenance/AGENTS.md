# pool-maintenance

The budget on this repository's declared onevcs pool maintenance
(`../onevcs.toml` → the justfile's `pool-maintenance` recipe →
`cargo-prune-superseded --keep-within 2d target`), and `measure.sh`, which
exists only to measure it.

- **Never in a gate tier.** `measure.sh` builds the repository's test targets
  twice, so the budget runs only in `budgets-host`, which neither `check` tier
  names: `just budgets`, which ci.yml's check job runs on its Linux leg after
  the release pull request's full sweep. `scripts/check-nx-graph.mjs` refuses
  any other target that runs this budget's check, and a `check` recipe that
  names the budget targets. The gate's targets here only lint and
  format-check `measure.sh`.
- **The figure is a steady-state pass**, never a first pass over accumulated
  history: `measure.sh` clears the backlog untimed, builds every Rust test
  selection the gate's `test` targets build — read from each `project.json`'s
  `bash scripts/cargo-test.sh …` command, never restated, the uninstrumented e2e
  one included (`cargo-test.sh --uninstrumented --build-only`) — under
  an alternative feature selection (`+sdk-schema`, new metadata hashes for every
  crate here), backdates what that build created past the window, rebuilds the
  ordinary selections, and times one declared pass. It refuses a pass that
  removed nothing, one that removed an entry the alternative build did not
  create, or one that changed this repository's ordinary unit/hash entries.
  It is silent on success; set `POOL_MAINTENANCE_EVIDENCE` to an existing
  file to get the selections, both passes' pruner lines and the unit/hash
  entries kept (`ONEBUDGETSPEC_RESULT` must be the fresh empty file
  onebudgetspec names, and both pruner reports must parse as onebudgetspec's
  `{value, detail}`, or it refuses).
- **Its own target directory.** Every build and both passes use
  `target/pool-maintenance/target` (`CARGO_TARGET_DIR`; the declared recipe
  runs with `--working-directory target/pool-maintenance`, so its `target` is
  that one), never the checkout's `target/`: `just budgets` runs it beside Nx
  tasks that build there, and their entries appearing in, or being pruned by, a
  timed pass would error the budget. It persists between runs (warm rebuilds;
  the backlog pass bounds it).
  `just pool-maintenance-isolation` (`measure-isolation-test.sh`) proves it:
  the budget must report a figure while a scratch crate keeps writing
  superseded, backdated builds into `target/debug`, and leave them all there.
- **It really prunes** its own target directory, as the maintenance does a
  slot's: run it only in your own worktree.
- **One pin.** The pruner's version is the justfile's
  `PRUNE_SUPERSEDED_VERSION`; `just bootstrap` installs it (on the `stable`
  toolchain — it needs a newer rustc than `rust-toolchain.toml` pins) and
  `pool-maintenance` refuses any other version on PATH.
