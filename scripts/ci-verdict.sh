#!/usr/bin/env bash
# Decide whether the full sweep CI ran over the tagged tree permits release publication.
# CONTRACT: a failed check job refuses release; only a complete check matrix whose every job ran the full sweep successfully skips the release gate; once CI ends, a non-Ubuntu check job without a successful sweep refuses release, and an Ubuntu one alone runs the sweep on the Ubuntu release runner
#
# The sweep is not the tagged commit's own push run: release-plz batches merges
# behind its release pull request, so CI sweeps the release there (ci.yml's
# `Full sweep (just check all)` step, chosen by scripts/ci-gate-tier.sh) and a
# push to main runs only the affected tier. The run read here is therefore the
# merged release pull request's, when its head carried exactly the tagged tree;
# otherwise (a hand-made Release, or a release pull request main moved under) a
# `workflow_dispatch` of ci.yml with tier=all on the tagged commit. Either way
# the tree is what was proven, so the release re-runs nothing that sweep ran.
#
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

# The step of ci.yml's check job that runs the full sweep. Its success in all
# three check jobs is the verdict; the affected tier's step does not count.
readonly SWEEP_STEP="Full sweep (just check all)"
# Every release pull request release-plz opens is on a branch with this prefix;
# check-ci-gate-tier.sh holds it to ci-gate-tier.sh's and release-plz.yml's.
readonly RELEASE_BRANCH_PREFIX="release-plz-"

# `needs_check` is the only thing the workflow reads; everything else here is for
# the person reading the log of a release that behaved unexpectedly.
decide() {
  local needs_check="$1" why="$2"
  printf 'ci-verdict: %s\n' "$why"
  if [ -n "${GITHUB_OUTPUT:-}" ] && ! printf 'needs_check=%s\n' "$needs_check" >>"$GITHUB_OUTPUT"; then
    refuse "ci-verdict: could not record the verdict for $sha" \
      "it was writing needs_check=$needs_check to \$GITHUB_OUTPUT ($GITHUB_OUTPUT)" \
      "re-run this release; the runner provides a writable \$GITHUB_OUTPUT, so a failed write is the runner's, not CI's verdict"
  fi
  exit 0
}

# Stop the release without deciding anything for the workflow to act on, saying
# which state was met, what it was reading when it met it, and what a person
# does about it. This is every state but the two decide() is given: it is not a
# licence to run the gate here, because a verdict this could not read may still
# be a refusal.
refuse() {
  local headline="$1" detail="$2" next="$3"
  printf '::error::%s\n' "$headline" >&2
  printf 'ci-verdict: %s\n' "$detail" >&2
  printf '  Next: %s\n' "$next" >&2
  exit 1
}

# The tree a commit carries, as the git data API states it.
tree_of() {
  local commit="$1" tree
  if ! tree="$(gh api "repos/$repo/git/commits/$commit" --jq .tree.sha 2>"$work/gh-error")"; then
    sed 's/^/    gh: /' "$work/gh-error" >&2
    refuse "ci-verdict: could not read the tree of $commit" "it was reading repos/$repo/git/commits/$commit" "restore contents:read and GH_TOKEN access, then re-run this release"
  fi
  [[ "$tree" =~ ^[0-9a-f]{40}$ ]] ||
    refuse "ci-verdict: the tree of $commit was unreadable" "it was reading repos/$repo/git/commits/$commit" "inspect the API response before releasing"
  printf '%s' "$tree"
}

