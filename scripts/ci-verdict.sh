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
# A run that is still RUNNING is waited for rather than duplicated: a second full
# sweep of the same commit, alongside the one already sweeping it, is the cost
# this exists to avoid. The wait is bounded, and a bound that runs out falls back
# to running the gate here.
#
# An answer that cannot be READ (no `gh`, no credential, an API error) is the
# same as an absent one: the release proves the commit itself rather than
# publishing on an unread verdict or dying with nothing published.
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

# Read CI's answer for this commit, or decide the whole thing here: it sets
# $conclusion (the newest FINISHED run's, or `none`), $for_sha (how many runs
# exist for the commit at all), $run_id and $run_url — but an answer that cannot
# be read at all is decided and exited from inside, because there is nothing to
# poll for when the API itself is unreachable.
conclusion=none
for_sha=0
pending=0
run_id=
run_url=
read_verdict_or_decide() {
  local runs summary
  if ! runs="$(gh api "repos/$repo/actions/workflows/$workflow/runs?head_sha=$sha&per_page=100" 2>"$work/gh-error")"; then
    sed 's/^/    gh: /' "$work/gh-error" >&2
    decide true "could not read $workflow runs for $sha (gh said why above), so CI's verdict is unknown; running the gate here instead. To restore the fast path give this job the actions:read permission and a GH_TOKEN, and check that $workflow exists in $repo."
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
  # time that is not a string sorts as the oldest rather than as the newest —
  # jq orders objects ABOVE strings, so an unchecked one would win the selection
  # outright.
  if ! summary="$(printf '%s' "$runs" | jq -r --arg sha "$sha" '
    [ .workflow_runs[]?
      | select((.head_sha | type) == "string" and .head_sha == $sha)
      | select((.status | type) == "string") ] as $mine
    | ([ $mine[] | select(.status != "completed") ] | length) as $pending
    | ([ $mine[] | select(.status == "completed")
         | select((.id | type) == "number" and (.conclusion | type) == "string") ]
       | sort_by((((.run_started_at // .created_at) | strings) // ""), .id) | last) as $newest
    | if $newest == null then
        "none\t\($mine | length)\t\($pending)\t\t"
      else
        "\($newest.conclusion // "none")\t\($mine | length)\t\($pending)\t\($newest.id)\t\($newest.html_url // "")"
      end
  ' 2>"$work/jq-error")"; then
    sed 's/^/    jq: /' "$work/jq-error" >&2
    decide true "the $workflow runs for $sha did not parse as workflow-run JSON (jq said why above), so CI's verdict is unknown; running the gate here instead."
  fi

  IFS=$'\t' read -r conclusion for_sha pending run_id run_url <<<"$summary"
}

# A run that is still going is WAITED for, never duplicated: running the whole
# gate here beside the one already running it is the second sweep of one commit
# this script exists to avoid. Silent while it waits — the decision below says
# how long it took.
for poll in $(seq 1 "$wait_attempts"); do
  read_verdict_or_decide
  if [ "$pending" -eq 0 ]; then
    break
  fi
  if [ "$poll" -lt "$wait_attempts" ]; then
    sleep "$wait_delay"
  fi
done

if [ "$pending" -gt 0 ]; then
  decide true "CI still had $pending unfinished run(s) for $sha after $wait_attempts polls over ~$((wait_attempts * wait_delay))s; running the gate here instead."
fi

case "$conclusion" in
  success)
    decide false "CI run $run_id concluded success for $sha ($run_url); publishing without re-running the gate."
    ;;
  failure | timed_out | startup_failure)
    printf '::error::CI run %s concluded %s for the commit this release tags, %s. A commit CI refused must not publish.\n' \
      "$run_id" "$conclusion" "$sha" >&2
    printf 'ci-verdict: the failing run is %s\n' "${run_url:-<the API returned no URL>}" >&2
    printf '  Next: read that run, fix the commit on the main branch, and release the fix. Re-running this release cannot make the refusal go away.\n' >&2
    exit 1
    ;;
  none)
    if [ "$for_sha" -gt 0 ]; then
      decide true "CI's $for_sha run(s) for $sha reported no usable conclusion; running the gate here instead."
    fi
    decide true "CI has no run for $sha; running the gate here instead."
    ;;
  *)
    decide true "CI run $run_id concluded $conclusion for $sha (${run_url:-no URL}), which is not a pass; running the gate here instead."
    ;;
esac
