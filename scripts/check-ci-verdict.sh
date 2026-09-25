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
case "$*" in
  */jobs\?*)
    if [ -n "${GH_JOBS_FAIL:-}" ]; then
      echo 'gh: jobs endpoint refused (HTTP 403)' >&2
      exit 1
    fi
    if [ -n "${GH_JOBS_RESPONSE:-}" ]; then
      printf '%s' "$GH_JOBS_RESPONSE"
      exit 0
    fi
    run_id="$(sed -n 's@.*actions/runs/\([0-9]*\)/jobs.*@\1@p' <<<"$*")"
    jq -c --argjson id "$run_id" '
      [ .[] .workflow_runs[] | select(.id == $id) | . as $run
        | ($run.check_jobs // ["ubuntu-latest", "macos-latest", "windows-latest"]
           | if type == "array" and all(.[]; type == "string") then
               map({name:("check (" + . + ")"), id:($id + 100), head_sha:$run.head_sha,
                    status:(if $run.status == "completed" then "completed" else "in_progress" end),
                    conclusion:(if $run.status == "completed" then $run.conclusion else null end)})
             else . end)
      ] | flatten | [{jobs:.}]
    ' "$GH_STATE/last-response"
    exit ;;
esac
count="$GH_STATE/calls"
n=$(( $(cat "$count" 2>/dev/null || echo 0) + 1 ))
printf '%s' "$n" >"$count"
if [ -f "$GH_RUNS.$n" ]; then response="$(cat "$GH_RUNS.$n")"; else response="$(cat "$GH_RUNS")"; fi
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
  '['*) printf '%s' "$response" >"$GH_STATE/last-response" ;;
  *) printf '[%s]' "$response" >"$GH_STATE/last-response" ;;
esac
cat "$GH_STATE/last-response"
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
    GH_CALLS="$tmp/calls" GH_RUNS="$tmp/runs" GH_STATE="$tmp/state" GH_FAIL="${GH_FAIL:-}" GH_JOBS_FAIL="${GH_JOBS_FAIL:-}" GH_JOBS_RESPONSE="${GH_JOBS_RESPONSE:-}" \
    PATH="$tmp/bin:$PATH" \
    REPO="${REPO_OVERRIDE-owner/repo}" SHA="${SHA_OVERRIDE-$SHA_UNDER_TEST}" \
    CI_WORKFLOW="${WORKFLOW_OVERRIDE-ci.yml}" \
    CI_WAIT_ATTEMPTS="${WAIT_ATTEMPTS_OVERRIDE-3}" CI_WAIT_DELAY="${WAIT_DELAY_OVERRIDE-0}" \
    GITHUB_OUTPUT="${OUTPUT_OVERRIDE-$tmp/github-output}" \
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

run_with_jobs() {
  local id="$1" workflow_conclusion="$2" ubuntu="$3" macos="$4" windows="$5" jobs
  jobs="$(jq -nc --arg sha "$SHA_UNDER_TEST" --arg u "$ubuntu" --arg m "$macos" --arg w "$windows" '
    ["ubuntu-latest", "macos-latest", "windows-latest"] as $names
    | [$u, $m, $w] | to_entries
    | map(select(.value != "absent") | {id:(200 + .key), name:("check (" + $names[.key] + ")"), head_sha:$sha,
      status:(if .value == "pending" then "in_progress" else "completed" end),
      conclusion:(if .value == "pending" then null else .value end)})
  ')"
  workflow_run_json "$id" "$SHA_UNDER_TEST" completed "\"$workflow_conclusion\"" 2026-01-01T00:00:00Z |
    jq -c --argjson jobs "$jobs" '. + {check_jobs:$jobs}'
}

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
expect_said "$tmp/calls" 'actions/runs/11/jobs?filter=latest&per_page=100'

# The same verdict, with nowhere to record it: the workflow would read no
# decision, so the release stops saying so rather than failing through the shell.
mkdir -p "$tmp/unwritable-output"
OUTPUT_OVERRIDE="$tmp/unwritable-output" run_case "{\"workflow_runs\":[$(workflow_run_json 11 "$SHA_UNDER_TEST" completed '"success"' 2026-01-02T00:00:00Z)]}" \
  "an unwritable workflow output refuses"
