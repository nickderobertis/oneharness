#!/usr/bin/env bash
#
# Behavioral test of the LF-contract gate.
#
# That gate's whole job is to fail, and it guards a property no Linux or macOS
# run can observe directly — so a gate that quietly stopped detecting anything
# would read exactly like a repository with nothing left to catch. Drive it at a
# tracked file that is deliberately not pinned and watch it go red.
#
# Quiet on success, one line. On failure it prints what the gate said.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

# A blob committed with carriage returns, rather than an unpinned file left to
# a checkout to convert: whether a given git converts one is the very thing this
# gate has no control over, so the red case cannot be built out of it.
crlf_blob="tests/fixtures/crlf-checkout.txt"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

fail() {
  echo "check-lf-contracts-test: $1" >&2
  [ -s "$work/out" ] && cat "$work/out" >&2
  exit 1
}

# The real contracts pass. Anchoring on this first means a later red is the
# unpinned file rather than a gate that rejects everything.
if ! bash scripts/check-lf-contracts.sh >"$work/out" 2>&1; then
  fail "the checked-in contracts should pass the LF gate"
fi

if bash scripts/check-lf-contracts.sh "$crlf_blob" >"$work/out" 2>&1; then
  fail "a contract reaching the working tree with CRLF should have failed the gate"
fi
grep -q "$crlf_blob" "$work/out" ||
  fail "the gate failed but did not name the file with carriage returns"

# An untracked path must be an error carrying its next action, not a silent
# pass: the check reads what git wrote, and nothing written is not nothing wrong.
if bash scripts/check-lf-contracts.sh docs/not-a-contract.md >"$work/out" 2>&1; then
  fail "a path git cannot check out should have failed the gate"
fi
grep -q "must be in the" "$work/out" ||
  fail "the gate refused an untracked path without saying what to do about it"

echo "check-lf-contracts-test: the LF gate goes red for a CRLF contract and an unreadable one"
