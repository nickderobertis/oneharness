#!/usr/bin/env bash
#
# Behavioral test of the scratch-leak gate.
#
# A gate nobody has watched fail is not known to work — and this one's whole job
# is to fail. So it is driven against a command that leaks a scratch directory
# and asserted to go red naming it, against one that cleans up after itself and
# asserted to stay green, and against a failing command to prove the command's
# own status still wins.
#
# Its output contract is exercised from both ends too, because the two halves
# pull against each other: a clean run must say nothing at all, and each way of
# going red must still carry every line the wrapped command wrote.
#
# Quiet on success, one line. On failure it prints what the gate said.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

gate="scripts/check-temp-leaks.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# Watch only this test's own scratch root, so a real `oneharness` run happening
# elsewhere on the host cannot decide the verdict.
export OH_SCRATCH_ROOTS="$work"

fail() {
  echo "check-temp-leaks-test: $1" >&2
  [ -s "$work/out" ] && cat "$work/out" >&2
  echo "  fix: make scripts/check-temp-leaks.sh satisfy the case above, then rerun 'bash scripts/check-temp-leaks-test.sh'." >&2
  exit 1
}

# What a wrapped command writes, on both streams, so a replay can be told apart
# from a gate that merely happens to print something.
chatter_out="a line the wrapped command wrote to stdout"
chatter_err="a line the wrapped command wrote to stderr"
chatter="printf '%s\\n' \"$chatter_out\"; printf '%s\\n' \"$chatter_err\" >&2"

# Run the gate with the streams kept apart, leaving the combined text in
# `$work/out` for the diagnostics `fail` prints.
run_gate() {
  local status=0
  bash "$gate" bash -c "$1" >"$work/stdout" 2>"$work/stderr" || status=$?
  cat "$work/stdout" "$work/stderr" >"$work/out"
  printf '%s' "$status"
}

# A command that cleans up after itself is green, and its own exit status is
# what comes back. Anchoring on this first means a later red is the leak rather
# than a gate that rejects everything.
if ! bash "$gate" bash -c "mkdir -p '$work/oneharness-tidy' && rmdir '$work/oneharness-tidy'" >"$work/out" 2>&1; then
  fail "a command that removed its own scratch directory should have passed"
fi

# A command that leaves one behind is red, and the gate names it. The prefix here
# is written out rather than read from `io::scratch::PREFIX` on purpose: the gate
# reads that constant, so a literal is what notices the two drifting apart.
if bash "$gate" bash -c "mkdir -p '$work/oneharness-leaked'" >"$work/out" 2>&1; then
  fail "a command that left a scratch directory behind should have failed"
fi
grep -q "oneharness-leaked" "$work/out" ||
  fail "the gate failed but did not name the directory that was left behind"
rm -rf "$work/oneharness-leaked"

# A directory some other tool left is not a scratch directory: the sweep is
# keyed on the prefix `io::scratch` mints, not on anything in the temp dir.
if ! bash "$gate" bash -c "mkdir -p '$work/some-other-tool'" >"$work/out" 2>&1; then
  fail "a directory outside the scratch prefix must not be reported"
fi
rm -rf "$work/some-other-tool"

# A directory that was already there is not this run's leak.
mkdir -p "$work/oneharness-pre-existing"
if ! bash "$gate" true >"$work/out" 2>&1; then
  fail "a directory that predates the run must not be reported as its leak"
fi
rm -rf "$work/oneharness-pre-existing"

# A temp *file* is not a leak: the temp directory is shared with real
# `oneharness` runs, which write and clean up files of their own.
if ! bash "$gate" bash -c "touch '$work/oneharness-left.txt'" >"$work/out" 2>&1; then
  fail "a temp file must not be read as a leaked scratch directory"
fi
rm -f "$work/oneharness-left.txt"

# A clean run says nothing at all — not the command's output, not the gate's.
# A gate that echoed a passing 1,187-test suite would bury the run that failed.
status=$(run_gate "$chatter")
[ "$status" -eq 0 ] || fail "a chatty command that did not leak should have passed; got $status"
[ ! -s "$work/stdout" ] ||
  fail "a successful run must leave stdout empty; it carried: $(cat "$work/stdout")"
[ ! -s "$work/stderr" ] ||
  fail "a successful run must leave stderr empty; it carried: $(cat "$work/stderr")"

# A failing command keeps its own status, so the gate never turns a red suite
# green (or reports a leak in place of the failure that caused it) — and every
# line it wrote comes back, on stderr, which is where a gate's diagnostics go.
status=$(run_gate "$chatter; exit 3")
[ "$status" -eq 3 ] || fail "the command's exit status must survive the gate; got $status"
grep -qF "$chatter_out" "$work/stderr" ||
  fail "a failing command's stdout must be replayed"
grep -qF "$chatter_err" "$work/stderr" ||
  fail "a failing command's stderr must be replayed"

# A command that fails without printing anything leaves a bare exit code from a
# step the caller did not run itself, so the gate accounts for it.
status=$(run_gate "exit 4")
[ "$status" -eq 4 ] || fail "a silent failing command must keep its status; got $status"
grep -q "exited 4 without printing anything" "$work/stderr" ||
  fail "a silent failure must be reported rather than left as a bare exit code"
grep -q "run that command directly" "$work/stderr" ||
  fail "the silent-failure report must say what to do next"
[ ! -s "$work/stdout" ] ||
  fail "the gate's own diagnostics belong on stderr; stdout carried: $(cat "$work/stdout")"

# ...and a command that failed *and* spoke is accounted for by its own output,
# not by that stand-in.
status=$(run_gate "$chatter; exit 4")
[ "$status" -eq 4 ] || fail "a chatty failing command must keep its status; got $status"
grep -q "without printing anything" "$work/stderr" &&
  fail "a command that printed must not be reported as silent"

# A leak is the other way of going red, and it replays just as much: on a clean
# exit the command's own output is the only account of what it was doing when it
# abandoned the directory.
status=$(run_gate "$chatter; mkdir -p '$work/oneharness-chatty-leak'")
[ "$status" -eq 1 ] || fail "a command that leaked should have failed; got $status"
grep -qF "$chatter_out" "$work/stderr" ||
  fail "a leaking command's stdout must be replayed"
grep -qF "$chatter_err" "$work/stderr" ||
  fail "a leaking command's stderr must be replayed"
grep -q "oneharness-chatty-leak" "$work/stderr" ||
  fail "the leak diagnostic must survive alongside the replayed output"
rm -rf "$work/oneharness-chatty-leak"

# ...including when it also leaked: the leak is still named, but the failure
# that probably caused it is what the caller is sent to first.
status=0
bash "$gate" bash -c "mkdir -p '$work/oneharness-failed-and-leaked'; exit 3" >"$work/out" 2>&1 || status=$?
[ "$status" -eq 3 ] ||
  fail "a command that failed AND leaked must keep its own status; got $status"
grep -q "oneharness-failed-and-leaked" "$work/out" ||
  fail "the leak must still be named even when the command's status wins"
rm -rf "$work/oneharness-failed-and-leaked"

# Asked to watch nothing at all, the gate says what to pass rather than
# reporting a vacuous pass.
status=0
bash "$gate" >"$work/out" 2>&1 || status=$?
[ "$status" -eq 2 ] || fail "a gate with no command must be a usage error; got $status"
grep -q "no command to run" "$work/out" ||
  fail "the usage error must say what is missing"

echo "check-temp-leaks-test: the scratch-leak gate goes red for a leaked directory and green otherwise"
