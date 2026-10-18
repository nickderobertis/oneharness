#!/usr/bin/env bash
# Hermetic check for oh_echo_without_context (scripts/e2e-lib.sh), the live
# qwen journey's echo turn. It must start qwen with none of the checkout's or
# the machine's context or memory files loaded, and must still fail when the
# reply lacks the marker. The real built oneharness drives a qwen double that
# applies qwen's own context-discovery rules (packages/core memory discovery:
# QWEN.md and AGENTS.md, global under $QWEN_HOME or ~/.qwen, then upward from
# the cwd to the git root, or to HOME's parent when there is none, plus
# auto-memory under QWEN_CODE_MEMORY_BASE_DIR / QWEN_RUNTIME_DIR / QWEN_HOME).
# The double answers the way gpt-4o-mini did on macOS CI once context loaded;
# given OPENAI_BASE_URL, it sends the files it loaded there instead, so the
# live alarm oh_context_free_enforce can be held red and green here. That alarm
# runs the same two observations against real qwen in scripts/e2e-qwen.sh,
# which is what keeps the double's discovery rules honest.
# llmlint: ignore-file[contracts_have_one_source_or_a_drift_gate] The double's discovery rules restate qwen's, an external CLI the deterministic gate cannot run; real qwen is their drift alarm, as codex-stall-endpoint.py's is real codex: oh_context_free_enforce in scripts/e2e-qwen.sh (the live-qwen suite, run by the required macOS job) fails if a checkout turn stops sending the files the double says it loads, or a context-free turn starts sending any.
# llmlint: ignore-file[e2e_not_mocked] This drives the real built oneharness CLI and the live suite's own helper; only the paid qwen CLI is replaced, by a double applying qwen's documented context discovery, because what is under test is which files a launch exposes to it, not the model. scripts/e2e-qwen.sh remains the real-CLI drift alarm.
set -euo pipefail

case $(uname -s) in
  MINGW* | MSYS* | CYGWIN*)
    echo "check-qwen-context-free: skipped on Windows because this Unix behavioral harness relies on an extensionless executable double" >&2
    exit 0
    ;;
esac

fail() {
  echo "check-qwen-context-free: $1" >&2
  echo "  Re-run with: bash -x scripts/check-qwen-context-free.sh" >&2
  exit 1
}

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)" ||
  fail "could not resolve the repository root from ${BASH_SOURCE[0]}; run this script by its path inside a readable checkout"

command -v jq >/dev/null 2>&1 || fail "jq is not installed; the live helpers parse oneharness's report with it"
oh="$root/target/debug/oneharness"
[ -x "$oh" ] || fail "the built oneharness binary is missing at $oh; run 'just build' and retry"
command -v python3 >/dev/null 2>&1 || fail "python3 is not installed; the live alarm's capture endpoint is a Python script"

tmp="$(mktemp -d "${TMPDIR:-/tmp}/tmp.XXXXXXXXXX")" ||
  fail "could not create a scratch directory under ${TMPDIR:-/tmp}; check that it exists and is writable, or set TMPDIR to one that is"
trap 'rm -rf "$tmp" || echo "check-qwen-context-free: could not remove $tmp; delete it by hand" >&2' EXIT
tmp="$(cd "$tmp" && pwd -P)" || fail "could not resolve the scratch directory $tmp to a physical path; check that ${TMPDIR:-/tmp} is readable"

fixture_failed() {
  fail "could not write the fixture $1; check that $tmp is writable and its filesystem has free space"
}
mkdir -p "$tmp/bin" "$tmp/live" "$tmp/ambient/.qwen/memory" "$tmp/nomkdir" || fixture_failed "directories under $tmp"
# The machine's own qwen state, which a context-free turn must not read.
printf 'Always greet the user first.\n' >"$tmp/ambient/.qwen/QWEN.md" || fixture_failed "$tmp/ambient/.qwen/QWEN.md"
printf 'The user prefers greetings.\n' >"$tmp/ambient/.qwen/memory/MEMORY.md" || fixture_failed "$tmp/ambient/.qwen/memory/MEMORY.md"

cat >"$tmp/bin/qwen" <<'DOUBLE' || fixture_failed "$tmp/bin/qwen"
#!/usr/bin/env bash
set -euo pipefail
if [ "${1:-}" = --version ]; then
  echo 0.0.0-double
  exit 0