expect_refused
expect_said "$tmp/err" "could not record the verdict for $SHA_UNDER_TEST"

GH_JOBS_FAIL=1 run_case "{\"workflow_runs\":[$(workflow_run_json 11 "$SHA_UNDER_TEST" completed '"success"' 2026-01-02T00:00:00Z)]}" \
  "the check jobs endpoint refuses after the run list succeeds"
unset GH_JOBS_FAIL
expect_refused
expect_said "$tmp/err" "could not read check jobs in CI run 11"

GH_JOBS_RESPONSE='not json' run_case "{\"workflow_runs\":[$(workflow_run_json 11 "$SHA_UNDER_TEST" completed '"success"' 2026-01-02T00:00:00Z)]}" \
  "a jobs endpoint returning malformed JSON"
unset GH_JOBS_RESPONSE
expect_refused
expect_said "$tmp/err" "check jobs in CI run 11 were unreadable"

GH_JOBS_RESPONSE='[{"jobs":{}}]' run_case "{\"workflow_runs\":[$(workflow_run_json 11 "$SHA_UNDER_TEST" completed '"success"' 2026-01-02T00:00:00Z)]}" \
  "a jobs endpoint returning no jobs array"
unset GH_JOBS_RESPONSE
expect_refused
expect_said "$tmp/err" "check jobs in CI run 11 were unreadable"

# `gh api --slurp` answers one document. A second one after it must refuse
# rather than be ignored: here only the ignored one carries the failure.
passing_jobs="$(run_with_jobs 11 success success success success | jq -c '[{jobs:.check_jobs}]')"
GH_JOBS_RESPONSE="$passing_jobs$(printf '%s' "$passing_jobs" | jq -c '.[0].jobs[1].conclusion = "failure"')" \
  run_case "{\"workflow_runs\":[$(workflow_run_json 11 "$SHA_UNDER_TEST" completed '"success"' 2026-01-02T00:00:00Z)]}" \
  "a jobs endpoint returning a second JSON document after a passing one"
unset GH_JOBS_RESPONSE
expect_refused
expect_said "$tmp/err" "check jobs in CI run 11 were unreadable"
expect_said "$tmp/err" "2 JSON documents"

run_case "[{\"workflow_runs\":[$(workflow_run_json 16 "$SHA_UNDER_TEST" completed '"success"' 2026-01-02T00:00:00Z)]}][{\"workflow_runs\":[$(workflow_run_json 17 "$SHA_UNDER_TEST" completed '"failure"' 2026-01-03T00:00:00Z)]}]" \
  "a runs endpoint returning a second JSON document after a passing one"
expect_refused
expect_said "$tmp/err" "CI runs for $SHA_UNDER_TEST were unreadable"
expect_said "$tmp/err" "2 JSON documents"

for mutation in '.check_jobs[0].id = 1.5' \
                '.check_jobs[0].head_sha = "2222222222222222222222222222222222222222"' \
                '.check_jobs[0].status = "unknown"' \
                '.check_jobs[1] |= (.status = "in_progress" | .conclusion = "failure")' \
                '.check_jobs[0].name = "check (bad\tname)"'; do
  fixture="$(run_with_jobs 125 success success success success | jq -c "$mutation")"
  run_case "{\"workflow_runs\":[$fixture]}" "an invalid check job field: $mutation"
  expect_refused
  expect_said "$tmp/err" "invalid field or unknown conclusion"
done

# The check jobs decide, not the workflow's own state: a run still going (a
# later job, or the run's own bookkeeping) whose check matrix already concluded
# is CI's word either way.
fixture="$(run_with_jobs 126 success success success success | jq -c '.status = "in_progress" | .conclusion = null')"
run_case "{\"workflow_runs\":[$fixture]}" "a complete passing check matrix while the workflow is still in progress"
expect_status 0
expect_needs_check false
expect_said "$tmp/out" "CI run 126 check jobs concluded success"

fixture="$(run_with_jobs 127 success success failure pending | jq -c '.status = "in_progress" | .conclusion = null')"
run_case "{\"workflow_runs\":[$fixture]}" "a failed check job while the workflow is still in progress"
expect_refused
expect_said "$tmp/err" "CI run 127 check job 201 (check (macos-latest)) concluded failure"
expect_said "$tmp/err" "https://example.invalid/run/127"

