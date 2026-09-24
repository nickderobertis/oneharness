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

# One another checkout's suite made while this run was going, and whose maker is
# still alive, is that suite's scratch — not this run's leak.
sleep 60 &
foreign=$!
if ! bash "$gate" bash -c "mkdir -p '$work/oneharness-foreign-$foreign'" >"$work/out" 2>&1; then
  kill "$foreign"
  fail "a scratch directory whose making process is still alive must not be reported as this run's leak"
fi
kill "$foreign"
wait "$foreign" 2>/dev/null || true
rm -rf "$work/oneharness-foreign-$foreign"

# ...but one the watched command's own child made is this run's, even while that
# child outlives the command.
bash "$gate" bash -c "sleep 60 >/dev/null 2>&1 & echo \$! > '$work/child'; mkdir -p '$work/oneharness-outlived-'\$!" >"$work/out" 2>&1 &&
  outlived=0 || outlived=$?
child=$(cat "$work/child")
kill "$child" 2>/dev/null || true
[ "$outlived" -ne 0 ] ||
  fail "a scratch directory made by a child of the watched command should be reported even while that child lives"
grep -q "oneharness-outlived-$child" "$work/out" ||
  fail "the gate failed but did not name the directory the command's child left behind"
rm -rf "$work/oneharness-outlived-$child" "$work/child"

# ...while one whose maker has exited is still a leak, pid suffix and all.
sh -c 'exit 0' &
exited=$!
wait "$exited"
if bash "$gate" bash -c "mkdir -p '$work/oneharness-abandoned-$exited'" >"$work/out" 2>&1; then
  fail "a scratch directory whose making process has exited should have been reported"
fi
grep -q "oneharness-abandoned-$exited" "$work/out" ||
  fail "the gate failed but did not name the abandoned directory"
rm -rf "$work/oneharness-abandoned-$exited"

# A root reached through a symlink still watches the directory behind it.
mkdir -p "$work/behind-a-symlink"
ln -s "$work/behind-a-symlink" "$work/through-a-symlink"
if OH_SCRATCH_ROOTS="$work/through-a-symlink" \
  bash "$gate" bash -c "mkdir -p '$work/behind-a-symlink/oneharness-under-a-symlink'" >"$work/out" 2>&1; then
  fail "a leak under a symlinked root should have been reported"
fi
grep -q "oneharness-under-a-symlink" "$work/out" ||
  fail "the gate went red under a symlinked root without naming the directory left behind"
rm -rf "$work/through-a-symlink" "$work/behind-a-symlink"

# A root that exists but cannot be swept is refused before the command runs,
# rather than skipped into a clean verdict.
touch "$work/not-a-directory"
set +e
OH_SCRATCH_ROOTS="$work/not-a-directory" bash "$gate" bash -c "touch '$work/ran'" >"$work/out" 2>&1
status=$?
set -e
[ "$status" -eq 2 ] || fail "an unsweepable scratch root should be a usage error (exit 2), got $status"
[ ! -e "$work/ran" ] || fail "the gate should not run its command over an unsweepable scratch root"
grep -q "cannot watch scratch root '$work/not-a-directory'" "$work/out" ||
  fail "the gate should name the scratch root it cannot watch"
rm -f "$work/not-a-directory"

# So is a root list that names no root at all.
for roots in ":" "::"; do
  set +e
  OH_SCRATCH_ROOTS="$roots" bash "$gate" bash -c "touch '$work/ran'" >"$work/out" 2>&1
  status=$?
  set -e
  [ "$status" -eq 2 ] || fail "OH_SCRATCH_ROOTS='$roots' should be a usage error (exit 2), got $status"
  [ ! -e "$work/ran" ] || fail "the gate should not run its command when OH_SCRATCH_ROOTS='$roots' names no root"
  grep -q "names no scratch root to watch" "$work/out" ||
    fail "the gate should say OH_SCRATCH_ROOTS='$roots' names no root"
done

# A sweep `find` could not finish is refused rather than read as a clean listing.
# Only an entry that vanished mid-sweep — another process cleaning up on the
# shared temp dir — is not a failure.
mkdir -p "$work/fakebin"
cat >"$work/fakebin/find" <<'FIND'
#!/usr/bin/env bash
printf '%b\n' "${FAKE_FIND_ERROR//@ROOT@/$1}" >&2
exit 1
FIND
chmod +x "$work/fakebin/find"
set +e
PATH="$work/fakebin:$PATH" FAKE_FIND_ERROR="find: '@ROOT@/oneharness-x': Input/output error" \
  bash "$gate" bash -c "touch '$work/ran'" >"$work/out" 2>&1
