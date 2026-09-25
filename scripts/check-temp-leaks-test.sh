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
if ! work="$(mktemp -d)"; then
  echo "check-temp-leaks-test: could not create its scratch directory under ${TMPDIR:-/tmp}." >&2
  echo "  fix: point TMPDIR at a writable directory with free space, then rerun 'bash scripts/check-temp-leaks-test.sh'." >&2
  exit 1
fi
# Scratch left behind is a failure, so a run whose cases all passed still exits
# non-zero when it cannot give its directory back.
cleanup() {
  local status=$?
  if ! rm -rf "$work"; then
    echo "check-temp-leaks-test: could not remove its scratch directory; fix: rm -rf $work" >&2
    [ "$status" -ne 0 ] || status=1
  fi
  exit "$status"
}
trap cleanup EXIT

# Watch only this test's own scratch root, so a real `oneharness` run happening
# elsewhere on the host cannot decide the verdict.
export OH_SCRATCH_ROOTS="$work"

fail() {
  echo "check-temp-leaks-test: $1" >&2
  [ -s "$work/out" ] && cat "$work/out" >&2
  echo "  fix: make scripts/check-temp-leaks.sh satisfy the case above, then rerun 'bash scripts/check-temp-leaks-test.sh'." >&2
  exit 1
}

# A case this platform cannot stage is said rather than passed over: gathered
# into one stderr line at the end, which survives the recipe discarding stdout.
skipped=""
skip() {
  skipped+="${skipped:+; }$1"
}

# Whether this shell can make a real symlink. Git Bash on Windows cannot without
# developer mode: its `ln -s` copies the target, or refuses one that is missing.
can_symlink() {
  ln -s "$work/symlink-target" "$work/symlink-probe" 2>/dev/null &&
    [ -L "$work/symlink-probe" ] && rm -f "$work/symlink-probe"
}
mkdir -p "$work/symlink-target"
symlinks=0
if can_symlink; then symlinks=1; fi
rm -rf "$work/symlink-target" "$work/symlink-probe"

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

# The fix a leak gets is its own suite's helper, never another suite's: a Node or
# Python directory told to use the Rust `ScratchDir` sends its reader nowhere.
rust_fix="io::scratch::ScratchDir"
node_fix="npm/oneharness-sdk/test/scratch.mjs"
python_fix="python/oneharness-sdk/test/scratch.py"
leak_fixes() {
  local dir=$1 wanted=$2 fix
  run_gate "mkdir -p '$work/$dir'" >/dev/null
  rm -rf "${work:?}/$dir"
  for fix in "$rust_fix" "$node_fix" "$python_fix"; do
    if [ "$fix" = "$wanted" ]; then
      grep -qF "$fix" "$work/stderr" || fail "a leaked $dir should have been told to use $fix"
    else
      ! grep -qF "$fix" "$work/stderr" || fail "a leaked $dir was told to use $fix, which does not make it"
    fi
  done
}
leak_fixes oneharness-cli-a1b2c3-4242 "$rust_fix"
leak_fixes oneharness-sdk-int-0a1b2c3d4e5f-4242 "$node_fix"
leak_fixes oneharness-python-watch-ww2lnpwx-4242 "$python_fix"

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

# Another checkout's run is stood in for by a process started outside the gate,
# so it lacks the run's token: it makes `<stem>-<its pid>` only once the watched
# command asks, then stays alive, and the watched command waits to see it made.
foreign_run() {
  local stem=$1
  # shellcheck disable=SC2016  # expanded by the foreign process, not here
  bash -c 'until [ -e "$1.go" ]; do sleep 0.1; done; mkdir "$1-$$"; touch "$1.made"; exec sleep 60' \
    foreign "$work/$stem" >/dev/null 2>&1 &
  echo "$!"
}
# The watched command's half: ask the foreign run for its directory, and wait
# (bounded) until it exists, so the directory is made while this run is going.
ask_foreign="touch \"\$1.go\"; for _ in \$(seq 100); do [ -e \"\$1.made\" ] && exit 0; sleep 0.1; done; exit 9"

