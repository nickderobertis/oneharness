#!/usr/bin/env bash
# llmlint: ignore-file[new_code_lands_in_a_project] The rule presumes an Nx project graph; this repository has none by a recorded decision (`AGENTS.md`: root `just` delegates to Cargo/Bun without Nx because the two-package graph is static), so no project definition can cover this file. What runs it is release.yml's `gate` job, and scripts/check-ci-verdict.sh covers it from `just lint-workflows`.
# Decide whether the tagged commit's CI run permits release publication.
# CONTRACT: a failed check job refuses release; only a complete successful check matrix skips the release gate; an absent or incomplete matrix runs the gate after CI ends
# Reads REPO, SHA, CI_WORKFLOW, CI_WAIT_ATTEMPTS and CI_WAIT_DELAY. Writes
# needs_check=true|false to GITHUB_OUTPUT when set.
set -euo pipefail

repo="${REPO:-${GITHUB_REPOSITORY:-}}"
sha="${SHA:-${GITHUB_SHA:-}}"
workflow="${CI_WORKFLOW:-ci.yml}"
wait_attempts="${CI_WAIT_ATTEMPTS:-60}"
wait_delay="${CI_WAIT_DELAY:-30}"

usage() {
  printf 'ci-verdict: %s\n' "$1" >&2
  printf '  Next: call it as the release workflow does — REPO=owner/name SHA=<40-hex commit> scripts/ci-verdict.sh. In CI both come from the release event, whose sha is the tagged commit.\n' >&2
  exit 2
}

[ -n "$repo" ] || usage "no repository to query (\$REPO and \$GITHUB_REPOSITORY are both empty)"
[ -n "$sha" ] || usage "no commit to ask about (\$SHA and \$GITHUB_SHA are both empty)"
case "$sha" in
  *[!0-9a-f]* | "") usage "\$SHA '$sha' is not a commit sha" ;;
esac
[ "${#sha}" -eq 40 ] || usage "\$SHA '$sha' is not a full 40-character commit sha; an abbreviated sha selects nothing through the runs API"
# Each of the three below is interpolated into a GitHub API path or handed to
# `sleep`, so none is taken on trust.
[[ "$repo" =~ ^[A-Za-z0-9._-]+/[A-Za-z0-9._-]+$ ]] ||
  usage "\$REPO '$repo' is not an owner/name repository"
case "${repo%%/*}" in . | ..) usage "\$REPO '$repo' has an invalid owner component" ;; esac
case "${repo#*/}" in . | ..) usage "\$REPO '$repo' has an invalid repository component" ;; esac
[[ "$workflow" =~ ^[A-Za-z0-9._-]+\.ya?ml$ ]] ||
  usage "\$CI_WORKFLOW '$workflow' is not a workflow file name"
case "$wait_attempts" in
  "" | *[!0-9]*) usage "\$CI_WAIT_ATTEMPTS '$wait_attempts' is not a whole number of polls" ;;
esac
[ "${#wait_attempts}" -le 4 ] && [ "$wait_attempts" -ge 1 ] && [ "$wait_attempts" -le 1000 ] ||
  usage "\$CI_WAIT_ATTEMPTS '$wait_attempts' must be between 1 and 1000 polls"
case "$wait_delay" in
  "" | *[!0-9]*) usage "\$CI_WAIT_DELAY '$wait_delay' is not a whole number of seconds" ;;
esac
[ "${#wait_delay}" -le 4 ] && [ "$wait_delay" -le 3600 ] || usage "\$CI_WAIT_DELAY '$wait_delay' exceeds the 3600-second bound"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

endpoint="repos/$repo/actions/workflows/$workflow/runs?head_sha=$sha&event=push&branch=main&per_page=100"

# `needs_check` is the only thing the workflow reads; everything else here is for
# the person reading the log of a release that behaved unexpectedly.
decide() {
  local needs_check="$1" why="$2"
  printf 'ci-verdict: %s\n' "$why"
  if [ -n "${GITHUB_OUTPUT:-}" ]; then
    printf 'needs_check=%s\n' "$needs_check" >>"$GITHUB_OUTPUT"
  fi
  exit 0
}

