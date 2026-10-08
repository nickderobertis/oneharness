#!/usr/bin/env bash
#
# Behavioral test of the CI gate-tier placement: scripts/ci-gate-tier.sh fed the
# events GitHub sends — release-plz's release pull request, an ordinary pull
# request, a push to main, a manual dispatch — against a real scratch history,
# and ci.yml read back to prove its `check` job hands that decision to the one
# recipe. The release workflow trusts a release pull request's sweep by the
# step name asserted here, so a rename on either side fails this first.
#
# Quiet on success, one line. On failure it names the case and what to change.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

tier_script="$root/scripts/ci-gate-tier.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

fail() {
  echo "check-ci-gate-tier: $1" >&2
  [ -s "$tmp/out" ] && sed 's/^/  stdout: /' "$tmp/out" >&2
  [ -s "$tmp/err" ] && sed 's/^/  stderr: /' "$tmp/err" >&2
  echo "  Next: restore that behaviour in scripts/ci-gate-tier.sh or .github/workflows/ci.yml, then re-run 'bash scripts/check-ci-gate-tier.sh'." >&2
  exit 1
}

# A main line with a feature branch off it, the shape a pull request checkout has.
repo="$tmp/repo"
git init -q -b main "$repo"
g() { git -C "$repo" -c user.name=t -c user.email=t@t "$@"; }
g commit -q --allow-empty -m one
first="$(g rev-parse HEAD)"
g commit -q --allow-empty -m two
fork="$(g rev-parse HEAD)"
g commit -q --allow-empty -m main-moves-on
main_tip="$(g rev-parse HEAD)"
g update-ref refs/remotes/origin/main "$main_tip"
g checkout -q -b feature "$fork"
g commit -q --allow-empty -m feature-work

# $1 = case name; the rest are VAR=value assignments. Leaves stdout in $tmp/out,
# stderr in $tmp/err, the GITHUB_OUTPUT file in $tmp/gh, the status in $status.
run_case() {
  local name="$1"
  shift
  : >"$tmp/gh"
  status=0
  (cd "$repo" && env -i PATH="$PATH" HOME="$tmp" GITHUB_OUTPUT="$tmp/gh" "$@" bash "$tier_script") \
    >"$tmp/out" 2>"$tmp/err" || status=$?
  case_name="$name"
}

expect_tier() {
  [ "$status" -eq 0 ] || fail "$case_name: exited $status, expected a tier"
  grep -qx "tier=$1" "$tmp/out" || fail "$case_name: expected tier=$1"
  grep -qx "tier=$1" "$tmp/gh" || fail "$case_name: tier=$1 never reached \$GITHUB_OUTPUT, so the job's steps cannot read it"
  if [ -n "${2:-}" ]; then
    grep -qx "base=$2" "$tmp/gh" || fail "$case_name: expected base=$2 in \$GITHUB_OUTPUT"
  else
    if grep -q '^base=' "$tmp/gh"; then fail "$case_name: the full sweep has no base, but one was written"; fi
  fi
}

expect_refusal() {
  [ "$status" -eq 2 ] || fail "$case_name: exited $status, expected a refusal (exit 2)"
  grep -q "$1" "$tmp/err" || fail "$case_name: the refusal does not say '$1'"
  [ ! -s "$tmp/gh" ] || fail "$case_name: a refused event still wrote a tier to \$GITHUB_OUTPUT"
}

run_case "release pull request" EVENT_NAME=pull_request HEAD_REF=release-plz-2026-10-07T12-00-00Z BASE_SHA="$main_tip"
expect_tier all

run_case "ordinary pull request" EVENT_NAME=pull_request HEAD_REF=feature BASE_SHA="$main_tip"
expect_tier affected "$fork"

run_case "branch merely mentioning release-plz" EVENT_NAME=pull_request HEAD_REF=fix-release-plz-config BASE_SHA="$main_tip"
expect_tier affected "$fork"

g checkout -q main
run_case "push to main" EVENT_NAME=push BEFORE="$fork"
expect_tier affected "$fork"

run_case "push with no previous tip" EVENT_NAME=push BEFORE=0000000000000000000000000000000000000000
expect_tier affected "$(g rev-parse HEAD^)"

run_case "dispatch of the sweep" EVENT_NAME=workflow_dispatch DISPATCH_TIER=all
expect_tier all

run_case "dispatch of the affected tier" EVENT_NAME=workflow_dispatch DISPATCH_TIER=affected
expect_tier affected "$main_tip"

run_case "pull request without a base" EVENT_NAME=pull_request HEAD_REF=feature BASE_SHA='main; rm -rf /'
expect_refusal "is not a 40-character commit sha"

run_case "pull request whose base is not fetched" EVENT_NAME=pull_request HEAD_REF=feature BASE_SHA=1111111111111111111111111111111111111111
expect_refusal "fetch-depth: 0"

run_case "dispatch of an unknown tier" EVENT_NAME=workflow_dispatch DISPATCH_TIER=nightly
expect_refusal "neither 'affected' nor 'all'"

run_case "schedule" EVENT_NAME=schedule
expect_refusal "has no gate tier"

run_case "a GITHUB_OUTPUT the step cannot append to" EVENT_NAME=workflow_dispatch DISPATCH_TIER=all GITHUB_OUTPUT="$tmp/missing/gh"
expect_refusal "could not append 'tier=all'"

# Without origin/main there is no merge base for a dispatched affected tier.
g update-ref -d refs/remotes/origin/main
run_case "dispatch of the affected tier without origin/main" EVENT_NAME=workflow_dispatch DISPATCH_TIER=affected
expect_refusal "no origin/main in this clone"
g update-ref refs/remotes/origin/main "$main_tip"

