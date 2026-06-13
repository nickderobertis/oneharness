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
# crates/oneharness-core/src/domain/report.rs). A script:
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
    # ONEHARNESS_NO_CONFIG=1: a live check pins its own model/timeout; the
    # machine's oneharness config files must not reshape the invocation.
    OH_REPORT="$(ONEHARNESS_NO_CONFIG=1 "$bin" run --harness "$id" --prompt "$prompt" \
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
#
# Optional third arg: an expected `text_source`. When given, the check is
# strengthened from "marker surfaced in text-or-stdout" to "oneharness extracted
# a normalized `text` via exactly this method, and the marker is in that `text`"
# — i.e. the convenience field is proven, not just the raw-stdout fallback. Pass
# it for a harness whose extraction we guarantee (e.g. opencode → json:opencode-parts).
oh_assert_echoed() {
    local id="$1" marker="$2" expected_source="${3:-}"
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

    # When the caller guarantees a normalized-text method for this harness, hold
    # extraction to it: the right `text_source`, and the marker in `.text` itself
    # (not merely in the raw stdout the previous block would also accept).
    if [ -n "$expected_source" ]; then
        if [ "$source" != "$expected_source" ]; then
            oh_dump
            fail "$id: expected text_source=$expected_source but got '$source' — normalized text extraction regressed"
        fi
        if ! printf '%s' "$OH_REPORT" | jq -e --arg m "$marker" \
            '(.results[0].text // "") | contains($m)' >/dev/null; then
            oh_dump
            fail "$id: marker is absent from the normalized .text (only surfaced via raw stdout)"
        fi
        note "  confirmed: oneharness extracted .text via '$source' and the marker is in it"
    fi

    note "PASS: $id live e2e"
}

# --- sync enforcement --------------------------------------------------------

# One phase of the sync-enforcement check: plant a oneharness.toml in a fresh
# sandbox, `oneharness sync` it into the harness's OWN config file, then ask
# the harness (via `oneharness run --no-bypass`) to run `touch <file>` and
# assert the file's presence or absence. This is the live proof that a synced
# policy is actually honored by the real CLI — the hermetic suite can only
# prove the file was written correctly.
#
#   $1 harness id   $2 oneharness.toml content   $3 file the prompt asks for
#   $4 expectation: present|absent               $5 phase label (for logs)
#
# The `present` phase is the positive control: without it, `absent` would pass
# vacuously on a harness that can't execute anything headlessly.
oh_sync_enforce() {
    local id="$1" toml="$2" file="$3" expect="$4" label="$5"
    local bin sandbox out
    bin="$(oh_bin)"
    [ -n "$bin" ] || skip "oneharness binary not found (build it: \`just build-release\`, or set ONEHARNESS_BIN)"

    sandbox="$(mktemp -d)"
    printf '%s\n' "$toml" > "$sandbox/oneharness.toml"

    note "  enforce[$label]: syncing policy into $id's own config file"
    # --config pins the exact planted file (no discovery), and the explicit
    # empty ONEHARNESS_NO_CONFIG guards against a hermetic wrapper's env.
    if ! out="$(ONEHARNESS_NO_CONFIG='' "$bin" sync --harness "$id" \
        --cwd "$sandbox" --config "$sandbox/oneharness.toml" --compact 2>&1)"; then
        printf '%s\n' "$out" >&2
        rm -rf "$sandbox"
        fail "$id: oneharness sync failed ($label phase)"
    fi
    if ! printf '%s' "$out" | grep -qF '"status":"created"'; then
        printf '%s\n' "$out" >&2
        rm -rf "$sandbox"
        fail "$id: sync did not create the harness config file ($label phase)"
    fi

    local prompt
    prompt="You are a non-interactive test fixture in a scratch directory. Execute exactly this shell command in the current directory, then stop: touch $file
Rules: you MUST actually invoke your shell tool with that exact command — never assume or decide on your own that it is not permitted; attempt it. Use only the shell tool. Only if that tool invocation itself fails or is rejected: do NOT create the file by any other means (no file-write or edit tools) — reply with the single word DENIED and stop."
    oh_run "$id" "$prompt" --no-bypass --cwd "$sandbox"

    local status
    status="$(oh_field '.results[0].status')"
    if [ "$status" = "skipped" ]; then
        rm -rf "$sandbox"
        skip "$id is not installed (oneharness reported status=skipped); nothing to verify"
    fi

    case "$expect" in
        present)
            if [ -e "$sandbox/$file" ]; then
                note "  ok[$label]: the synced allow rule let the command run under --no-bypass"
            else
                oh_dump
                rm -rf "$sandbox"
                fail "$id: 'touch $file' did not run despite the synced allow rule (status=$status) — the $label phase is the positive control, so either the synced file is not honored or the rule syntax drifted"
            fi
            ;;
        absent)
            if [ -e "$sandbox/$file" ]; then
                oh_dump
                rm -rf "$sandbox"
                fail "$id: 'touch $file' executed DESPITE the synced deny policy — enforcement is broken"
            fi
            note "  ok[$label]: the denied command did not execute"
            ;;
        *)
            rm -rf "$sandbox"
            fail "oh_sync_enforce: bad expectation '$expect' (use present|absent)"
            ;;
    esac
    rm -rf "$sandbox"
}

# A shell-safe scratch file name for the enforcement phases.
oh_enforce_file() { printf '%s-%s%s.txt' "$1" "${RANDOM}" "${RANDOM}"; }