# The jobs endpoint is paginated like the runs one: a required job on a later
# page still counts.
paged_jobs="$(run_with_jobs 11 success success success success | jq -c '[{jobs:.check_jobs[0:1]},{jobs:.check_jobs[1:]}]')"
GH_JOBS_RESPONSE="$paged_jobs" \
  run_case "{\"workflow_runs\":[$(workflow_run_json 11 "$SHA_UNDER_TEST" completed '"success"' 2026-01-02T00:00:00Z)]}" \
  "check jobs split across two API pages"
unset GH_JOBS_RESPONSE
expect_status 0
expect_needs_check false
expect_said "$tmp/out" "CI run 11 check jobs concluded success"

run_case "[{\"workflow_runs\":[$(workflow_run_json 12 "$OTHER_SHA" completed '"failure"' 2026-01-05T00:00:00Z)]},{\"workflow_runs\":[$(workflow_run_json 13 "$SHA_UNDER_TEST" completed '"success"' 2026-01-02T00:00:00Z)]}]" \
  "a tagged commit whose run is on the next API page"
expect_status 0
expect_needs_check false
expect_said "$tmp/out" "CI run 13 check jobs concluded success"

run_case "{\"workflow_runs\":[{\"id\":14,\"head_sha\":\"$SHA_UNDER_TEST\",\"event\":\"pull_request\",\"head_branch\":\"feature\",\"status\":\"completed\",\"conclusion\":\"success\",\"run_started_at\":\"2026-01-05T00:00:00Z\"},$(workflow_run_json 15 "$SHA_UNDER_TEST" completed '"failure"' 2026-01-02T00:00:00Z)]}" \
  "a PR success cannot override main's failed push on the same SHA"
expect_refused
expect_said "$tmp/err" "CI run 15 check job"
grep -Fq "head_sha=$SHA_UNDER_TEST" "$tmp/calls" || {
  cat "$tmp/calls" >&2
  fail "$description: the API was not asked about the tagged commit"
}

# Only a foreign commit's run: this commit has no verdict, whatever that run
# says — and with no run, no macOS or Windows job verified it either.
run_case "{\"workflow_runs\":[$(workflow_run_json 15 "$OTHER_SHA" completed '"success"' 2026-01-05T00:00:00Z)]}" \
  "a run for a different commit only"
expect_refused
expect_said "$tmp/err" "no main-branch run of ci.yml for $SHA_UNDER_TEST"
expect_said "$tmp/err" "check (macos-latest) has no verdict"

# A re-run after a failure: the newest finished run for the commit is CI's word.
run_case "{\"workflow_runs\":[$(workflow_run_json 20 "$SHA_UNDER_TEST" completed '"failure"' 2026-01-01T00:00:00Z),$(workflow_run_json 21 "$SHA_UNDER_TEST" completed '"success"' 2026-01-03T00:00:00Z)]}" \
  "a re-run that succeeded after a failure"
expect_status 0
expect_needs_check false

# Each failing check conclusion must stop release and name the CI run.
for refusal in failure timed_out startup_failure; do
  run_case "{\"workflow_runs\":[$(workflow_run_json 30 "$SHA_UNDER_TEST" completed "\"$refusal\"" 2026-01-01T00:00:00Z)]}" \
    "a CI run that concluded $refusal for the tagged commit"
  expect_refused
  expect_said "$tmp/err" "CI run 30 check job"
  expect_said "$tmp/err" "https://example.invalid/run/30"
done

# A cancelled workflow with no concluded check job: the Ubuntu gate here could
# stand in for one of the three, so it stands in for none.
run_case "{\"workflow_runs\":[$(workflow_run_json 40 "$SHA_UNDER_TEST" completed '"cancelled"' 2026-01-01T00:00:00Z)]}" \
  "a cancelled CI run"
expect_refused
expect_said "$tmp/err" "CI run 40 check job check (macos-latest) has no success verdict for $SHA_UNDER_TEST (cancelled)"
expect_said "$tmp/err" "https://example.invalid/run/40"

