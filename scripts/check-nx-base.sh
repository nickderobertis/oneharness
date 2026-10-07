#!/usr/bin/env bash
#
# Behavioral test of the affected tier's base derivation (scripts/nx-base.sh),
# which `just check` and every other affected recipe key off: NX_BASE as a
# commit SHA, as a plain ref name, as a value it must refuse, and unset — where
# the base is the merge base with origin/main — each against a real scratch
# history. Then the recipe itself, handed an invalid NX_BASE, must stop before a
# single target runs and say which variable it refused.
#
# Quiet on success, one line.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

fail() {
  echo "check-nx-base: $1" >&2
  [ -s "$tmp/out" ] && sed 's/^/  stdout: /' "$tmp/out" >&2
  [ -s "$tmp/err" ] && sed 's/^/  stderr: /' "$tmp/err" >&2
  exit 1
}

repo="$tmp/repo"
git init -q -b main "$repo"
g() { git -C "$repo" -c user.name=t -c user.email=t@t "$@"; }
g commit -q --allow-empty -m one
fork="$(g rev-parse HEAD)"
g commit -q --allow-empty -m main-moves-on
g update-ref refs/remotes/origin/main "$(g rev-parse HEAD)"
g branch -q release-train "$fork"
g checkout -q -b feature "$fork"
g commit -q --allow-empty -m feature-work

# $1 = case; NX_BASE is exported by the caller or deliberately absent.
derive() {
  case_name="$1"
  status=0
  (cd "$repo" && bash "$root/scripts/nx-base.sh") >"$tmp/out" 2>"$tmp/err" || status=$?
}

expect_base() {
  [ "$status" -eq 0 ] || fail "$case_name: exited $status, expected a base"
  [ "$(cat "$tmp/out")" = "$1" ] || fail "$case_name: expected base '$1'"
  grep -q "$2" "$tmp/err" || fail "$case_name: did not say the base came from $2"
}

unset NX_BASE
derive "NX_BASE unset"
expect_base "$fork" "merge-base with origin/main"

NX_BASE="$fork" derive "NX_BASE as a commit SHA"
expect_base "$fork" "(NX_BASE)"

NX_BASE=release-train derive "NX_BASE as a plain ref name"
expect_base release-train "(NX_BASE)"

NX_BASE=origin/main derive "NX_BASE as a remote-tracking ref"
expect_base origin/main "(NX_BASE)"

for invalid in '' '--output=x' 'main..feature' 'main;rm -rf /' 'HEAD~1' 'no-such-ref'; do
  NX_BASE="$invalid" derive "NX_BASE='$invalid'"
  [ "$status" -eq 1 ] || fail "$case_name: exited $status, expected a refusal"
  [ ! -s "$tmp/out" ] || fail "$case_name: a refused NX_BASE still printed a base"
  grep -q 'NX_BASE' "$tmp/err" || fail "$case_name: the refusal does not name NX_BASE"
done

# No origin/main and no NX_BASE: nothing to key off, so it says how to get one.
g update-ref -d refs/remotes/origin/main
unset NX_BASE
derive "no origin/main"
[ "$status" -eq 1 ] || fail "$case_name: exited $status, expected a refusal"
grep -q "fetch origin main' or set NX_BASE" "$tmp/err" || fail "$case_name: the refusal does not say how to provide a base"

# The recipe refuses before any target runs: Nx is never reached.
status=0
(cd "$root" && NX_BASE='main..feature' just check) >"$tmp/out" 2>"$tmp/err" || status=$?
case_name="just check with an invalid NX_BASE"
[ "$status" -ne 0 ] || fail "$case_name: the recipe accepted it"
grep -q 'nx-base: NX_BASE must be a plain git ref name or commit SHA' "$tmp/err" ||
  fail "$case_name: the recipe failed without naming NX_BASE as the reason"
if grep -qi 'nx run\|Successfully ran\|NX ' "$tmp/out" "$tmp/err"; then
  fail "$case_name: Nx ran before the base was refused"
fi

echo "check-nx-base: ok"
