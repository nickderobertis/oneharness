#!/usr/bin/env bash
# llmlint: ignore-file[new_code_lands_in_a_project] The rule presumes an Nx project graph; this repository has none by a recorded decision (`AGENTS.md`: root `just` delegates to Cargo/Bun without Nx because the two-package graph is static), so no project definition can cover this file. What runs it is release.yml's `gate` job, and scripts/check-ci-verdict.sh covers it from `just lint-workflows`.
# Read CI's verdict for the exact commit a release was tagged at, so the release
# consumes the gate CI already ran instead of running it a second time.
#
# release.yml starts from `release: published`, and the commit it publishes is
# the one CI already gated on `main`. Re-running that gate spends a runner for
# twenty minutes and exposes an approved commit to an unrelated transient
# failure DURING publication, where a red run is read as a broken release.
#
# So this answers one question — what did CI say about THIS commit? — and the
# workflow acts on it:
#
#   success                    -> needs_check=false, publish without re-checking.
#   failure/timed out/startup  -> exit 1 naming the run, because a commit CI
#                                 refused must not publish.
#   cancelled or absent        -> needs_check=true, run the gate here. CI on
#                                 `main` cancels in progress per ref, so a later
#                                 push can cancel the tagged commit's run, and a
#                                 hand-made Release may have no CI run at all.
#
# That rule in one sentence — the line every other copy of it is checked
# against, by scripts/check-ci-verdict.sh, so a document cannot come to promise
# something this script does not do:
#
# CONTRACT: only a cancelled CI run and no CI run at all make the release run the gate itself; every other state refuses rather than standing in for a verdict it could not read
#
# Those two are the ONLY states that run the gate here, because they are the
# only two where CI reached no verdict at all: repeating the checks then
# establishes something nobody knew. Every other state — an answer that could
# not be read, did not parse, carries a conclusion this does not recognize, or
# has not arrived before the wait runs out — exits 1 naming what it was reading.
# A verdict may well EXIST in each of those and this simply could not see it, so
# running the gate here would let a green run in this job stand in for a red run
# in CI. A release that reuses evidence must never be able to manufacture it.
#
# A run that is still RUNNING is waited for rather than duplicated: a second full
# sweep of the same commit, alongside the one already sweeping it, is the cost
# this exists to avoid. The wait is bounded, and a bound that runs out refuses
# rather than deciding the commit is unverified — CI is still deciding it.
#
# Reads:
#   REPO         owner/name to query (default $GITHUB_REPOSITORY)
#   SHA          the tagged commit, full 40-hex (default $GITHUB_SHA)
#   CI_WORKFLOW  the workflow file whose verdict counts (default ci.yml)
#   CI_WAIT_ATTEMPTS / CI_WAIT_DELAY
#                how long to wait on a run that has not finished (default 60
#                polls, 30s apart)
# Writes `needs_check=true|false` to $GITHUB_OUTPUT when set, and says on stdout
# what it decided and why. `gh` must be authenticated with `actions: read`.
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
[[ "$workflow" =~ ^[A-Za-z0-9._-]+\.ya?ml$ ]] ||
  usage "\$CI_WORKFLOW '$workflow' is not a workflow file name"
case "$wait_attempts" in
  "" | *[!0-9]*) usage "\$CI_WAIT_ATTEMPTS '$wait_attempts' is not a whole number of polls" ;;
esac
[ "$wait_attempts" -ge 1 ] || usage "\$CI_WAIT_ATTEMPTS '$wait_attempts' never asks; the bound must allow at least one poll"
case "$wait_delay" in
  "" | *[!0-9]*) usage "\$CI_WAIT_DELAY '$wait_delay' is not a whole number of seconds" ;;
esac

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

endpoint="repos/$repo/actions/workflows/$workflow/runs?head_sha=$sha&per_page=100"

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