run_case "{\"workflow_runs\":[$(run_with_jobs 120 cancelled success success success)]}" \
  "a cancelled workflow whose complete check matrix passed"
expect_status 0
expect_needs_check false
expect_said "$tmp/out" "CI run 120 check jobs concluded success"

run_case "{\"workflow_runs\":[$(run_with_jobs 121 cancelled success failure success)]}" \
  "a cancelled workflow with one failed check job"
expect_refused
expect_said "$tmp/err" "CI run 121 check job"

# The Ubuntu check job alone without a verdict: the gate this Ubuntu runner can
# run is the one that job would have run, so it runs here.
for gap in cancelled skipped stale absent pending; do
  run_case "{\"workflow_runs\":[$(run_with_jobs 122 cancelled "$gap" success success)]}" \
    "an Ubuntu check job that is $gap beside two successes"
  expect_status 0
  expect_needs_check true
  expect_said "$tmp/out" "CI run 122 check job check (ubuntu-latest) has no success verdict"
  expect_said "$tmp/out" "running the gate here on Ubuntu"
done

# A macOS or Windows check job without a verdict has no stand-in on this runner,
# so the release refuses naming that job and the run, whatever Ubuntu says.
for gap in cancelled skipped stale absent pending; do
  run_case "{\"workflow_runs\":[$(run_with_jobs 123 cancelled success "$gap" success)]}" \
    "a macOS check job that is $gap beside two successes"
  expect_refused
  expect_said "$tmp/err" "CI run 123 check job check (macos-latest) has no success verdict"
  expect_said "$tmp/err" "https://example.invalid/run/123"
  run_case "{\"workflow_runs\":[$(run_with_jobs 128 cancelled success success "$gap")]}" \
    "a Windows check job that is $gap beside two successes"
  expect_refused
  expect_said "$tmp/err" "CI run 128 check job check (windows-latest) has no success verdict"
done
expect_said "$tmp/err" "(never finished)"

run_case "{\"workflow_runs\":[$(run_with_jobs 129 cancelled cancelled success skipped)]}" \
  "Ubuntu and Windows check jobs both without a verdict"
expect_refused
expect_said "$tmp/err" "check job check (windows-latest) has no success verdict for $SHA_UNDER_TEST (skipped)"

run_case "{\"workflow_runs\":[$(run_with_jobs 130 failure cancelled success failure)]}" \
  "a failed check job beside an Ubuntu job without a verdict"
expect_refused
expect_said "$tmp/err" "CI run 130 check job 202 (check (windows-latest)) concluded failure"

# While the run is still going, an Ubuntu job without a verdict may yet get one,
# so it is waited for rather than gated here.
run_polling_case "an Ubuntu check job cancelled while CI is still running, then rerun" \
  "{\"workflow_runs\":[$(run_with_jobs 131 cancelled cancelled success success | jq -c '.status = "in_progress" | .conclusion = null')]}" \
  "{\"workflow_runs\":[$(run_with_jobs 131 success success success success)]}"
expect_status 0
expect_needs_check false
expect_said "$tmp/out" "CI run 131 check jobs concluded success"

# A check job the release does not know, or one it sees twice, is a matrix this
# selector was not written against.
fixture="$(run_with_jobs 132 success success success success | jq -c '.check_jobs += [.check_jobs[0] | .name = "check (freebsd-latest)" | .id = 299]')"
run_case "{\"workflow_runs\":[$fixture]}" "an undeclared check job beside a passing matrix"
expect_refused
expect_said "$tmp/err" "undeclared or repeated check job"
fixture="$(run_with_jobs 133 success success success success | jq -c '.check_jobs += [.check_jobs[1] | .conclusion = "cancelled" | .id = 299]')"
run_case "{\"workflow_runs\":[$fixture]}" "a check job listed twice"
expect_refused
expect_said "$tmp/err" "undeclared or repeated check job"

run_case "{\"workflow_runs\":[$(run_with_jobs 124 cancelled success mystery success)]}" \
  "a successful check beside an unknown job conclusion"
expect_refused
expect_said "$tmp/err" "unknown conclusion"

