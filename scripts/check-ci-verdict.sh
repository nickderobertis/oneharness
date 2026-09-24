#!/usr/bin/env bash
# llmlint: ignore-file[new_code_lands_in_a_project] The rule presumes an Nx project graph; this repository has none by a recorded decision (`AGENTS.md`: root `just` delegates to Cargo/Bun without Nx because the two-package graph is static), so no project definition can cover this file. What runs it is `just lint-workflows`, in `check` and CI.
# Drive CI verdict selection through a stand-in GitHub API, including reruns
# and unreadable responses that cannot safely authorize publication.
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
  response="$(cat "$GH_RUNS.$n")"
else
  response="$(cat "$GH_RUNS")"
fi
# Older fixtures omit these stable GitHub fields; give them the values of a
# main-branch push while preserving any explicit event and branch in a case.
if decorated="$(printf '%s' "$response" | jq -c 'walk(if type == "object" and has("head_sha") then
  (if has("event") then . else . + {event:"push"} end)
  | (if has("head_branch") then . else . + {head_branch:"main"} end)
else . end)' 2>/dev/null)"; then
  response="$decorated"
fi
# gh --slurp returns an array of pages. A fixture may supply multiple pages;
# the usual single-page fixtures are wrapped as gh would wrap them.
case "$response" in
  '['*) printf '%s' "$response" ;;
  *) printf '[%s]' "$response" ;;
esac
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
#
# Strip runner-provided fallbacks so missing-input cases exercise that absence.
invoke() {
  : >"$tmp/calls"
  : >"$tmp/github-output"
  rm -rf "$tmp/state"
  mkdir -p "$tmp/state"
  set +e
  env -u GITHUB_REPOSITORY -u GITHUB_SHA \
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

# ...and this holds that list complete. Every ambient GitHub variable
# ci-verdict.sh reads must be one invoke() either SETS (GITHUB_OUTPUT, which the
# cases read back) or STRIPS (the two fallbacks above). A new one added there and
# left out here reintroduces exactly the defect described above, which is
# invisible until a release runs — so it is caught here, where it is cheap.
# Only whole-line comments are dropped, so the script's own documentation of the
# three may mention them freely.
handled_ambient="GITHUB_OUTPUT GITHUB_REPOSITORY GITHUB_SHA"
read_ambient="$(sed 's/^[[:space:]]*#.*//' scripts/ci-verdict.sh |
  grep -o 'GITHUB_[A-Z_]*' | sort -u | tr '\n' ' ')"
[ "$read_ambient" = "$handled_ambient " ] ||
  fail "scripts/ci-verdict.sh reads the ambient variables [${read_ambient% }], but this test only handles [$handled_ambient]. Set the new one in invoke() if a case must control it, or strip it with 'env -u' if no case may see the runner's own value; a GitHub runner sets most of them, and an unstripped fallback makes a case here pass locally and fail only during a release"

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
  grep -Fq -- "$needle" "$where" || {
    cat "$tmp/out" "$tmp/err" >&2
    fail "$description: expected to say '$needle'"
  }
}

# A refusal stops the release having decided NOTHING the workflow could act on —
# an emitted needs_check is what would turn an unread verdict into a locally
# manufactured one — and says enough for a person to act: the state met, what it
# was reading, and what to do next.
expect_refused() {
  expect_status 1
  if [ -s "$tmp/github-output" ]; then
    cat "$tmp/github-output" >&2
    fail "$description: a refusal must decide nothing for the workflow to act on"
  fi
  expect_said "$tmp/err" "  Next: "
}

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
expect_said "$tmp/calls" '--paginate --slurp'
expect_said "$tmp/calls" '&event=push&branch=main&'

run_case "[{\"workflow_runs\":[$(workflow_run_json 12 "$OTHER_SHA" completed '"failure"' 2026-01-05T00:00:00Z)]},{\"workflow_runs\":[$(workflow_run_json 13 "$SHA_UNDER_TEST" completed '"success"' 2026-01-02T00:00:00Z)]}]" \
  "a tagged commit whose run is on the next API page"