# One another checkout's suite made while this run was going, and whose maker is
# still alive, is that suite's scratch — not this run's leak.
foreign=$(foreign_run oneharness-foreign)
if ! bash "$gate" bash -c "$ask_foreign" watched "$work/oneharness-foreign" >"$work/out" 2>&1; then
  kill "$foreign"
  fail "a scratch directory another live run made while this one was going must not be reported as this run's leak"
fi
[ -d "$work/oneharness-foreign-$foreign" ] || fail "the foreign run never made its directory"
grep -q "left out as other checkouts' runs.*$work/oneharness-foreign-$foreign (pid $foreign)" "$work/out" ||
  fail "the gate left out another live run's directory without saying which one or why"
kill "$foreign"
wait "$foreign" 2>/dev/null || true
rm -rf "$work/oneharness-foreign-$foreign" "$work/oneharness-foreign.go" "$work/oneharness-foreign.made"

# The SDK suites' names carry a random part before the pid (Python's `mkdtemp`
# and Node's `randomBytes` make it), and attribution reads only the trailing
# pid: another run's live one is left out, while the same shape ending in the
# watched command's own pid is a leak.
# Without the pid — the shape those helpers used to make — nothing names a
# maker, so a concurrent SDK run elsewhere on the host failed this run's gate.
foreign=$(foreign_run oneharness-python-installed-mq_z5o)
if ! bash "$gate" bash -c "$ask_foreign" watched "$work/oneharness-python-installed-mq_z5o" >"$work/out" 2>&1; then
  kill "$foreign"
  fail "an SDK-shaped scratch directory another live run made while this one was going must not be reported as this run's leak"
fi
[ -d "$work/oneharness-python-installed-mq_z5o-$foreign" ] || fail "the foreign SDK-shaped run never made its directory"
kill "$foreign"
wait "$foreign" 2>/dev/null || true
rm -rf "$work/oneharness-python-installed-mq_z5o-$foreign" "$work/oneharness-python-installed-mq_z5o.go" "$work/oneharness-python-installed-mq_z5o.made"
if bash "$gate" bash -c "mkdir -p \"$work/oneharness-sdk-probe-3fa9c1-\$\$\"; echo \$\$ > '$work/own'" >"$work/out" 2>&1; then
  fail "an SDK-shaped scratch directory ending in the watched command's own pid should have been reported"
fi
own=$(cat "$work/own")
grep -q "oneharness-sdk-probe-3fa9c1-$own" "$work/out" ||
  fail "the gate failed but did not name the SDK-shaped directory the watched command left"
rm -rf "$work/oneharness-sdk-probe-3fa9c1-$own" "$work/own"

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

# A child of the watched command that outlives it after clearing the run's
# marker cannot be told from another checkout's run, so its directory is left
# out — but named, never dropped silently.
bash "$gate" bash -c "env -u OH_TEMP_LEAKS_RUN sleep 60 >/dev/null 2>&1 & echo \$! > '$work/child'; mkdir -p '$work/oneharness-unmarked-'\$!" >"$work/out" 2>&1 &&
  unmarked=0 || unmarked=$?
child=$(cat "$work/child")
kill "$child" 2>/dev/null || true
[ "$unmarked" -eq 0 ] ||
  fail "a directory whose live maker lacks the run's marker should be left out, got exit $unmarked"
grep -q "left out as other checkouts' runs.*$work/oneharness-unmarked-$child (pid $child)" "$work/out" ||
  fail "the gate left out a directory whose live maker cleared the run's marker without naming it"
rm -rf "$work/oneharness-unmarked-$child" "$work/child"

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