# A root commit with no history in common with main: a base it cannot reach,
# and a push whose HEAD has no parent to fall back to.
g checkout -q --orphan unrelated
g commit -q --allow-empty -m unrelated-root
run_case "pull request sharing no history with its base" EVENT_NAME=pull_request HEAD_REF=unrelated BASE_SHA="$main_tip"
expect_refusal "shares no history with the base $main_tip"
run_case "push of a root commit with no previous tip" EVENT_NAME=push BEFORE=0000000000000000000000000000000000000000
expect_refusal "HEAD has no parent"
g checkout -q main
: "$first"

# ci.yml: the `check` job keeps its context names, takes a full-depth checkout,
# selects the tier with the script above, and runs the one recipe either way —
# with no job-level condition that could leave a required context unreported.
ci=".github/workflows/ci.yml"
job="$(awk '/^  check:$/ { on = 1; print; next } on && /^  [A-Za-z]/ { exit } on' "$ci")"
[ -n "$job" ] || fail "$ci has no top-level 'check' job"
printf '%s\n' "$job" | grep -qx '        os: \[ubuntu-latest, macos-latest, windows-latest\]' ||
  fail "$ci check matrix is not exactly ubuntu/macos/windows; the required contexts are 'check (<os>)' for those three"
if printf '%s\n' "$job" | grep -qE '^    if:'; then
  fail "$ci check job has a job-level 'if:'; a required context must report on every pull request"
fi
printf '%s\n' "$job" | grep -qx '          fetch-depth: 0' ||
  fail "$ci check job's checkout is shallow; the affected tier needs the merge base's history"
printf '%s\n' "$job" | grep -qx '        run: bash scripts/ci-gate-tier.sh' ||
  fail "$ci check job does not select its tier with scripts/ci-gate-tier.sh"
# shellcheck disable=SC2016 # GitHub expressions, matched as literal workflow text and never expanded.
for var in 'EVENT_NAME: ${{ github.event_name }}' 'HEAD_REF: ${{ github.head_ref }}' \
  'BASE_SHA: ${{ github.event.pull_request.base.sha }}' 'BEFORE: ${{ github.event.before }}' \
  'DISPATCH_TIER: ${{ inputs.tier }}'; do
  printf '%s\n' "$job" | grep -qxF "          $var" ||
    fail "$ci tier step lacks '$var', so the script cannot see what this run is for"
done
affected="$(printf '%s\n' "$job" | awk '/- name: Affected tier \(just check\)$/ { on = 1; next } on && /- (name|uses):/ { exit } on')"
sweep="$(printf '%s\n' "$job" | awk '/- name: Full sweep \(just check all\)$/ { on = 1; next } on && /- (name|uses):/ { exit } on')"
[ -n "$affected" ] || fail "$ci check job has no step named 'Affected tier (just check)'"
[ -n "$sweep" ] || fail "$ci check job has no step named 'Full sweep (just check all)'; scripts/ci-verdict.sh reads that name"
printf '%s\n' "$affected" | grep -qxF "        if: steps.tier.outputs.tier == 'affected'" ||
  fail "$ci affected step does not run exactly when the tier is 'affected'"
# shellcheck disable=SC2016 # A GitHub expression, matched as literal workflow text.
printf '%s\n' "$affected" | grep -qxF '          NX_BASE: ${{ steps.tier.outputs.base }}' ||
  fail "$ci affected step does not hand the derived base to the recipe as NX_BASE"
printf '%s\n' "$affected" | grep -qx '        run: just check' ||
  fail "$ci affected step does not run 'just check'"
printf '%s\n' "$sweep" | grep -qxF "        if: steps.tier.outputs.tier == 'all'" ||
  fail "$ci sweep step does not run exactly when the tier is 'all'"
printf '%s\n' "$sweep" | grep -qx '        run: just check all' ||
  fail "$ci sweep step does not run 'just check all'"
grep -qF '"Full sweep (just check all)"' scripts/ci-verdict.sh ||
  fail "scripts/ci-verdict.sh no longer reads the step 'Full sweep (just check all)'; the release would trust a run it cannot tell was a sweep"

# The release pull request is recognized by its branch prefix in three places —
# the tier selector, the release's verdict reader, and release-plz.yml finding
# its own pull request — and all three must name the same prefix.
prefix_of() { sed -n 's/^readonly RELEASE_BRANCH_PREFIX="\(.*\)"$/\1/p' "$1"; }
tier_prefix="$(prefix_of scripts/ci-gate-tier.sh)"
verdict_prefix="$(prefix_of scripts/ci-verdict.sh)"
plz_prefix="$(sed -n 's/.*startswith("\([^"]*\)").*/\1/p' .github/workflows/release-plz.yml | head -n 1)"
[ -n "$tier_prefix" ] || fail "scripts/ci-gate-tier.sh declares no 'readonly RELEASE_BRANCH_PREFIX=\"...\"'"
[ "$verdict_prefix" = "$tier_prefix" ] ||
  fail "scripts/ci-verdict.sh recognizes release pull requests by '$verdict_prefix' but scripts/ci-gate-tier.sh sweeps '$tier_prefix'; the release would read a run that never swept, or none"
[ "$plz_prefix" = "$tier_prefix" ] ||
  fail ".github/workflows/release-plz.yml finds its release pull request by '$plz_prefix' but scripts/ci-gate-tier.sh sweeps '$tier_prefix'"

# The other required contexts keep their names and never wait on `check`.
for name in pr-title deny llmlint; do
  grep -qx "  $name:" "$ci" || fail "$ci lost its '$name' job, a required context"
done
if grep -qE '^    needs:' "$ci"; then
  fail "$ci gained a 'needs:' edge; a required context behind a failed or skipped job would go unreported"
fi

echo "check-ci-gate-tier: ok"