expect_status 0
expect_needs_check false
expect_said "$tmp/out" "CI run 13 concluded success"

run_case "{\"workflow_runs\":[{\"id\":14,\"head_sha\":\"$SHA_UNDER_TEST\",\"event\":\"pull_request\",\"head_branch\":\"feature\",\"status\":\"completed\",\"conclusion\":\"success\",\"run_started_at\":\"2026-01-05T00:00:00Z\"},$(workflow_run_json 15 "$SHA_UNDER_TEST" completed '"failure"' 2026-01-02T00:00:00Z)]}" \
  "a PR success cannot override main's failed push on the same SHA"
expect_refused
expect_said "$tmp/err" "CI run 15 concluded failure"
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
  expect_refused
  expect_said "$tmp/err" "CI run 30 concluded $refusal"
  expect_said "$tmp/err" "https://example.invalid/run/30"
done

# Cancelled — the first of the only two states that run the gate in the release.
# CI was stopped before it reached any verdict, so nothing is being stood in for.
run_case "{\"workflow_runs\":[$(workflow_run_json 40 "$SHA_UNDER_TEST" completed '"cancelled"' 2026-01-01T00:00:00Z)]}" \
  "a cancelled CI run"
expect_status 0
expect_needs_check true
expect_said "$tmp/out" "CI reached no verdict"
expect_said "$tmp/out" "running the gate here instead"

# Absent — the second, and the last. A hand-made Release, or a tag CI never saw.
run_case '{"workflow_runs":[]}' "no CI run for the tagged commit"
expect_status 0
expect_needs_check true
expect_said "$tmp/out" "no run for $SHA_UNDER_TEST"
expect_said "$tmp/out" "CI reached no verdict"

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

# A run that never finishes inside the bound. CI has not finished DECIDING this
# commit, and the verdict it is about to reach may be a refusal — so the bound
# running out is not the same as CI having been cancelled, and the release stops
# rather than gating the commit itself and publishing on its own green.
run_case "{\"workflow_runs\":[$(workflow_run_json 51 "$SHA_UNDER_TEST" in_progress null 2026-01-01T00:00:00Z)]}" \
  "a CI run that does not finish inside the bound"
expect_refused
expect_said "$tmp/err" "still had 1 unfinished run(s)"
expect_said "$tmp/err" "has not finished deciding this commit"

# Two finished runs that started at the same instant: the tie is broken by run
# id, so the later run is CI's word and a coin flip never decides a release.
run_case "{\"workflow_runs\":[$(workflow_run_json 81 "$SHA_UNDER_TEST" completed '"success"' 2026-01-02T00:00:00Z),$(workflow_run_json 80 "$SHA_UNDER_TEST" completed '"failure"' 2026-01-02T00:00:00Z)]}" \
  "two finished runs that started at the same instant"
expect_status 0
expect_needs_check false
expect_said "$tmp/out" "CI run 81 concluded success"

# A run whose own FIELDS cannot be read is the same hazard one level down, and
# it is the subtler one: a malformed run that is merely dropped takes the
# selection one step closer to empty, and empty is `absent` — the state that
# runs the gate. Each of these is a sole run, so dropping it would have reported
# "CI has no run for this commit" and gated it here.
#
# A finished run whose conclusion is absent, or of a type this cannot act on:
# the run RAN, so CI very likely reached a verdict this could not read.
run_case '{"workflow_runs":[{"id":90,"head_sha":"'"$SHA_UNDER_TEST"'","status":"completed","conclusion":null,"run_started_at":"2026-01-01T00:00:00Z","html_url":"https://example.invalid/run/90"}]}' \
  "a finished run with no conclusion"
expect_refused
expect_said "$tmp/err" "1 run(s) in CI's answer"
expect_said "$tmp/err" "it was reading repos/owner/repo/actions/workflows/ci.yml/runs"

# A control character in a conclusion must not shift the tab-delimited fields
# the script reads from jq's summary.
run_case '{"workflow_runs":[{"id":114,"head_sha":"'"$SHA_UNDER_TEST"'","status":"completed","conclusion":"success\t0\t0\t114","run_started_at":"2026-01-01T00:00:00Z"}]}' \
  "a conclusion containing a tab"
