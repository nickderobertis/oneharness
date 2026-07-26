#!/usr/bin/env bash
# Hermetic boundary coverage for e2e-variants.sh; no provider API is contacted.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/bin"
mkdir -p "$tmp/live"

fail() {
    echo "e2e-variants-test: $1" >&2
    exit 1
}

assert_contains() {
    local file="$1" pattern="$2" description="$3"
    grep -q "$pattern" "$file" ||
        fail "$description; inspect the captured output by running: bash -x scripts/e2e-variants-test.sh"
}

oh="$root/target/debug/oneharness"
mock="$root/target/debug/oneharness-mock-harness"
[ -x "$oh" ] || fail "built oneharness binary is missing; run 'cargo build' and retry"
[ -x "$mock" ] ||
    fail "provider subprocess double is missing; run 'cargo build --features mock-harness --bin oneharness-mock-harness' and retry"

# The real oneharness process launches these provider-boundary adapters. Each
# adapter delegates to the repository's shipped subprocess double with the
# provider's real output shape, derived from the prompt oneharness delivered.
for tool in claude codex; do
    cp "$root/scripts/e2e-variants-provider-double.sh" "$tmp/bin/$tool"
    chmod +x "$tmp/bin/$tool"
done

common_env=(
    PATH="$tmp/bin:$PATH"
    ONEHARNESS_BIN="$oh"
    OH_E2E_PROVIDER_DOUBLE="$mock"
    TMPDIR="$tmp/live"
    ANTHROPIC_API_KEY="test-anthropic-material"
    OPENAI_API_KEY="test-openai-material"
    OH_E2E_VARIANTS_API_ONLY=1
)

evidence="$tmp/evidence.log"
env "${common_env[@]}" OH_E2E_EVIDENCE_FILE="$evidence" \
    bash "$root/scripts/e2e-variants.sh" >"$tmp/success.out" 2>"$tmp/success.err"
assert_contains "$evidence" 'IDENTITY claude-code:apikey' \
    "successful run omitted Claude API identity evidence"
assert_contains "$evidence" 'ASSERT fallback: first_failure_kind=auth' \
    "successful run omitted same-harness fallback evidence"
assert_contains "$tmp/success.err" 'live variants: ok' \
    "successful run omitted its completion diagnostic"

if env "${common_env[@]}" OH_E2E_EVIDENCE_FILE="$tmp/bin" \
    bash "$root/scripts/e2e-variants.sh" >"$tmp/path.out" 2>"$tmp/path.err"; then
    fail "directory evidence target unexpectedly succeeded"
fi
assert_contains "$tmp/path.err" 'remove the non-file target or choose a new file path' \
    "invalid evidence target omitted its recovery action"

if env "${common_env[@]}" OH_E2E_DOUBLE_VALID_FAILURE=1 \
    bash "$root/scripts/e2e-variants.sh" >"$tmp/report.out" 2>"$tmp/report.err"; then
    fail "nonzero provider report unexpectedly succeeded"
fi
assert_contains "$tmp/report.err" 'exited nonzero (status=nonzero exit_code=7' \
    "valid nonzero report omitted its structured status diagnostic"

if env "${common_env[@]}" OH_E2E_DOUBLE_INVALID_REPORT=1 \
    bash "$root/scripts/e2e-variants.sh" >"$tmp/failure.out" 2>"$tmp/failure.err"; then
    fail "invalid provider report unexpectedly succeeded"
fi
assert_contains "$tmp/failure.err" 'before producing a valid report' \
    "invalid provider report omitted its exit diagnostic"

if env "${common_env[@]}" OH_E2E_VARIANTS_API_ONLY= \
    OH_CLAUDE_CONFIG_A="$tmp/missing" OH_CLAUDE_CONFIG_B="$tmp" \
    bash "$root/scripts/e2e-variants.sh" >"$tmp/config.out" 2>"$tmp/config.err"; then
    fail "missing Claude config directory unexpectedly succeeded"
fi
assert_contains "$tmp/config.err" 'must name an existing directory' \
    "missing Claude config directory omitted its validation diagnostic"

if env "${common_env[@]}" OH_E2E_VARIANTS_API_ONLY= \
    OH_CLAUDE_CONFIG_A=$'bad\npath' OH_CLAUDE_CONFIG_B="$tmp" \
    bash "$root/scripts/e2e-variants.sh" >"$tmp/config.out" 2>"$tmp/config.err"; then
    fail "multiline Claude config directory unexpectedly succeeded"
fi
assert_contains "$tmp/config.err" 'must be a non-empty single-line directory path' \
    "multiline Claude config directory omitted its validation diagnostic"

echo "e2e-variants-test: ok"
