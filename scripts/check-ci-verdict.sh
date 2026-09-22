#!/usr/bin/env bash
# llmlint: ignore-file[new_code_lands_in_a_project] The rule presumes an Nx project graph; this repository has none by a recorded decision (`AGENTS.md`: root `just` delegates to Cargo/Bun without Nx because the two-package graph is static), so no project definition can cover this file. What runs it is `just lint-workflows`, in `check` and CI.
# Hermetic behavioral test for scripts/ci-verdict.sh, against a stand-in GitHub
# API.
#
# The verdict decides whether a release publishes an unverified commit, and it
# runs exactly once per release — where a wrong answer either wastes the gate or,
# far worse, publishes a commit CI refused. A real API cannot rehearse a
# cancelled run or a missing one, so `gh` is stubbed and every branch is driven:
# the selection (a foreign commit's run must not answer for ours, and the newest
# finished run wins), then success, failure, cancelled, absent, unfinished, and
# an answer that cannot be read at all.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/bin"

# The commit under release, and one that merely looks like it.
SHA_UNDER_TEST=1111111111111111111111111111111111111111
OTHER_SHA=2222222222222222222222222222222222222222

# A `gh` that records the endpoint it was asked for and answers `api` from a
# fixture file — or fails like an unauthenticated one when GH_FAIL is set.
cat >"$tmp/bin/gh" <<'STUB'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$GH_CALLS"
if [ -n "${GH_FAIL:-}" ]; then
  echo 'gh: Resource not accessible by integration (HTTP 403)' >&2
  exit 1
fi
cat "$GH_RUNS"
STUB
chmod +x "$tmp/bin/gh"

fail() {
  echo "check-ci-verdict: $1" >&2
  echo "  Next: re-run with 'bash -x scripts/check-ci-verdict.sh'. The stubbed gh records every endpoint it was asked for in \$tmp/calls, and the case's own output is above — a broken assertion here means a release would skip the gate, run it needlessly, or publish a commit CI refused." >&2
  exit 1
}

# $1 the runs JSON the stubbed API answers with, $2 case description; leaves the
# exit status in $status, stdout in $tmp/out, stderr in $tmp/err, and the
# workflow output file in $tmp/github-output.
run_case() {
  printf '%s' "$1" >"$tmp/runs"
  : >"$tmp/calls"
  : >"$tmp/github-output"
  set +e
  GH_CALLS="$tmp/calls" GH_RUNS="$tmp/runs" GH_FAIL="${GH_FAIL:-}" \
    PATH="$tmp/bin:$PATH" \
    REPO="${REPO_OVERRIDE-owner/repo}" SHA="${SHA_OVERRIDE-$SHA_UNDER_TEST}" \
    GITHUB_OUTPUT="$tmp/github-output" \
    bash "$root/scripts/ci-verdict.sh" >"$tmp/out" 2>"$tmp/err"
  status=$?
  set -e
  description="$2"
}

expect_status() {
  [ "$status" -eq "$1" ] || {
    cat "$tmp/out" "$tmp/err" >&2
    fail "$description: expected exit $1, got $status"
  }
}

expect_needs_check() {
  grep -Fxq "needs_check=$1" "$tmp/github-output" || {
    cat "$tmp/github-output" "$tmp/out" "$tmp/err" >&2
    fail "$description: expected needs_check=$1 in the workflow output"
  }
  # One decision per run: a second line would leave the workflow reading the
  # last write rather than the verdict.
  [ "$(wc -l <"$tmp/github-output")" -eq 1 ] || {
    cat "$tmp/github-output" >&2
    fail "$description: wrote more than one workflow output line"
  }
}

expect_said() {
  local where="$1" needle="$2"
  grep -Fq "$needle" "$where" || {
    cat "$tmp/out" "$tmp/err" >&2
    fail "$description: expected to say '$needle'"
  }
}

run() { printf '{"id":%s,"head_sha":"%s","status":"%s","conclusion":%s,"run_started_at":"%s","html_url":"https://example.invalid/run/%s"}' "$1" "$2" "$3" "$4" "$5" "$1"; }

# The tagged commit passed; a LATER run of a different commit failed in the same
# answer. The selection is the whole point: CI on `main` runs per commit, so a
# foreign run must never answer for this one — and being the newest must not
# make it the answer either.
run_case "{\"workflow_runs\":[$(run 10 "$OTHER_SHA" completed '"failure"' 2026-01-05T00:00:00Z),$(run 11 "$SHA_UNDER_TEST" completed '"success"' 2026-01-02T00:00:00Z)]}" \
  "a success for the tagged commit beside a newer foreign failure"
