#!/usr/bin/env bash
#
# Behavioral test of the scratch-prefix drift check.
#
# A gate nobody has watched fail is not known to work — and this one's whole job
# is to fail. So each case below breaks one thing it reads and asserts it goes
# red naming the file.
#
# Quiet on success, one line. On failure it prints what the check said.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

check="scripts/check-scratch-prefixes.sh"
node_prefixes="npm/oneharness-sdk/test/scratch.mjs"
rust_prefix="crates/oneharness-core/src/io/scratch.rs"
leak_gate="scripts/check-temp-leaks.sh"
work="$(mktemp -d)"
# Restored from copies rather than from git: one of these files is untracked in a
# fresh checkout, and a case that edits it must still put it back.
cp "$node_prefixes" "$work/node-prefixes"
cp "$rust_prefix" "$work/rust-prefix"
cp "$leak_gate" "$work/leak-gate"
restore() {
  cp "$work/node-prefixes" "$node_prefixes"
  cp "$work/rust-prefix" "$rust_prefix"
  cp "$work/leak-gate" "$leak_gate"
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

# A prefix renamed inside the sweep is red too: the leak gate would route that
# suite's leak to another suite's fix.
sed -i.bak 's/^export const PREFIX = ".*";$/export const PREFIX = "oneharness-node-";/' "$node_prefixes"
rm -f "$node_prefixes.bak"
if bash "$check" >"$work/out" 2>&1; then
  fail "a prefix renamed away from the one the leak gate routes its fix by should have failed the check"
fi
grep -q "$node_prefixes uses 'oneharness-node-', but scripts/check-temp-leaks.sh routes this suite's fix by 'oneharness-sdk-'" "$work/out" ||
  fail "the check failed but did not name the renamed prefix and the one the leak gate routes by"
restore

# ...and so is the leak gate's routing renamed alone, since the check reads the
# value each suite must declare from the gate itself.
# shellcheck disable=SC2016  # the gate's own `${prefix}` spelling, not an expansion
sed -i.bak 's/^  node_suite="\${prefix}sdk-"$/  node_suite="${prefix}node-"/' "$leak_gate"
rm -f "$leak_gate.bak"
if bash "$check" >"$work/out" 2>&1; then
  fail "a leak gate routing a suite by a name its helper does not declare should have failed the check"
fi
grep -q "$node_prefixes uses 'oneharness-sdk-', but $leak_gate routes this suite's fix by 'oneharness-node-'" "$work/out" ||
  fail "the check failed but did not name the gate's routing the helper disagrees with"
restore

# A declaration that is gone is red too: an absent prefix is not a passing one.
sed -i.bak 's/^export const PREFIX = ".*";$/const gone = 1;/' "$node_prefixes"
rm -f "$node_prefixes.bak"
if bash "$check" >"$work/out" 2>&1; then
  fail "a removed prefix declaration should have failed the check"
fi
grep -q "declares no scratch prefix" "$work/out" || fail "the check failed but did not say the declaration is missing"
restore

# A helper whose names no longer end in the maker's pid is red, naming the file:
# the leak gate would count every such directory another run made as a leak.
# shellcheck disable=SC2016  # the Node source's own template spelling, not an expansion
sed -i.bak 's/-${process.pid}`/`/' "$node_prefixes"
rm -f "$node_prefixes.bak"
if bash "$check" >"$work/out" 2>&1; then
  fail "a scratch helper whose names drop the maker's pid should have failed the check"
fi
grep -q "$node_prefixes no longer ends its scratch names in the maker's process id" "$work/out" ||
  fail "the check failed but did not name the helper that dropped the pid"
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
