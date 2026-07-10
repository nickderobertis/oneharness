#!/usr/bin/env bash
#
# Prune mistakenly-published GitHub Releases (and their git tags) whose tag
# matches a version pattern.
#
# Context: a runaway release automation cut ~532 `v0.2.x` GitHub Releases
# (v0.2.0 .. v0.2.5xx), each with binary assets. Nothing legitimate lives in
# the 0.2.x line — real releases are v0.1.x (early) and v0.3.x (current). The
# junk releases make `asdf list-all oneharness` crawl: the asdf-oneharness
# plugin lists versions through the GitHub *Releases* API, paginating 100 at a
# time, so 532 extra releases turn every `list-all` into ~6 API round-trips.
# Deleting the releases is what fixes it (deleting only the tags would not —
# asdf reads releases, not tags); `--cleanup-tag` removes the tag too so the
# repo is left clean.
#
# This can NOT run from a Claude Code web/agent session: that proxy blocks
# release deletion, tag-ref writes, and `git push --delete` (all 403). Run it
# locally (or in Actions) with credentials that can write to the repo.
#
# Requires: `gh` (authenticated: `gh auth status`) and `jq`.
#
# Safety: dry-run by default — it only lists what it *would* delete. Pass
# --execute to actually delete. The pattern is anchored and defaults to the
# 0.2.x line only; v0.1.x, v0.3.x, and oneharness-core-* are never matched.
#
# Usage:
#   scripts/prune-mistake-releases.sh                 # dry run, default v0.2.x
#   scripts/prune-mistake-releases.sh --execute       # actually delete v0.2.x
#   scripts/prune-mistake-releases.sh --pattern '^v0\.2\.[0-9]+$' --execute
#
# Env:
#   REPO   override the target repo (default: nickderobertis/oneharness)
set -euo pipefail

REPO="${REPO:-nickderobertis/oneharness}"
PATTERN='^v0\.2\.[0-9]+$'
EXECUTE=0

while [ $# -gt 0 ]; do
  case "$1" in
    --execute) EXECUTE=1 ;;
    --pattern) shift; PATTERN="${1:?--pattern needs a value}" ;;
    --pattern=*) PATTERN="${1#--pattern=}" ;;
    -h|--help) sed -n '2,30p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done

for tool in gh jq; do
  command -v "$tool" >/dev/null 2>&1 || { echo "error: '$tool' is required but not on PATH" >&2; exit 1; }
done
gh auth status >/dev/null 2>&1 || { echo "error: run 'gh auth login' first (need a token with 'contents' write on ${REPO})" >&2; exit 1; }

echo "Repository : ${REPO}"
echo "Pattern    : ${PATTERN}"
echo "Mode       : $([ "$EXECUTE" -eq 1 ] && echo 'EXECUTE (deletes releases + tags)' || echo 'dry run (no changes)')"
echo

# Every release tag, across all pages, filtered to the pattern. --paginate walks
# every page; --jq emits one tag_name per line; grep keeps only matches.
mapfile -t targets < <(
  gh api --paginate -X GET "/repos/${REPO}/releases?per_page=100" --jq '.[].tag_name' \
    | grep -E "$PATTERN" || true
)

count=${#targets[@]}
if [ "$count" -eq 0 ]; then
  echo "No releases match ${PATTERN} — nothing to do."
  exit 0
fi

echo "Matched ${count} release(s):"
printf '  %s\n' "${targets[@]}" | head -n 10
[ "$count" -gt 10 ] && echo "  … and $((count - 10)) more"
echo

if [ "$EXECUTE" -eq 0 ]; then
  echo "Dry run only. Re-run with --execute to delete these ${count} releases and their tags."
  exit 0
fi

deleted=0
failed=0
for tag in "${targets[@]}"; do
  # Belt-and-suspenders: never act on a tag the anchored pattern didn't match.
  [[ "$tag" =~ $PATTERN ]] || { echo "skip (pattern guard): ${tag}" >&2; continue; }
  if gh release delete "$tag" --repo "$REPO" --cleanup-tag --yes >/dev/null 2>&1; then
    deleted=$((deleted + 1))
    printf '\rdeleted %d/%d …' "$deleted" "$count"
  else
    failed=$((failed + 1))
    echo >&2
    echo "failed to delete ${tag} (continuing)" >&2
  fi
done
echo
echo "Done: ${deleted} deleted, ${failed} failed, of ${count} matched."
[ "$failed" -eq 0 ]
