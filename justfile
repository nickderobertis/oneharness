# Canonical command surface for oneharness.
#
# `just bootstrap` must work from a clean clone; `just check` is the full quality
# gate and fails on any issue (no warnings-only mode). Recipes are quiet on
# success and specific on failure.

set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

# Keep shebang recipes in a repository-local executable directory when the
# environment's default tempdir is noexec. Keep this outside Cargo's ignored
# `target/`: release-plz rejects files that are both tracked and ignored.
# llmlint: ignore[changed_behavior_has_e2e] `python-sdk-check` is itself a shebang recipe in the required `check`/`gate` path, so every gate exercises this boundary through Just's real script execution.
set tempdir := ".just-tmp"

# Feature that builds the test-only mock harness fixture the e2e tests drive.
FEATURES := "mock-harness"

# Minimum line coverage the gate enforces. The skill's default is 95%; see
# AGENTS.md "Tests are context engineering" for why this is a hard gate.
COVERAGE_MIN := "95"
COVERAGE_LINKER := justfile_directory() / "scripts/coverage-linker.sh"

# List available recipes.
default:
    @just --list

# Set up from a clean clone: toolchain components + fetched dependencies. The
# `llvm-tools-preview` component is what `cargo llvm-cov` needs to instrument the
# build for the coverage gate.
bootstrap:
    rustup component add rustfmt clippy llvm-tools-preview
    cargo fetch --locked
    @just sdk-install
    ./scripts/setup-llmlint.sh
    git config core.hooksPath .githooks

# Full quality gate: format check, lint (Rust + shell), tests *with enforced
# coverage*, build, artifact smoke. Fails on any issue. `coverage` re-runs the
# workspace suite under instrumentation and fails below {{COVERAGE_MIN}}% lines;
# `test` stays in the gate as the fast, un-instrumented pass/fail signal.
check: fmt-check lint lint-sh lint-workflows sdk-check python-sdk-check test coverage build smoke
    @echo "check: ok"

# Complete pre-push gate: deterministic product/dependency checks, followed by
# llmlint validation and its changed-file LLM judge when local credentials exist.
gate remote="origin" base="": check deps-check
    @comparison=$(scripts/comparison-base.sh "{{remote}}" "{{base}}"); just lint-llm-local "$comparison"

# Verify formatting without modifying files.
fmt-check:
    cargo fmt --all -- --check

# Format the codebase in place.
format:
    cargo fmt --all

# Lint with clippy across the workspace (core + binary); any warning is an error.
lint:
    cargo clippy --workspace --all-targets --features {{FEATURES}} -- -D warnings

# Alias for `lint`.
clippy: lint

