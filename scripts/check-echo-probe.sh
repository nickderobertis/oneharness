#!/usr/bin/env bash
# Hermetic check for the live echo probe (scripts/e2e-lib.sh): oh_prompt asks
# plainly for a reply of the marker alone, oh_run hands that request to the
# harness intact and launches it once, and oh_assert_echoed passes a reply
# carrying the marker and fails one without it, with no retry. The real built
# oneharness drives a qwen double that records every launch and the argv it
# was given, then answers with the marker unless told to omit it.
# llmlint: ignore-file[e2e_not_mocked] This drives the real built oneharness CLI and the live suite's own oh_run and oh_assert_echoed; only the paid qwen CLI is replaced, because what is under test is the probe's request and its verdict on a reply, not the model. The live-* suites remain the real-harness drift alarm for the same helpers.
set -euo pipefail

case $(uname -s) in
  MINGW* | MSYS* | CYGWIN*)
    echo "check-echo-probe: skipped on Windows because this Unix behavioral harness relies on an extensionless executable double" >&2
    exit 0
    ;;
esac

fail() {
  echo "check-echo-probe: $1" >&2
  echo "  Re-run with: bash -x scripts/check-echo-probe.sh" >&2
  exit 1
}

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)" ||
  fail "could not resolve the repository root from ${BASH_SOURCE[0]}; run this script by its path inside a readable checkout"

command -v jq >/dev/null 2>&1 || fail "jq is not installed; the live helpers parse oneharness's report with it"
oh="$root/target/debug/oneharness"
[ -x "$oh" ] || fail "the built oneharness binary is missing at $oh; run 'just build' and retry"

tmp="$(mktemp -d "${TMPDIR:-/tmp}/tmp.XXXXXXXXXX")" ||
  fail "could not create a scratch directory under ${TMPDIR:-/tmp}; check that it exists and is writable, or set TMPDIR to one that is"
trap 'rm -rf "$tmp" || echo "check-echo-probe: could not remove $tmp; delete it by hand" >&2' EXIT

mkdir -p "$tmp/bin" || fail "could not create $tmp/bin; check that $tmp is writable"
cat >"$tmp/bin/qwen" <<'DOUBLE' || fail "could not write the qwen double; check that $tmp/bin is writable"
#!/usr/bin/env bash
set -euo pipefail
if [ "${1:-}" = --version ]; then
  echo 0.0.0-double
  exit 0
fi
prompt="$(cat)" || {
  echo "qwen echo double: cannot read stdin; inspect the runner stdin pipe and rerun bash -x scripts/check-echo-probe.sh" >&2
  exit 1
}
{
  echo launch
  for a in "$@"; do printf 'arg=%s\n' "$a"; done
  printf 'stdin=%s\n' "$prompt"
} >>"$ECHO_DOUBLE_RECORD" || {
  echo "qwen double: could not append to its record $ECHO_DOUBLE_RECORD; check that its directory is writable" >&2
  exit 1
}
[ -n "${ECHO_DOUBLE_STDERR:-}" ] && printf '%s\n\n' "$ECHO_DOUBLE_STDERR" >&2
[ -n "${ECHO_DOUBLE_EXIT:-}" ] && exit "$ECHO_DOUBLE_EXIT"
if [ -n "${ECHO_DOUBLE_OMIT:-}" ]; then
  echo "I'm ready to assist you! If there's something specific you need help with, please let me know."
  exit 0
fi
if ! printf '%s\n' "$prompt" | grep -o 'ONEHARNESS-LIVE-[0-9]*' | head -n 1; then
  echo 'qwen echo double: no marker on stdin; inspect oh_prompt and rerun bash -x scripts/check-echo-probe.sh' >&2
  exit 1
fi
DOUBLE
chmod +x "$tmp/bin/qwen" || fail "could not make the qwen double executable"

record="$tmp/record"
# How many times the double launched for the last probe: none when it never
# wrote its record, and a failure when the record is there but unreadable.
count_launches() {
  launches=0
  [ -e "$record" ] || return 0
  [ -r "$record" ] || fail "could not read the double's record $record; check that $tmp is readable"
  launches="$(grep -cx launch "$record")" || [ "$launches" = 0 ] ||
    fail "could not count the launches in the double's record $record; check that it is readable"
}

# One probe as a live suite runs it: a fresh marker, oh_run with oh_prompt's
# request, then oh_assert_echoed. Captures output in $out, the exit code in $rc.
probe() {
  rm -f "$record" || fail "could not clear the previous probe's record $record; check that $tmp is writable"
  set +e
  # shellcheck disable=SC2016 # expanded by the driven shell, which has the helpers
  out="$(
    env PATH="$tmp/bin:$PATH" ONEHARNESS_BIN="$oh" ECHO_DOUBLE_RECORD="$record" "$@" \
      bash -c 'set -euo pipefail; source "$1/scripts/e2e-lib.sh"
        [ -z "${ECHO_PROBE_RECORDED:-}" ] || oh_telemetry_setup qwen
        m="$(oh_marker)"; echo "marker=$m"; echo "prompt=$(oh_prompt "$m")"
        oh_run qwen "$(oh_prompt "$m")"; oh_assert_echoed qwen "$m"' _ "$root" 2>&1
  )"
  rc=$?
  set -e
  count_launches
}