# ...and so is one whose live maker's environment this gate may not read — here
# pid 1, another user's process, where `ps` prints the command without it.
# Root reads every environment, so there the case cannot be staged.
if [ "$(id -u)" -ne 0 ]; then
  if bash "$gate" bash -c "mkdir -p '$work/oneharness-unreadable-1'" >"$work/out" 2>&1; then
    fail "a scratch directory whose maker's environment cannot be read should have been reported"
  fi
  grep -q "oneharness-unreadable-1" "$work/out" ||
    fail "the gate failed but did not name the directory whose maker it could not read"
  rm -rf "$work/oneharness-unreadable-1"
else
  skip "a maker whose environment cannot be read: root reads every process's environment"
fi

# A root the command creates is swept after it, so a leak inside it is caught.
if OH_SCRATCH_ROOTS="$work/made-later" \
  bash "$gate" bash -c "mkdir -p '$work/made-later/oneharness-in-a-new-root'" >"$work/out" 2>&1; then
  fail "a leak under a scratch root the command created should have been reported"
fi
grep -q "oneharness-in-a-new-root" "$work/out" ||
  fail "the gate went red under a root the command created without naming the directory left behind"
rm -rf "$work/made-later"

# A root reached through a symlink still watches the directory behind it.
if [ "$symlinks" -eq 1 ]; then
  mkdir -p "$work/behind-a-symlink"
  ln -s "$work/behind-a-symlink" "$work/through-a-symlink"
  if OH_SCRATCH_ROOTS="$work/through-a-symlink" \
    bash "$gate" bash -c "mkdir -p '$work/behind-a-symlink/oneharness-under-a-symlink'" >"$work/out" 2>&1; then
    fail "a leak under a symlinked root should have been reported"
  fi
  grep -q "oneharness-under-a-symlink" "$work/out" ||
    fail "the gate went red under a symlinked root without naming the directory left behind"
  rm -rf "$work/through-a-symlink" "$work/behind-a-symlink"
fi

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
[ -z "$FAKE_FIND_ERROR" ] || printf '%b\n' "${FAKE_FIND_ERROR//@ROOT@/$1}" >&2
exit "${FAKE_FIND_STATUS:-1}"
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
# ...even when find reports the error and still exits 0: what it said decides.
set +e
PATH="$work/fakebin:$PATH" FAKE_FIND_STATUS=0 FAKE_FIND_ERROR="find: '@ROOT@/oneharness-x': Input/output error" \
  bash "$gate" bash -c "touch '$work/ran'" >"$work/out" 2>&1
status=$?
set -e
[ "$status" -eq 2 ] || fail "a sweep whose find exited 0 after an error should be a usage error (exit 2), got $status"
[ ! -e "$work/ran" ] || fail "the gate should not run its command over a sweep find reported an error in"
# The root itself gone, a vanished entry beside a real failure, and a failure
# that says nothing at all are not a vanished entry either.
for diagnostic in \
  "" \
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
# The sweep after the command is held to the same rule: armed only by the
# command, the same failures turn a clean run red and a vanished entry does not.
real_find=$(command -v find)
cat >"$work/fakebin/find" <<'FIND'
#!/usr/bin/env bash
[ -e "$FAKE_FIND_ARMED" ] || exec "$REAL_FIND" "$@"
printf '%b\n' "${FAKE_FIND_ERROR//@ROOT@/$1}" >&2
exit 1
FIND
arm="touch '$work/armed'"
set +e
PATH="$work/fakebin:$PATH" REAL_FIND="$real_find" FAKE_FIND_ARMED="$work/armed" \
  FAKE_FIND_ERROR="find: '@ROOT@/oneharness-x': Input/output error" \
  bash "$gate" bash -c "$arm" >"$work/out" 2>&1
status=$?
set -e
[ "$status" -eq 2 ] || fail "a sweep after the command that find could not finish should fail the gate (exit 2), got $status"
grep -q "Input/output error" "$work/out" ||
  fail "the gate should say what stopped its sweep after the command"
rm -f "$work/armed"
if ! PATH="$work/fakebin:$PATH" REAL_FIND="$real_find" FAKE_FIND_ARMED="$work/armed" \
  FAKE_FIND_ERROR="find: '@ROOT@/oneharness-x': No such file or directory" \
  bash "$gate" bash -c "$arm" >"$work/out" 2>&1; then
  fail "an entry that vanished during the sweep after the command must not fail the gate"
