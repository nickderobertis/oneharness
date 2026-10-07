# AGENTS (rust-coverage)

Subtree rules for the Rust line-coverage floor.

- **Coverage is enforced at the skill default, 95% lines** (`COVERAGE_MIN`,
  default 95, in `scripts/rust-coverage.sh`), via `cargo llvm-cov` — an external
  tool, handled like `cargo-deny` and shellcheck: CI installs it
  (`cargo-llvm-cov` + the `llvm-tools-preview` rustup component, added by `just
  bootstrap`) and this project's `coverage` (part of `just check`) fails the gate
  below the bar. Each Rust project's `test` runs its crate instrumented
  (`scripts/cargo-test.sh`) and merges that run's raw profiles into one indexed
  profile, `target/coverage/<record>.profdata` (one small file, so the target can
  cache it, where the raw profiles of a suite that spawns the binary number in
  the thousands). `coverage` depends on every one of them and runs `cargo llvm-cov
  report --fail-under-lines` over all of them and every workspace member — the
  metric and the files the single `--workspace` run it replaced measured. So the
  `oneharness-core` engine is gated alongside the binary. The runs are read off
  the project definitions (every `lang:rust` project whose `test` runs
  `cargo-test.sh`), and `check-nx-graph.mjs` holds `coverage`'s `dependsOn` to
  that same set. The report maps the profiles onto the instrumented objects in
  `target/llvm-cov-target`, so it first rebuilds each run's selection
  build-only: whichever profiles the cache replayed, the objects are this
  tree's, never a later build's that could drop the replayed counts — or carry
  code the tree no longer has and pass a floor it should fail.
  The threshold is line coverage, not region/branch: the hermetic mock-harness
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
