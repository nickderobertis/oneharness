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
{
  echo launch
  for a in "$@"; do printf 'arg=%s\n' "$a"; done
} >>"$ECHO_DOUBLE_RECORD" || {
  echo "qwen double: could not append to its record $ECHO_DOUBLE_RECORD; check that its directory is writable" >&2
  exit 1
}
if [ -n "${ECHO_DOUBLE_OMIT:-}" ]; then
  echo "I'm ready to assist you! If there's something specific you need help with, please let me know."
  exit 0
fi
printf '%s\n' "$@" | grep -o 'ONEHARNESS-LIVE-[0-9]*' | head -n 1
DOUBLE
chmod +x "$tmp/bin/qwen" || fail "could not make the qwen double executable"

record="$tmp/record"
# One probe as a live suite runs it: a fresh marker, oh_run with oh_prompt's
# request, then oh_assert_echoed. Captures output in $out, the exit code in $rc.
probe() {
  rm -f "$record" || fail "could not clear the previous probe's record $record; check that $tmp is writable"
  set +e
  # shellcheck disable=SC2016 # expanded by the driven shell, which has the helpers
  out="$(
    env PATH="$tmp/bin:$PATH" ONEHARNESS_BIN="$oh" ECHO_DOUBLE_RECORD="$record" "$@" \
      bash -c "set -euo pipefail; source '$root/scripts/e2e-lib.sh'
        m=\"\$(oh_marker)\"; echo \"marker=\$m\"; echo \"prompt=\$(oh_prompt \"\$m\")\"
        oh_run qwen \"\$(oh_prompt \"\$m\")\"; oh_assert_echoed qwen \"\$m\"" 2>&1
  )"
  rc=$?
  set -e
  launches="$(grep -cx launch "$record" 2>/dev/null || true)"
}

# 1. A reply carrying the marker passes, after exactly one launch that was
#    handed the request unaltered.
probe
[ "$rc" -eq 0 ] || fail "a reply carrying the marker failed the probe: $out"
case "$out" in
  *"PASS: qwen live e2e"*) ;;
  *) fail "a reply carrying the marker did not reach the probe's verdict: $out" ;;
esac
[ "$launches" = 1 ] || fail "a passing probe launched the harness ${launches:-0} times, not once: $out"
marker="$(sed -n 's/^marker=//p' <<<"$out")"
prompt="$(sed -n 's/^prompt=//p' <<<"$out")"
[ "$prompt" = "oneharness end-to-end test. Reply with exactly this string and nothing else: $marker" ] ||
  fail "oh_prompt no longer asks plainly for the marker alone: $prompt"
grep -qxF "arg=$prompt" "$record" ||
  fail "the harness was not handed the probe's request intact: $(cat "$record")"

# 2. A reply without the marker fails, naming the missing marker, with no retry.
probe ECHO_DOUBLE_OMIT=1
[ "$rc" -ne 0 ] || fail "a reply without the marker passed the probe: $out"
case "$out" in
  *"FAIL: qwen ran but the unique marker never surfaced"*) ;;
  *) fail "a reply without the marker failed for another reason: $out" ;;
esac
[ "$launches" = 1 ] || fail "a failing probe launched the harness ${launches:-0} times, not once: $out"

echo "check-echo-probe: ok"
