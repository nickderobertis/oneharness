# Canonical command surface for oneharness.
#
# `just bootstrap` must work from a clean clone; `just check` is the full quality
# gate and fails on any issue (no warnings-only mode). Recipes are quiet on
# success and specific on failure.

set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

# Keep shebang recipes in a repository-local executable directory when the
# environment's default tempdir is noexec. Keep this outside Cargo's ignored
# `target/`: release-plz rejects files that are both tracked and ignored.
# llmlint: ignore[changed_behavior_has_e2e] `check` is itself a shebang recipe in the required `check`/`gate` path, so every gate exercises this boundary through Just's real script execution.
set tempdir := ".just-tmp"

# List available recipes.
default:
    @just --list

# Set up from a clean clone: toolchain components + every ecosystem's one
# locked resolve (Cargo, the Bun workspace that also carries Nx, the uv
# workspace under python/), the llmlint toolchain and the pre-push hook. The
# `llvm-tools-preview` component is what `cargo llvm-cov` needs to instrument
# each crate's tests for the coverage floor.
bootstrap:
    rustup component add rustfmt clippy llvm-tools-preview
    cargo fetch --locked
    @just js-install
    uv sync --project python --frozen --no-install-workspace --quiet
    ./scripts/setup-llmlint.sh
    git config core.hooksPath .githooks

# The quality gate, through the Nx project graph. `just check` is the AFFECTED
# tier: every gate target of each project the diff since the base can reach
# (scripts/nx-base.sh: NX_BASE when set — a plain ref name or a commit SHA, and
# nothing else — or the merge base with origin/main). `just check all` is the
# FULL SWEEP: the same targets on every project, with the computation cache
# skipped, so no replayed result can stand in for a clean run. CI picks the tier
# per event (scripts/ci-gate-tier.sh); the release PR is where the sweep runs.
# The targets are the gate's, in the order a contributor wants their verdicts:
# every project declares whichever apply to it, and the live and exploration
# suites declare none of them, so no tier ever runs one.
check tier="affected":
    #!/usr/bin/env bash
    set -euo pipefail
    case {{ quote(tier) }} in
        affected) base="$(bash scripts/nx-base.sh)"; bash scripts/nx affected --base="$base" -t format,lint,typecheck,build,test,e2e,coverage ;;
        all) bash scripts/nx run-many --all --exclude='live-*,explore-*' -t format,lint,typecheck,build,test,e2e,coverage --skip-nx-cache ;;
        *) printf "check: unknown tier '%s' — use 'affected' (the default) or 'all'\n" {{ quote(tier) }} >&2; exit 2 ;;
    esac
    echo "check ({{tier}}): ok"

# Complete pre-push gate: deterministic product/dependency/API checks, followed
# by llmlint validation and its changed-file LLM judge when local credentials
# exist.
gate remote="origin" base="": check deps-check package-crates semver-check
    @comparison=$(scripts/comparison-base.sh "{{remote}}" "{{base}}"); just lint-llm-local "$comparison"

# One gate target over the affected projects (the default) or every project
# (`all`), through the same graph `check` uses. The recipes below are this
# with the target named.
_tier target tier:
    #!/usr/bin/env bash
    set -euo pipefail
    case {{ quote(tier) }} in
        affected) base="$(bash scripts/nx-base.sh)"; exec bash scripts/nx affected --base="$base" -t {{ target }} ;;
        all) exec bash scripts/nx run-many --all --exclude='live-*,explore-*' -t {{ target }} ;;
        *) printf "unknown tier '%s' — use 'affected' (the default) or 'all'\n" {{ quote(tier) }} >&2; exit 2 ;;
    esac

# Every test suite: each project's unit/integration `test` and the SDKs'
# packaged-artifact `e2e`. Scratch-space leaks fail the run (each suite runs
# under scripts/check-temp-leaks.sh).
test tier="affected": (_tier "test,e2e" tier)

# Lint (clippy -D warnings, rustdoc, biome, ruff, shellcheck, the project-graph
# boundaries) and type-check (tsc, mypy).
lint tier="affected": (_tier "lint,typecheck" tier)

