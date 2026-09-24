#!/usr/bin/env bash
# llmlint: ignore-file[new_code_lands_in_a_project] The rule presumes an Nx project graph; this repository has none by a recorded decision (`AGENTS.md`: root `just` delegates to Cargo/Bun without Nx because the two-package graph is static), so no project definition can cover this file. What runs it is release.yml's `gate` job, and scripts/check-ci-verdict.sh covers it from `just lint-workflows`.
# Decide whether the tagged commit's CI run permits release publication.
# CONTRACT: only a cancelled CI run and no CI run at all make the release run the gate itself; every other state refuses rather than standing in for a verdict it could not read
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

# Read CI's answer for this commit: it sets $conclusion (the newest FINISHED
# run's, or `none`), $pending (how many runs for the commit have not finished),
# $unreadable (how many this could not make sense of), $run_id and $run_url —
# but an answer that cannot be read at all refuses from inside, because there is
# nothing to poll for when the API itself is unreachable and nothing to fall
# back to when a verdict may exist unread.
conclusion=none
pending=0
unreadable=0
run_id=
run_url=
read_verdict_or_refuse() {
  local runs summary
  if ! runs="$(gh api --paginate --slurp "$endpoint" 2>"$work/gh-error")"; then
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
  # are type-checked — and a run that fails the check is COUNTED, never merely
  # dropped. Dropping is what makes an unreadable answer dangerous here: every
  # run discarded brings the selection closer to empty, and empty is `absent`,
  # the one state that runs the gate. So $unreadable is the count of runs this
  # could not make sense of, and any of them stops the release:
  #
  #   * a run whose head_sha is not a string — there is no telling whose it is,
  #     so it cannot be excluded as somebody else's;
  #   * one of OURS whose status is neither a string nor one of the states the
  #     runs API documents — it is neither finished nor pending, so it would
  #     vanish from both counts;
  #   * one of ours that finished without a usable id and conclusion — dropping
  #     it would hand the verdict to an older run it supersedes.
  #
  # The status is enumerated rather than treated as "completed, or else still
  # going", for the same reason the CONCLUSION is enumerated below: a value this
  # does not recognize is not evidence of anything. Taking every unrecognized
  # status for `pending` would hold the release for the whole bound and then
  # report "CI still had 1 unfinished run(s) ... wait for that run to finish" —
  # sending a reader after a run that is not running, at the end of a wait that
  # was never going to end. A status outside the list refuses AT ONCE and names
  # itself, and if GitHub adds a state the fix is to add it here.
  #
  # A finished run needs a whole-number ID and a usable timestamp to establish
  # which rerun is newest. The URL is only
  # ever printed, but it arrives on a tab-delimited line this script then splits,
  # so anything unprintable is dropped from it here.
  if ! summary="$(printf '%s' "$runs" | jq -r --arg sha "$sha" '
    def valid_id: type == "number" and . > 0 and floor == .;
    def valid_instant: type == "string" and (. as $t | try ((fromdateiso8601 | todateiso8601) == $t) catch false);
    if (type) != "array" or length == 0 or any(.[]; (type) != "object" or (has("workflow_runs") | not) or ((.workflow_runs | type) != "array")) then
      error("the answer carries no workflow_runs array, so it is not a runs response at all")
    else map(.workflow_runs) | add end
    | . as $all
    | [ $all[] | select((.head_sha | type) == "string" and .head_sha == $sha) ] as $matching
    | [ $matching[] | select(.event == "push" and .head_branch == "main") ] as $mine
    | [ $mine[] | select((.status | type) == "string") ] as $typed
    | [ $typed[] | select(.status == "completed") ] as $finished
    | [ $typed[] | select(.status as $s
                          | ["queued", "in_progress", "waiting", "requested", "pending", "action_required"]
                          | index($s)) ] as $unfinished
    | ( ([ $all[] | select((.head_sha | type) != "string") ] | length)
      + ([ $matching[] | select((.event | type) != "string" or (.head_branch | type) != "string") ] | length)
      + (($mine | length) - ($typed | length))
      + (($typed | length) - ($finished | length) - ($unfinished | length))
      + ([ $finished[]
           | select((.id | valid_id | not) or (((.run_started_at // .created_at) | valid_instant) | not)
                    or ((.conclusion | type) != "string")
                    or ((.conclusion | length) == 0)
                    or (.conclusion | test("[^a-z_]"))) ] | length)
      ) as $unreadable
    | ($unfinished | length) as $pending
    | ([ $finished[] | select((.id | valid_id) and ((.run_started_at // .created_at) | valid_instant)
                             and (.conclusion | type) == "string"
                             and ((.conclusion | length) > 0)
                             and (.conclusion | test("[^a-z_]") | not)) ]
       | sort_by((.run_started_at // .created_at), .id) | last) as $newest
    | if $newest == null then
        "none\t\($pending)\t\($unreadable)\t\t"
      else
        "\($newest.conclusion)\t\($pending)\t\($unreadable)\t\($newest.id)\t\((($newest.html_url | strings) // "") | gsub("[^!-~]"; ""))"
      end
  ' 2>"$work/jq-error")"; then
    sed 's/^/    jq: /' "$work/jq-error" >&2
    refuse \
      "ci-verdict: could not read CI's verdict for $sha; the answer was not a readable list of workflow runs (jq's own words are above). Whether CI passed or refused this commit is unknown, so the release stops here." \
      "it was reading $endpoint" \
      "read that endpoint by hand — an HTML error page, a proxy's response, or any answer without a workflow_runs array arrives here — then re-run this release once it answers normally. This job will not run the gate in CI's place: a verdict it could not read may well be a refusal, and an answer whose SHAPE is unrecognized says nothing about whether CI passed."
  fi

  IFS=$'\t' read -r conclusion pending unreadable run_id run_url <<<"$summary"

  # Refused here rather than after the wait: there is nothing to poll for when
  # the answer itself does not make sense, and every further poll would re-read
  # the same malformed runs.
  if [ "$unreadable" -gt 0 ]; then
    refuse \
      "ci-verdict: $unreadable run(s) in CI's answer for $sha could not be read — a head_sha, event, branch, status, whole-number id, timestamp or conclusion was missing or invalid, or named a status this does not recognize. Dropping them would leave this reporting fewer runs than CI has, so the release stops here." \
      "it was reading $endpoint" \
      "read that endpoint by hand and compare it with what the runs API documents; if the shape has changed, update the fields scripts/ci-verdict.sh type-checks, and if GitHub has added a run status, add it to the statuses that script enumerates. This job will not run the gate in CI's place: discarding the runs it cannot parse is how an answer it could not read would come to look like no answer at all."
  fi
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
    if [ -n "$run_id" ]; then
      refuse \
        "ci-verdict: CI run $run_id concluded none for $sha; this is not an absent CI run, so the release stops here." \
        "the run is ${run_url:-<the API returned no URL>}" \
        "read that run and determine why CI supplied no recognized verdict before releasing again."
    fi
    # Every run of ours was readable (checked above) and none was pending, so
    # `none` means there were none of ours at all — genuinely absent.
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
