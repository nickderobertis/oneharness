# AGENTS (rust-coverage)

Subtree rules for the Rust line-coverage floor. Root `AGENTS.md` still applies.

- **Coverage is enforced at the skill default, 95% lines** (`FLOOR` in
  `scripts/rust-coverage.mjs`, overridable through `COVERAGE_MIN`), via `cargo
  llvm-cov` — an external tool, handled like `cargo-deny` and shellcheck: CI
  installs it (`cargo-llvm-cov` + the `llvm-tools-preview` rustup component, added
  by `just bootstrap`) and this project's `coverage` (part of `just check`) fails
  the gate below the bar. Each Rust project's `test` runs its crate instrumented
  (`scripts/cargo-test.sh`) and leaves its line record in
  `target/coverage/<record>.lcov`; `coverage` depends on every one of them and
  counts a line covered when ANY run executed it — what the single `--workspace`
  run it replaced measured, over the same files. So the `oneharness-core` engine
  is gated alongside the binary. The records rather than the raw profiles are
  what is combined because a cache-replayed `test` must carry its contribution
  without the instrumented objects that produced it. A stale instrumented build
  can only add uncovered lines, never hide one; `cargo llvm-cov clean
  --workspace` clears it.
  The threshold is line coverage, not region/branch: the hermetic mock-harness
  suite drives whole user journeys (high-leverage line coverage), and a few
  I/O-failure arms in `crates/oneharness-core/src/io/runner.rs` (spawn/wait
  errors) and `io/config.rs` are intentionally left ungated rather than faked with
  brittle environment manipulation. Measured coverage sits above 95% lines; keep
  new behavior covered rather than lowering the floor. Coverage is a
  platform-independent property of the suite, so it is enforced on Linux/macOS and
  skipped on Windows, where llvm-cov does not attribute the integration tests'
  subprocess-spawned binary coverage (a tooling limitation — the binary reads ~0%
  there). The functional gate still runs on all three platforms; only the coverage
  *measurement* is Linux/macOS.