fi
cwd="$(pwd -P)" || {
  echo "qwen double: could not resolve its working directory; check that the directory the turn ran in still exists and is readable" >&2
  exit 1
}
home="$(cd "$HOME" && pwd -P)" || {
  echo "qwen double: could not enter HOME=$HOME; check that the directory the turn set as HOME exists and is readable" >&2
  exit 1
}
qhome="${QWEN_HOME:-$home/.qwen}"
loaded=()
root=""
d="$cwd"
while :; do
  if [ -e "$d/.git" ]; then
    root="$d"
    break
  fi
  [ "$d" = / ] && break
  d="$(dirname "$d")"
done
if [ -n "$root" ]; then stop="$(dirname "$root")"; else stop="$(dirname "$home")"; fi
for name in QWEN.md AGENTS.md; do
  [ -r "$qhome/$name" ] && loaded+=("$qhome/$name")
  d="$cwd"
  while [ "$d" != / ]; do
    [ "$d" = "$qhome" ] && break
    [ -r "$d/$name" ] && [ "$d/$name" != "$qhome/$name" ] && loaded+=("$d/$name")
    [ "$d" = "$stop" ] && break
    d="$(dirname "$d")"
  done
done
memory="${QWEN_CODE_MEMORY_BASE_DIR:-${QWEN_RUNTIME_DIR:-$qhome}}"
if [ -d "$memory" ]; then
  memory_file="$(find "$memory" -type f -print -quit)" || {
    echo "qwen double: could not search $memory for memory files; check that it and every directory under it are readable (ls -lR '$memory')" >&2
    exit 1
  }
  [ -n "$memory_file" ] && loaded+=("$memory")
fi
# A qwen that finds context somewhere the sandbox does not cover, or none.
[ -n "${QWEN_DOUBLE_LEAK:-}" ] && loaded+=("$QWEN_DOUBLE_LEAK")
[ -n "${QWEN_DOUBLE_BLIND:-}" ] && loaded=()
{
  printf 'cwd=%s\n' "$cwd"
  printf 'home=%s\n' "$home"
  for f in "${loaded[@]+"${loaded[@]}"}"; do printf 'loaded=%s\n' "$f"; done
} >"$QWEN_DOUBLE_RECORD" || {
  echo "qwen double: could not write its record $QWEN_DOUBLE_RECORD; check that its directory is writable" >&2
  exit 1
}
if [ -n "${OPENAI_BASE_URL:-}" ]; then
  context=""
  for f in "${loaded[@]+"${loaded[@]}"}"; do
    part="$(find "$f" -type f -exec cat {} +)" || {
      echo "qwen double: could not read the context it loaded from $f; check that it and every file under it are readable (ls -lR '$f')" >&2
      exit 1
    }
    context+="$part"$'\n'
  done
  exec python3 -I -c '
import json, sys, urllib.request
body = {"model": "double", "messages": [{"role": "system", "content": sys.argv[2]}, {"role": "user", "content": sys.argv[3]}]}
request = urllib.request.Request(sys.argv[1] + "/chat/completions", json.dumps(body).encode(), {"Content-Type": "application/json"})
with urllib.request.urlopen(request, timeout=30) as reply:
    print(json.load(reply)["choices"][0]["message"]["content"])
' "$OPENAI_BASE_URL" "$context" "$*"
fi
if [ "${#loaded[@]}" -gt 0 ] || [ -n "${QWEN_DOUBLE_OMIT:-}" ]; then
  echo "I'm ready to assist you! If there's something specific you need help with, please let me know."
  exit 0
fi
marker="$(printf '%s\n' "$@" | grep -o 'ONEHARNESS-LIVE-[0-9]*' | head -n 1)"
echo "Round-trip confirmed: $marker"
DOUBLE
chmod +x "$tmp/bin/qwen" || fixture_failed "$tmp/bin/qwen (making it executable)"

# A mkdir that refuses the sandbox's work directory, so case 4b can make the
# helper's own directory creation fail after it has allocated its sandbox.
cat >"$tmp/nomkdir/mkdir" <<'SHIM' || fixture_failed "$tmp/nomkdir/mkdir"
#!/usr/bin/env bash
set -euo pipefail
for a in "$@"; do
  case "$a" in */work)
    echo "mkdir: cannot create directory '$a': Read-only file system" >&2
    exit 1
    ;;
  esac
