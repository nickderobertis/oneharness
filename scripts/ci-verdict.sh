#!/usr/bin/env bash
# llmlint: ignore-file[new_code_lands_in_a_project] The rule presumes an Nx project graph; this repository has none by a recorded decision (`AGENTS.md`: root `just` delegates to Cargo/Bun without Nx because the two-package graph is static), so no project definition can cover this file. What runs it is release.yml's `test` job, and scripts/check-ci-verdict.sh covers it from `just lint-workflows`.
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
#   cancelled, absent, or not  -> needs_check=true, run the gate here. CI on
#   finished                      `main` cancels in progress per ref, so a later
#                                 push can cancel the tagged commit's run, and a
#                                 hand-made Release may have no CI run at all.
#
# An answer that cannot be READ (no `gh`, no credential, an API error) is the
# same as an absent one: the release proves the commit itself rather than
# publishing on an unread verdict or dying with nothing published.
#
# Reads:
#   REPO         owner/name to query (default $GITHUB_REPOSITORY)
#   SHA          the tagged commit, full 40-hex (default $GITHUB_SHA)
#   CI_WORKFLOW  the workflow file whose verdict counts (default ci.yml)
# Writes `needs_check=true|false` to $GITHUB_OUTPUT when set, and says on stdout
# what it decided and why. `gh` must be authenticated with `actions: read`.
set -euo pipefail

repo="${REPO:-${GITHUB_REPOSITORY:-}}"
sha="${SHA:-${GITHUB_SHA:-}}"
workflow="${CI_WORKFLOW:-ci.yml}"

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

if ! runs="$(gh api "repos/$repo/actions/workflows/$workflow/runs?head_sha=$sha&per_page=100" 2>"$work/gh-error")"; then
  sed 's/^/    gh: /' "$work/gh-error" >&2
  decide true "could not read $workflow runs for $sha (gh said why above), so CI's verdict is unknown; running the gate here instead. To restore the fast path give this job the actions:read permission and a GH_TOKEN, and check that $workflow exists in $repo."
fi

# The API's own `head_sha` filter is not trusted to be the whole answer: this
# decides whether a gate runs, so the sha is matched again here. Runs that have
# not finished say nothing yet, so only completed ones are selected, newest
# first by start time with the run id breaking a tie.
if ! summary="$(printf '%s' "$runs" | jq -r --arg sha "$sha" '
  [ .workflow_runs[]? | select(.head_sha == $sha) ] as $mine
  | [ $mine[] | select(.status == "completed") ]
  | sort_by(.run_started_at // .created_at, .id)
  | last
  | if . == null then
      "none\t\($mine | length)\t\t"
    else
      "\(.conclusion // "none")\t\($mine | length)\t\(.id)\t\(.html_url // "")"
    end
' 2>"$work/jq-error")"; then
  sed 's/^/    jq: /' "$work/jq-error" >&2
  decide true "the $workflow runs for $sha did not parse as workflow-run JSON (jq said why above), so CI's verdict is unknown; running the gate here instead."
fi

IFS=$'\t' read -r conclusion for_sha run_id run_url <<<"$summary"

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
      decide true "CI has $for_sha run(s) for $sha but none has finished; running the gate here instead."
    fi
    decide true "CI has no run for $sha; running the gate here instead."
    ;;
  *)
    decide true "CI run $run_id concluded $conclusion for $sha (${run_url:-no URL}), which is not a pass; running the gate here instead."
    ;;
esac