# Read CI's answer for this commit: it sets $conclusion (the newest FINISHED
# run's, or `none`), $for_sha (how many runs exist for the commit at all),
# $run_id and $run_url — but an answer that cannot be read at all refuses from
# inside, because there is nothing to poll for when the API itself is
# unreachable and nothing to fall back to when a verdict may exist unread.
conclusion=none
for_sha=0
pending=0
run_id=
run_url=
read_verdict_or_refuse() {
  local runs summary
  if ! runs="$(gh api "$endpoint" 2>"$work/gh-error")"; then
    sed 's/^/    gh: /' "$work/gh-error" >&2
    refuse \
      "ci-verdict: could not read CI's verdict for $sha; gh refused the query (its own words are above). Whether CI passed or refused this commit is unknown, so the release stops here." \
      "it was reading $endpoint" \
      "give this job the actions:read permission and a GH_TOKEN, check that $workflow exists in $repo, then re-run this release. This job will not run the gate in CI's place: a verdict it could not read may well be a refusal, and a green run here would stand in for it."
  fi

  # The API's own `head_sha` filter is not trusted to be the whole answer: this
  # decides whether a gate runs, so the sha is matched again here. Runs that
  # have not finished say nothing yet, so only completed ones are selected,
  # newest last by start time with the run id breaking a tie — and the unfinished
  # ones are counted, because a rerun in flight is CI still deciding, whatever an
  # older run of the same commit concluded.
  #
  # A parseable answer is not yet a trustworthy one, so the fields this acts on
  # are type-checked here: a run whose id or conclusion is missing or of the
  # wrong type cannot become the verdict that publishes a release, and a start
  # time that is not an ISO-8601 instant sorts as the oldest rather than as the
  # newest — jq orders objects above strings, and `not-a-timestamp` above any
  # digit, so either would win the selection outright unchecked. The URL is only
  # ever printed, but it arrives on a tab-delimited line this script then splits,
  # so anything unprintable is dropped from it here.
  if ! summary="$(printf '%s' "$runs" | jq -r --arg sha "$sha" '
    if (type) != "object" or (has("workflow_runs") | not) or ((.workflow_runs | type) != "array") then
      error("the answer carries no workflow_runs array, so it is not a runs response at all")
    else . end
    | [ .workflow_runs[]
      | select((.head_sha | type) == "string" and .head_sha == $sha)
      | select((.status | type) == "string") ] as $mine
    | ([ $mine[] | select(.status != "completed") ] | length) as $pending
    | ([ $mine[] | select(.status == "completed")
         | select((.id | type) == "number" and (.conclusion | type) == "string") ]
       | sort_by((((.run_started_at // .created_at) | strings
                    | select(test("^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}([.][0-9]+)?(Z|[+-][0-9]{2}:?[0-9]{2})$"))) // ""),
                  .id) | last) as $newest
    | if $newest == null then
        "none\t\($mine | length)\t\($pending)\t\t"
      else
        "\($newest.conclusion // "none")\t\($mine | length)\t\($pending)\t\($newest.id)\t\((($newest.html_url | strings) // "") | gsub("[^!-~]"; ""))"
      end
  ' 2>"$work/jq-error")"; then
    sed 's/^/    jq: /' "$work/jq-error" >&2
    refuse \
      "ci-verdict: could not read CI's verdict for $sha; the answer was not a readable list of workflow runs (jq's own words are above). Whether CI passed or refused this commit is unknown, so the release stops here." \
      "it was reading $endpoint" \
      "read that endpoint by hand — an HTML error page, a proxy's response, or any answer without a workflow_runs array arrives here — then re-run this release once it answers normally. This job will not run the gate in CI's place: a verdict it could not read may well be a refusal, and an answer whose SHAPE is unrecognized says nothing about whether CI passed."
  fi

  IFS=$'\t' read -r conclusion for_sha pending run_id run_url <<<"$summary"
}

# A run that is still going is WAITED for, never duplicated: running the whole
# gate here beside the one already running it is the second sweep of one commit
# this script exists to avoid. Silent while it waits — the decision below says
# how long it took.
for poll in $(seq 1 "$wait_attempts"); do
  read_verdict_or_refuse
  if [ "$pending" -eq 0 ]; then
    break
  fi
  if [ "$poll" -lt "$wait_attempts" ]; then
    sleep "$wait_delay"
  fi
done

# The wait running out is not a cancelled run: CI is still deciding this commit,
# and the verdict it is about to reach may be a refusal.
if [ "$pending" -gt 0 ]; then
  refuse \
    "ci-verdict: CI still had $pending unfinished run(s) for $sha after $wait_attempts polls over ~$((wait_attempts * wait_delay))s. CI has not finished deciding this commit, so the release stops here." \
    "it was reading $endpoint" \
    "wait for that run to finish and re-run this release workflow; if it will never finish, cancel it and re-run, which makes this a cancelled run and gets the gate run here. This job will not run the gate while CI is still deciding: the verdict CI is about to reach may be a refusal."
fi

case "$conclusion" in
  success)
    decide false "CI run $run_id concluded success for $sha ($run_url); publishing without re-running the gate."
    ;;
  failure | timed_out | startup_failure)
    refuse \
      "CI run $run_id concluded $conclusion for the commit this release tags, $sha. A commit CI refused must not publish." \
      "the failing run is ${run_url:-<the API returned no URL>}" \
      "read that run, fix the commit on the main branch, and release the fix. Re-running this release cannot make the refusal go away."
    ;;
  cancelled)
    # CI was stopped before it reached a verdict — on `main` a later push cancels
    # the tagged commit's run — so nobody knows anything about this commit yet,
    # and running the gate here establishes it.
    decide true "CI run $run_id was cancelled for $sha (${run_url:-no URL}), so CI reached no verdict; running the gate here instead."
    ;;
  none)
    if [ "$for_sha" -gt 0 ]; then
      # Finished runs exist but none carried a usable id and conclusion. That is
      # an answer this could not read, not an absent one.
      refuse \
        "ci-verdict: CI's $for_sha finished run(s) for $sha reported no usable id and conclusion, so their verdict could not be read. The release stops here." \
        "it was reading $endpoint" \
        "read those runs by hand and re-run this release once the API reports them normally. This job will not run the gate in CI's place: a verdict it could not read may well be a refusal."
    fi
    decide true "CI has no run for $sha, so CI reached no verdict; running the gate here instead."
    ;;
  *)
    # GitHub has several other conclusions (neutral, action_required, skipped,
    # stale), and one of them may mean CI declined to gate this commit — or that
    # it did and this does not know how to read it. Either way it is not the
    # absence of a verdict, so it is not this job's to overwrite with one.
    refuse \
      "ci-verdict: CI run $run_id concluded $conclusion for $sha, which is neither a pass, a refusal, nor the absence of a verdict this can act on. The release stops here." \
      "the run is ${run_url:-<the API returned no URL>}" \
      "read that run and decide: release the commit again once CI has gated it, or add $conclusion to the conclusions scripts/ci-verdict.sh enumerates if it is one the release may act on. This job will not run the gate in CI's place: a conclusion it does not recognize may well be a refusal."
    ;;
esac