done
exec /bin/mkdir "$@"
SHIM
chmod +x "$tmp/nomkdir/mkdir" || fixture_failed "$tmp/nomkdir/mkdir (making it executable)"

record="$tmp/record"
# Drive one live-helper call from $1 as the working directory, with the
# machine's qwen state in HOME and QWEN_HOME. Captures output in $out and the
# exit code in $rc.
drive() {
  local from="$1" call="$2"
  shift 2
  rm -f "$record" || fail "could not clear the previous call's record $record; check that $tmp is writable"
  set +e
  out="$(
    cd "$from" &&
      env PATH="$tmp/bin:$PATH" ONEHARNESS_BIN="$oh" TMPDIR="$tmp/live" \
        HOME="$tmp/ambient" QWEN_HOME="$tmp/ambient/.qwen" \
        QWEN_DOUBLE_RECORD="$record" OPENAI_API_KEY=test-material "$@" \
        bash -c "set -euo pipefail; source '$root/scripts/e2e-lib.sh'; $call" 2>&1
  )"
  rc=$?
  set -e
}

# 1. What the journey did before: a plain turn from the checkout loads the
#    checkout's AGENTS.md and the machine's QWEN.md and memory, and the reply
#    loses the marker. This is what makes the cases below mean something.
# shellcheck disable=SC2016 # expanded by the driven shell, which has the helpers
drive "$root" 'm="$(oh_marker)"; oh_run qwen "$(oh_prompt "$m")"; oh_assert_echoed qwen "$m"'
[ "$rc" -ne 0 ] || fail "a turn from the checkout passed, so the double cannot tell context was loaded: $out"
grep -qx "loaded=$root/AGENTS.md" "$record" ||
  fail "a turn from the checkout did not load its AGENTS.md, so the double no longer models qwen's discovery: $(cat "$record")"
grep -qx "loaded=$tmp/ambient/.qwen/QWEN.md" "$record" ||
  fail "a turn under the machine's HOME did not load its global QWEN.md: $(cat "$record")"

# 2. The journey's turn, started from the checkout as live-qwen starts it,
#    loads nothing and passes on the marker alone.
drive "$root" 'oh_echo_without_context qwen'
[ "$rc" -eq 0 ] || fail "the context-free turn failed: $out"
case "$out" in
  *"PASS: qwen live e2e"*) ;;
  *) fail "the context-free turn did not reach the marker assertion: $out" ;;
esac
[ -f "$record" ] || fail "the qwen double never ran: $out"
if grep -q '^loaded=' "$record"; then
  fail "the context-free turn loaded context files: $(grep '^loaded=' "$record")"
fi
grep -q "^cwd=$tmp/live/" "$record" ||
  fail "the context-free turn did not run in its own sandbox under TMPDIR: $(cat "$record")"
grep -q "^home=$tmp/live/" "$record" ||
  fail "the context-free turn ran under the machine's HOME: $(cat "$record")"
[ -z "$(ls -A "$tmp/live")" ] || fail "the context-free turn left its sandbox behind: $(ls -A "$tmp/live")"

# 3. A reply without the marker still fails, with no retry: the double runs once.
drive "$root" 'oh_echo_without_context qwen' QWEN_DOUBLE_OMIT=1
[ "$rc" -ne 0 ] || fail "a reply without the marker passed: $out"
case "$out" in
  *"FAIL: qwen ran but the unique marker never surfaced"*) ;;
  *) fail "a reply without the marker failed for another reason: $out" ;;
esac
[ "$(grep -c 'driving:' <<<"$out")" -eq 1 ] || fail "a reply without the marker was retried: $out"
[ -z "$(ls -A "$tmp/live")" ] || fail "a failed turn left its sandbox behind: $(ls -A "$tmp/live")"