# Lint shell scripts with shellcheck; any finding is an error. Like `cargo-deny`,
# shellcheck is an external tool: CI installs it, and this prints an install hint
# if it is missing rather than failing cryptically.
lint-sh:
    if ! command -v shellcheck >/dev/null 2>&1; then echo "shellcheck not installed: 'apt-get install shellcheck' / 'brew install shellcheck' / https://github.com/koalaman/shellcheck#installing" >&2; exit 1; fi
    shellcheck scripts/*.sh

# Drift gates for the live-e2e matrix, Rust toolchain, and release lifecycle,
# plus the hermetic behavioral test of the idempotent crates.io publisher.
lint-workflows: build build-mock-harness
    @bash scripts/check-pr-title-e2e.sh >/dev/null
    @bash scripts/check-e2e-matrix.sh >/dev/null
    @bash scripts/check-workflows.sh >/dev/null
    @bash scripts/check-workflows-e2e.sh >/dev/null
    @bash scripts/check-publish-crates.sh >/dev/null
    @bash scripts/check-publish-npm.sh >/dev/null
    @bash scripts/check-local-gate.sh >/dev/null
    @bash scripts/check-sdk-install.sh >/dev/null
    @bash scripts/check-build-mock-harness.sh >/dev/null
    @bash scripts/check-codex-usage-schema.sh >/dev/null
    @bash scripts/check-codex-usage-schema-test.sh >/dev/null
    @bash scripts/check-usage-enforce.sh >/dev/null
    @bash scripts/check-control-probes.sh >/dev/null
    @bash scripts/check-control-probe-http.sh >/dev/null
    @bash scripts/check-control-enforce.sh >/dev/null
    @bash scripts/e2e-variants-test.sh >/dev/null
    @echo 'lint-workflows: ok'

# Run the test suite across the workspace (core unit tests + binary unit and
# integration tests; prefers nextest, falls back to cargo test).
test:
    if command -v cargo-nextest >/dev/null 2>&1; then cargo nextest run --workspace --features {{FEATURES}} --locked; else cargo test --workspace --features {{FEATURES}} --locked; fi

# Run the workspace suite under instrumentation and FAIL if line coverage drops
# below {{COVERAGE_MIN}}%. This is the coverage gate (part of `just check` and
# CI): a behavior the tests never execute is a hole, and the number makes it
# visible. `--workspace` so the `oneharness-core` engine is measured alongside the
# binary. Uses nextest when present (same runner as `test`), else `cargo test`.
# `just coverage-html` writes a browsable report for finding the uncovered lines.
#
# Coverage is a platform-independent property of the test suite, so it is measured
# on Linux/macOS where llvm-cov's instrumentation attributes subprocess-spawned
# binary coverage reliably. On Windows that attribution is broken — the integration
# tests in tests/cli.rs drive the *built* binary as a subprocess, and its profraw
# data is not collected, so the binary crate reads as ~0% there (a tooling
# limitation, not a real gap). The full functional gate (test/build/smoke) still
# runs on Windows; only the coverage measurement is skipped, with a notice.
coverage:
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ "${OS:-}" == "Windows_NT" ]]; then
        echo "coverage: skipped on Windows (llvm-cov subprocess attribution under-reports; measured on Linux/macOS — see justfile)"
        exit 0
    fi
    if command -v cargo-nextest >/dev/null 2>&1; then
        RUSTFLAGS="${RUSTFLAGS:-} -C linker={{COVERAGE_LINKER}}" cargo llvm-cov nextest --workspace --features {{FEATURES}} --locked --fail-under-lines {{COVERAGE_MIN}}
    else
        RUSTFLAGS="${RUSTFLAGS:-} -C linker={{COVERAGE_LINKER}}" cargo llvm-cov --workspace --features {{FEATURES}} --locked --fail-under-lines {{COVERAGE_MIN}}
    fi

# Browsable coverage report (kept out of the gate; opens uncovered lines per file).
coverage-html:
    cargo llvm-cov --workspace --features {{FEATURES}} --locked --html
    @echo "report: target/llvm-cov/html/index.html"

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
    @RUSTFLAGS="-D warnings" cargo build --quiet --locked || { echo "build failed; fix the compiler diagnostics above and rerun 'just build'" >&2; exit 1; }

# Build the provider-process double used by hermetic boundary tests.
build-mock-harness:
    @RUSTFLAGS="-D warnings" cargo build --quiet --locked --features {{FEATURES}} --bin oneharness-mock-harness || { echo "mock-harness build failed; fix the compiler diagnostics above and rerun 'just build-mock-harness'" >&2; exit 1; }

# Optimized release build (the distributed artifact).
build-release:
    cargo build --release --locked

# Hermetic npm-packaging e2e: assemble the host's per-platform npm package from a
# just-built binary, stage it under the `oneharness-cli` launcher, and prove the
# launcher shim resolves and execs it. Runs inside `smoke` (Node-gated) too; this
# recipe is the standalone way to iterate on scripts/npm-build.mjs. Needs Node.
npm-e2e: build
    bash scripts/npm-e2e.sh target/debug/oneharness

# Advisory + license audit. Separate from `check`: needs a network advisory DB.
deps-check:
    if ! command -v cargo-deny >/dev/null 2>&1; then echo "cargo-deny not installed: cargo install cargo-deny --locked" >&2; exit 1; fi
    if ! command -v cargo-machete >/dev/null 2>&1; then echo "cargo-machete not installed: cargo install cargo-machete --locked" >&2; exit 1; fi
    cargo deny check
    cargo machete

# Upgrade dependencies, then re-run the full gate.
upgrade:
    cargo update
    cd npm/oneharness-sdk && bun update
    @just check

# Regenerate TypeScript declarations and runtime schemas from Rust wire types.
sdk-generate:
    bun run --cwd npm/oneharness-sdk generate

# Install the Node SDK's dependencies into *this* checkout — the one per-checkout
# artifact `bootstrap` creates, and gitignored, so `sdk-check` must depend on it
# rather than assume a bootstrapped caller. Runs on every gate, so it stays quiet
# on success; bun's `--silent` would drop failure reasons too, hence the capture.
# Enforced by scripts/check-sdk-install.sh.
sdk-install:
    @out=$(bun install --cwd npm/oneharness-sdk --frozen-lockfile 2>&1) || { printf '%s\n' "$out" >&2; echo "Node SDK dependency install failed; the bun output above says why. If npm/oneharness-sdk/package.json changed, refresh the lockfile with 'bun install --cwd npm/oneharness-sdk'; otherwise check network access to the npm registry and rerun 'just sdk-install'." >&2; exit 1; }

# Strict Node SDK gate, including the Rust->TypeScript drift check and real CLI e2e.
sdk-check: build build-mock-harness sdk-install
    bun run --cwd npm/oneharness-sdk generate:check
    bun run --cwd npm/oneharness-sdk format:check
    bun run --cwd npm/oneharness-sdk lint
    bun run --cwd npm/oneharness-sdk typecheck
    bun run --cwd npm/oneharness-sdk test
    bun run --cwd npm/oneharness-sdk build
    bun run --cwd npm/oneharness-sdk test:package

# Regenerate Python declarations and runtime schemas from Rust wire types.
python-sdk-generate:
    uv run --no-project --python 3.9 --with-requirements python/oneharness-sdk/requirements-dev.txt python python/oneharness-sdk/scripts/generate.py

# Strict Python SDK gate on the oldest supported interpreter, including generated
# contract drift, branch-aware coverage, and a release-stamped wheel exercised
# through its installed public import against the real CLI subprocess.
python-sdk-check:
    #!/usr/bin/env bash
    set -euo pipefail
    log=$(mktemp)
    trap 'rm -f "$log"' EXIT
    if ! (
        set -euo pipefail
        if ! command -v uv >/dev/null 2>&1; then
            echo "uv not installed: https://docs.astral.sh/uv/getting-started/installation/" >&2
            exit 1
        fi
        cargo build --locked --features {{FEATURES}} --bins
        run=(uv run --no-project --python 3.9 --with-requirements python/oneharness-sdk/requirements-dev.txt)
        "${run[@]}" python python/oneharness-sdk/scripts/generate.py --check
        "${run[@]}" ruff format --check python/oneharness-sdk
        "${run[@]}" ruff check python/oneharness-sdk
        "${run[@]}" mypy --config-file python/oneharness-sdk/pyproject.toml python/oneharness-sdk/src python/oneharness-sdk/scripts python/oneharness-sdk/test
        rm -f target/python-sdk.coverage
        COVERAGE_FILE=target/python-sdk.coverage PYTHONPATH=python/oneharness-sdk/src "${run[@]}" coverage run --rcfile=python/oneharness-sdk/pyproject.toml -m unittest discover -s python/oneharness-sdk/test -p 'test_*.py'
        COVERAGE_FILE=target/python-sdk.coverage "${run[@]}" coverage report --rcfile=python/oneharness-sdk/pyproject.toml
        "${run[@]}" python python/oneharness-sdk/test/package_e2e.py
    ) >"$log" 2>&1; then
        cat "$log" >&2
        exit 1
    fi
    echo "python-sdk-check: ok"

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

ONEHARNESS_LIVE_INSTALL_DIR := justfile_directory() / "target/e2e-install/bin"
ONEHARNESS_BIN := ONEHARNESS_LIVE_INSTALL_DIR / "oneharness"

# Build and install the release binary through scripts/install.sh, so the live
# scripts drive the same installed shape users get from a GitHub Release.
_live-install:
    cargo build --release --locked
    bash scripts/install-e2e.sh "target/release/oneharness" "{{ONEHARNESS_LIVE_INSTALL_DIR}}"

live-claude: _live-install
    ONEHARNESS_BIN="{{ONEHARNESS_BIN}}" bash scripts/e2e-claude.sh

live-codex: _live-install
    ONEHARNESS_BIN="{{ONEHARNESS_BIN}}" bash scripts/e2e-codex.sh

live-opencode: _live-install
    ONEHARNESS_BIN="{{ONEHARNESS_BIN}}" bash scripts/e2e-opencode.sh

live-goose: _live-install
    ONEHARNESS_BIN="{{ONEHARNESS_BIN}}" bash scripts/e2e-goose.sh

live-qwen: _live-install
    ONEHARNESS_BIN="{{ONEHARNESS_BIN}}" bash scripts/e2e-qwen.sh

live-crush: _live-install
    ONEHARNESS_BIN="{{ONEHARNESS_BIN}}" bash scripts/e2e-crush.sh

live-copilot: _live-install
    ONEHARNESS_BIN="{{ONEHARNESS_BIN}}" bash scripts/e2e-copilot.sh

live-cursor: _live-install
    ONEHARNESS_BIN="{{ONEHARNESS_BIN}}" bash scripts/e2e-cursor.sh

# Per-FEATURE live check (not a harness): drive a real harness through
# `run --schema` and assert a schema-valid structured round-trip — the drift
# alarm for Claude Code's native `--json-schema` delivery. See scripts/e2e-schema.sh.
live-schema: _live-install
    ONEHARNESS_BIN="{{ONEHARNESS_BIN}}" bash scripts/e2e-schema.sh

# Per-FEATURE live check (not a harness): drive each control-capable harness
# through `run --control` + a separate `oneharness interrupt` and assert the
# real turn's work actually stops. Deliberately out of the per-harness suites:
# every phase drives a multi-step turn then waits out a 15s freeze window, which
# is far too slow for a per-PR job. See scripts/e2e-control.sh.
live-control: _live-install
    ONEHARNESS_BIN="{{ONEHARNESS_BIN}}" bash scripts/e2e-control.sh # llmlint: ignore[tool_output_is_signal] Like every other `live-*` recipe this forwards a live e2e transcript: the phase lines are what attribute a failure — or a hang inside a 15s freeze window — to a step in a log nobody can attach a debugger to. The script owns that contract (see its file-level ignore).

live-variants: _live-install
    ONEHARNESS_BIN="{{ONEHARNESS_BIN}}" bash scripts/e2e-variants.sh

# Install the real CLIs required by the harness-variant live test.
live-variants-tools:
    @log="$(mktemp)"; if npm install -g @anthropic-ai/claude-code@2.1.220 @openai/codex@0.145.0 opencode-ai@1.18.5 @qwen-code/qwen-code@0.21.0 @charmland/crush@0.87.0 >"$log" 2>&1; then rm -f "$log"; echo "live-variants-tools: installed Claude Code, Codex, OpenCode, Qwen Code, and Crush"; else cat "$log" >&2; rm -f "$log"; echo "live-variants-tools: npm install failed; rerun 'just live-variants-tools', then run 'npm config get registry' and verify that registry URL is reachable if it fails again" >&2; exit 1; fi

# Run every per-harness live check plus the per-feature ones; skips count as
# passes, only real failures fail.
live-all: _live-install
    #!/usr/bin/env bash
    set -uo pipefail
    export ONEHARNESS_BIN="{{ONEHARNESS_BIN}}"
    fails=0
    for h in claude codex opencode goose qwen crush copilot cursor schema variants; do
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
    gh-secrets sync

# Install/refresh the optional llmlint toolchain. Idempotent.
setup-llmlint:
    ./scripts/setup-llmlint.sh

# Optional LLM-as-judge lint; non-deterministic and out of `check`.
lint-llm *paths:
    PATH="$HOME/.local/bin:$PATH"; export PATH; if ! command -v llmlint >/dev/null 2>&1; then echo "llmlint not installed: run 'just setup-llmlint'" >&2; exit 1; fi; llmlint {{paths}}

# Deterministic llmlint config/ignore/version-bump validation.
lint-llm-validate *args:
    PATH="$HOME/.local/bin:$PATH"; export PATH; if ! command -v llmlint >/dev/null 2>&1; then echo "llmlint not installed: run 'just setup-llmlint'" >&2; exit 1; fi; llmlint validate {{args}}

# llmlint scoped to changed files since the merge-base with main.
lint-llm-diff base="origin/main" *args:
    PATH="$HOME/.local/bin:$PATH"; export PATH; if ! command -v llmlint >/dev/null 2>&1; then echo "llmlint not installed: run 'just setup-llmlint'" >&2; exit 1; fi; llmlint --diff --diff-base "{{base}}" {{args}}

# Local complete-gate tier. Validation is model-free; the judge uses an API key
# bootstrap when provided, otherwise the committed authenticated fallback chain.
lint-llm-local base:
    scripts/local-llmlint-gate.sh "{{base}}"