expect_refused
expect_said "$tmp/err" "1 run(s) in CI's answer"

run_case '{"workflow_runs":[{"id":115,"head_sha":"'"$SHA_UNDER_TEST"'","status":"completed","conclusion":"success\n","run_started_at":"2026-01-01T00:00:00Z"}]}' \
  "a conclusion containing a newline"
expect_refused
expect_said "$tmp/err" "1 run(s) in CI's answer"

# A finished run whose id is not a number: it cannot be named to the reader, and
# an unnamed run must not be the one that publishes or refuses a release.
run_case '{"workflow_runs":[{"id":"ninety-one","head_sha":"'"$SHA_UNDER_TEST"'","status":"completed","conclusion":"success","run_started_at":"2026-01-01T00:00:00Z"}]}' \
  "a finished run whose id is not a number"
expect_refused
expect_said "$tmp/err" "1 run(s) in CI's answer"

run_case "{\"workflow_runs\":[$(workflow_run_json 91.5 "$SHA_UNDER_TEST" completed '"success"' 2026-01-01T00:00:00Z)]}" \
  "a finished run with a fractional id"
expect_refused
expect_said "$tmp/err" "1 run(s) in CI's answer"

# A run with no readable head_sha: there is no telling WHOSE it is, so it can
# be excluded as neither ours nor somebody else's.
for anonymous in '{"id":92,"status":"completed","conclusion":"failure"}' \
                 '{"id":93,"head_sha":null,"status":"completed","conclusion":"failure"}' \
                 '{"id":94,"head_sha":{},"status":"completed","conclusion":"failure"}'; do
  run_case "{\"workflow_runs\":[$anonymous]}" "a run whose head_sha cannot be read: $anonymous"
  expect_refused
  expect_said "$tmp/err" "1 run(s) in CI's answer"
done

# One of OURS with no readable status: neither finished nor pending, so it would
# vanish from both counts and leave the commit looking untouched by CI.
for statusless in '{"id":95,"head_sha":"SHA","conclusion":"failure"}' \
                  '{"id":96,"head_sha":"SHA","status":7,"conclusion":"failure"}'; do
  run_case "{\"workflow_runs\":[${statusless//SHA/$SHA_UNDER_TEST}]}" \
    "a run of ours whose status cannot be read: $statusless"
  expect_refused
  expect_said "$tmp/err" "1 run(s) in CI's answer"
done

# A status that IS a string but names no state the runs API documents. Read as
# "not completed, so still going" it would be waited out for the whole bound and
# then reported as a run that had not finished — sending a reader after a run
# that is not running, at the end of a wait that could never end. It is an
# unreadable run, exactly as an unrecognized CONCLUSION is an unreadable verdict,
# and it refuses at once.
run_case '{"workflow_runs":[{"id":112,"head_sha":"'"$SHA_UNDER_TEST"'","status":"borked","conclusion":"success","run_started_at":"2026-01-01T00:00:00Z"}]}' \
  "a run of ours whose status names no documented state"
expect_refused
expect_said "$tmp/err" "1 run(s) in CI's answer"
expect_said "$tmp/err" "named a status this does not recognize"
[ "$(grep -c 'head_sha' "$tmp/calls")" -eq 1 ] || {
  cat "$tmp/calls" >&2
  fail "$description: an unrecognized status must refuse at once, not be polled for"
}

# ...and every state the API does document must still be waited for rather than
# refused, so enumerating them cannot turn a run that is genuinely still going
# into a stopped release.
for going in queued in_progress waiting requested pending action_required; do
  run_polling_case "a run of ours that is $going, then finishes" \
    "{\"workflow_runs\":[$(workflow_run_json 113 "$SHA_UNDER_TEST" "$going" null 2026-01-01T00:00:00Z)]}" \
    "{\"workflow_runs\":[$(workflow_run_json 113 "$SHA_UNDER_TEST" completed '"success"' 2026-01-01T00:00:00Z)]}"
  expect_status 0
  expect_needs_check false
  expect_said "$tmp/out" "CI run 113 concluded success"
