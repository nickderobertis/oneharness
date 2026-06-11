# shellcheck shell=bash
# Shared helpers for oneharness's live e2e checks (scripts/e2e-<harness>.sh).
#
# These are the opt-in, network-bound counterpart to the hermetic `tests/cli.rs`
# suite: instead of a mock fixture, each script drives a *real* harness CLI
# through the real `oneharness` binary and asserts on the JSON report — proving
# the adapter's argv actually works end to end against the live tool, the
# process envelope is reported, and `text` extraction lands.
#
# The contract under test is oneharness's own stdout report (see README and
# src/domain/report.rs). A script:
#   1. picks a high-entropy MARKER the model cannot reproduce from memory,
#   2. asks the harness (via oneharness) to echo exactly that marker,
#   3. asserts status=ok / exit_code=0 and that the marker surfaced in the
#      harness output — so a pass means the model genuinely ran end to end,
#      not merely that the process exited cleanly.
#
# A missing harness or a missing oneharness binary is a SKIP, never a failure —
# the same "absence is data, not a crash" stance the tool itself takes.
#
# Sourced by each script AFTER it sets `set -euo pipefail`. Requires `jq`.

# Repo root, regardless of where the script is invoked from.
OH_REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# --- reporting -------------------------------------------------------------

note() { printf '%s\n' "$*" >&2; }
fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }
skip() { printf 'SKIP: %s\n' "$*" >&2; exit 0; }

# A required tool's absence is a SKIP, not a failure (e.g. no jq on the box).
need() { command -v "$1" >/dev/null 2>&1 || skip "required tool not found: $1"; }

# At least one of the named env vars must be non-empty, else SKIP. Used for the
# local auth preflight; CI verifies the secret up front and hard-fails instead.
need_env() {
    local label="$1"
    shift
    local v
    for v in "$@"; do
        [ -n "${!v:-}" ] && return 0
    done
    skip "no $label configured (set one of: $*)"
}

# --- locating oneharness ---------------------------------------------------

# Resolve the oneharness binary to drive the harnesses with. Honors
# $ONEHARNESS_BIN, then PATH, then a local debug/release build. Empty if none.
oh_bin() {
    if [ -n "${ONEHARNESS_BIN:-}" ]; then
        printf '%s' "$ONEHARNESS_BIN"
        return
    fi
    if command -v oneharness >/dev/null 2>&1; then
        printf 'oneharness'
        return
    fi
    local cand
    for cand in "$OH_REPO_ROOT/target/release/oneharness" "$OH_REPO_ROOT/target/debug/oneharness"; do
        [ -x "$cand" ] && {
            printf '%s' "$cand"
            return
        }
    done
    printf ''
}

# --- driving a harness -----------------------------------------------------

# A high-entropy marker the model cannot reproduce from memory, so its presence
# in the output proves this run produced it.
oh_marker() { printf 'ONEHARNESS-LIVE-%s%s%s' "${RANDOM}" "${RANDOM}" "${RANDOM}"; }

# The echo prompt: ask the harness to emit exactly the marker and nothing else.
oh_prompt() {
    printf 'You are a non-interactive test fixture. Output exactly the following token on a single line, with no explanation, no quotes, and no code fences: %s' "$1"
}

# Run one prompt through oneharness against a real harness. Stores the JSON
# report in $OH_REPORT. Honors $OH_TIMEOUT (default 120) and, when set,
# $OH_MODEL (passed as --model). Any extra args are forwarded to `oneharness
# run` verbatim. Provider keys reach the harness via the inherited environment.
OH_REPORT=""
oh_run() {
    local id="$1" prompt="$2"
    shift 2
    local bin
    bin="$(oh_bin)"
    [ -n "$bin" ] || skip "oneharness binary not found (build it: \`just build-release\`, or set ONEHARNESS_BIN)"

    local model_args=()
    [ -n "${OH_MODEL:-}" ] && model_args+=(--model "$OH_MODEL")

    local errf
    errf="$(mktemp)"
    note "  driving: $bin run --harness $id (timeout ${OH_TIMEOUT:-120}s${OH_MODEL:+, model $OH_MODEL})"
    OH_REPORT="$("$bin" run --harness "$id" --prompt "$prompt" \
        --timeout "${OH_TIMEOUT:-120}" --compact \
        "${model_args[@]+"${model_args[@]}"}" "$@" 2>"$errf")" || true

    if [ -z "$OH_REPORT" ]; then
        note "  oneharness emitted no JSON on stdout. Its stderr:"
        sed 's/^/    /' "$errf" >&2 || true
        rm -f "$errf"
        fail "oneharness produced no report for $id (is the binary the right version?)"
    fi
    rm -f "$errf"
}

# Read a field from the stored report.
oh_field() { printf '%s' "$OH_REPORT" | jq -r "$1"; }

# Pretty-print the result entry plus stdout/stderr tails on failure, so a CI log
# shows exactly what the harness did.
oh_dump() {
    note "  ── oneharness result ──"
    printf '%s' "$OH_REPORT" \
        | jq '.results[0] | {harness, available, status, exit_code, duration_ms, text_source, error}' >&2 2>/dev/null \
        || printf '%s\n' "$OH_REPORT" >&2
    note "  ── harness stdout tail ──"
    oh_field '.results[0].stdout // ""' | tail -30 | sed 's/^/    /' >&2 || true
    note "  ── harness stderr tail ──"
    oh_field '.results[0].stderr // ""' | tail -20 | sed 's/^/    /' >&2 || true
}

# The shared conclusion for every harness. Given the harness id and the marker
# planted in the prompt, assert oneharness reported a clean run AND the marker
# surfaced in the harness output. Treats an uninstalled harness as a SKIP.
oh_assert_echoed() {
    local id="$1" marker="$2"
    local status available exit_code source

    status="$(oh_field '.results[0].status')"
    available="$(oh_field '.results[0].available')"

    if [ "$status" = "skipped" ] || [ "$available" != "true" ]; then
        skip "$id is not installed (oneharness reported status=$status); nothing to verify"
    fi

    exit_code="$(oh_field '.results[0].exit_code')"
    source="$(oh_field '.results[0].text_source')"

    if [ "$status" != "ok" ]; then
        oh_dump
        fail "$id did not run cleanly: status=$status, exit_code=$exit_code"
    fi
    note "  ok: $id ran and exited cleanly (status=ok, exit_code=0, duration=$(oh_field '.results[0].duration_ms')ms)"

    # The marker must appear in the harness's own output — its normalized text
    # or, failing that, the raw stdout. This is what makes the check meaningful:
    # exit 0 alone could be an empty turn; the marker proves the model ran.
    if printf '%s' "$OH_REPORT" | jq -e --arg m "$marker" \
        '.results[0] | ((.text // "") + "\n" + (.stdout // "")) | contains($m)' >/dev/null; then
        if [ "$source" != "null" ]; then
            note "  confirmed: marker surfaced; oneharness extracted text via '$source'"
        else
            note "  confirmed: marker surfaced in raw stdout (oneharness left text null for this format)"
        fi
    else
        oh_dump
        fail "$id ran but the unique marker never surfaced — the model did not echo it back"
    fi

    note "PASS: $id live e2e"
}
