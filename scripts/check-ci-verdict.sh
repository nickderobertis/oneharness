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
# fixture file — the Nth call from "$GH_RUNS.N" when that exists, so a case can
# make CI finish between two polls — or fails like an unauthenticated one when
# GH_FAIL is set.
cat >"$tmp/bin/gh" <<'STUB'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$GH_CALLS"
if [ -n "${GH_FAIL:-}" ]; then
  echo 'gh: Resource not accessible by integration (HTTP 403)' >&2
  exit 1
fi
count="$GH_STATE/calls"
n=$(( $(cat "$count" 2>/dev/null || echo 0) + 1 ))
printf '%s' "$n" >"$count"
if [ -f "$GH_RUNS.$n" ]; then
  cat "$GH_RUNS.$n"
else
  cat "$GH_RUNS"
fi
STUB
chmod +x "$tmp/bin/gh"

fail() {
  echo "check-ci-verdict: $1" >&2
  echo "  Next: re-run with 'bash -x scripts/check-ci-verdict.sh'. The stubbed gh records every endpoint it was asked for in \$tmp/calls, and the case's own output is above — a broken assertion here means a release would skip the gate, run it needlessly, or publish a commit CI refused." >&2
  exit 1
}

# Leaves the exit status in $status, stdout in $tmp/out, stderr in $tmp/err, and
# the workflow output file in $tmp/github-output. The wait is three polls with no
# delay, so a case that polls costs nothing.
invoke() {
  : >"$tmp/calls"
  : >"$tmp/github-output"
  rm -rf "$tmp/state"
  mkdir -p "$tmp/state"
  set +e
  GH_CALLS="$tmp/calls" GH_RUNS="$tmp/runs" GH_STATE="$tmp/state" GH_FAIL="${GH_FAIL:-}" \
    PATH="$tmp/bin:$PATH" \
    REPO="${REPO_OVERRIDE-owner/repo}" SHA="${SHA_OVERRIDE-$SHA_UNDER_TEST}" \
    CI_WORKFLOW="${WORKFLOW_OVERRIDE-ci.yml}" \
    CI_WAIT_ATTEMPTS="${WAIT_ATTEMPTS_OVERRIDE-3}" CI_WAIT_DELAY="${WAIT_DELAY_OVERRIDE-0}" \
    GITHUB_OUTPUT="$tmp/github-output" \
    bash "$root/scripts/ci-verdict.sh" >"$tmp/out" 2>"$tmp/err"
  status=$?
  set -e
  description="$1"
}

# $1 the runs JSON the stubbed API answers with every time, $2 case description.
run_case() {
  rm -f "$tmp"/runs.*
  printf '%s' "$1" >"$tmp/runs"
  invoke "$2"
}

# $1 case description, then one answer per successive poll; the last one repeats
# for any poll beyond them.
run_polling_case() {
  # `label` rather than `description`: bash scoping would make a local of that
  # name shadow the global invoke() sets, and every later failure would then be
  # reported under the previous case's name.
  local label="$1" answer n=0
  shift
  rm -f "$tmp"/runs.*
  for answer in "$@"; do
    n=$((n + 1))
    printf '%s' "$answer" >"$tmp/runs.$n"
  done
  printf '%s' "$answer" >"$tmp/runs"
  invoke "$label"
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

# One workflow-run object as the API returns it: id, commit, status,
# conclusion, start time.
workflow_run_json() { printf '{"id":%s,"head_sha":"%s","status":"%s","conclusion":%s,"run_started_at":"%s","html_url":"https://example.invalid/run/%s"}' "$1" "$2" "$3" "$4" "$5" "$1"; }

# The tagged commit passed; a LATER run of a different commit failed in the same
# answer. The selection is the whole point: CI on `main` runs per commit, so a
# foreign run must never answer for this one — and being the newest must not
# make it the answer either.
run_case "{\"workflow_runs\":[$(workflow_run_json 10 "$OTHER_SHA" completed '"failure"' 2026-01-05T00:00:00Z),$(workflow_run_json 11 "$SHA_UNDER_TEST" completed '"success"' 2026-01-02T00:00:00Z)]}" \
  "a success for the tagged commit beside a newer foreign failure"
expect_status 0
expect_needs_check false
expect_said "$tmp/out" "concluded success for $SHA_UNDER_TEST"
grep -Fq "head_sha=$SHA_UNDER_TEST" "$tmp/calls" || {
  cat "$tmp/calls" >&2
  fail "$description: the API was not asked about the tagged commit"
}

# Only a foreign commit's run: this commit has no verdict, whatever that run says.
run_case "{\"workflow_runs\":[$(workflow_run_json 15 "$OTHER_SHA" completed '"success"' 2026-01-05T00:00:00Z)]}" \
  "a run for a different commit only"
expect_status 0
expect_needs_check true
expect_said "$tmp/out" "no run for $SHA_UNDER_TEST"

# A re-run after a failure: the newest finished run for the commit is CI's word.
run_case "{\"workflow_runs\":[$(workflow_run_json 20 "$SHA_UNDER_TEST" completed '"failure"' 2026-01-01T00:00:00Z),$(workflow_run_json 21 "$SHA_UNDER_TEST" completed '"success"' 2026-01-03T00:00:00Z)]}" \
  "a re-run that succeeded after a failure"
expect_status 0
expect_needs_check false

# Every conclusion that means CI refused this commit must stop the release and
# name the run, so the reader goes to it rather than to this workflow. A run
# that timed out or failed to start refused it exactly as a failing one did.
for refusal in failure timed_out startup_failure; do
  run_case "{\"workflow_runs\":[$(workflow_run_json 30 "$SHA_UNDER_TEST" completed "\"$refusal\"" 2026-01-01T00:00:00Z)]}" \
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
run_case "{\"workflow_runs\":[$(workflow_run_json 40 "$SHA_UNDER_TEST" completed '"cancelled"' 2026-01-01T00:00:00Z)]}" \
  "a cancelled CI run"