# 1. A reply carrying the marker passes, after exactly one launch that was
#    handed the request unaltered.
probe
[ "$rc" -eq 0 ] || fail "a reply carrying the marker failed the probe: $out"
case "$out" in
  *"PASS: qwen live e2e"*) ;;
  *) fail "a reply carrying the marker did not reach the probe's verdict: $out" ;;
esac
[ "$launches" = 1 ] || fail "a passing probe launched the harness $launches times, not once: $out"
marker="$(sed -n 's/^marker=//p' <<<"$out")" || fail "could not read the probe's marker from its output: $out"
prompt="$(sed -n 's/^prompt=//p' <<<"$out")" || fail "could not read the probe's request from its output: $out"
[ -n "$marker" ] || fail "the probe printed no marker: $out"
[ "$prompt" = "oneharness end-to-end test. Reply with exactly this string and nothing else: $marker" ] ||
  fail "oh_prompt no longer asks plainly for the marker alone: $prompt"
grep -qxF "stdin=$prompt" "$record" ||
  fail "the harness was not handed the probe's request intact: $(cat "$record")"

# 2. A reply without the marker fails, naming the missing marker, with no retry.
probe ECHO_DOUBLE_OMIT=1
[ "$rc" -ne 0 ] || fail "a reply without the marker passed the probe: $out"
case "$out" in
  *"FAIL: qwen ran but the unique marker never surfaced"*) ;;
  *) fail "a reply without the marker failed for another reason: $out" ;;
esac
[ "$launches" = 1 ] || fail "a failing probe launched the harness $launches times, not once: $out"
case "$out" in
  *"never surfaced — the model did not echo it back; the run reported"*)
    fail "a reply without the marker and with nothing on stderr reported an error it does not have: $out"
    ;;
esac

# 3. A reply without the marker from a run whose harness reported an error
#    names that error in the failure, not only that the marker is missing.
probe ECHO_DOUBLE_OMIT=1 ECHO_DOUBLE_STDERR="[API Error: 429 Too Many Requests]"
[ "$rc" -ne 0 ] || fail "a reply without the marker passed the probe: $out"
case "$out" in
  *"FAIL: qwen ran but the unique marker never surfaced — the model did not echo it back; the run reported: [API Error: 429 Too Many Requests]"*) ;;
  *) fail "a reply without the marker did not name the run's own error in its failure: $out" ;;
esac

# 4. A run that fails names its own error beside its status.
probe ECHO_DOUBLE_EXIT=3 ECHO_DOUBLE_STDERR="provider: 401 invalid api key"
[ "$rc" -ne 0 ] || fail "a run that exited 3 passed the probe: $out"
case "$out" in
  *"FAIL: qwen did not run cleanly: status=nonzero, exit_code=3 — the run reported: "*) ;;
  *) fail "a failed run did not name its own error in its failure: $out" ;;
esac

# 5. Behind the live suites' network recorder on a Linux host that refuses
#    pasta its user namespace (ubuntu-24.04's AppArmor), the run goes on
#    unmeasured, and a reply without the marker is not blamed on pasta's
#    words: they are the recorder's, kept in its line, not the run's error.
if [ "$(uname -s)" = Linux ] && command -v python3 >/dev/null 2>&1; then
  mkdir -p "$tmp/refused" || fail "could not create $tmp/refused; check that $tmp is writable"
  cat >"$tmp/refused/pasta" <<'PASTA' || fail "could not write the refused pasta double; check that $tmp/refused is writable"
#!/usr/bin/env bash
set -euo pipefail
echo "Couldn't write to /proc/self/uid_map: Operation not permitted" >&2
echo "Couldn't configure user mappings" >&2
echo 'clone: Operation not permitted' >&2
exit 1
PASTA
  chmod +x "$tmp/refused/pasta" || fail "could not make the refused pasta double executable; check permissions on $tmp/refused/pasta and rerun this check"
  probe ECHO_DOUBLE_OMIT=1 ECHO_PROBE_RECORDED=1 OH_E2E_TELEMETRY_DIR="$tmp/telemetry" \
    PATH="$tmp/bin:$tmp/refused:$PATH"
  [ "$rc" -ne 0 ] || fail "a reply without the marker passed the recorded probe; fix oh_assert_echoed to reject a missing marker, then rerun this check: $out"
  case "$out" in
    *"FAIL: qwen ran but the unique marker never surfaced"*) ;;
    *) fail "a recorded reply without the marker failed for another reason; inspect the captured failure and fix the probe setup before rerunning this check: $out" ;;
  esac
  case "$out" in
    *"the run reported"* | *"uid_map"* | *"clone: Operation not permitted"*)
      fail "the refused recorder's own words reached the run's stderr as the harness's; fix pre-launch stderr isolation in e2e-netmeasure.py, then rerun this check: $out"
      ;;
  esac
  grep -q "Couldn't write to /proc/self/uid_map" "$tmp/telemetry/qwen.jsonl" ||
    fail "the recorder did not record why the launch went unmeasured; fix the unavailable measurement reason in e2e-netmeasure.py, then rerun this check: $(cat "$tmp/telemetry/qwen.jsonl")"
fi

echo "check-echo-probe: ok"