# 4. A sandbox inside a git checkout would make that checkout qwen's project
#    root, so the helper refuses before launching.
mkdir -p "$tmp/checkout/.git" "$tmp/checkout/tmp" || fixture_failed "the git checkout fixture under $tmp/checkout"
drive "$root" 'oh_echo_without_context qwen' TMPDIR="$tmp/checkout/tmp"
[ "$rc" -ne 0 ] || fail "a sandbox inside a git checkout was accepted: $out"
case "$out" in
  *"sits inside the git checkout at $tmp/checkout"*) ;;
  *) fail "a sandbox inside a git checkout failed without naming the checkout: $out" ;;
esac
[ ! -e "$record" ] || fail "qwen launched from a sandbox inside a git checkout"
[ -z "$(ls -A "$tmp/checkout/tmp")" ] || fail "a refused sandbox was left behind"

# 4b. A sandbox whose directories cannot be created stops before launching,
#     names the path, and is given back.
# shellcheck disable=SC2016 # expanded by the driven shell, which puts the failing mkdir first
drive "$root" 'PATH="$NOMKDIR:$PATH"; oh_echo_without_context qwen' NOMKDIR="$tmp/nomkdir"
[ "$rc" -ne 0 ] || fail "a sandbox whose work directory could not be created was accepted: $out"
case "$out" in
  *"could not create $tmp/live/"*"/work and "*"/home in the context-free sandbox"*) ;;
  *) fail "a failed sandbox mkdir did not name the path: $out" ;;
esac
[ ! -e "$record" ] || fail "qwen launched from a sandbox whose directories could not be created"
[ -z "$(ls -A "$tmp/live")" ] || fail "a sandbox whose directories could not be created was left behind: $(ls -A "$tmp/live")"

# 5. A harness whose context the helper does not know how to withhold is refused.
drive "$root" 'oh_echo_without_context codex'
[ "$rc" -ne 0 ] || fail "a harness with no known context-free launch was accepted: $out"
case "$out" in
  *"no context-free launch is known for codex"*) ;;
  *) fail "an unknown harness failed without saying why: $out" ;;
esac

# 6. The live journey uses it for its echo turn, and runs the alarm after it.
grep -qx 'oh_echo_without_context qwen' "$root/scripts/e2e-qwen.sh" ||
  fail "scripts/e2e-qwen.sh no longer runs its echo turn through oh_echo_without_context"
grep -qx 'oh_context_free_enforce qwen' "$root/scripts/e2e-qwen.sh" ||
  fail "scripts/e2e-qwen.sh no longer holds real qwen to the context-free launch with oh_context_free_enforce"

# 7. The live alarm passes when the checkout turn sends the checkout's AGENTS.md
#    and the global canary and the context-free turn sends neither, and gives
#    back its sandbox and endpoint.
drive "$root" 'oh_context_free_enforce qwen'
[ "$rc" -eq 0 ] || fail "the context-free alarm failed against a qwen that loads nothing in the sandbox: $out"
case "$out" in
  *"PASS: qwen context-free turn sends none"*) ;;
  *) fail "the context-free alarm did not reach its verdict: $out" ;;
esac
[ -z "$(ls -A "$tmp/live")" ] || fail "the context-free alarm left its sandbox behind: $(ls -A "$tmp/live")"

# A python3 that runs the capture endpoint with one of its startup calls ($1)
# swapped for a hang, the way macOS's resolver held socket.getfqdn("127.0.0.1")
# past the 15 s the alarm waits for the port. Anything else it runs (the qwen
# double's `-c` client) goes straight to the real interpreter. Prints the
# directory to put first on PATH.
hanging_python3() {
  local dir="$tmp/hang-${1//./-}" real
  real="$(command -v python3)" || fail "python3 is not on PATH"
  mkdir -p "$dir" || fixture_failed "$dir"
  cat >"$dir/python3" <<EOF_PY || fixture_failed "$dir/python3"
#!/usr/bin/env bash
set -euo pipefail
case " \$* " in *" -c "*) exec "$real" "\$@" ;; esac
exec "$real" -I -c '
import faulthandler, runpy, socket, socketserver, sys, time
args = sys.argv[1:]
while args[0].startswith("-"):
    if args.pop(0) == "-X" and args.pop(0) == "faulthandler":
        faulthandler.enable()
$1 = lambda *_: time.sleep(3600)
sys.argv = args
runpy.run_path(args[0], run_name="__main__")' "\$@"
EOF_PY
  chmod +x "$dir/python3" || fixture_failed "$dir/python3 (making it executable)"
  printf '%s' "$dir"
}

