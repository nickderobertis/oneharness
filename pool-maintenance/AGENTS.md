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
  history: `measure.sh` clears the backlog untimed, builds every instrumented
  Rust test selection the gate's `test` targets build — read from each
  `project.json`'s `bash scripts/cargo-test.sh …` command, never restated — under
  an alternative feature selection (`+sdk-schema`, new metadata hashes for every
  crate here), backdates what that build created past the window, rebuilds the
  ordinary selections, and times one declared pass. It refuses a pass that
  removed nothing, one that removed an entry the alternative build did not
  create, or one that changed this repository's ordinary unit/hash entries.
- **It really prunes** the checkout it runs in, as the maintenance does: run it
  only in your own worktree.
- **One pin.** The pruner's version is the justfile's
  `PRUNE_SUPERSEDED_VERSION`; `just bootstrap` installs it (on the `stable`
  toolchain — it needs a newer rustc than `rust-toolchain.toml` pins) and
  `pool-maintenance` refuses any other version on PATH.
