#!/usr/bin/env bash
# The live tier's network recorder: one harness launch, measured.
#
#   e2e-netshim.sh --suite S --harness H --real PATH --self SHIM --file FILE -- ARGS...
#
# scripts/e2e-lib.sh's oh_telemetry_setup writes a two-line shim per harness
# executable a suite launches (`codex`, `claude`, ...) that execs this with the
# real executable it resolved beforehand, and puts those shims first on PATH.
# For each launch this appends to FILE (target/e2e-telemetry/network/<suite>.jsonl):
#
#   {"suite","phase","harness","kind","launch_id","argv"}            before it
#   one measurement line joined by launch_id                          after it
#
# `phase` is OH_E2E_PHASE, which the suite's helpers export; `kind` is derived
# from the argv (_kind below). On Linux the launch runs under
# scripts/e2e-netmeasure.py, whose line carries the bytes its network namespace
# exchanged ("method": "netns-pasta"); where that cannot measure — macOS, a
# Linux host without pasta — it runs the launch directly and the line is
# "method": "unavailable" with a reason. Wherever python3 runs it, the
# measurer's own-session supervisor writes the start line too, so a caller that
# SIGKILLs this shim's process group (oneharness does, 100 ms after a SIGTERM)
# never leaves a start line without its measurement. Without python3 (Windows,
# a bare Linux host) this shim records the launch itself. Either way stdin,
# stdout and stderr are the launch's own and the exit status is the launch's.
#
# OH_E2E_OS (default: `uname -s`) selects the platform, so a test can drive the
# fallback a macOS or Windows leg takes. A recorder that cannot record exits
# 126 (refused before the launch) or 125 (the launch ran but its line could not
# be written), saying why, rather than passing an unrecorded launch off as one.
set -euo pipefail

MARKER="oneharness-network-shim"

die() {
  printf 'e2e-netshim: %s\n' "$*" >&2
  exit 126
}

suite="" harness="" real="" self="" file=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --)
      shift
      break
      ;;
    --suite | --harness | --real | --self | --file)
      [ "$#" -ge 2 ] || die "option '$1' needs a value; regenerate the shim with oh_telemetry_setup"
      ;;
    *) die "unknown option '$1'; the shim oh_telemetry_setup writes passes --suite --harness --real --self --file --" ;;
  esac
  case "$1" in
    --suite) suite="$2" ;;
    --harness) harness="$2" ;;
    --real) real="$2" ;;
    --self) self="$2" ;;
    --file) file="$2" ;;
  esac
  shift 2
done
[ -n "$suite" ] && [ -n "$harness" ] && [ -n "$real" ] && [ -n "$self" ] && [ -n "$file" ] ||
  die "--suite, --harness, --real, --self and --file are all required; regenerate the shim with oh_telemetry_setup"

_canonical() {
  local dir="${1%/*}" name="${1##*/}"
  [ "$dir" != "$1" ] || dir=.
  dir="$(cd "$dir" 2>/dev/null && pwd -P)" || return 1
  printf '%s/%s' "$dir" "$name"
}

# A shim that would launch itself — or another shim — would recurse or record
# one launch twice; it refuses instead, before anything runs or is recorded.
[ -x "$real" ] || die "the executable this shim wraps, $real, is gone; rerun the suite so oh_telemetry_setup resolves it again"
real_path="$(_canonical "$real")" || die "cannot resolve the directory of $real; check its permissions, then rerun the suite"
self_path="$(_canonical "$self")" || die "cannot resolve the directory of the shim $self; rerun the suite so oh_telemetry_setup writes it again"
# Bytes, not text: BSD tr refuses a native binary's head under a UTF-8 locale.
head_bytes="$(head -c 512 "$real" | LC_ALL=C tr -d '\0')" ||
  die "cannot read $real to check it is not a network shim; check its permissions, then rerun the suite"
wraps_a_shim=false
case "$head_bytes" in *"$MARKER"*) wraps_a_shim=true ;; esac
if [ "$real_path" = "$self_path" ] || [ "$wraps_a_shim" = true ]; then
  die "refusing to wrap $real: it is this shim (or another network shim), not the real $harness; resolve the real executable before putting the shim first on PATH"
fi

# The launch kind, from the argv alone. Codex: `exec` (fresh or `exec
# resume`), `app-server` (stdio: a controlled turn or a usage probe — the phase
# tells them apart), `shared-app-server` (`app-server --listen`), `local` for a
# launch that runs no turn (`--version`, `features`, `execpolicy`, `sandbox`,
# `login status`), else the subcommand itself. Other harnesses: `local` for a
# version/help/auth-status probe, `turn` for anything else.
_kind() {
  local h="$1" a sub=""
  shift
  if [ "$h" != codex ]; then
    case "${1:-}:${2:-}" in
      --version:* | -v:* | -V:* | version:* | --help:* | -h:* | auth:status | login:status) echo local ;;
      *) echo turn ;;
    esac
    return
  fi
  while [ "$#" -gt 0 ]; do
    a="$1"
    case "$a" in
      -c | --config | --enable | --disable | -m | --model | -p | --profile | -C | --cd | -s | --sandbox | -a | --ask-for-approval | -i | --image)
        shift 2 || break
        continue
        ;;
      --version | -V | --help | -h)
        [ -z "$sub" ] && echo local && return
        ;;
      -*) ;;
      *)
        sub="$a"
        break
        ;;
    esac
    shift
  done
  case "$sub" in
    exec | e) echo exec ;;
    app-server)
      local arg
      for arg in "$@"; do
        case "$arg" in --listen | --listen=*)
          echo shared-app-server
          return
          ;;
        esac
      done
      echo app-server
      ;;
    features | execpolicy | sandbox | "") echo local ;;
    login)
      [ "${2:-}" = status ] && echo local || echo login
      ;;
    *) echo "$sub" ;;
  esac
}