done

# A malformed run of somebody ELSE's is not ours to refuse over: its head_sha is
# readable and says so, and the release must not stop because an unrelated
# commit's run is odd.
run_case "{\"workflow_runs\":[{\"id\":97,\"head_sha\":\"$OTHER_SHA\"},$(workflow_run_json 98 "$SHA_UNDER_TEST" completed '"success"' 2026-01-01T00:00:00Z)]}" \
  "a malformed run belonging to a different commit"
expect_status 0
expect_needs_check false
expect_said "$tmp/out" "CI run 98 concluded success"

# Two malformed runs are counted, not collapsed: the message tells the reader
# how much of CI's answer this could not make sense of.
run_case "{\"workflow_runs\":[{\"id\":99,\"head_sha\":\"$SHA_UNDER_TEST\",\"status\":\"completed\",\"conclusion\":null},{\"id\":null,\"head_sha\":null}]}" \
  "two runs that cannot be read"
expect_refused
expect_said "$tmp/err" "2 run(s) in CI's answer"

# An undated run may be newer than a dated one, so it cannot be discarded
# while selecting the verdict.
run_case '{"workflow_runs":[
  {"id":100,"head_sha":"'"$SHA_UNDER_TEST"'","status":"completed","conclusion":"failure","run_started_at":{},"html_url":"https://example.invalid/run/100"},
  {"id":99,"head_sha":"'"$SHA_UNDER_TEST"'","status":"completed","conclusion":"success","run_started_at":"2026-05-01T00:00:00Z","html_url":"https://example.invalid/run/99"}]}' \
  "a run whose start time is not a timestamp"
expect_refused
expect_said "$tmp/err" "1 run(s) in CI's answer"

# A non-instant string leaves the same uncertainty.
run_case '{"workflow_runs":[
  {"id":110,"head_sha":"'"$SHA_UNDER_TEST"'","status":"completed","conclusion":"failure","run_started_at":"not-a-timestamp","html_url":"https://example.invalid/run/110"},
  {"id":109,"head_sha":"'"$SHA_UNDER_TEST"'","status":"completed","conclusion":"success","run_started_at":"2026-05-01T00:00:00Z","html_url":"https://example.invalid/run/109"}]}' \
  "a run whose start time is a string but not an instant"
expect_refused
expect_said "$tmp/err" "1 run(s) in CI's answer"

run_case "{\"workflow_runs\":[$(workflow_run_json 116 "$SHA_UNDER_TEST" completed '"failure"' 2026-99-01T00:00:00Z),$(workflow_run_json 117 "$SHA_UNDER_TEST" completed '"success"' 2026-05-01T00:00:00Z)]}" \
  "an impossible month must not outrank a real CI run"
expect_refused
expect_said "$tmp/err" "1 run(s) in CI's answer"

# Even the sole run needs a valid timestamp to establish a readable verdict.
run_case '{"workflow_runs":[
  {"id":111,"head_sha":"'"$SHA_UNDER_TEST"'","status":"completed","conclusion":"success","run_started_at":"whenever","html_url":"https://example.invalid/run/111"}]}' \
  "a sole run whose start time is not an instant"
expect_refused
expect_said "$tmp/err" "1 run(s) in CI's answer"

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

# A conclusion nobody enumerated — GitHub has several, and a new one must read
# as neither a pass nor the absence of a verdict. Gating the commit here would
# let this release publish on its own green over an answer CI did give.
run_case "{\"workflow_runs\":[$(workflow_run_json 60 "$SHA_UNDER_TEST" completed '"neutral"' 2026-01-01T00:00:00Z)]}" \
  "a CI run with a conclusion this script does not enumerate"
expect_refused
expect_said "$tmp/err" "concluded neutral"
expect_said "$tmp/err" "neither a pass, a refusal, nor the absence of a verdict"
expect_said "$tmp/err" "https://example.invalid/run/60"

run_case "{\"workflow_runs\":[$(workflow_run_json 61 "$SHA_UNDER_TEST" completed '"none"' 2026-01-01T00:00:00Z)]}" \
  "a literal none conclusion from a completed run"