# Which CI run swept the tagged tree: sets run_sha, run_event, run_branch and
# run_source. The merged release pull request is that run when its head carried
# exactly this tree; otherwise only a dispatched sweep of the tagged commit is.
locate_sweep() {
  local pulls_endpoint="repos/$repo/commits/$sha/pulls" pulls selected head_sha head_ref number head_tree tag_tree
  if ! pulls="$(gh api --paginate --slurp "$pulls_endpoint" 2>"$work/gh-error")"; then
    sed 's/^/    gh: /' "$work/gh-error" >&2
    refuse "ci-verdict: could not read the pull requests behind $sha" "it was reading $pulls_endpoint" "restore pull-requests:read and GH_TOKEN access, then re-run this release"
  fi
  if ! selected="$(printf '%s' "$pulls" | jq -rs --arg prefix "$RELEASE_BRANCH_PREFIX" '
    if length != 1 then error("the answer is \(length) JSON documents; `gh api --slurp` returns one") else .[0] end
    | if type != "array" or any(.[]; type != "array") then error("the answer is not a list of pull request pages") else add // [] end
    | [ .[] | select(type == "object" and (.merged_at | type) == "string" and .base.ref == "main"
                     and (.head.ref | type) == "string" and (.head.ref | startswith($prefix))) ]
    | sort_by(.merged_at) | last
    | if . == null then "none" else "\(.number)\t\(.head.sha)\t\(.head.ref)" end
  ' 2>"$work/jq-error")"; then
    sed 's/^/    jq: /' "$work/jq-error" >&2
    refuse "ci-verdict: the pull requests behind $sha were unreadable" "it was reading $pulls_endpoint" "inspect the API response, then re-run this release"
  fi
  # A dispatch is made on the release tag (or on main while it is at $sha), so
  # its run may name either as its branch; the commit is what identifies it.
  run_sha="$sha" run_event=workflow_dispatch run_branch=""
  run_source="no merged release pull request carried $sha"
  if [ "$selected" != none ]; then
    IFS=$'\t' read -r number head_sha head_ref <<<"$selected"
    [[ "$number" =~ ^[0-9]+$ && "$head_sha" =~ ^[0-9a-f]{40}$ && "$head_ref" =~ ^[A-Za-z0-9._/-]+$ ]] ||
      refuse "ci-verdict: release pull request fields behind $sha were unreadable" "it was reading $pulls_endpoint" "inspect the pull request's number, head sha and head ref before releasing"
    # Assigned rather than compared inline: a refusal inside a substitution in a
    # test would end only that subshell, and two failed lookups would compare
    # equal. As assignments, `set -e` stops the release on either.
    head_tree="$(tree_of "$head_sha")"
    tag_tree="$(tree_of "$sha")"
    if [ "$head_tree" = "$tag_tree" ]; then
      run_sha="$head_sha" run_event=pull_request run_branch="$head_ref"
      run_source="release pull request #$number ($head_ref) carried exactly this tree at $head_sha"
    else
      run_source="release pull request #$number's head $head_sha carried a different tree than $sha"
    fi
  fi
  printf 'ci-verdict: %s; reading its %s run of %s\n' "$run_source" "$run_event" "$workflow"
  endpoint="repos/$repo/actions/workflows/$workflow/runs?head_sha=$run_sha&event=$run_event${run_branch:+&branch=$run_branch}&per_page=100"
}

