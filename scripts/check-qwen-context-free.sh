#!/usr/bin/env bash
# Hermetic check for oh_echo_without_context (scripts/e2e-lib.sh), the live
# qwen journey's echo turn. It must start qwen with none of the checkout's or
# the machine's context or memory files loaded, and must still fail when the
# reply lacks the marker. The real built oneharness drives a qwen double that
# applies qwen's own context-discovery rules (packages/core memory discovery:
# QWEN.md and AGENTS.md, global under $QWEN_HOME or ~/.qwen, then upward from
# the cwd to the git root, or to HOME's parent when there is none, plus
# auto-memory under QWEN_CODE_MEMORY_BASE_DIR / QWEN_RUNTIME_DIR / QWEN_HOME).
# The double answers the way gpt-4o-mini did on macOS CI once context loaded.
# llmlint: ignore-file[e2e_not_mocked] This drives the real built oneharness CLI and the live suite's own helper; only the paid qwen CLI is replaced, by a double applying qwen's documented context discovery, because what is under test is which files a launch exposes to it, not the model. scripts/e2e-qwen.sh remains the real-CLI drift alarm.
set -euo pipefail

case $(uname -s) in
  MINGW* | MSYS* | CYGWIN*)
    echo "check-qwen-context-free: skipped on Windows because this Unix behavioral harness relies on an extensionless executable double" >&2
    exit 0
    ;;
esac

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

fail() {
  echo "check-qwen-context-free: $1" >&2
  echo "  Re-run with: bash -x scripts/check-qwen-context-free.sh" >&2
  exit 1
}

command -v jq >/dev/null 2>&1 || fail "jq is not installed; the live helpers parse oneharness's report with it"
oh="$root/target/debug/oneharness"
[ -x "$oh" ] || fail "the built oneharness binary is missing at $oh; run 'cargo build' and retry"

tmp="$(mktemp -d "${TMPDIR:-/tmp}/tmp.XXXXXXXXXX")" ||
  fail "could not create a scratch directory under ${TMPDIR:-/tmp}; check that it exists and is writable, or set TMPDIR to one that is"
trap 'rm -rf "$tmp" || echo "check-qwen-context-free: could not remove $tmp; delete it by hand" >&2' EXIT
tmp="$(cd "$tmp" && pwd -P)" || fail "could not resolve the scratch directory $tmp to a physical path; check that ${TMPDIR:-/tmp} is readable"

# Stop with the path when a fixture cannot be written.
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
cwd="$(pwd -P)"
home="$(cd "$HOME" && pwd -P)"
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
if [ -d "$memory" ] && [ -n "$(find "$memory" -type f -print -quit)" ]; then
  loaded+=("$memory")
fi
{
  printf 'cwd=%s\n' "$cwd"
  printf 'home=%s\n' "$home"
  for f in "${loaded[@]+"${loaded[@]}"}"; do printf 'loaded=%s\n' "$f"; done
} >"$QWEN_DOUBLE_RECORD"
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

# 6. The live journey uses it for its echo turn.
grep -qx 'oh_echo_without_context qwen' "$root/scripts/e2e-qwen.sh" ||
  fail "scripts/e2e-qwen.sh no longer runs its echo turn through oh_echo_without_context"

echo "check-qwen-context-free: ok"