expect_refused
expect_said "$tmp/err" "CI run 61 concluded none"

# An answer that could not be read is NOT an absent one: CI may have refused this
# commit and simply not been reachable to say so. Running the gate here would put
# a green run in this release where a red run in CI belongs, so it refuses and
# names the endpoint it was reading and the permission that restores the read.
GH_FAIL=1 run_case '{"workflow_runs":[]}' "an API that refuses the query"
unset GH_FAIL
expect_refused
expect_said "$tmp/err" "could not read CI's verdict for $SHA_UNDER_TEST"
expect_said "$tmp/err" "it was reading repos/owner/repo/actions/workflows/ci.yml/runs"
expect_said "$tmp/err" "actions:read"
expect_said "$tmp/err" "HTTP 403"

# Unparseable JSON is the same kind of unread answer, and refuses for the same
# reason: an HTML error page arrives here exactly like this.
run_case 'not json at all' "an API answering with something that is not JSON"
expect_refused
expect_said "$tmp/err" "was not a readable list of workflow runs"
expect_said "$tmp/err" "it was reading repos/owner/repo/actions/workflows/ci.yml/runs"

# An answer that PARSES but carries no workflow_runs array is the sharpest form
# of this: traversing it selects nothing, and selecting nothing is `absent` —
# the one state that runs the gate. So a truncated answer, a different endpoint's
# JSON, or a proxy's `{"message": ...}` would have gated the commit here and
# published on this job's own green. Each shape must refuse instead.
for shape in \
  '{"total_count":0}' \
  '{"message":"Not Found","documentation_url":"https://docs.github.com/rest"}' \
  '{"workflow_runs":{}}' \
  '{"workflow_runs":null}' \
  '[]' \
  'null'; do
  run_case "$shape" "an API answering $shape, which parses but is not a runs response"
  expect_refused
  expect_said "$tmp/err" "was not a readable list of workflow runs"
done

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

# The rule above is stated in prose in five other places, and the predecessor of
# this change got one of those copies wrong — a document promising a fallback
# the script does not do is how a reader comes to trust a release that stopped.
# So scripts/ci-verdict.sh carries ONE canonical sentence and every copy quotes
# it verbatim; this reconciles them.
#
# Matching is whitespace- and comment-marker-insensitive, because each copy
# wraps the sentence to its own column and its own comment syntax. A copy that
# reworded it, or that a future change edits on one side only, fails here.
contract="$(sed -n 's/^# CONTRACT: //p' scripts/ci-verdict.sh)"
[ -n "$contract" ] || fail "scripts/ci-verdict.sh no longer carries a '# CONTRACT: ' line, which is the one source every other statement of the fallback rule is checked against"
[ "$(sed -n 's/^# CONTRACT: //p' scripts/ci-verdict.sh | wc -l)" -eq 1 ] ||
  fail "scripts/ci-verdict.sh carries more than one '# CONTRACT: ' line; the rule has one source or it has none"

# Drop comment markers and fold every run of whitespace, so a sentence wrapped
# across two `#` lines of YAML reads the same as one line of Markdown.
flatten() { tr -d '#' | tr '\n' ' ' | tr -s '[:space:]' ' '; }
flat_contract="$(printf '%s' "$contract" | flatten)"

for stated_in in \
  .github/workflows/release.yml \
  scripts/check-workflows.sh \
  AGENTS.md \
  README.md \
  release-plz.toml; do
  if ! flatten <"$stated_in" | grep -Fq "$flat_contract"; then
    echo "check-ci-verdict: $stated_in does not state the CI-verdict fallback rule as scripts/ci-verdict.sh states it" >&2
    echo "  Next: quote this sentence there, wrapped however that file wraps (the comment markers and line breaks do not matter, the words do):" >&2
    printf '    %s\n' "$contract" >&2
    echo "  If the RULE changed, change it in scripts/ci-verdict.sh's '# CONTRACT: ' line first — that line is the one source, and this check is what stops a document promising a fallback the script does not do." >&2
    exit 1
  fi
done

echo "check-ci-verdict: ok"