status=$?
set -e
[ "$status" -eq 2 ] || fail "a sweep find could not finish should be a usage error (exit 2), got $status"
[ ! -e "$work/ran" ] || fail "the gate should not run its command over a sweep it could not finish"
grep -q "Input/output error" "$work/out" ||
  fail "the gate should say what stopped its sweep"
# The root itself gone, and a vanished entry beside a real failure, are not a
# vanished entry either.
for diagnostic in \
  "find: '@ROOT@': No such file or directory" \
  "find: '@ROOT@/oneharness-x': No such file or directory\nfind: '@ROOT@/oneharness-y': Input/output error"; do
  set +e
  PATH="$work/fakebin:$PATH" FAKE_FIND_ERROR="$diagnostic" bash "$gate" true >"$work/out" 2>&1
  status=$?
  set -e
  [ "$status" -eq 2 ] || fail "a sweep that said '$diagnostic' should be a usage error (exit 2), got $status"
done
for diagnostic in \
  "find: '@ROOT@/oneharness-x': No such file or directory" \
  "find: @ROOT@/oneharness-x: No such file or directory"; do
  if ! PATH="$work/fakebin:$PATH" FAKE_FIND_ERROR="$diagnostic" bash "$gate" true >"$work/out" 2>&1; then
    fail "an entry that vanished mid-sweep ('$diagnostic') must not fail the gate"
  fi
done
rm -rf "$work/fakebin" "$work/ran"

# So is a symlink leading nowhere, which is not an absent root: whatever it was
# meant to watch, sweeping it would see nothing.
ln -s "$work/nowhere" "$work/dangling"
set +e
OH_SCRATCH_ROOTS="$work/dangling" bash "$gate" bash -c "touch '$work/ran'" >"$work/out" 2>&1
status=$?
set -e
[ "$status" -eq 2 ] || fail "a dangling symlink as a scratch root should be a usage error (exit 2), got $status"
[ ! -e "$work/ran" ] || fail "the gate should not run its command over a dangling scratch root"
grep -q "cannot watch scratch root '$work/dangling'" "$work/out" ||
  fail "the gate should name the dangling scratch root it cannot watch"
rm -f "$work/dangling"

# So is one the command leaves unsweepable, which would otherwise read as clean.
mkdir -p "$work/goes-away"
set +e
OH_SCRATCH_ROOTS="$work/goes-away" bash "$gate" bash -c "rmdir '$work/goes-away' && touch '$work/goes-away'" >"$work/out" 2>&1
status=$?
set -e
[ "$status" -eq 2 ] || fail "a scratch root the command left unsweepable should fail the gate (exit 2), got $status"
grep -q "cannot watch scratch root '$work/goes-away'" "$work/out" ||
  fail "the gate should name the scratch root the command left unsweepable"
rm -f "$work/goes-away"

# And one it can enter but not list. Root reads every directory, so there the
# case cannot be staged.
if [ "$(id -u)" -ne 0 ]; then
  mkdir -p "$work/unlistable"
  chmod 311 "$work/unlistable"
  set +e
  OH_SCRATCH_ROOTS="$work/unlistable" bash "$gate" true >"$work/out" 2>&1
  status=$?
  set -e
  chmod 755 "$work/unlistable"
  [ "$status" -eq 2 ] || fail "a scratch root the gate cannot list should be a usage error (exit 2), got $status"
  grep -q "cannot watch scratch root '$work/unlistable'" "$work/out" ||
    fail "the gate should name the scratch root it cannot list"
  rmdir "$work/unlistable"
fi

# A command that fails and leaves a root unsweepable keeps its own status, and
# the unsweepable root is still named.
mkdir -p "$work/goes-away"
set +e
OH_SCRATCH_ROOTS="$work/goes-away" bash "$gate" bash -c "rmdir '$work/goes-away' && touch '$work/goes-away' && exit 3" >"$work/out" 2>&1
status=$?
set -e
[ "$status" -eq 3 ] || fail "a failed command's exit status 3 should win over an unsweepable root, got $status"
grep -q "cannot watch scratch root '$work/goes-away'" "$work/out" ||
  fail "the gate should still name the scratch root a failed command left unsweepable"
rm -f "$work/goes-away"

# Resolving the roots leaves the command in the directory it was started from.
mkdir -p "$work/started-here"
if ! (cd "$work/started-here" && bash "$repo_root/$gate" bash -c "pwd -P > '$work/cwd'") >"$work/out" 2>&1; then
  fail "the gate should run a clean command to success"
fi
[ "$(cat "$work/cwd" 2>/dev/null)" = "$(cd "$work/started-here" && pwd -P)" ] ||
  fail "the gate should run its command in the caller's directory, not in a scratch root"
rm -rf "$work/started-here" "$work/cwd"

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
