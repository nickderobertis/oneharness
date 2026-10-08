# AGENTS (scripts)

Subtree rules for the repository's tooling scripts.

- **Shell scripts are formatted with shfmt and linted with shellcheck**, the
  versions `.shell-tool-versions` pins, through `scripts/shell-check.sh` (this
  project's `format` and `lint`, part of `just check`; `githooks` runs the same
  over `.githooks/`). The style is the flags in `shell-check.sh` (`-i 2 -ci`);
  `just format` writes it. `just bootstrap` installs the pinned tools
  (`scripts/shell-tools.sh`); the targets run only those, never a PATH copy.
- **This project owns the scripts; it does not run them.** Every script belongs
  here by path, so a change to one always selects this `lint`. Which project
  RUNS it is declared by that project's target inputs: the drift gates belong to
  `ci-contracts`, `release-tooling`, `e2e-support`, `workspace` and
  `workspace-integration`, the per-crate
  test runner and coverage merge to the Rust projects and `rust-coverage`, the
  installer to `install-surface`, the npm build to `npm-launcher`, and the live
  and exploration drivers to the `live-*` and `explore-*` projects. A new script
  is listed in the inputs of the target that runs it, and a drift script runs
  through `scripts/with-portable-sed.sh` — in a `test`, under
  `scripts/shell-test.sh <project>` so the shell coverage floor reads it
  (`check-workflows-portable-test.sh` holds every project definition to both).