# Verify formatting without modifying files.
fmt-check tier="affected": (_tier "format" tier)

# Format the codebase in place, every project (uncached by design).
format:
    bash scripts/nx run-many --all -t format-write

# The coverage floors: Rust's 95% lines over every crate's instrumented run
# (rust-coverage, which runs each Rust `test` first and is skipped on Windows),
# beside the Node SDK's and the Python SDK's own floors in their `test` targets.
coverage tier="affected": (_tier "coverage" tier)

# Browsable Rust coverage report (kept out of the gate): one instrumented run of
# every crate, as the floor measures them, rendered to HTML.
coverage-html:
    cargo llvm-cov nextest --workspace --features oneharness/mock-harness --locked --html
    @echo "report: target/llvm-cov/html/index.html"

# The repository's drift gates alone — the workflow, release, live-helper and
# workspace contracts — linted and run (every target of the four tooling
# projects; what `check` runs of them is decided by what a change reaches).
lint-workflows:
    bash scripts/nx run-many -p ci-contracts,release-tooling,e2e-support,workspace -t lint,test

# Run the binary e2e journeys alone (the oneharness-e2e project's test target).
e2e:
    bash scripts/nx run oneharness-e2e:test

# Package the reusable crate and the binary exactly as Cargo will verify them at
# publish time. It guards a release from the PR that precedes it — `just gate`
# and ci.yml's `package` job — and deliberately NOT from `check`, which
# release.yml runs at the tag: there the binary already pins the core version
# that same run publishes, so this check is structurally red and would take the
# whole distribution down with it. Its own step for the same reason `deps-check`
# is: it needs the network-fetched crates.io index and the release tags.
package-crates:
    @bash scripts/package-crates.sh

# Hermetic end-to-end smoke of the *built* binary (the oneharness-e2e project's
# `test` runs it, through scripts/check-smoke-env.sh, on every platform). Drives
# list/detect/print-command + one mock spawn; no network.
smoke:
    @ONEHARNESS_HARNESSES="codex:undeclared-smoke-sentinel" bash scripts/smoke.sh

# Opt-in live smoke against installed, authenticated harnesses. Makes real (paid)
# model calls and needs network, so it is deliberately out of `check` and CI.
smoke-live:
    bash scripts/smoke.sh --live

# Opt-in live check of `scripts/release-probe.sh` against the real public
# registries: every declared release target answers the version it currently
# serves, a name no registry has served answers with nothing, and a registry the
# probe cannot read is refused. Needs network, so it is out of `check` and CI
# like `smoke-live`; the refusals are held inside the gate by
# scripts/check-release-probe.sh.
release-probe-live:
    @bash scripts/release-probe-live.sh

# Debug build of the binary (the same command as the oneharness project's Nx
# `build`, with the recovery hint a direct caller needs).
build:
    @RUSTFLAGS="-D warnings" cargo build -p oneharness --quiet --locked || { echo "build failed; fix the compiler diagnostics above and rerun 'just build'" >&2; exit 1; }

# Build the provider-process double used by hermetic boundary tests (the
# oneharness-mock-harness project's Nx `build`).
build-mock-harness:
    @RUSTFLAGS="-D warnings" cargo build -p oneharness-mock-harness --quiet --locked || { echo "mock-harness build failed; fix the compiler diagnostics above and rerun 'just build-mock-harness'" >&2; exit 1; }

# Optimized release build (the distributed artifact). Extra args reach cargo,
# e.g. `just build-release --target aarch64-pc-windows-msvc`.
build-release *args:
    @RUSTFLAGS="-D warnings" cargo build --release --locked {{ args }} || { echo "release build failed; fix the compiler diagnostics above and rerun 'just build-release {{ args }}'" >&2; exit 1; }

# Hermetic npm-packaging e2e: assemble the host's per-platform npm package from a
# just-built binary, stage it under the `oneharness-cli` launcher, and prove the
# launcher shim resolves and execs it. The npm-launcher project's `test` runs it
# in the gate; this recipe is the standalone way to iterate on
# scripts/npm-build.mjs. Needs Node.
npm-e2e: build
    @just npm-e2e-bin target/debug/oneharness

