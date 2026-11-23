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
  cargo-llvm-cov's target directory: `report` reads them all, and one an older
  build left there (CI restores the target directory from cache) adds lines no
  test ran.
- **The build may live off the checkout.** `cargo-test.sh` and
  `rust-coverage.sh` find the objects and raw profiles through
  `scripts/llvm-cov-target-dir.sh`, cargo-llvm-cov's own rule
  (`<cargo target dir>/llvm-cov-target`; its `CARGO_LLVM_COV_TARGET_DIR`
  override is refused, not followed), so `CARGO_TARGET_DIR=/tmp/...` is measured where it was built. The merged
  per-run profiles stay in the checkout's `target/coverage`, the Nx outputs.
  Profiles the cache replayed from a build elsewhere are not this build's:
  after moving the target directory, run the Rust `test` targets and
  `rust-coverage:coverage` with `--skip-nx-cache`.
  `check-cargo-test-target-dir.sh` (`rust-coverage-integration`) holds
  `cargo-test.sh` to it.
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
