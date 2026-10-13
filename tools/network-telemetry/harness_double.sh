#!/usr/bin/env bash
# A harness executable's stand-in for the network recorder's hermetic tests,
# installed under the executable's own name (`codex`, `claude`, ...) on PATH.
# It answers as the harness would through the repository's mock harness
# (OH_TEST_MOCK) and, when OH_TEST_TRANSFER="PORT DOWN UP [flags]" is set, an
# exec or app-server launch (of phase OH_TEST_TRANSFER_PHASE, when set) first
# runs transfer.py's loopback client as its child, so the recorder has bytes
# to count. OH_TEST_ARGV_LOG, when set, gets each launch's argv as one
# JSON line.
#
# Codex's launches are the ones the budget tests drive end to end:
#   --version            the version oneharness asks before each launch
#   exec ...             a turn whose agent message echoes the prompt's marker
#   app-server           a usage probe (OH_E2E_PHASE=usage-probe) answering
#                        `account/rateLimits/read`, else a controlled turn
#                        echoing its input
# and, under OH_TEST_PLUGINS_EVIDENCE, appends the mock's plugins record of an
# exec or app-server launch there as one {process, text, catalog_request,
# plugin_load_ran} line;
# and a prompt naming `sleep N` runs that sleep, as an agent's shell tool would.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
mock="${OH_TEST_MOCK:?OH_TEST_MOCK must name the built oneharness-mock-harness}"
tool="$(basename "$0")"
tool="${tool%.cmd}"
case "${1:-}" in
  --double-tool=*)
    tool="${1#--double-tool=}"
    shift
    ;;
esac

refuse() {
  echo "harness_double: $*" >&2
  exit 1
}

if [ -n "${OH_TEST_ARGV_LOG:-}" ]; then
  printf '%s\0' "$tool" "$@" | jq -Rsc 'split("\u0000")[:-1] | {tool: .[0], argv: .[1:]}' >>"$OH_TEST_ARGV_LOG" ||
    refuse "cannot append its argv to OH_TEST_ARGV_LOG=$OH_TEST_ARGV_LOG; check that the test's scratch directory is writable"
fi

# The prompt: the argv after the subcommand, or stdin for `exec -`.
prompt="$*"
for arg in "$@"; do
  if [ "$arg" = - ]; then
    prompt="$(cat)"
  fi
done
marker="$(printf '%s' "$prompt" | grep -oE 'ONEHARNESS-LIVE-[0-9]+|OH_VARIANT_[0-9]+_[0-9]+_[a-z]+' | head -1 || true)"
seconds="$(printf '%s' "$prompt" | grep -oE 'sleep [0-9]+' | head -1 | cut -d' ' -f2 || true)"
if [ -n "$seconds" ]; then
  sleep "$seconds"
  [ -z "${OH_TEST_RAN_LOG:-}" ] || printf 'sleep %s\n' "$seconds" >>"$OH_TEST_RAN_LOG" ||
    refuse "cannot append to OH_TEST_RAN_LOG=$OH_TEST_RAN_LOG; check that the test's scratch directory is writable"
fi

if [ "$tool" != codex ]; then
  printf '%s\n' "${marker:-$prompt}"
  exit 0
fi

subcommand=""
for arg in "$@"; do
  case "$arg" in
    exec | app-server | login | --version)
      subcommand="$arg"
      break
      ;;
  esac
done
# A turn's or a probe's own traffic — never `--version`'s — and, with
# OH_TEST_TRANSFER_PHASE, only that phase's.
if [ -n "${OH_TEST_TRANSFER:-}" ] && { [ "$subcommand" = exec ] || [ "$subcommand" = app-server ]; } &&
  [ "${OH_TEST_TRANSFER_PHASE:-${OH_E2E_PHASE:-}}" = "${OH_E2E_PHASE:-}" ]; then
  read -r -a transfer <<<"$OH_TEST_TRANSFER"
  python3 "$here/transfer.py" client "${transfer[@]}" ||
    refuse "its loopback transfer (${OH_TEST_TRANSFER}) failed; transfer.py's error is above — check the test's server is still listening"
fi
case "$subcommand" in
  login)
    echo "Logged in using ChatGPT"
    exit 0
    ;;
  exec)
    MOCK_STDOUT="$(jq -nc --arg text "$marker" '{type: "item.completed", item: {id: "m1", type: "agent_message", text: $text}}')"
    export MOCK_STDOUT
    ;;
  app-server)
    if [ "${OH_E2E_PHASE:-}" = usage-probe ]; then
      export MOCK_REPLY_AFTER_LINES=3
      MOCK_STDOUT="$(jq -nc '{id: 2, result: {
        rateLimitsByLimitId: {codex: {limitId: "codex", limitName: null, secondary: null, planType: "pro",
          primary: {usedPercent: 31, windowDurationMins: 10080, resetsAt: 1785000000}}},
        rateLimitResetCredits: {availableCount: 0, credits: []}}}')"
      export MOCK_STDOUT
    else
      export MOCK_CODEX_APP_SERVER_LOG="${OH_TEST_APP_SERVER_LOG:-/dev/null}"
      export MOCK_CODEX_COMPLETE_TURN=1 MOCK_CODEX_ECHO_PROMPT=1
    fi
    ;;
esac
if [ -z "${OH_TEST_PLUGINS_EVIDENCE:-}" ]; then
  exec "$mock" "$@"
fi
# The mock's own plugins record of this launch, as the per-process evidence
# e2e-variants.sh's opted-in phase reads in place of Codex's log: its text is
# this launch's argv, which carries the prompt's marker.
record="$(mktemp)" || refuse "cannot create a scratch file for the mock's plugins record; check TMPDIR is writable"
trap 'rm -f "$record" || { echo "harness_double: could not remove its scratch file $record; delete it by hand" >&2; exit 1; }' EXIT
rc=0
MOCK_CODEX_PLUGINS_LOG="$record" "$mock" "$@" || rc=$?
if [ -s "$record" ]; then
  printf '%s\0' "$@" | jq -Rsc --slurpfile mock "$record" --arg pid "$$" \
    '{process: $pid, text: (split("\u0000")[:-1] | join(" ")), catalog_request: $mock[0].catalog_fetched,
      plugin_load_ran: ($mock[0].loaded | length > 0)}' >>"$OH_TEST_PLUGINS_EVIDENCE" ||
    refuse "cannot turn the mock's plugins record $record into evidence in $OH_TEST_PLUGINS_EVIDENCE (jq's error is above)"
fi
exit "$rc"
