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

# Full quality gate: format check, lint (Rust + shell), tests, build, artifact
# smoke. Fails on any issue.
check: fmt-check lint lint-sh test build smoke
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

# Lint shell scripts with shellcheck; any finding is an error. Like `cargo-deny`,
# shellcheck is an external tool: CI installs it, and this prints an install hint
# if it is missing rather than failing cryptically.
lint-sh:
    if ! command -v shellcheck >/dev/null 2>&1; then echo "shellcheck not installed: 'apt-get install shellcheck' / 'brew install shellcheck' / https://github.com/koalaman/shellcheck#installing" >&2; exit 1; fi
    shellcheck scripts/*.sh

# Run the test suite (prefers nextest; falls back to cargo test).
test:
    if command -v cargo-nextest >/dev/null 2>&1; then cargo nextest run --features {{FEATURES}} --locked; else cargo test --features {{FEATURES}} --locked; fi

# Run only the end-to-end CLI tests.
e2e:
    if command -v cargo-nextest >/dev/null 2>&1; then cargo nextest run --features {{FEATURES}} --test cli --locked; else cargo test --features {{FEATURES}} --test cli --locked; fi

# Hermetic end-to-end smoke of the *built* binary (part of `check`; CI runs it on
# every platform). Drives list/detect/print-command + one mock spawn; no network.
smoke:
    bash scripts/smoke.sh

# Opt-in live smoke against installed, authenticated harnesses. Makes real (paid)
# model calls and needs network, so it is deliberately out of `check` and CI.
smoke-live:
    bash scripts/smoke.sh --live

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
