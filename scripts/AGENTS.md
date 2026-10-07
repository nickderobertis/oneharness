# AGENTS (scripts)

Subtree rules for the repository's tooling scripts.

- **Shell scripts are linted with shellcheck** — an external tool, handled like
  `cargo-deny`: CI installs it and this project's `lint` (part of `just check`)
  enforces it over every script here; install it locally (`apt-get`/`brew
  install shellcheck`) to run the full gate.
- **This project owns the scripts; it does not run them.** Every script belongs
  here by path, so a change to one always selects this `lint`. Which project
  RUNS it is declared by that project's target inputs: the drift gates belong to
  `ci-contracts`, `release-tooling`, `e2e-support` and `workspace`, the per-crate
  test runner and coverage merge to the Rust projects and `rust-coverage`, the
  installer to `install-surface`, the npm build to `npm-launcher`, and the live
  and exploration drivers to the `live-*` and `explore-*` projects. A new script
  is listed in the inputs of the target that runs it, and a drift script runs
  through `scripts/with-portable-sed.sh` (`check-workflows-portable-test.sh`
  holds every project definition to that).