# The same e2e against an already-built binary, such as a cross-target release
# build: `just npm-e2e-bin target/<triple>/release/oneharness.exe`.
npm-e2e-bin bin:
    bash scripts/npm-e2e.sh {{ quote(bin) }}

# Advisory + license audit: the repo-level `supply-chain` target (cargo deny +
# cargo machete). Separate from `check`: it needs a network advisory DB.
deps-check:
    if ! command -v cargo-deny >/dev/null 2>&1; then echo "cargo-deny not installed: cargo install cargo-deny --locked" >&2; exit 1; fi
    if ! command -v cargo-machete >/dev/null 2>&1; then echo "cargo-machete not installed: cargo install cargo-machete --locked" >&2; exit 1; fi
    bash scripts/nx run workspace:supply-chain

# Refuse an API break the release-driving commit subject does not declare.
# Separate from `check` for the same reasons `deps-check` is: it resolves the
# published baseline over the network, and it needs a newer rustc than
# rust-toolchain.toml pins (so it names one; set SEMVER_TOOLCHAIN to override).
# Absent tooling skips locally and is red in CI (OH_SEMVER_NO_SKIP=1).
semver-check:
    @bash scripts/semver-check.sh

# Upgrade dependencies in every ecosystem's one lockfile, then re-run the full
# sweep.
upgrade:
    cargo update
    bun update
    uv lock --project python --upgrade
    @just check all

# Regenerate docs/sdk-parity.md from the capability manifest and the SDK sources.
parity-audit:
    @node scripts/parity-audit.mjs
    @echo "parity-audit: docs/sdk-parity.md regenerated"

# Regenerate TypeScript declarations and runtime schemas from Rust wire types.
sdk-generate:
    bun run --cwd npm/oneharness-sdk generate

# Install the Bun workspace (the Node SDK, the npm launcher and Nx itself) from
# the root bun.lock into *this* checkout — gitignored, so anything that needs it
# installs it rather than assuming a bootstrapped caller (scripts/nx does the
# same before every Nx run). Quiet on success; bun's `--silent` would drop
# failure reasons too, hence the capture. Enforced by scripts/check-js-install.sh.
js-install:
    @out=$(bun install --frozen-lockfile 2>&1) || { printf '%s\n' "$out" >&2; echo "Node workspace dependency install failed; the bun output above says why. If a package.json changed, refresh the root bun.lock with 'bun install'; otherwise check network access to the npm registry and rerun 'just js-install'." >&2; exit 1; }

# Compile the Node SDK's publishable `dist/` — the release's spelling of the
# node-sdk project's `build`, which the release runs without Nx because it skips
# the gate when CI already swept the tree. Same command, so they cannot drift.
sdk-build: js-install
    bun run --cwd npm/oneharness-sdk build

# Strict SDK gates, one project at a time: every gate target of the Node SDK
# (generated-contract drift, biome, tsc, the unit suite under its 95% coverage
# threshold, the build, and the packed-artifact e2e) or of the Python SDK
# (generated-contract drift, ruff, mypy on Python 3.9, the unit suite under 95%
# branch-inclusive coverage, and the release-stamped wheel e2e).
sdk-check:
    bash scripts/nx run-many -p node-sdk -t format,lint,typecheck,build,test,e2e

python-sdk-check:
    bash scripts/nx run-many -p python-sdk -t format,lint,typecheck,test,e2e

# Regenerate Python declarations and runtime schemas from Rust wire types.
python-sdk-generate:
    uv sync --project python --frozen --no-install-workspace --quiet
    uv run --project python --frozen --no-sync python python/oneharness-sdk/scripts/generate.py

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
# A green is recorded per (workspace content, resolved base commit, judge config)
# and replayed rather than rolled again — `ONEHARNESS_LLMLINT_REJUDGE=1` forces a
# fresh roll that neither reads nor records one.
lint-llm-local base:
    scripts/local-llmlint-gate.sh "{{base}}"