fi
rm -rf "$work/fakebin" "$work/ran" "$work/armed"

# A temp dir the gate cannot write its own files into is refused, and said. Both
# fixtures are ones no user can write through, root included: a path that does
# not exist, and a regular file.
touch "$work/a-file"
for unwritable in "$work/no-such-dir" "$work/a-file"; do
  set +e
  TMPDIR="$unwritable" bash "$gate" bash -c "touch '$work/ran'" >"$work/out" 2>&1
  status=$?
  set -e
  [ "$status" -eq 2 ] || fail "a temp dir the gate cannot write to ($unwritable) should be a usage error (exit 2), got $status"
  [ ! -e "$work/ran" ] || fail "the gate should not run its command without its own files ($unwritable)"
  grep -q "could not create its own working directory under $unwritable" "$work/out" ||
    fail "the gate should say it could not create its working directory under $unwritable"
done
rm -f "$work/a-file"
# ...even where a bare `mktemp -d` would make one elsewhere, as it did on macOS:
# stood in for by a `mktemp` that ignores an unusable TMPDIR unless handed a
# template naming the directory.
mkdir -p "$work/fakebin" "$work/elsewhere"
real_mktemp=$(command -v mktemp)
cat >"$work/fakebin/mktemp" <<'MKTEMP'
#!/usr/bin/env bash
[ "$#" -eq 1 ] && [ "$1" = "-d" ] && exec "$REAL_MKTEMP" -d "$FALLBACK_TMP/tmp.XXXXXX"
exec "$REAL_MKTEMP" "$@"
MKTEMP
chmod +x "$work/fakebin/mktemp"
set +e
PATH="$work/fakebin:$PATH" REAL_MKTEMP="$real_mktemp" FALLBACK_TMP="$work/elsewhere" \
  TMPDIR="$work/no-such-dir" bash "$gate" bash -c "touch '$work/ran'" >"$work/out" 2>&1
status=$?
set -e
[ "$status" -eq 2 ] || fail "a temp dir the gate cannot write to should be refused even by a mktemp that falls back elsewhere, got $status"
[ ! -e "$work/ran" ] || fail "the gate should not run its command over a temp dir it cannot write to"
rm -rf "$work/fakebin" "$work/elsewhere"

# So is a symlink leading nowhere, which is not an absent root: whatever it was
# meant to watch, sweeping it would see nothing.
if [ "$symlinks" -eq 1 ]; then
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
fi

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

# So is one the command removes outright: a root that is gone lists nothing,
# which is not the same as listing no leak.
mkdir -p "$work/removed"
set +e
OH_SCRATCH_ROOTS="$work/removed" bash "$gate" bash -c "mkdir '$work/removed/oneharness-in-removed' && rm -rf '$work/removed'" >"$work/out" 2>&1
status=$?
set -e
[ "$status" -eq 2 ] || fail "a scratch root the command removed should fail the gate (exit 2), got $status"
grep -q "cannot watch scratch root '$work/removed': it was removed while the command ran" "$work/out" ||
  fail "the gate should name the scratch root the command removed"
# ...while one that never existed and still does not is not watched at all.
if ! OH_SCRATCH_ROOTS="$work/never-made" bash "$gate" true >"$work/out" 2>&1; then
  fail "a scratch root absent before and after the command must not fail the gate"
fi

# And one it can enter but not list — staged only where the fixture holds, since
# root lists every directory and Windows ignores the mode.
mkdir -p "$work/unlistable"
chmod 311 "$work/unlistable"
if ! ls "$work/unlistable" >/dev/null 2>&1; then
  set +e
  OH_SCRATCH_ROOTS="$work/unlistable" bash "$gate" true >"$work/out" 2>&1
  status=$?
  set -e
  chmod 755 "$work/unlistable"
  [ "$status" -eq 2 ] || fail "a scratch root the gate cannot list should be a usage error (exit 2), got $status"
  grep -q "cannot watch scratch root '$work/unlistable'" "$work/out" ||
    fail "the gate should name the scratch root it cannot list"
