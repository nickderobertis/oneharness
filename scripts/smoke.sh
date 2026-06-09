#!/usr/bin/env bash
#
# End-to-end smoke of the *built* oneharness binary — the artifact a user runs,
# not the test-compiled crate. Two modes:
#
#   (default) hermetic — drive the built binary through `list`, `detect --all`,
#       and `run --all --print-command`, then one real spawn+parse against the
#       mock-harness fixture. No network, no auth, fully deterministic. This is
#       what `just check` (and therefore CI) requires on every platform.
#
#   --live             — additionally fire a real prompt at whatever harnesses
#       are installed and authenticated, skipping cleanly when none are. Opt-in:
#       it needs binaries, auth, and network and makes real (paid) model calls,
#       so it is never part of the gate or CI. Run it via `just smoke-live`.
#
# Output is context the next agent reads: near-silent on success (one line),
# and on failure the exact step, command, captured output, and a suggested fix.
set -euo pipefail

LIVE=0
case "${1:-}" in
  --live) LIVE=1 ;;
  "") ;;
  *)
    echo "smoke: unknown argument: $1 (use --live, or no argument)" >&2
    exit 2
    ;;
esac

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

PROMPT="oneharness smoke: reply with the single word pong"
LAST_CMD=""

# Resolve a built binary path, tolerating the Windows `.exe` suffix.
exe_path() {
  if [ -x "$1" ]; then printf '%s' "$1"; return 0; fi
  if [ -x "$1.exe" ]; then printf '%s' "$1.exe"; return 0; fi
  return 1
}

fail() {
  # $1 message, $2 command (optional), $3 captured output (optional), $4 fix (optional)
  echo "smoke: FAIL — $1" >&2
  [ -n "${2:-}" ] && echo "  command: $2" >&2
  if [ -n "${3:-}" ]; then
    echo "  output:" >&2
    printf '%s\n' "$3" >&2
  fi
  [ -n "${4:-}" ] && echo "  fix: $4" >&2
  exit 1
}

assert_contains() {
  # $1 haystack, $2 fixed-string needle, $3 fix hint (optional)
  if ! printf '%s' "$1" | grep -qF -- "$2"; then
    fail "expected output to contain: $2" "$LAST_CMD" "$1" "${3:-}"
  fi
}

count_matches() {
  # Count occurrences of a fixed string; cross-platform, and zero-safe so a
  # no-match grep (exit 1) doesn't trip `set -e`/`pipefail`.
  { printf '%s' "$1" | grep -oF -- "$2" || true; } | wc -l | tr -d '[:space:]'
}

resolve_oneharness() {
  if [ -n "${ONEHARNESS_BIN:-}" ]; then printf '%s' "$ONEHARNESS_BIN"; return 0; fi
  local c
  for c in target/release/oneharness target/debug/oneharness; do
    if p="$(exe_path "$c")"; then printf '%s' "$p"; return 0; fi
  done
  echo "smoke: building oneharness (debug)…" >&2
  cargo build --locked >&2
  exe_path target/debug/oneharness || fail "could not find oneharness after build" \
    "" "" "run 'just build' and retry"
}

resolve_mock() {
  local c
  for c in target/release/oneharness-mock-harness target/debug/oneharness-mock-harness; do
    if p="$(exe_path "$c")"; then printf '%s' "$p"; return 0; fi
  done
  echo "smoke: building mock-harness fixture…" >&2
  cargo build --locked --features mock-harness --bin oneharness-mock-harness >&2
  exe_path target/debug/oneharness-mock-harness || fail \
    "could not find mock-harness fixture after build" \
    "" "" "run 'cargo build --features mock-harness --bin oneharness-mock-harness'"
}

oh="$(resolve_oneharness)"

# 1. `list` — the registry, with each adapter's example command.
LAST_CMD="$oh list --compact"
out="$($oh list --compact)" || fail "list exited non-zero" "$LAST_CMD"
assert_contains "$out" '"schema_version"'
assert_contains "$out" '"claude-code"'
n_list="$(count_matches "$out" '"default_bin"')"
[ "$n_list" -ge 8 ] || fail "list reported $n_list harness(es), expected >= 8" "$LAST_CMD" "$out"

# 2. `detect --all` — probe availability without requiring any to be present.
LAST_CMD="$oh detect --all --compact"
out="$($oh detect --all --compact)" || fail "detect exited non-zero" "$LAST_CMD"
assert_contains "$out" '"detected"'
assert_contains "$out" '"available"'

# 3. `run --all --print-command` — build every adapter's argv, execute nothing.
LAST_CMD="$oh run --all --print-command --prompt <prompt> --compact"
out="$($oh run --all --print-command --prompt "$PROMPT" --compact)" \
  || fail "print-command dry run exited non-zero" "$LAST_CMD"
assert_contains "$out" '"dry_run":true'
assert_contains "$out" '"status":"planned"'
n_run="$(count_matches "$out" '"harness":')"
[ "$n_run" = "$n_list" ] \
  || fail "print-command planned $n_run harness(es) but list has $n_list" "$LAST_CMD" "$out"

# 4. Real spawn + capture + extract, hermetically, via the mock-harness fixture.
#    The mock emits a Claude-shaped result so this also proves the normalized
#    envelope (text, usage, session_id) is lifted out of harness-specific stdout.
mock="$(resolve_mock)"
mock_stdout='{"type":"result","result":"pong","session_id":"smoke-sess","total_cost_usd":0.0012,"usage":{"input_tokens":42,"output_tokens":1}}'
LAST_CMD="ONEHARNESS_BIN_CLAUDE_CODE=$mock MOCK_STDOUT=<claude-json> $oh run --harness claude-code --prompt <prompt> --compact"
out="$(ONEHARNESS_BIN_CLAUDE_CODE="$mock" MOCK_STDOUT="$mock_stdout" \
  "$oh" run --harness claude-code --prompt "$PROMPT" --compact)" \
  || fail "mock run exited non-zero" "$LAST_CMD"
assert_contains "$out" '"status":"ok"' "the mock spawn/parse path is broken"
assert_contains "$out" '"text":"pong"' "json:result extraction is broken"
assert_contains "$out" '"usage_source":"json"' "usage normalization is broken"
assert_contains "$out" '"session_id":"smoke-sess"' "session_id surfacing is broken"

if [ "$LIVE" -eq 0 ]; then
  echo "smoke: ok (hermetic — list, detect, print-command, mock run)"
  exit 0
fi

# --live: exercise the real adapters against installed, authenticated harnesses.
LAST_CMD="$oh detect --all --compact"
det="$($oh detect --all --compact)" || fail "detect exited non-zero" "$LAST_CMD"
if ! printf '%s' "$det" | grep -qF '"available":true'; then
  echo "smoke: ok (hermetic) — no harnesses installed/authenticated, live step skipped"
  exit 0
fi

LAST_CMD="$oh run --all --prompt <prompt> --timeout 90 --compact"
out="$($oh run --all --prompt "$PROMPT" --timeout 90 --compact)" || true
if ! printf '%s' "$out" | grep -qF '"status":"ok"'; then
  fail "no installed harness returned an ok result" "$LAST_CMD" "$out" \
    "check each harness's auth/network; run 'oneharness detect --all' to see availability"
fi
echo "smoke: ok (hermetic + live — at least one installed harness returned ok)"
