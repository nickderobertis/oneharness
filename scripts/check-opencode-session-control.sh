#!/usr/bin/env bash
# Hermetic check for the live OpenCode session_id phase (scripts/e2e-opencode.sh):
# its positive control retries a turn that never invoked the shell tool, fails
# without a retry when the agent attempted the touch and no file appeared, and
# a failure dumps that phase's own turn rather than an earlier phase's report.
# The real built oneharness syncs the gate and drives an opencode double that
# answers in OpenCode's JSON event shape; the double honors the synced gate by
# reading its deny needle, since the plugin runtime is what it stands in for.
# llmlint: ignore-file[e2e_not_mocked] This drives the real built oneharness CLI (sync and run) and the live suite's own oh_opencode_session_id_forwarded; only the paid opencode CLI is replaced, because what is under test is the phase's retry and verdict on each kind of turn, not the model. live-opencode remains the real-harness drift alarm for the same function.
set -euo pipefail

case $(uname -s) in
  MINGW* | MSYS* | CYGWIN*)
    echo "check-opencode-session-control: skipped on Windows because this Unix behavioral harness relies on an extensionless executable double" >&2
    exit 0
    ;;
esac

fail() {
  echo "check-opencode-session-control: $1" >&2
  echo "  Re-run with: bash -x scripts/check-opencode-session-control.sh" >&2
  exit 1
}

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)" ||
  fail "could not resolve the repository root from ${BASH_SOURCE[0]}; run this script by its path inside a readable checkout"

command -v jq >/dev/null 2>&1 || fail "jq is not installed; the live helpers parse oneharness's report with it"
oh="$root/target/debug/oneharness"
[ -x "$oh" ] || fail "the built oneharness binary is missing at $oh; run 'just build' and retry"

tmp="$(mktemp -d "${TMPDIR:-/tmp}/tmp.XXXXXXXXXX")" ||
  fail "could not create a scratch directory under ${TMPDIR:-/tmp}; check that it exists and is writable, or set TMPDIR to one that is"
trap 'rm -rf "$tmp" || echo "check-opencode-session-control: could not remove $tmp; delete it by hand" >&2' EXIT

mkdir -p "$tmp/bin" "$tmp/sandboxes" || fail "could not create the scratch layout under $tmp; check that it is writable"
# The double's behavior per launch, from SESSION_DOUBLE_PLAN (one word per
# launch, the last repeating): `refuse` answers without a tool call, `touch`
# runs the touch unless the synced gate denies on session_id, and `attempt`
# reports the touch as a tool call without creating the file.
cat >"$tmp/bin/opencode" <<'DOUBLE' || fail "could not write the opencode double; check that $tmp/bin is writable"
#!/usr/bin/env bash
set -euo pipefail
if [ "${1:-}" = --version ]; then
  echo 0.0.0-double
  exit 0
fi
echo launch >>"$SESSION_DOUBLE_RECORD" || {
  echo "opencode double: could not append to its record $SESSION_DOUBLE_RECORD; check that its directory is writable" >&2
  exit 1
}
n="$(grep -cx launch "$SESSION_DOUBLE_RECORD")"
read -r -a plan <<<"$SESSION_DOUBLE_PLAN"
step="${plan[$((n > ${#plan[@]} ? ${#plan[@]} - 1 : n - 1))]}"
target="$(printf '%s\n' "$@" | grep -o 'touch [^ ]*\.txt' | head -n 1 | cut -c7-)"
[ -n "$target" ] || {
  echo "opencode double: no touch target in its arguments: $*" >&2
  exit 1
}
tool() {
  jq -cn --arg c "touch $target" --arg s "$1" \
    '{type: "tool_use", sessionID: "ses_double", part: {type: "tool", tool: "bash", callID: "call_1", state: {status: $s, input: {command: $c}, output: ""}}}'
}
case "$step" in
  refuse) ;;
  attempt) tool completed ;;
  touch)
    gate=0
    grep -rqF -- '"--deny-if-contains","session_id"' .opencode/plugin || gate=$?
    case "$gate" in
      0) tool error ;;
      1)
        touch "$target" || {
          echo "opencode double: could not create $target; check that its sandbox is writable" >&2
          exit 1
        }
        tool completed
        ;;
      *)
        echo "opencode double: could not read the synced plugin under $PWD/.opencode/plugin (grep exit $gate)" >&2
        exit 1
        ;;
    esac
    ;;
  *)
    echo "opencode double: unknown plan step $step" >&2
    exit 1
    ;;
