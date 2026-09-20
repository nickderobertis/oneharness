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

# A client argument the gate cannot read is refused as a usage error naming it
# — not a stack trace, and not a client with no methods, which would fail every
# capability against a file that was never the client.
# Both are exit 2 — a usage error, distinct from the gate's own red (exit 1) —
# and both print how the script is called.
# The path is matched by its basename alone, never by the spelling mktemp gave
# the shell: under Git Bash MSYS rewrites `/tmp/tmp.XXXX/absent.ts` into
# `C:/Users/RUNNER~1/AppData/Local/Temp/tmp.XXXX/absent.ts` before node sees it,
# and the gate names the argument it was handed. The basename is what both
# spellings share — the directory part is respelled and partly 8.3-shortened,
# so anchoring anywhere above the file asserts on the host, not on the gate.
# What gives each assertion its teeth is the half beside the name: the ENOENT
# code for the unreadable client, the class it looked for for the non-client.
usage_line='usage: node scripts/sdk-coverage.mjs \[<typescript client>\] \[<python client>\]'
status=0
node scripts/sdk-coverage.mjs "$work/absent.ts" "$python" >"$work/out" 2>&1 || status=$?
[ "$status" -eq 2 ] ||
  fail "a client path that does not exist should be a usage error (exit 2), got exit $status; route the readFileSync failure in methods() through usage()"
grep -q "cannot read the client at .*absent\.ts (ENOENT)" "$work/out" ||
  fail "the gate did not name the client path it could not read; keep 'cannot read the client at <path> (<code>)' in methods()"
grep -q "$usage_line" "$work/out" ||
  fail "the unreadable-client refusal did not say how the gate is called; keep the usage line in usage()"
# A readable file that is not a client, so the run's diagnostic can land in
# `$work/out` — the one file `fail` shows — rather than in the file under test.
printf 'export const notAClient = 1;\n' >"$work/not-a-client.ts"
status=0
node scripts/sdk-coverage.mjs "$typescript" "$work/not-a-client.ts" >"$work/out" 2>&1 || status=$?
[ "$status" -eq 2 ] ||
  fail "a file that declares no client class should be a usage error (exit 2), got exit $status; route a missing class declaration in methods() through usage()"
grep -q "not-a-client\.ts does not declare .class OneHarness., so it is not a client this gate reads" "$work/out" ||
  fail "the gate did not say which file declares no client class; keep '<path> does not declare \`class OneHarness\`' in methods()"
grep -q "$usage_line" "$work/out" ||
  fail "the no-client refusal did not say how the gate is called; keep the usage line in usage()"

echo "check-sdk-coverage-test: the coverage gate goes red for a missing method in each SDK and refuses a file that is not a client"
