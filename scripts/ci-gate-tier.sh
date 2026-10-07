#!/usr/bin/env bash
# Decide which gate tier a CI `check` job owes, and the base the affected tier
# keys off. ci.yml's `check` job runs this, then `just check` (affected) or
# `just check all` (the full sweep) — the same recipe either way.
#
# CONTRACT: the release-plz release pull request runs the full sweep; every
# other pull request and every push to main runs the affected tier against an
# explicitly derived base; a manual dispatch runs whichever tier it asks for.
#
# Why the sweep sits there: release-plz batches merges behind its release pull
# request, so the tree that ships is the one that pull request carries, and no
# merge job ever swept it. The release workflow then reads that run's verdict
# instead of sweeping the same tree again (scripts/ci-verdict.sh).
#
# Reads:
#   EVENT_NAME     github.event_name
#   HEAD_REF       github.head_ref — the pull request's head branch
#   BASE_SHA       github.event.pull_request.base.sha
#   BEFORE         github.event.before — the previous tip, on push
#   DISPATCH_TIER  the workflow_dispatch `tier` input (affected | all)
# Writes `tier=affected|all` and, for the affected tier, `base=<40-hex sha>` —
# one per line on stdout, and appended to $GITHUB_OUTPUT when that is set.
set -euo pipefail

# Every release pull request release-plz opens is on a branch with this prefix
# (release-plz.yml finds its own pull request the same way); check-ci-gate-tier.sh
# holds this, ci-verdict.sh and release-plz.yml to one spelling.
readonly RELEASE_BRANCH_PREFIX="release-plz-"

refuse() {
  echo "ci-gate-tier: $1" >&2
  echo "  Next: $2" >&2
  exit 2
}

is_sha() {
  [[ "$1" =~ ^[0-9a-f]{40}$ ]]
}

resolves() {
  git rev-parse --verify --quiet "$1^{commit}" >/dev/null
}

emit() {
  local line
  for line in "$@"; do
    printf '%s\n' "$line"
    if [ -n "${GITHUB_OUTPUT:-}" ]; then
      printf '%s\n' "$line" >>"$GITHUB_OUTPUT" ||
        refuse "could not append '$line' to \$GITHUB_OUTPUT ($GITHUB_OUTPUT)" "re-run the job; the runner provides a writable \$GITHUB_OUTPUT"
    fi
  done
}

affected_since() {
  local base="$1" why="$2"
  is_sha "$base" || refuse "the derived base '$base' is not a commit sha" "derive it from a full-depth checkout (actions/checkout with fetch-depth: 0)"
  echo "ci-gate-tier: affected tier since $base ($why)" >&2
  emit tier=affected "base=$base"
}

event="${EVENT_NAME:-}"
case "$event" in
  pull_request)
    head_ref="${HEAD_REF:-}"
    if [[ "$head_ref" == "$RELEASE_BRANCH_PREFIX"* ]]; then
      echo "ci-gate-tier: full sweep — '$head_ref' is release-plz's release pull request" >&2
      emit tier=all
      exit 0
    fi
    base_sha="${BASE_SHA:-}"
    is_sha "$base_sha" || refuse "BASE_SHA '$base_sha' is not a 40-character commit sha" "pass github.event.pull_request.base.sha as BASE_SHA"
    resolves "$base_sha" || refuse "the pull request's base $base_sha is not in this clone" "check out with fetch-depth: 0 so the base branch's history is present"
    merge_base="$(git merge-base "$base_sha" HEAD)" ||
      refuse "HEAD shares no history with the base $base_sha" "check out with fetch-depth: 0"
    affected_since "$merge_base" "merge base with the pull request's base"
    ;;
  push)
    before="${BEFORE:-}"
    if is_sha "$before" && [ "$before" != 0000000000000000000000000000000000000000 ] && resolves "$before"; then
      affected_since "$before" "the branch tip before this push"
    else
      # A new branch, or a tip this clone does not hold: the parent is the
      # narrowest base that still reaches every change in this commit.
      parent="$(git rev-parse --verify --quiet 'HEAD^{commit}^' || true)"
      [ -n "$parent" ] || refuse "this push has no usable previous tip and HEAD has no parent" "dispatch ci.yml with tier=all for this commit"
      affected_since "$(git rev-parse "$parent")" "HEAD's parent; the push carried no usable previous tip"
    fi
    ;;
  workflow_dispatch)
    case "${DISPATCH_TIER:-affected}" in
      all)
        echo "ci-gate-tier: full sweep — requested by dispatch" >&2
        emit tier=all
        ;;
      affected)
        resolves origin/main || refuse "no origin/main in this clone to derive the merge base from" "check out with fetch-depth: 0"
        affected_since "$(git merge-base origin/main HEAD)" "merge base with origin/main"
        ;;
      *) refuse "DISPATCH_TIER '${DISPATCH_TIER}' is neither 'affected' nor 'all'" "dispatch ci.yml with tier=affected or tier=all" ;;
    esac
    ;;
  *) refuse "event '$event' has no gate tier" "run this from ci.yml on pull_request, push or workflow_dispatch" ;;
esac