esac
jq -cn '{type: "text", sessionID: "ses_double", part: {type: "text", text: "done"}}'
DOUBLE
chmod +x "$tmp/bin/opencode" || fail "could not make the opencode double executable"

# The phase under test, read out of e2e-opencode.sh (whose top level drives
# every phase) once, and refused unless the whole definition came out.
phase_src="$tmp/session-phase.sh"
sed -n '/^oh_opencode_session_id_forwarded() {/,/^}/p' "$root/scripts/e2e-opencode.sh" >"$phase_src" ||
  fail "could not read oh_opencode_session_id_forwarded out of $root/scripts/e2e-opencode.sh; check that it is readable"
[ "$(tail -n 1 "$phase_src")" = "}" ] ||
  fail "scripts/e2e-opencode.sh no longer defines oh_opencode_session_id_forwarded() at the start of a line, closed by a lone '}'; update this check's extraction"

record="$tmp/record"
# One phase as live-opencode runs it: the extracted definition over
# e2e-lib.sh's helpers. Captures output in $out, the exit code in $rc, launches in $launches.
phase() {
  rm -f "$record" || fail "could not clear the previous phase's record $record; check that $tmp is writable"
  set +e
  # shellcheck disable=SC2016 # expanded by the driven shell, which has the helpers
  out="$(
    env PATH="$tmp/bin:$PATH" TMPDIR="$tmp/sandboxes" ONEHARNESS_BIN="$oh" \
      SESSION_DOUBLE_RECORD="$record" SESSION_DOUBLE_PLAN="$1" \
      bash -c 'set -euo pipefail; source "$1/scripts/e2e-lib.sh"
        source "$2"
        OH_REPORT="{\"results\":[{\"stdout\":\"EARLIER-PHASE-REPORT\"}]}"
        oh_opencode_session_id_forwarded' _ "$root" "$phase_src" 2>&1
  )"
  rc=$?
  set -e
  launches=0
  if [ -e "$record" ]; then
    launches="$(grep -cx launch "$record")" || [ "$launches" = 0 ] ||
      fail "could not count the launches in the double's record $record"
  fi
}

# 1. A control turn that never invoked the shell tool is retried, and the
#    phase passes once a turn runs the touch and the session_id gate blocks it.
phase "refuse touch"
[ "$rc" -eq 0 ] || fail "a refusal followed by a compliant turn failed the phase: $out"
case "$out" in
  *"ok[control]: the unmatched command ran (attempt 2 of 2)"*"PASS: opencode session_id forwarding"*) ;;
  *) fail "a refusal followed by a compliant turn did not pass on the second control attempt: $out" ;;
esac
[ "$launches" = 3 ] || fail "a retried control plus the proof launched the harness $launches times, not 3: $out"

# 2. Two refusals fail, naming both attempts, with this phase's turn dumped.
phase "refuse"
[ "$rc" -ne 0 ] || fail "two refusals passed the phase: $out"
case "$out" in
  *"positive control never ran"*"after 2 attempt(s)"*) ;;
  *) fail "two refusals failed for another reason: $out" ;;
esac
[ "$launches" = 2 ] || fail "two refusals launched the harness $launches times, not 2: $out"
case "$out" in
  *EARLIER-PHASE-REPORT*) fail "a failed control dumped an earlier phase's report instead of its own turn: $out" ;;
esac

# 3. A touch the agent attempted that left no file fails at once, without a
#    retry, and the dump shows that turn's own tool call.
phase "attempt"
[ "$rc" -ne 0 ] || fail "an attempted touch that created no file passed the phase: $out"
case "$out" in
  *"after 1 attempt(s)"*) ;;
  *) fail "an attempted touch that created no file was retried or failed for another reason: $out" ;;
esac
[ "$launches" = 1 ] || fail "an attempted touch launched the harness $launches times, not once: $out"
case "$out" in
  *"touch "*ohsess-allow-*) ;;
  *) fail "a failed control did not dump its own turn's tool call: $out" ;;
esac

echo "check-opencode-session-control: ok"
