# Canonical command surface for oneharness.
#
# `just bootstrap` must work from a clean clone; `just check` is the full quality
# gate and fails on any issue (no warnings-only mode). Recipes are quiet on
# success and specific on failure.

set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

# Feature that builds the test-only mock harness fixture the e2e tests drive.
FEATURES := "mock-harness"

# List available recipes.
default:
    @just --list

# Set up from a clean clone: toolchain components + fetched dependencies.
bootstrap:
    rustup component add rustfmt clippy
    cargo fetch --locked

# Full quality gate: format check, lint, tests, build. Fails on any issue.
check: fmt-check lint test build
    @echo "check: ok"

# Verify formatting without modifying files.
fmt-check:
    cargo fmt --all -- --check

# Format the codebase in place.
format:
    cargo fmt --all

# Lint with clippy; any warning is an error.
lint:
    cargo clippy --all-targets --features {{FEATURES}} -- -D warnings

# Alias for `lint`.
clippy: lint

# Run the test suite (prefers nextest; falls back to cargo test).
test:
    if command -v cargo-nextest >/dev/null 2>&1; then cargo nextest run --features {{FEATURES}} --locked; else cargo test --features {{FEATURES}} --locked; fi

# Run only the end-to-end CLI tests.
e2e:
    if command -v cargo-nextest >/dev/null 2>&1; then cargo nextest run --features {{FEATURES}} --test cli --locked; else cargo test --features {{FEATURES}} --test cli --locked; fi

# Debug build.
build:
    cargo build --locked

# Optimized release build (the distributed artifact).
build-release:
    cargo build --release --locked

# Advisory + license audit. Separate from `check`: needs a network advisory DB.
deps-check:
    if ! command -v cargo-deny >/dev/null 2>&1; then echo "cargo-deny not installed: cargo install cargo-deny --locked" >&2; exit 1; fi
    cargo deny check

# Upgrade dependencies, then re-run the full gate.
upgrade:
    cargo update
    @just check

# Verbose, install-free diagnostics (kept out of the gate).
doctor:
    rustc --version
    cargo --version
    cargo tree --edges normal

# Run the CLI through cargo, e.g. `just run -- list`.
run *ARGS:
    cargo run --quiet -- {{ARGS}}
