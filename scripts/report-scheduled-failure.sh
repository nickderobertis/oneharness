#!/usr/bin/env bash
# Announce a failed SCHEDULED workflow run as a GitHub issue.
#
# A schedule has no pull request to turn red and nobody waiting on it, so a
# nightly failure that announces itself nowhere is one nobody reads — which is
# the same as not running the suite at all. This gives it somewhere to be seen.
#
# One open issue, commented on each further night, so a week of failures is one
# thread rather than seven. A file rather than inline workflow YAML because the
# create-vs-comment branch is real behavior: `scripts/report-scheduled-failure-test.sh`
# drives both halves against a stubbed `gh`.
#
# Reads (all required except RUN_URL):
#   REPO      owner/name to file against
#   TITLE     the issue title, which is also how an existing one is found
#   BODY      the issue/comment body
#   RUN_URL   appended to the body when set
# `gh` must be authenticated (GH_TOKEN in CI).
set -euo pipefail

for required in REPO TITLE BODY; do
	if [ -z "${!required:-}" ]; then
		echo "report-scheduled-failure: \$$required is required" >&2
		exit 2
	fi
done

# shellcheck disable=SC2153  # BODY is an input, not a typo for the local below
body="$BODY"
[ -n "${RUN_URL:-}" ] && body="$body"$'\n\n'"Run: $RUN_URL"

# `--search "<title> in:title"` rather than a label: a label has to exist first,
# and a workflow that has to create one before it can report a failure has one
# more way to fail while reporting a failure.
existing="$(gh issue list --repo "$REPO" --state open --search "$TITLE in:title" \
	--json number,title --jq "first(.[] | select(.title == \"$TITLE\") | .number) // empty")"

if [ -n "$existing" ]; then
	gh issue comment "$existing" --repo "$REPO" --body "$body"
	echo "report-scheduled-failure: commented on #$existing"
else
	gh issue create --repo "$REPO" --title "$TITLE" --body "$body"
	echo "report-scheduled-failure: opened a new issue"
fi
