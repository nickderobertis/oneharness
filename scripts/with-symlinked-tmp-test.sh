#!/usr/bin/env bash
#
# Behavioral test of the symlinked-TMPDIR lane (`scripts/with-symlinked-tmp.sh`,
# which `just test-symlinked-tmp` runs the CLI journeys through).
#
# The lane is only worth its minutes while the command it wraps really sees a
# `$TMPDIR` spelled through a symlink, while a scratch directory leaked behind
# that symlink still turns it red, and while it runs nothing off Linux — so each
# of those is driven here against a probe command rather than read off the
# script.
#
# Quiet on success, one line. On failure it prints what the lane said.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

lane="scripts/with-symlinked-tmp.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# The lane's scratch root lands under this test's own directory, and the leak
# gate runs on its default roots, exactly as the recipe runs it.
export TMPDIR="$work"
unset OH_SCRATCH_ROOTS OH_SYMLINKED_TMP_UNAME

fail() {
  echo "with-symlinked-tmp-test: $1" >&2
  [ -s "$work/out" ] && cat "$work/out" >&2
  echo "  fix: make $lane satisfy the case above, then rerun 'bash scripts/with-symlinked-tmp-test.sh'." >&2
  exit 1
}

# The wrapped command sees a `$TMPDIR` that is a symlink to a real directory.
# shellcheck disable=SC2016  # the probe's $TMPDIR must expand inside the wrapped command, not here
probe='[ -L "$TMPDIR" ] || { echo "TMPDIR is not a symlink: $TMPDIR"; exit 7; }
printf "%s\n" "$TMPDIR" > "$1/seen"
(cd "$TMPDIR" && pwd -P) > "$1/resolved"'
if ! bash "$lane" bash -c "$probe" probe "$work" >"$work/out" 2>&1; then
  fail "the wrapped command should have seen a TMPDIR spelled through a symlink"
fi
seen=$(cat "$work/seen")
resolved=$(cat "$work/resolved")
[ "$seen" != "$resolved" ] ||
  fail "TMPDIR ($seen) should be spelled differently from the directory it resolves to"
[ ! -e "$seen" ] && [ ! -e "$resolved" ] ||
  fail "the lane should remove its symlinked root once the command ends: $seen"
[ -z "$(find "$work" -mindepth 1 -maxdepth 1 -name 'symlinked-tmp.*')" ] ||
  fail "the lane left its scratch root behind under $work"

# A scratch directory leaked behind the symlink turns the lane red, naming it.
prefix=$(sed -n 's/^pub const PREFIX: &str = "\(.*\)";$/\1/p' crates/oneharness-core/src/io/scratch.rs)
[ -n "$prefix" ] || fail "could not read the scratch prefix from crates/oneharness-core/src/io/scratch.rs"
if bash "$lane" bash -c "mkdir \"\$TMPDIR/${prefix}leaked-behind-a-symlink\"" >"$work/out" 2>&1; then
  fail "a scratch directory leaked behind the symlinked TMPDIR should have turned the lane red"
fi
grep -q "${prefix}leaked-behind-a-symlink" "$work/out" ||
  fail "the lane went red without naming the directory left behind the symlink"

# The wrapped command's own failure is the lane's failure.
set +e
bash "$lane" bash -c 'exit 5' >"$work/out" 2>&1
status=$?
set -e
[ "$status" -eq 5 ] || fail "the wrapped command's exit status 5 should be the lane's, got $status"

# Off Linux it runs nothing and says so.
if ! OH_SYMLINKED_TMP_UNAME=Darwin bash "$lane" bash -c "touch '$work/ran'" >"$work/out" 2>&1; then
  fail "the lane should succeed off Linux"
fi
[ ! -e "$work/ran" ] || fail "the lane should not run its command off Linux"
grep -q "skipped on Darwin" "$work/out" || fail "the lane should say it skipped off Linux"

echo "with-symlinked-tmp-test: the lane hands its command a symlinked TMPDIR, catches a leak behind it, and runs nothing off Linux"
