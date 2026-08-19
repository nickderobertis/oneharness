#!/usr/bin/env bash
#
# Behavioral test of the scratch-prefix drift check.
#
# A gate nobody has watched fail is not known to work — and this one's whole job
# is to fail. So it is driven against a checkout whose Node prefix has drifted
# out of the sweep, one whose declaration is gone, and one whose Rust constant is
# gone, and asserted to go red naming the file each time.
#
# Quiet on success, one line. On failure it prints what the check said.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

check="scripts/check-scratch-prefixes.sh"
node_prefixes="npm/oneharness-sdk/test/scratch.mjs"
rust_prefix="crates/oneharness-core/src/io/scratch.rs"
work="$(mktemp -d)"
# Restored from copies rather than from git: one of these files is untracked in a
# fresh checkout, and a case that edits it must still put it back.
cp "$node_prefixes" "$work/node-prefixes"
cp "$rust_prefix" "$work/rust-prefix"
restore() {
  cp "$work/node-prefixes" "$node_prefixes"
  cp "$work/rust-prefix" "$rust_prefix"
}
trap 'restore; rm -rf "$work"' EXIT

fail() {
  echo "check-scratch-prefixes-test: $1" >&2
  [ -s "$work/out" ] && cat "$work/out" >&2
  echo "  fix: make $check satisfy the case above, then rerun 'bash scripts/check-scratch-prefixes-test.sh'." >&2
  exit 1
}

# The checked-in suites pass. Anchoring on this first means a later red is the
# drift rather than a check that rejects everything.
if ! bash "$check" >"$work/out" 2>&1; then
  fail "the checked-in scratch prefixes should pass the check"
fi

# A prefix outside the sweep is red, and the check names the file and the prefix
# it must start with.
sed -i.bak 's/^export const PREFIX = ".*";$/export const PREFIX = "sdk-scratch-";/' "$node_prefixes"
rm -f "$node_prefixes.bak"
if bash "$check" >"$work/out" 2>&1; then
  fail "a prefix outside the leak gate's sweep should have failed the check"
fi
grep -q "$node_prefixes" "$work/out" || fail "the check failed but did not name the drifted file"
grep -q "oneharness-" "$work/out" || fail "the check failed but did not say what the prefix must start with"
restore

# A declaration that is gone is red too: an absent prefix is not a passing one.
sed -i.bak 's/^export const PREFIX = ".*";$/const gone = 1;/' "$node_prefixes"
rm -f "$node_prefixes.bak"
if bash "$check" >"$work/out" 2>&1; then
  fail "a removed prefix declaration should have failed the check"
fi
grep -q "declares no scratch prefix" "$work/out" || fail "the check failed but did not say the declaration is missing"
restore

# ...and so is a Rust constant that moved, since every comparison depends on it.
sed -i.bak 's/^pub const PREFIX: &str = ".*";$/pub const MOVED: \&str = "oneharness-";/' "$rust_prefix"
rm -f "$rust_prefix.bak"
if bash "$check" >"$work/out" 2>&1; then
  fail "a missing Rust prefix constant should have failed the check"
fi
grep -q "could not read the scratch prefix" "$work/out" || fail "the check failed but did not name the missing constant"
restore

echo "check-scratch-prefixes-test: the prefix drift check goes red for a prefix the leak gate cannot see"
