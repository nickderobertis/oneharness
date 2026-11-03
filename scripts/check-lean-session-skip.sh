#!/usr/bin/env bash
# Hermetic check for how oh_lean_session_enforce (scripts/e2e-lib.sh), the live
# lean-session phase, reads a skipped first turn. A harness that is not
# installed is a skip; a harness oneharness refused for the run (an
# `unsupported_input` refusal of the attached image) is a failure that prints
# the classified cause, never a skip that hides it as a missing install.
# Both reports come from this checkout's built oneharness: an ONEHARNESS_BIN
# wrapper rewrites only the `--harness` the phase passes, to a codex binary
# that does not exist (not installed) or to qwen driven by the mock harness
# (qwen has no image input, so oneharness refuses the run before it starts).
# Every other argument the phase builds reaches the real binary unchanged. A
# third case drives the phase through a stand-in oneharness whose compaction
# fails, and holds the phase to leaving no scratch space outside its sandbox.
# llmlint: ignore-file[e2e_not_mocked] This drives the live suite's own phase and the real built oneharness CLI; only which harness that CLI is pointed at is rewritten, because what is under test is how the phase classifies oneharness's real refusal and not-installed reports, and no installed harness can be made to refuse its image on demand. scripts/e2e-claude.sh and scripts/e2e-codex.sh run the same phase against the real harnesses.
set -euo pipefail

case $(uname -s) in
  MINGW* | MSYS* | CYGWIN*)
    echo "check-lean-session-skip: skipped on Windows because this Unix behavioral harness relies on an extensionless executable wrapper" >&2
    exit 0
    ;;
esac

fail() {
  echo "check-lean-session-skip: $1" >&2
  echo "  Re-run with: bash -x scripts/check-lean-session-skip.sh" >&2
  exit 1
}

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)" ||
  fail "could not resolve the repository root from ${BASH_SOURCE[0]}; run this script by its path inside a readable checkout"

command -v jq >/dev/null 2>&1 || fail "jq is not installed; the live helpers parse oneharness's report with it"
oh="$root/target/debug/oneharness"
mock="$root/target/debug/oneharness-mock-harness"
[ -x "$oh" ] || fail "the built oneharness binary is missing at $oh; run 'just build' and retry"
[ -x "$mock" ] || fail "the built mock harness is missing at $mock; run 'just build-mock-harness' and retry"

tmp="$(mktemp -d "${TMPDIR:-/tmp}/tmp.XXXXXXXXXX")" ||
  fail "could not create a scratch directory under ${TMPDIR:-/tmp}; check that it exists and is writable, or set TMPDIR to one that is"
trap 'rm -rf "$tmp" || echo "check-lean-session-skip: could not remove $tmp; delete it by hand" >&2' EXIT

mkdir -p "$tmp/bin" "$tmp/sandboxes" || fail "could not create directories under $tmp; check that it is writable"
cat >"$tmp/bin/oneharness" <<'WRAPPER' || fail "could not write the oneharness wrapper under $tmp/bin; check that it is writable"
#!/usr/bin/env bash
set -euo pipefail
args=()
while [ "$#" -gt 0 ]; do
  case "$1" in
    --harness)
      args+=(--harness "$LEAN_TEST_HARNESS" --bin "$LEAN_TEST_HARNESS=$LEAN_TEST_HARNESS_BIN")
      shift 2
      ;;
    *)
      args+=("$1")
      shift
      ;;
  esac
done
exec "$LEAN_TEST_ONEHARNESS" "${args[@]}"
WRAPPER
chmod +x "$tmp/bin/oneharness" || fail "could not make $tmp/bin/oneharness executable"

# A stand-in oneharness for the failed-compaction case, which no real harness
# can be made to produce on demand: both lean turns answer `ok` with a claude
# stream that loads only Bash and Read (the second one compacting), and
# `compact` answers `failed` with the harness's error on stderr, exit 1.
cat >"$tmp/bin/oneharness-failing-compact" <<'STUB' || fail "could not write the failing-compact stub under $tmp/bin; check that it is writable"
#!/usr/bin/env bash
set -euo pipefail
case "$1" in
  run)
    stream='{"type":"system","subtype":"init","tools":["Bash","Read"],"mcp_servers":[],"skills":[],"slash_commands":[]}'
    for a in "$@"; do
      [ "$a" = --auto-compact-tokens ] && stream="$stream"$'\n''{"type":"system","status":"compacting"}'
    done
    jq -nc --arg stdout "$stream" '{results: [{harness: "claude-code", available: true, status: "ok",
      text: "red", calls: [{input_tokens: 1, output_tokens: 1}], stdout: $stdout, diagnostics: []}]}'
    ;;
  compact)
    echo "claude-code compaction failed: harness exited 1" >&2
    printf '%s\n' '{"harness":"claude-code","status":"failed","error":"harness exited 1"}'
    exit 1
    ;;