else
  chmod 755 "$work/unlistable"
  skip "a scratch root the gate cannot list: this user can still list a directory whose read permission is removed"
fi
rmdir "$work/unlistable"

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

# A working directory the gate cannot remove afterwards is named, with how to
# remove it, and a failed command's status still wins.
mkdir -p "$work/refusing-rm"
real_rm=$(command -v rm)
cat >"$work/refusing-rm/rm" <<RM
#!/usr/bin/env bash
for arg in "\$@"; do
  case "\$arg" in */check-temp-leaks.*) exit 1 ;; esac
done
exec $real_rm "\$@"
RM
chmod +x "$work/refusing-rm/rm"
for wrapped in "true:1" "exit 3:3"; do
  set +e
  TMPDIR="$work/refusing-rm" PATH="$work/refusing-rm:$PATH" bash "$gate" bash -c "${wrapped%%:*}" >"$work/out" 2>&1
  status=$?
  set -e
  [ "$status" -eq "${wrapped##*:}" ] ||
    fail "a working directory the gate cannot remove after '${wrapped%%:*}' should exit ${wrapped##*:}, got $status"
  left=$(find "$work/refusing-rm" -mindepth 1 -maxdepth 1 -name 'check-temp-leaks.*')
  [ -n "$left" ] || fail "the refusing rm should have left the gate's working directory behind to report"
  grep -q "could not remove its working directory $left" "$work/out" ||
    fail "the gate should name the working directory it could not remove after '${wrapped%%:*}'"
  grep -q "rm -rf $left" "$work/out" || fail "the gate should say how to remove the working directory it left"
  "$real_rm" -rf "$left"
done
rm -rf "$work/refusing-rm"

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
tail -n 1 "$work/stderr" | grep -q "fix: run that command directly" ||
  fail "the silent-failure report must end with what to do next; it ended: $(tail -n 1 "$work/stderr")"
[ ! -s "$work/stdout" ] ||
  fail "the gate's own diagnostics belong on stderr; stdout carried: $(cat "$work/stdout")"

# ...and a command that failed *and* spoke is accounted for by its own output,
# not by that stand-in.
status=$(run_gate "$chatter; exit 4")
[ "$status" -eq 4 ] || fail "a chatty failing command must keep its status; got $status"
grep -q "without printing anything" "$work/stderr" &&
  fail "a command that printed must not be reported as silent"
tail -n 1 "$work/stderr" | grep -q "fix: resolve the first error in that output" ||
  fail "a chatty failure must end with what to do next; it ended: $(tail -n 1 "$work/stderr")"

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
tail -n 1 "$work/out" | grep -q "fix: run that command directly" ||
  fail "a command that failed AND leaked must end with what to do about the failure; it ended: $(tail -n 1 "$work/out")"
rm -rf "$work/oneharness-failed-and-leaked"

# Asked to watch nothing at all, the gate says what to pass rather than
# reporting a vacuous pass.
status=0
bash "$gate" >"$work/out" 2>&1 || status=$?
[ "$status" -eq 2 ] || fail "a gate with no command must be a usage error; got $status"
grep -q "no command to run" "$work/out" ||
  fail "the usage error must say what is missing"

if [ "$symlinks" -eq 0 ]; then
  skip "a leak under a symlinked root, and a dangling symlink as a scratch root: this shell cannot create a symlink (ln -s copies or refuses; on Windows it needs developer mode)"
fi
# One line either way; a skip goes to stderr so the gate, which discards
# stdout, still shows what went unproven.
done_line="check-temp-leaks-test: the scratch-leak gate goes red for a leaked directory and green otherwise"
if [ -z "$skipped" ]; then
  echo "$done_line"
else
  echo "$done_line; skipped what this platform cannot stage — $skipped" >&2
fi
