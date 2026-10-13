#!/usr/bin/env bash
# A harness executable's stand-in for the network recorder's hermetic tests,
# installed under the executable's own name (`codex`, `claude`, ...) on PATH.
# It answers as the harness would through the repository's mock harness
# (OH_TEST_MOCK) and, when OH_TEST_TRANSFER="PORT DOWN UP [flags]" is set,
# first starts transfer.py's loopback client as its child, so the recorder has
# bytes to count. OH_TEST_ARGV_LOG, when set, gets each launch's argv as one
# JSON line.
#
# Codex's launches are the ones the budget tests drive end to end:
#   --version            the version oneharness asks before each launch
#   exec ...             a turn whose agent message echoes the prompt's marker
#   app-server           a usage probe (OH_E2E_PHASE=usage-probe) answering
#                        `account/rateLimits/read`, else a controlled turn
#                        echoing its input; under OH_TEST_PLUGINS_LOG the mock
#                        records its plugins evidence there
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

if [ -n "${OH_TEST_ARGV_LOG:-}" ]; then
  printf '%s\0' "$tool" "$@" | jq -Rsc 'split("\u0000")[:-1] | {tool: .[0], argv: .[1:]}' >>"$OH_TEST_ARGV_LOG"
fi
if [ -n "${OH_TEST_TRANSFER:-}" ]; then
  # shellcheck disable=SC2086 # the variable is the client's own word list
  python3 "$here/transfer.py" client $OH_TEST_TRANSFER
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
  [ -z "${OH_TEST_RAN_LOG:-}" ] || printf 'sleep %s\n' "$seconds" >>"$OH_TEST_RAN_LOG"
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
[ -z "${OH_TEST_PLUGINS_LOG:-}" ] || export MOCK_CODEX_PLUGINS_LOG="$OH_TEST_PLUGINS_LOG"
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
exec "$mock" "$@"