esac
STUB
chmod +x "$tmp/bin/oneharness-failing-compact" || fail "could not make $tmp/bin/oneharness-failing-compact executable"

# Runs the phase against one rewritten harness; sets `out` to its stderr and
# `rc` to its exit status.
#   $1 the harness the wrapper points oneharness at, $2 that harness's binary,
#   $3 the phase's harness id (default codex), $4 the oneharness the phase
#   drives (default the --harness-rewriting wrapper)
run_phase() {
  rc=0
  out="$(
    env -u OH_E2E_NO_SKIP -u OH_E2E_TELEMETRY_FILE \
      TMPDIR="$tmp/sandboxes" ONEHARNESS_BIN="${4:-$tmp/bin/oneharness}" \
      LEAN_TEST_ONEHARNESS="$oh" LEAN_TEST_HARNESS="$1" LEAN_TEST_HARNESS_BIN="$2" \
      bash -c "set -euo pipefail; source '$root/scripts/e2e-lib.sh'; oh_lean_session_enforce ${3:-codex}" 2>&1 >/dev/null
  )" || rc=$?
}

# No scratch file or directory the phase made under TMPDIR survives it.
#   $1 what the phase just did, for the failure message
assert_no_leftovers() {
  local leftover
  leftover="$(find "$tmp/sandboxes" -mindepth 1 -maxdepth 1)" ||
    fail "could not list $tmp/sandboxes to check the phase removed its scratch space"
  [ -z "$leftover" ] || fail "the phase left scratch space behind after $1: $leftover"
}

# A harness that is not installed: the phase skips, naming the cause.
run_phase codex "$tmp/missing/codex"
[ "$rc" -eq 0 ] || fail "a harness that is not installed must skip the phase (exit 0), got exit $rc. Its output:
$out"
case "$out" in
  *"SKIP: codex is not installed"*) ;;
  *) fail "a harness that is not installed must skip with its cause named; the phase printed:
$out" ;;
esac

# An installed harness oneharness refused for the run: the phase fails, prints
# the report and names the classified cause, and does not skip.
run_phase qwen "$mock"
[ "$rc" -eq 1 ] || fail "an installed harness oneharness refused (unsupported_input) must fail the phase (exit 1), got exit $rc. Its output:
$out"
case "$out" in
  *"SKIP:"*) fail "a refused harness must not be reported as a skip; the phase printed:
$out" ;;
esac
case "$out" in
  *"FAIL: codex: oneharness refused the lean turn (failure_kind=unsupported_input)"*) ;;
  *) fail "the failure must name the classified cause failure_kind=unsupported_input; the phase printed:
$out" ;;
esac
case "$out" in
  *"has no image input"*) ;;
  *) fail "the failure must print the report, whose error says the harness has no image input; the phase printed:
$out" ;;
esac
assert_no_leftovers "a skipped and a refused turn"

# A compaction that fails: the phase fails, prints oneharness's own stderr and
# outcome, and leaves no scratch file outside the sandbox it removes.
run_phase claude-code "$mock" claude-code "$tmp/bin/oneharness-failing-compact"
[ "$rc" -eq 1 ] || fail "a failed compaction must fail the phase (exit 1), got exit $rc. Its output:
$out"
case "$out" in
  *"FAIL: claude-code: oneharness compact did not complete"*) ;;
  *) fail "a failed compaction must fail naming oneharness compact; the phase printed:
$out" ;;
esac
case "$out" in
  *"claude-code compaction failed: harness exited 1"*) ;;
  *) fail "the failure must replay oneharness compact's own stderr; the phase printed:
$out" ;;
esac
assert_no_leftovers "a failed compaction"
