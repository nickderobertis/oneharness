#!/usr/bin/env bash
# llmlint: ignore-file[new_code_lands_in_a_project] The rule presumes an Nx project graph; this repository has none by a recorded decision (`AGENTS.md`: root `just` delegates to Cargo/Bun without Nx because the two-package graph is static), so no project definition can cover this file and `just lint-workflows` is what runs it.
#
# Behavioral test of the symlinked-TMPDIR lane (`scripts/with-symlinked-tmp.sh`,
# which `just test-symlinked-tmp` runs the CLI journeys through).
#
# The lane is only worth its minutes while the command it wraps really sees a
# `$TMPDIR` spelled through a symlink, while a scratch directory leaked behind
# that symlink still turns it red, and while it runs nothing off Linux (nor under
# a platform override it does not recognize) — so each of those is driven here
# against a probe command rather than read off the script.
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

prefix=$(sed -n 's/^pub const PREFIX: &str = "\(.*\)";$/\1/p' crates/oneharness-core/src/io/scratch.rs)
[ -n "$prefix" ] || fail "could not read the scratch prefix from crates/oneharness-core/src/io/scratch.rs"
if bash "$lane" bash -c "mkdir \"\$TMPDIR/${prefix}leaked-behind-a-symlink\"" >"$work/out" 2>&1; then
  fail "a scratch directory leaked behind the symlinked TMPDIR should have turned the lane red"
fi
grep -q "${prefix}leaked-behind-a-symlink" "$work/out" ||
  fail "the lane went red without naming the directory left behind the symlink"

set +e
bash "$lane" bash -c 'exit 5' >"$work/out" 2>&1
status=$?
set -e
[ "$status" -eq 5 ] || fail "the wrapped command's exit status 5 should be the lane's, got $status"

if ! OH_SYMLINKED_TMP_UNAME=Darwin bash "$lane" bash -c "touch '$work/ran'" >"$work/out" 2>&1; then
  fail "the lane should succeed off Linux"
fi
[ ! -e "$work/ran" ] || fail "the lane should not run its command off Linux"
grep -q "skipped on Darwin" "$work/out" || fail "the lane should say it skipped off Linux"

set +e
bash "$lane" >"$work/out" 2>&1
status=$?
set -e
[ "$status" -eq 2 ] || fail "the lane with no command should be a usage error (exit 2), got $status"
grep -q "no command to run" "$work/out" || fail "the lane with no command should say so"

set +e
TMPDIR="$work/does-not-exist" bash "$lane" bash -c "touch '$work/ran'" >"$work/out" 2>&1
status=$?
set -e
[ "$status" -eq 1 ] || fail "a TMPDIR the lane cannot build its root under should fail it (exit 1), got $status"
[ ! -e "$work/ran" ] || fail "the lane should not run its command without its symlinked root"
grep -q "could not build a symlinked temp root under $work/does-not-exist" "$work/out" ||
  fail "the lane should name the TMPDIR it could not build its root under"

# A root half-built before `ln` fails — a filesystem without symlinks, stood in
# for by an `ln` that refuses — is reported and still removed.
mkdir -p "$work/no-symlinks"
printf '#!/usr/bin/env bash\nexit 1\n' >"$work/no-symlinks/ln"
chmod +x "$work/no-symlinks/ln"
set +e
PATH="$work/no-symlinks:$PATH" bash "$lane" bash -c "touch '$work/ran'" >"$work/out" 2>&1
status=$?
set -e
[ "$status" -eq 1 ] || fail "a root the lane cannot symlink should fail it (exit 1), got $status"
[ ! -e "$work/ran" ] || fail "the lane should not run its command without its symlink"
grep -q "could not build a symlinked temp root under $work" "$work/out" ||
  fail "the lane should say it could not build its symlinked root"
[ -z "$(find "$work" -mindepth 1 -maxdepth 1 -name 'symlinked-tmp.*')" ] ||
  fail "the lane left its half-built root behind under $work"

set +e
OH_SYMLINKED_TMP_UNAME=Linx bash "$lane" bash -c "touch '$work/ran'" >"$work/out" 2>&1
status=$?
set -e
[ "$status" -eq 2 ] || fail "a misspelled platform override should be a usage error (exit 2), got $status"
[ ! -e "$work/ran" ] || fail "the lane should not run its command under a misspelled platform override"
grep -q "unrecognized platform 'Linx'" "$work/out" || fail "the lane should name the platform override it refused"

echo "with-symlinked-tmp-test: the lane hands its command a symlinked TMPDIR, catches a leak behind it, and runs nothing off Linux"
