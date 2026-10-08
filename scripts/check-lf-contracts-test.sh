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

# The shell-script half: the real tree passes, and a repository whose only pin
# is `*.sh` goes red on an extensionless bash script — like the `scripts/nx`
# that failed SC1017 on every line of a Windows checkout — and green once that
# script is pinned by path.
if ! bash scripts/check-lf-contracts.sh --shell >"$work/out" 2>&1; then
  fail "every tracked shell script should pass the LF gate's --shell check"
fi
fixture="$work/repo"
mkdir -p "$fixture/scripts"
cp scripts/check-lf-contracts.sh "$fixture/scripts/"
printf '#!/usr/bin/env bash\necho ok\n' >"$fixture/scripts/tool"
printf '*.sh text eol=lf\n' >"$fixture/.gitattributes"
git -C "$fixture" init -q
git -C "$fixture" add -A
if bash "$fixture/scripts/check-lf-contracts.sh" --shell >"$work/out" 2>&1; then
  fail "an extensionless bash script pinned by nothing should have failed the --shell check"
fi
grep -qx "  scripts/tool" "$work/out" ||
  fail "the --shell check failed but did not name the unpinned extensionless script"
grep -q "rerun 'bash scripts/check-lf-contracts.sh --shell'" "$work/out" ||
  fail "the --shell check named no rerun of itself"
printf '/scripts/tool text eol=lf\n' >>"$fixture/.gitattributes"
git -C "$fixture" add -A
if ! bash "$fixture/scripts/check-lf-contracts.sh" --shell >"$work/out" 2>&1; then
  fail "a pinned extensionless bash script should pass the --shell check"
fi

echo "check-lf-contracts-test: the LF gate goes red for a CRLF contract, an unreadable one and an unpinned shell script"