expect_status 0
expect_needs_check false
expect_said "$tmp/out" "concluded success for $SHA_UNDER_TEST"
grep -Fq "head_sha=$SHA_UNDER_TEST" "$tmp/calls" || {
  cat "$tmp/calls" >&2
  fail "$description: the API was not asked about the tagged commit"
}

# Only a foreign commit's run: this commit has no verdict, whatever that run says.
run_case "{\"workflow_runs\":[$(run 15 "$OTHER_SHA" completed '"success"' 2026-01-05T00:00:00Z)]}" \
  "a run for a different commit only"
expect_status 0
expect_needs_check true
expect_said "$tmp/out" "no run for $SHA_UNDER_TEST"

# A re-run after a failure: the newest finished run for the commit is CI's word.
run_case "{\"workflow_runs\":[$(run 20 "$SHA_UNDER_TEST" completed '"failure"' 2026-01-01T00:00:00Z),$(run 21 "$SHA_UNDER_TEST" completed '"success"' 2026-01-03T00:00:00Z)]}" \
  "a re-run that succeeded after a failure"
expect_status 0
expect_needs_check false

# Every conclusion that means CI refused this commit must stop the release and
# name the run, so the reader goes to it rather than to this workflow. A run
# that timed out or failed to start refused it exactly as a failing one did.
for refusal in failure timed_out startup_failure; do
  run_case "{\"workflow_runs\":[$(run 30 "$SHA_UNDER_TEST" completed "\"$refusal\"" 2026-01-01T00:00:00Z)]}" \
    "a CI run that concluded $refusal for the tagged commit"
  expect_status 1
  expect_said "$tmp/err" "CI run 30 concluded $refusal"
  expect_said "$tmp/err" "https://example.invalid/run/30"
  if [ -s "$tmp/github-output" ]; then
    cat "$tmp/github-output" >&2
    fail "$description: a refusal must decide nothing for the workflow to act on"
  fi
done

# Cancelled: CI answered nothing about this commit, so the release proves it.
run_case "{\"workflow_runs\":[$(run 40 "$SHA_UNDER_TEST" completed '"cancelled"' 2026-01-01T00:00:00Z)]}" \
  "a cancelled CI run"
expect_status 0
expect_needs_check true
expect_said "$tmp/out" "running the gate here instead"

# No run at all — a hand-made Release, or a tag CI never saw.
run_case '{"workflow_runs":[]}' "no CI run for the tagged commit"
expect_status 0
expect_needs_check true
expect_said "$tmp/out" "no run for $SHA_UNDER_TEST"

# Still running: a verdict that has not been reached is not a pass.
run_case "{\"workflow_runs\":[$(run 50 "$SHA_UNDER_TEST" in_progress null 2026-01-01T00:00:00Z)]}" \
  "an unfinished CI run"
expect_status 0
expect_needs_check true
expect_said "$tmp/out" "none has finished"

# An unread answer is an absent one: the release proves the commit itself rather
# than publishing on a verdict nobody saw.
GH_FAIL=1 run_case '{"workflow_runs":[]}' "an API that refuses the query"
unset GH_FAIL
expect_status 0
expect_needs_check true
expect_said "$tmp/out" "could not read ci.yml runs"
expect_said "$tmp/err" "HTTP 403"

# Unparseable JSON is the same kind of unread answer.
run_case 'not json at all' "an API answering with something that is not JSON"
expect_status 0
expect_needs_check true
expect_said "$tmp/out" "did not parse as workflow-run JSON"

# An input the runs API cannot be asked about is a wiring bug, not a verdict:
# each must refuse loudly rather than decide the release is unverified.
SHA_OVERRIDE=1111111 run_case '{"workflow_runs":[]}' "an abbreviated commit sha"
unset SHA_OVERRIDE
expect_status 2
expect_said "$tmp/err" "not a full 40-character commit sha"

SHA_OVERRIDE="zzzz111111111111111111111111111111111111" run_case '{"workflow_runs":[]}' "a sha that is not hexadecimal"
unset SHA_OVERRIDE
expect_status 2
expect_said "$tmp/err" "is not a commit sha"

SHA_OVERRIDE="" run_case '{"workflow_runs":[]}' "no commit to ask about"
unset SHA_OVERRIDE
expect_status 2
expect_said "$tmp/err" "no commit to ask about"

REPO_OVERRIDE="" run_case '{"workflow_runs":[]}' "no repository to query"
unset REPO_OVERRIDE
expect_status 2
expect_said "$tmp/err" "no repository to query"

echo "check-ci-verdict: ok"
