# AGENTS (rust-coverage)

Subtree rules for the Rust line-coverage floor.

- **Coverage is enforced at the skill default, 95% lines** (`COVERAGE_MIN`,
  default 95, in `scripts/rust-coverage.sh`), via `cargo llvm-cov` — an external
  tool, handled like `cargo-deny`: CI installs it
  (`cargo-llvm-cov` + the `llvm-tools-preview` rustup component, added by `just
  bootstrap`) and this project's `coverage` (part of `just check`) fails the gate
  below the bar.
- **One floor over every Rust run, measuring what `--workspace` measured.**
  `coverage` depends on every `lang:rust` project whose `test` runs
  `scripts/cargo-test.sh` (`check-nx-graph.mjs` refuses a missing one) and
  reports over every workspace member, so no crate's coverage slips outside
  the floor.
- **Replayed profiles are read against this tree's objects.** Each `test`
  caches its merged profile; `coverage` rebuilds the instrumented objects
  before reporting, so a stale build can never drop replayed counts or pass a
  floor the tree should fail. It then removes every other executable under
  `target/llvm-cov-target`: `report` reads them all, and one an older build
  left there (CI restores the target directory from cache) adds lines no test
  ran.
- The threshold is line coverage, not region/branch: the hermetic mock-harness
  suite drives whole user journeys (high-leverage line coverage), and a few
  I/O-failure arms in `crates/oneharness-core/src/io/runner.rs` (spawn/wait
  errors) and `io/config.rs` are intentionally left ungated rather than faked with
  brittle environment manipulation. Keep new behavior covered rather than
  lowering the floor. Coverage is a
  platform-independent property of the suite, so it is enforced on Linux/macOS and
  skipped on Windows, where llvm-cov does not attribute the integration tests'
  subprocess-spawned binary coverage (a tooling limitation — the binary reads ~0%
  there). The functional gate still runs on all three platforms; only the coverage
  *measurement* is Linux/macOS.