expect_status 0
expect_needs_check true
expect_said "$tmp/out" "running the gate here instead"

# No run at all — a hand-made Release, or a tag CI never saw.
run_case '{"workflow_runs":[]}' "no CI run for the tagged commit"
expect_status 0
expect_needs_check true
expect_said "$tmp/out" "no run for $SHA_UNDER_TEST"

# Still running: the run is WAITED for rather than duplicated, and the verdict
# that arrives is CI's. Running the whole gate beside the run already running it
# is the second sweep of one commit this script exists to avoid.
run_polling_case "a CI run that finishes while the release waits" \
  "{\"workflow_runs\":[$(workflow_run_json 50 "$SHA_UNDER_TEST" in_progress null 2026-01-01T00:00:00Z)]}" \
  "{\"workflow_runs\":[$(workflow_run_json 50 "$SHA_UNDER_TEST" completed '"success"' 2026-01-01T00:00:00Z)]}"
expect_status 0
expect_needs_check false
expect_said "$tmp/out" "concluded success"
[ "$(grep -c 'head_sha' "$tmp/calls")" -eq 2 ] || {
  cat "$tmp/calls" >&2
  fail "$description: expected the verdict to be asked for twice, once per poll"
}

# A run that never finishes inside the bound: the release stops waiting and
# proves the commit itself rather than publishing on no verdict at all.
run_case "{\"workflow_runs\":[$(workflow_run_json 51 "$SHA_UNDER_TEST" in_progress null 2026-01-01T00:00:00Z)]}" \
  "a CI run that does not finish inside the bound"
expect_status 0
expect_needs_check true
expect_said "$tmp/out" "still had 1 unfinished run(s)"

# Two finished runs that started at the same instant: the tie is broken by run
# id, so the later run is CI's word and a coin flip never decides a release.
run_case "{\"workflow_runs\":[$(workflow_run_json 81 "$SHA_UNDER_TEST" completed '"success"' 2026-01-02T00:00:00Z),$(workflow_run_json 80 "$SHA_UNDER_TEST" completed '"failure"' 2026-01-02T00:00:00Z)]}" \
  "two finished runs that started at the same instant"
expect_status 0
expect_needs_check false
expect_said "$tmp/out" "CI run 81 concluded success"

# A finished run whose conclusion is absent — or of a type this cannot act on —
# is not a verdict, however parseable the answer was.
run_case '{"workflow_runs":[{"id":90,"head_sha":"'"$SHA_UNDER_TEST"'","status":"completed","conclusion":null,"run_started_at":"2026-01-01T00:00:00Z","html_url":"https://example.invalid/run/90"}]}' \
  "a finished run with no conclusion"
expect_status 0
expect_needs_check true
expect_said "$tmp/out" "reported no usable conclusion"

# A rerun in flight beside an older finished run: CI is deciding this commit
# again, so the older verdict is not the answer — the rerun's is.
run_polling_case "a rerun in flight beside an older success" \
  "{\"workflow_runs\":[$(workflow_run_json 70 "$SHA_UNDER_TEST" completed '"success"' 2026-01-01T00:00:00Z),$(workflow_run_json 71 "$SHA_UNDER_TEST" in_progress null 2026-01-04T00:00:00Z)]}" \
  "{\"workflow_runs\":[$(workflow_run_json 70 "$SHA_UNDER_TEST" completed '"success"' 2026-01-01T00:00:00Z),$(workflow_run_json 71 "$SHA_UNDER_TEST" completed '"failure"' 2026-01-04T00:00:00Z)]}"
expect_status 1
expect_said "$tmp/err" "CI run 71 concluded failure"
if [ -s "$tmp/github-output" ]; then
  cat "$tmp/github-output" >&2
  fail "$description: an older success must not publish while CI is deciding again"
fi

# A conclusion nobody enumerated — GitHub has several, and a new one must not
# read as a pass.
run_case "{\"workflow_runs\":[$(workflow_run_json 60 "$SHA_UNDER_TEST" completed '"neutral"' 2026-01-01T00:00:00Z)]}" \
  "a CI run with a conclusion this script does not enumerate"
expect_status 0
expect_needs_check true
expect_said "$tmp/out" "concluded neutral"
expect_said "$tmp/out" "which is not a pass"

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

# The rest of what is interpolated into the API path, or handed to sleep.
REPO_OVERRIDE="owner/repo?ref=main" run_case '{"workflow_runs":[]}' "a repository that is not owner/name"
unset REPO_OVERRIDE
expect_status 2
expect_said "$tmp/err" "is not an owner/name repository"

WORKFLOW_OVERRIDE="../ci.yml/runs" run_case '{"workflow_runs":[]}' "a workflow that is not a file name"
unset WORKFLOW_OVERRIDE
expect_status 2
expect_said "$tmp/err" "is not a workflow file name"

WAIT_ATTEMPTS_OVERRIDE=0 run_case '{"workflow_runs":[]}' "a wait that never asks"
unset WAIT_ATTEMPTS_OVERRIDE
expect_status 2
expect_said "$tmp/err" "never asks"

WAIT_DELAY_OVERRIDE=soon run_case '{"workflow_runs":[]}' "a delay that is not a number of seconds"
unset WAIT_DELAY_OVERRIDE
expect_status 2
expect_said "$tmp/err" "is not a whole number of seconds"

echo "check-ci-verdict: ok"