kind="$(_kind "$harness" "$@")"
entropy="$(od -An -N6 -tx1 /dev/urandom | tr -d ' \n')" && [ "${#entropy}" -eq 12 ] ||
  die "could not read /dev/urandom for a launch id, so two launches could share one; check /dev/urandom, then rerun"
launch_id="$suite-$$-$entropy"
phase="${OH_E2E_PHASE:-unphased}"
# NUL-separated, so no argument is ever read as a jq option; each is cut to
# 200 characters (a prompt is evidence enough by its head).
argv='[]'
if [ "$#" -gt 0 ]; then
  argv="$(printf '%s\0' "$@" | jq -Rsc 'split("\u0000")[:-1] | map(if length > 200 then .[0:200] + "…" else . end)')" ||
    die "jq could not encode the launch's argv; install jq (the live suites need it) and rerun"
fi
start="$(jq -nc --arg suite "$suite" --arg phase "$phase" --arg harness "$harness" \
  --arg kind "$kind" --arg id "$launch_id" --argjson argv "$argv" \
  '{suite: $suite, phase: $phase, harness: $harness, kind: $kind, launch_id: $id, argv: $argv}' |
  tr -d '\r')" || die "jq could not write the start line; install jq (the live suites need it) and rerun"
# From here a stop must not end the launch before the measurer's handlers:
# ignored, it is either dropped (and the launch runs and is recorded) or held by
# e2e-netmeasure.py's defer_stops. The fallback below replaces this with its
# own handler.
trap '' TERM INT HUP
# Marks the launch as still being measured until its line is in, so the
# budget step waits for a namespace still winding down. Made before the start
# line, so no start line is ever written without one.
pending="$(dirname "$file")/.pending/$launch_id"

# A shared Codex app-server is started with a server marker
# (ONEHARNESS_APP_SERVER_MARKER), and stopping it stops every process carrying
# that marker. The recorder's own processes must not carry it — its supervisor
# writes the server's line only after the server's whole tree has exited — so
# the marker is taken off this process and given to the launch alone.
launch=("$real")
if [ -n "${ONEHARNESS_APP_SERVER_MARKER:-}" ]; then
  launch=(env "ONEHARNESS_APP_SERVER_MARKER=$ONEHARNESS_APP_SERVER_MARKER" "$real")
  unset ONEHARNESS_APP_SERVER_MARKER
fi

os="${OH_E2E_OS:-$(uname -s)}"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
case "$os" in
  Linux*) unmeasured=() reason="no python3 on PATH to run scripts/e2e-netmeasure.py" ;;
  *)
    reason="the netns-pasta method needs Linux network namespaces; this leg is $os, which the network budgets do not support"
    unmeasured=(--unmeasured "$reason")
    ;;
esac
# Git Bash's python3 cannot fork, so Windows keeps the fallback below.
case "$os" in MINGW* | MSYS* | CYGWIN* | Windows*) ;; *)
  if command -v python3 >/dev/null 2>&1; then
    exec python3 "$here/e2e-netmeasure.py" --line-file "$file" --launch-id "$launch_id" \
      --pending "$pending" --start-line "$start" ${unmeasured[@]+"${unmeasured[@]}"} \
      -- "${launch[@]}" "$@"
  fi
  ;;
esac

# The fallback: record the start, run the launch directly, and record that it
# was not measured.
if ! { mkdir -p "$(dirname "$pending")" && : >"$pending"; }; then
  die "cannot create $pending; check that $(dirname "$file") is writable, then rerun"
fi
if ! printf '%s\n' "$start" >>"$file"; then
  rm -f "$pending" ||
    die "cannot append to $file, nor remove its pending marker $pending, which the budget step would wait on; check that $(dirname "$file") is writable, remove the marker, then rerun"
  die "cannot append to $file; check that its directory is writable"
fi
# The trap is a handler, not an ignore, so the launch keeps default signal
# dispositions; a signal to the process group ends it and this still records.
trap ':' TERM INT HUP
t0="$(date +%s)"
rc=0
"${launch[@]}" "$@" || rc=$?
unrecorded() {
  printf 'e2e-netshim: %s exited %s, but its measurement line could not be written to %s (%s); check that the directory is writable, then rerun the suite\n' \
    "$real" "$rc" "$file" "$1" >&2
  exit 125
}
line="$(jq -nc --arg id "$launch_id" --arg reason "$reason" --argjson seconds "$(($(date +%s) - t0))" \
  --argjson exit "$rc" \
  '{launch_id: $id, method: "unavailable", reason: $reason, seconds: $seconds, exit: $exit}' | tr -d '\r')" ||
  unrecorded "jq failed"
printf '%s\n' "$line" >>"$file" || unrecorded "the append failed"
rm -f "$pending" || unrecorded "its pending marker $pending could not be removed"
exit "$rc"
