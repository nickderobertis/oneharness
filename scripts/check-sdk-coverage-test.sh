#!/usr/bin/env bash
#
# Behavioral test of the SDK coverage gate.
#
# A gate nobody has watched fail is not known to work — and this one's whole
# job is to fail. So it is driven against candidate clients with one method
# removed and asserted to go red, naming the method it wants, in each language
# independently.
#
# Quiet on success, one line. On failure it prints what the gate said.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

typescript="npm/oneharness-sdk/src/index.ts"
python="python/oneharness-sdk/src/oneharness_sdk/_client.py"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

fail() {
  echo "check-sdk-coverage-test: $1" >&2
  [ -s "$work/out" ] && cat "$work/out" >&2
  exit 1
}

# The real clients pass. Anchoring on this first means a later red is the
# removed method rather than a gate that rejects everything.
if ! node scripts/sdk-coverage.mjs >"$work/out" 2>&1; then
  fail "the checked-in clients should pass the coverage gate"
fi

# TypeScript: drop `usage`, one of the five verbs that shipped uncovered.
sed 's/^\tasync usage(/\tasync notUsage(/' "$typescript" >"$work/index.ts"
if cmp -s "$typescript" "$work/index.ts"; then
  fail "could not remove usage() from the TypeScript client; has it been renamed?"
fi
if node scripts/sdk-coverage.mjs "$work/index.ts" "$python" >"$work/out" 2>&1; then
  fail "a TypeScript client missing usage() should have failed the gate"
fi
grep -q "TypeScript has no .usage. for" "$work/out" ||
  fail "the gate failed but did not name the missing TypeScript method"

# Python: drop `sync`, whose `--global` is the option a keyword rename touches.
sed 's/^    async def sync(/    async def not_sync(/' "$python" >"$work/_client.py"
if cmp -s "$python" "$work/_client.py"; then
  fail "could not remove sync() from the Python client; has it been renamed?"
fi
if node scripts/sdk-coverage.mjs "$typescript" "$work/_client.py" >"$work/out" 2>&1; then
  fail "a Python client missing sync() should have failed the gate"
fi
grep -q "Python has no .sync. for" "$work/out" ||
  fail "the gate failed but did not name the missing Python method"

echo "check-sdk-coverage-test: the coverage gate goes red for a missing method in each SDK"
