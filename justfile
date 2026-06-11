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

# --- Per-harness live e2e against the real CLIs (opt-in) ---------------------
#
# `smoke-live` is the quick "does any installed harness work" check. These
# `live-<harness>` recipes are the allowlister-style per-harness conformance
# suite: each drives ONE real harness through oneharness with that provider's
# model/auth and asserts the JSON contract (plant a marker, harness echoes it,
# assert status==ok). A missing CLI or auth is a skip, never a failure. They are
# OUT of `check`/CI's core gate; the `.github/workflows/e2e-*.yml` workflows run
# them per harness, gated to the canonical repo. See scripts/e2e-lib.sh.

ONEHARNESS_BIN := justfile_directory() / "target/release/oneharness"

# Build the release binary the live scripts drive, so they never use a stale build.
_live-build:
    cargo build --release --locked

live-claude: _live-build
    ONEHARNESS_BIN="{{ONEHARNESS_BIN}}" bash scripts/e2e-claude.sh

live-codex: _live-build
    ONEHARNESS_BIN="{{ONEHARNESS_BIN}}" bash scripts/e2e-codex.sh

live-opencode: _live-build
    ONEHARNESS_BIN="{{ONEHARNESS_BIN}}" bash scripts/e2e-opencode.sh

live-goose: _live-build
    ONEHARNESS_BIN="{{ONEHARNESS_BIN}}" bash scripts/e2e-goose.sh

live-qwen: _live-build
    ONEHARNESS_BIN="{{ONEHARNESS_BIN}}" bash scripts/e2e-qwen.sh

live-crush: _live-build
    ONEHARNESS_BIN="{{ONEHARNESS_BIN}}" bash scripts/e2e-crush.sh

live-copilot: _live-build
    ONEHARNESS_BIN="{{ONEHARNESS_BIN}}" bash scripts/e2e-copilot.sh

live-cursor: _live-build
    ONEHARNESS_BIN="{{ONEHARNESS_BIN}}" bash scripts/e2e-cursor.sh

# Run every per-harness live check; skips count as passes, only real failures fail.
live-all: _live-build
    #!/usr/bin/env bash
    set -uo pipefail
    export ONEHARNESS_BIN="{{ONEHARNESS_BIN}}"
    fails=0
    for h in claude codex opencode goose qwen crush copilot cursor; do
        printf '\n=================== live: %s ===================\n' "$h"
        bash "scripts/e2e-$h.sh" || fails=$((fails + 1))
    done
    printf '\nlive-all: %d harness check(s) failed\n' "$fails"
    exit "$fails"

# Reads the repo-local gh-secrets.json manifest. Needs `gh-secrets` plus its
# stored Bitwarden + GitHub credentials (`gh-secrets auth ...`).
#
# Sync the e2e secrets from Bitwarden to .env + GitHub Actions (gh-secrets.json).
secrets-sync:
    if ! command -v gh-secrets >/dev/null 2>&1; then echo "gh-secrets not installed: see https://github.com/nickderobertis/github-secrets" >&2; exit 1; fi
    gh-secrets manifest sync