# A hand-made Release or a tag CI never saw has no run to read, so nothing
# verified it on macOS or Windows.
run_case '{"workflow_runs":[]}' "no CI run for the tagged commit"
expect_refused
expect_said "$tmp/err" "no main-branch run of ci.yml for $SHA_UNDER_TEST after 3 polls"

# The runs API can list a new push after the release's first query. Wait for
# the bound before treating an empty list as permanent absence.
run_polling_case "the tagged commit's CI run appears after the first query" \
  '{"workflow_runs":[]}' \
  "{\"workflow_runs\":[$(workflow_run_json 49 "$SHA_UNDER_TEST" completed '"success"' 2026-01-01T00:00:00Z)]}"
expect_status 0
expect_needs_check false
expect_said "$tmp/out" "CI run 49 check jobs concluded success"
[ "$(grep -c 'head_sha' "$tmp/calls")" -eq 2 ] || {
  cat "$tmp/calls" >&2
  fail "$description: expected two queries before deciding"
}

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
expect_said "$tmp/err" "did not reach a decisive verdict"
expect_said "$tmp/err" "after 3 polls"

# Two finished runs that started at the same instant: the tie is broken by run
# id, so the later run is CI's word and a coin flip never decides a release.
run_case "{\"workflow_runs\":[$(workflow_run_json 81 "$SHA_UNDER_TEST" completed '"success"' 2026-01-02T00:00:00Z),$(workflow_run_json 80 "$SHA_UNDER_TEST" completed '"failure"' 2026-01-02T00:00:00Z)]}" \
  "two finished runs that started at the same instant"
expect_status 0
expect_needs_check false
expect_said "$tmp/out" "CI run 81 check jobs concluded success"

# A run whose own FIELDS cannot be read is the same hazard one level down, and
# it is the subtler one: a malformed run that is merely dropped takes the
# selection one step closer to empty, and empty reads as `absent`. Each of these
# is a sole run, so dropping it would have waited out every poll and then
# blamed a missing CI run for what is an unreadable one.
#
# A finished run whose conclusion is absent, or of a type this cannot act on:
# the run RAN, so CI very likely reached a verdict this could not read.
run_case '{"workflow_runs":[{"id":90,"head_sha":"'"$SHA_UNDER_TEST"'","status":"completed","conclusion":null,"run_started_at":"2026-01-01T00:00:00Z","html_url":"https://example.invalid/run/90"}]}' \
  "a finished run with no conclusion"
expect_refused
expect_said "$tmp/err" "check job check (macos-latest) has no success verdict for $SHA_UNDER_TEST (no conclusion)"

# A control character in a conclusion must not shift the tab-delimited fields
# the script reads from jq's summary.
run_case '{"workflow_runs":[{"id":114,"head_sha":"'"$SHA_UNDER_TEST"'","status":"completed","conclusion":"success\t0\t0\t114","run_started_at":"2026-01-01T00:00:00Z"}]}' \
  "a conclusion containing a tab"
expect_refused
expect_said "$tmp/err" "invalid field or unknown conclusion"

run_case '{"workflow_runs":[{"id":115,"head_sha":"'"$SHA_UNDER_TEST"'","status":"completed","conclusion":"success\n","run_started_at":"2026-01-01T00:00:00Z"}]}' \
  "a conclusion containing a newline"
expect_refused
expect_said "$tmp/err" "invalid field or unknown conclusion"

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

# A run with no readable head_sha: there is no telling WHOSE it is, so the
# selector refuses rather than guess whether it is ours.
for anonymous in '{"id":92,"status":"completed","conclusion":"failure"}' \
                 '{"id":93,"head_sha":null,"status":"completed","conclusion":"failure"}' \
                 '{"id":94,"head_sha":{},"status":"completed","conclusion":"failure"}' \
                 '{"id":116,"head_sha":"not-a-commit","status":"completed","conclusion":"failure"}' \
                 '{"id":117,"head_sha":"abc1234","status":"completed","conclusion":"failure"}'; do
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
  expect_said "$tmp/out" "CI run 113 check jobs concluded success"
done

# A malformed run of somebody ELSE's is not ours to refuse over: its head_sha is
# readable and says so, and the release must not stop because an unrelated
# commit's run is odd.
run_case "{\"workflow_runs\":[{\"id\":97,\"head_sha\":\"$OTHER_SHA\"},$(workflow_run_json 98 "$SHA_UNDER_TEST" completed '"success"' 2026-01-01T00:00:00Z)]}" \
  "a malformed run belonging to a different commit"