# Stop the release without deciding anything for the workflow to act on, saying
# which state was met, what it was reading when it met it, and what a person
# does about it. This is every state but the four enumerated above: it is not a
# licence to run the gate here, because a verdict this could not read may still
# be a refusal.
refuse() {
  local headline="$1" detail="$2" next="$3"
  printf '::error::%s\n' "$headline" >&2
  printf 'ci-verdict: %s\n' "$detail" >&2
  printf '  Next: %s\n' "$next" >&2
  exit 1
}

# The run identifies the exact push; its workflow conclusion is not the gate
# verdict. The verdict comes from that run's check matrix jobs.
read_run() {
  local runs selected
  if ! runs="$(gh api --paginate --slurp "$endpoint" 2>"$work/gh-error")"; then
    sed 's/^/    gh: /' "$work/gh-error" >&2
    refuse "ci-verdict: could not read CI runs for $sha" "it was reading $endpoint" "restore actions:read and GH_TOKEN access, then re-run this release"
  fi
  if ! selected="$(printf '%s' "$runs" | jq -rs --arg sha "$sha" '
    def valid_id: type == "number" and . > 0 and floor == .;
    def valid_instant: type == "string" and (. as $t | try ((fromdateiso8601 | todateiso8601) == $t) catch false);
    if length != 1 then error("the answer is \(length) JSON documents; `gh api --slurp` returns one") else .[0] end
    | if type != "array" or length == 0 or any(.[]; type != "object" or (.workflow_runs | type) != "array") then
      error("the answer has no workflow_runs pages")
    else map(.workflow_runs) | add end
    | . as $all
    | [$all[] | select(.head_sha == $sha)] as $matching
    | [$matching[] | select(.event == "push" and .head_branch == "main")] as $mine
    | (([ $all[] | select((.head_sha | type) != "string") ] | length)
      + ([ $matching[] | select((.event | type) != "string" or (.head_branch | type) != "string") ] | length)
      + ([ $mine[] | select((.id | valid_id | not)
                            or ((.run_started_at // .created_at) | valid_instant | not)
                            or (.status as $s | ["completed", "queued", "in_progress", "waiting", "requested", "pending", "action_required"] | index($s) | not)) ] | length))
      as $unreadable
    | if $unreadable > 0 then "unreadable\t\($unreadable)"
      else ($mine | sort_by((.run_started_at // .created_at), .id) | last) as $run
        | if $run == null then "absent\t0"
          else "\($run.status)\t\($run.id)\t\((($run.html_url | strings) // "") | gsub("[^!-~]"; ""))"
          end
      end
  ' 2>"$work/jq-error")"; then
    sed 's/^/    jq: /' "$work/jq-error" >&2
    refuse "ci-verdict: CI runs for $sha were unreadable" "it was reading $endpoint" "inspect the API response, then re-run this release"
  fi
  IFS=$'\t' read -r run_status run_id run_url <<<"$selected"
  if [ "$run_status" = unreadable ]; then
    refuse "ci-verdict: $run_id run(s) in CI's answer for $sha could not be read" "it was reading $endpoint" "inspect the malformed run fields before releasing"
  fi
}

# Read the matrix jobs through the documented jobs endpoint. A workflow may be
# cancelled after those jobs passed, so its own conclusion cannot replace this.
read_jobs() {
  local jobs summary jobs_endpoint
  jobs_endpoint="repos/$repo/actions/runs/$run_id/jobs?filter=latest&per_page=100"
  if ! jobs="$(gh api --paginate --slurp "$jobs_endpoint" 2>"$work/gh-error")"; then
    sed 's/^/    gh: /' "$work/gh-error" >&2
    refuse "ci-verdict: could not read check jobs in CI run $run_id" "it was reading $jobs_endpoint" "restore the GitHub API read, then re-run this release"
  fi
  if ! summary="$(printf '%s' "$jobs" | jq -rs --arg sha "$sha" '
    def valid_id: type == "number" and . > 0 and floor == .;
    def valid_status: . as $s | ["completed", "queued", "in_progress", "waiting", "requested", "pending"] | index($s) != null;
    def known_conclusion: . == null or (. as $c | ["success", "failure", "timed_out", "startup_failure", "cancelled", "skipped", "stale"] | index($c) != null);
    if length != 1 then error("the answer is \(length) JSON documents; `gh api --slurp` returns one") else .[0] end
    | if type != "array" or length == 0 or any(.[]; type != "object" or (.jobs | type) != "array") then
      error("the answer has no jobs pages")
    else map(.jobs) | add end
    | . as $all
    | [ $all[] | select((.name | type) == "string" and (.name | test("^check \\("))) ] as $checks
    | ([$checks[] | select(.status == "completed" and (.conclusion as $c | ["failure", "timed_out", "startup_failure"] | index($c)))] | first) as $failed
    | if any($all[]; (.name | type) != "string")
         or any($checks[]; (.name | test("[^ -~]")) or (.id | valid_id | not) or .head_sha != $sha
                       or (.status | valid_status | not)
                       or (.conclusion | known_conclusion | not)) then
        "unreadable\t0\t"
      elif $failed != null then
        "failure\t\($failed.id)\t\($failed.name)"
      elif ([$checks[].name] | sort) == ["check (macos-latest)", "check (ubuntu-latest)", "check (windows-latest)"]
           and all($checks[]; .status == "completed" and .conclusion == "success") then
        "success\t0\t"
      elif any($checks[]; .status == "completed" and .conclusion == "success") then
        "partial\t0\t"
      elif any($checks[]; .status != "completed") then
        "pending\t0\t"
      elif any($checks[]; .conclusion != null and (.conclusion as $c | ["cancelled", "skipped", "stale"] | index($c) | not)) then
        "unreadable\t0\t"
      else "absent\t0\t" end
  ' 2>"$work/jq-error")"; then
    sed 's/^/    jq: /' "$work/jq-error" >&2
    refuse "ci-verdict: check jobs in CI run $run_id were unreadable" "it was reading $jobs_endpoint" "inspect the API response, then re-run this release"
  fi
  IFS=$'\t' read -r check_state job_id job_name <<<"$summary"
  if [ "$check_state" = unreadable ]; then
    refuse "ci-verdict: check jobs in CI run $run_id contained an invalid field or unknown conclusion" "it was reading $jobs_endpoint" "inspect those jobs before releasing"
  fi
}

# Only a complete set of successful check legs gates the commit. A completed
# run with a missing or non-successful leg needs the local gate; a failed leg
# refuses publication before that fallback can run.
for poll in $(seq 1 "$wait_attempts"); do
  read_run
  if [ "$run_status" = absent ]; then
    if [ "$poll" -eq "$wait_attempts" ]; then
      decide true "CI has no main-branch check run for $sha after $wait_attempts polls; running the gate here."
    fi
    sleep "$wait_delay"
    continue
  fi
  read_jobs
  case "$check_state" in
    success)
      decide false "CI run $run_id check jobs concluded success for $sha ($run_url); publishing without re-running the gate." ;;
    failure)
      refuse "CI run $run_id check job $job_id ($job_name) concluded failure for $sha" "the run is ${run_url:-<no URL>}" "fix the commit and release the fix" ;;
    absent)
      if [ "$run_status" = completed ]; then
        decide true "CI run $run_id has no complete check verdict for $sha; running the gate here."
      fi ;;
    partial)
      if [ "$run_status" = completed ]; then
        decide true "CI run $run_id has an incomplete check matrix for $sha; running the gate here."
      fi ;;
    pending)
      if [ "$run_status" = completed ]; then
        decide true "CI run $run_id ended with an incomplete check matrix for $sha; running the gate here."
      fi ;;
  esac
  if [ "$poll" -lt "$wait_attempts" ]; then sleep "$wait_delay"; fi
done
refuse "ci-verdict: CI check jobs for $sha did not reach a decisive verdict after $wait_attempts polls" "the run is ${run_url:-<no URL>}" "wait for CI to settle, then re-run this release"