# The run identifies the exact sweep; its workflow conclusion is not the gate
# verdict. The verdict comes from that run's check matrix jobs.
read_run() {
  local runs selected
  if ! runs="$(gh api --paginate --slurp "$endpoint" 2>"$work/gh-error")"; then
    sed 's/^/    gh: /' "$work/gh-error" >&2
    refuse "ci-verdict: could not read CI runs for $run_sha" "it was reading $endpoint" "restore actions:read and GH_TOKEN access, then re-run this release"
  fi
  if ! selected="$(printf '%s' "$runs" | jq -rs --arg sha "$run_sha" --arg event "$run_event" --arg branch "$run_branch" '
    def valid_id: type == "number" and . > 0 and floor == .;
    def valid_instant: type == "string" and (. as $t | try ((fromdateiso8601 | todateiso8601) == $t) catch false);
    if length != 1 then error("the answer is \(length) JSON documents; `gh api --slurp` returns one") else .[0] end
    | if type != "array" or length == 0 or any(.[]; type != "object" or (.workflow_runs | type) != "array") then
      error("the answer has no workflow_runs pages")
    else map(.workflow_runs) | add end
    | . as $all
    | [$all[] | select(.head_sha == $sha)] as $matching
    | [$matching[] | select(.event == $event and ($branch == "" or .head_branch == $branch))] as $mine
    | (([ $all[] | select(.head_sha | if type == "string" then test("^[0-9a-f]{40}$") | not else true end) ] | length)
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
    refuse "ci-verdict: CI runs for $run_sha were unreadable" "it was reading $endpoint" "inspect the API response, then re-run this release"
  fi
  IFS=$'\t' read -r run_status run_id run_url <<<"$selected"
  if [ "$run_status" = unreadable ]; then
    refuse "ci-verdict: $run_id run(s) in CI's answer for $run_sha could not be read" "it was reading $endpoint" "inspect the malformed run fields before releasing"
  fi
}

# Read the matrix jobs through the documented jobs endpoint. A workflow may be
# cancelled after those jobs passed, so its own conclusion cannot replace this.
# Until the run ends, a job without a verdict may still get one, so only a
# success or a failure decides; once it ends, a job without one never will.
read_jobs() {
  local jobs summary jobs_endpoint
  jobs_endpoint="repos/$repo/actions/runs/$run_id/jobs?filter=latest&per_page=100"
  if ! jobs="$(gh api --paginate --slurp "$jobs_endpoint" 2>"$work/gh-error")"; then
    sed 's/^/    gh: /' "$work/gh-error" >&2
    refuse "ci-verdict: could not read check jobs in CI run $run_id" "it was reading $jobs_endpoint" "restore the GitHub API read, then re-run this release"
  fi
  if ! summary="$(printf '%s' "$jobs" | jq -rs --arg sha "$run_sha" --arg run_status "$run_status" --arg sweep "$SWEEP_STEP" '
    def valid_id: type == "number" and . > 0 and floor == .;
    def valid_status: . as $s | ["completed", "queued", "in_progress", "waiting", "requested", "pending"] | index($s) != null;
    def known_conclusion: . == null or (. as $c | ["success", "failure", "timed_out", "startup_failure", "cancelled", "skipped", "stale"] | index($c) != null);
    if length != 1 then error("the answer is \(length) JSON documents; `gh api --slurp` returns one") else .[0] end
    | if type != "array" or length == 0 or any(.[]; type != "object" or (.jobs | type) != "array") then
      error("the answer has no jobs pages")
    else map(.jobs) | add end
    | . as $all
    | ["check (macos-latest)", "check (ubuntu-latest)", "check (windows-latest)"] as $declared
    | [ $all[] | select((.name | type) == "string" and (.name | test("^check \\("))) ] as $checks
    | ([$checks[] | select(.status == "completed" and (.conclusion as $c | ["failure", "timed_out", "startup_failure"] | index($c)))] | first) as $failed
    # A job is a sweep verdict only when it concluded success AND its sweep step
    # did: a green job whose sweep step was skipped ran the affected tier, which
    # answers for a diff, not for the tree.
    | def swept: .status == "completed" and .conclusion == "success"
                 and any(.steps[]; .name == $sweep and .conclusion == "success");
    if any($all[]; (.name | type) != "string")
         or any($checks[]; (.name | test("[^ -~]")) or (.id | valid_id | not) or .head_sha != $sha
                       or (.status | valid_status | not)
                       or (.conclusion | known_conclusion | not)
                       or (.status != "completed" and .conclusion != null)
                       or (.status == "completed" and ((.steps | type) != "array"
                           or any(.steps[]; type != "object" or (.name | type) != "string"
                                            or (.conclusion | known_conclusion | not))))
                       or (.name as $n | $declared | index($n) | not))
         or ([$checks[].name] | length) != ([$checks[].name] | unique | length) then
        "unreadable\t0\t\t"
      elif $failed != null then
        "failure\t\($failed.id)\t\($failed.name)\t\($failed.conclusion)"
      elif all($declared[]; . as $n | any($checks[]; .name == $n and swept)) then
        "success\t0\t\t"
      elif $run_status != "completed" then
        "pending\t0\t\t"
      else
        [ $declared[] | . as $n | ([$checks[] | select(.name == $n)] | first) as $job
          | select($job == null or ($job | swept | not))
          | {name: $n, id: ($job.id // 0),
             why: (if $job == null then "absent" elif $job.status != "completed" then "never finished"
                   elif $job.conclusion != "success" then ($job.conclusion // "no conclusion")
                   else "ran the affected tier, not the full sweep" end)} ]
        | (map(select(.name != "check (ubuntu-latest)")) + .) | first
        | "\(if .name == "check (ubuntu-latest)" then "here" else "elsewhere" end)\t\(.id)\t\(.name)\t\(.why)"
      end
  ' 2>"$work/jq-error")"; then
    sed 's/^/    jq: /' "$work/jq-error" >&2
    refuse "ci-verdict: check jobs in CI run $run_id were unreadable" "it was reading $jobs_endpoint" "inspect the API response, then re-run this release"
  fi
  IFS=$'\t' read -r check_state job_id job_name job_why <<<"$summary"
  if [ "$check_state" = unreadable ]; then
    refuse "ci-verdict: check jobs in CI run $run_id contained an invalid field or unknown conclusion, or an undeclared or repeated check job" "it was reading $jobs_endpoint" "inspect those jobs before releasing"
  fi
}

# This runner is Ubuntu, so the sweep it can run stands in for CI's Ubuntu check
# job only; a macOS or Windows job without a verdict has no stand-in here.
locate_sweep
for poll in $(seq 1 "$wait_attempts"); do
  read_run
  if [ "$run_status" = absent ]; then
    if [ "$poll" -eq "$wait_attempts" ]; then
      refuse "ci-verdict: CI has no $run_event run of $workflow for $run_sha after $wait_attempts polls, so check (macos-latest) has no sweep verdict for $sha's tree" "$run_source; the release runs on Ubuntu, so the sweep it could run here cannot verify macOS or Windows" "sweep the tagged tree (gh workflow run $workflow --ref <the release tag> -f tier=all), then re-run this release"
    fi
    sleep "$wait_delay"
    continue
  fi
  read_jobs
  case "$check_state" in
    success)
      decide false "CI run $run_id check jobs concluded success for $sha ($run_url), each one a full sweep of this tree; publishing without sweeping it again." ;;
    failure)
      refuse "CI run $run_id check job $job_id ($job_name) concluded $job_why for $sha" "the run is ${run_url:-<no URL>}" "fix the commit and release the fix" ;;
    elsewhere)
      refuse "CI run $run_id check job $job_name has no success verdict for $sha ($job_why)" "the run is ${run_url:-<no URL>}; the release runs on Ubuntu, so the sweep it could run here cannot verify that platform" "re-run $job_name in CI run $run_id until it sweeps successfully, or dispatch $workflow with tier=all on the release tag, then re-run this release" ;;
    here)
      decide true "CI run $run_id check job $job_name has no success verdict for $sha ($job_why) and every other check job swept it; running the gate here on Ubuntu (the full sweep, just check all)." ;;
  esac
  if [ "$poll" -lt "$wait_attempts" ]; then sleep "$wait_delay"; fi
done
refuse "ci-verdict: CI check jobs for $sha did not reach a decisive verdict after $wait_attempts polls" "the run is ${run_url:-<no URL>}" "wait for CI to settle, then re-run this release"