expect_status 0
expect_needs_check false
expect_said "$tmp/out" "CI run 98 check jobs concluded success"

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
expect_said "$tmp/err" "CI run 71 check job"
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
expect_said "$tmp/err" "unknown conclusion"
expect_said "$tmp/err" "actions/runs/60/jobs"

run_case "{\"workflow_runs\":[$(workflow_run_json 61 "$SHA_UNDER_TEST" completed '"none"' 2026-01-01T00:00:00Z)]}" \
  "a literal none conclusion from a completed run"
expect_refused
expect_said "$tmp/err" "unknown conclusion"

# An answer that could not be read is NOT an absent one: CI may have refused this
# commit and simply not been reachable to say so. Running the gate here would put
# a green run in this release where a red run in CI belongs, so it refuses and
# names the endpoint it was reading and the permission that restores the read.
GH_FAIL=1 run_case '{"workflow_runs":[]}' "an API that refuses the query"
unset GH_FAIL
expect_refused
expect_said "$tmp/err" "could not read CI runs for $SHA_UNDER_TEST"
expect_said "$tmp/err" "it was reading repos/owner/repo/actions/workflows/ci.yml/runs"
expect_said "$tmp/err" "actions:read"
expect_said "$tmp/err" "HTTP 403"

# Unparseable JSON is the same kind of unread answer, and refuses for the same
# reason: an HTML error page arrives here exactly like this.
run_case 'not json at all' "an API answering with something that is not JSON"
expect_refused
expect_said "$tmp/err" "were unreadable"
expect_said "$tmp/err" "it was reading repos/owner/repo/actions/workflows/ci.yml/runs"

# An answer that PARSES but carries no workflow_runs array is the sharpest form
# of this: traversing it selects nothing, and selecting nothing reads as
# `absent`. So a truncated answer, a different endpoint's JSON, or a proxy's
# `{"message": ...}` would have waited out every poll and then blamed a missing
# CI run. Each shape must refuse at once, naming the unread answer instead.
for shape in \
  '{"total_count":0}' \
  '{"message":"Not Found","documentation_url":"https://docs.github.com/rest"}' \
  '{"workflow_runs":{}}' \
  '{"workflow_runs":null}' \
  '[]' \
  'null'; do
  run_case "$shape" "an API answering $shape, which parses but is not a runs response"
  expect_refused
  expect_said "$tmp/err" "were unreadable"
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

REPO_OVERRIDE="../repo" run_case '{"workflow_runs":[]}' "a parent-directory owner"
unset REPO_OVERRIDE
expect_status 2
expect_said "$tmp/err" "invalid owner component"

REPO_OVERRIDE="owner/." run_case '{"workflow_runs":[]}' "a current-directory repository"
unset REPO_OVERRIDE
expect_status 2
expect_said "$tmp/err" "invalid repository component"

WORKFLOW_OVERRIDE="../ci.yml/runs" run_case '{"workflow_runs":[]}' "a workflow that is not a file name"
unset WORKFLOW_OVERRIDE
expect_status 2
expect_said "$tmp/err" "is not a workflow file name"

WAIT_ATTEMPTS_OVERRIDE=0 run_case '{"workflow_runs":[]}' "a wait that never asks"
unset WAIT_ATTEMPTS_OVERRIDE
expect_status 2
expect_said "$tmp/err" "between 1 and 1000 polls"

WAIT_DELAY_OVERRIDE=soon run_case '{"workflow_runs":[]}' "a delay that is not a number of seconds"
unset WAIT_DELAY_OVERRIDE
expect_status 2
expect_said "$tmp/err" "is not a whole number of seconds"

WAIT_DELAY_OVERRIDE=99999 run_case '{"workflow_runs":[]}' "a delay past the bound"
unset WAIT_DELAY_OVERRIDE
expect_status 2
expect_said "$tmp/err" "exceeds the 3600-second bound"

# Keep the four prose copies of the fallback rule aligned with the contract in
# scripts/ci-verdict.sh.
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