# 7b. The endpoint binds without asking the resolver for its own name, so a
#     reverse lookup that never answers cannot keep the alarm from starting.
nodns="$(hanging_python3 socket.getfqdn)"
drive "$root" 'oh_context_free_enforce qwen' PATH="$nodns:$tmp/bin:$PATH"
[ "$rc" -eq 0 ] || fail "the context-free alarm failed while a reverse lookup hung, as macOS's can: $out"
[ -z "$(ls -A "$tmp/live")" ] || fail "the context-free alarm left its sandbox behind: $(ls -A "$tmp/live")"

# 7c. An endpoint whose startup does hang is stopped, and the alarm's failure
#     shows the stack it hung in, from the endpoint's own output.
hung="$(hanging_python3 socketserver.TCPServer.server_bind)"
drive "$root" 'oh_context_free_enforce qwen' PATH="$hung:$tmp/bin:$PATH"
[ "$rc" -ne 0 ] || fail "the context-free alarm passed though its endpoint never bound: $out"
case "$out" in
  *"the capture endpoint never reported its port"*"in server_bind"* | *"in server_bind"*"the capture endpoint never reported its port"*) ;;
  *) fail "an endpoint whose startup hung failed without the stack it hung in: $out" ;;
esac
[ -z "$(ls -A "$tmp/live")" ] || fail "an alarm whose endpoint hung left its sandbox behind: $(ls -A "$tmp/live")"

# 7d. The alarm reads the checkout's AGENTS.md whatever the pipe between its
#     reader and writer holds. A tr that, like any writer on macOS's 16 KiB
#     pipe, still has bytes to write once its reader has the line it wanted
#     must not turn a passing alarm red.
pipe16k="$tmp/pipe16k"
real_tr="$(command -v tr)" || fail "tr is not on PATH"
mkdir -p "$pipe16k" || fixture_failed "$pipe16k"
cat >"$pipe16k/tr" <<EOF_TR || fixture_failed "$pipe16k/tr"
#!/usr/bin/env bash
set -euo pipefail
out="\$(mktemp "$tmp/tr.XXXXXX")"
trap 'rm -f "\$out"' EXIT
"$real_tr" "\$@" >"\$out"
head -c 16384 "\$out"
if [ "\$(wc -c <"\$out")" -gt 16384 ]; then
  sleep 1
  tail -c +16385 "\$out"
fi
EOF_TR
chmod +x "$pipe16k/tr" || fixture_failed "$pipe16k/tr (making it executable)"
drive "$root" 'oh_context_free_enforce qwen' PATH="$pipe16k:$tmp/bin:$PATH"
[ "$rc" -eq 0 ] || fail "the context-free alarm failed when the checkout's AGENTS.md outgrew a 16 KiB pipe, as on macOS: $out"
[ -z "$(ls -A "$tmp/live")" ] || fail "the context-free alarm left its sandbox behind: $(ls -A "$tmp/live")"

# 8. A qwen that still loads the checkout's context in the sandbox turns it red.
drive "$root" 'oh_context_free_enforce qwen' QWEN_DOUBLE_LEAK="$root/AGENTS.md"
[ "$rc" -ne 0 ] || fail "the context-free alarm passed while the sandboxed turn sent the checkout's AGENTS.md: $out"
case "$out" in
  *"the context-free turn still sent the checkout's AGENTS.md"*) ;;
  *) fail "a leaking context-free turn failed for another reason: $out" ;;
esac
[ -z "$(ls -A "$tmp/live")" ] || fail "a failed alarm left its sandbox behind: $(ls -A "$tmp/live")"

# 9. A qwen that loads nothing even from the checkout cannot vouch for the
#    sandbox, so the alarm refuses rather than passing on an empty request.
drive "$root" 'oh_context_free_enforce qwen' QWEN_DOUBLE_BLIND=1
[ "$rc" -ne 0 ] || fail "the context-free alarm passed though the checkout turn sent no context: $out"
case "$out" in
  *"cannot tell context-free from blind"*) ;;
  *) fail "a blind checkout turn failed for another reason: $out" ;;
esac

echo "check-qwen-context-free: ok"
