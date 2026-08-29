#!/usr/bin/env bash
# Hermetic behavioral test for scripts/report-workflow-failure.sh.
#
# The reporter is the only thing that makes an unattended failure visible — the
# nightly turn-control suite, and the release workflows a merged release PR fires
# and leaves to themselves — and it runs exactly when something is already
# broken, so the one time it matters is the one time nobody is watching it work.
# It is also the rare path CI cannot rehearse: a real run would file real issues.
#
# So `gh` is stubbed, and both branches are driven: no open issue (must CREATE
# one) and an open issue already there (must COMMENT, never open a second).
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

fail() {
	echo "report-workflow-failure-test: $1" >&2
	echo "  Next: re-run with \`bash -x scripts/report-workflow-failure-test.sh\` to see the reporter's own commands. The stubbed \`gh\` records every call it was given in \$tmp/calls, and each case's output is above — a broken assertion here means the reporter files, comments, or refuses differently than a nightly failure needs it to." >&2
	exit 1
}

# A `gh` that records its arguments and answers `issue list` from a file, so a
# case picks which branch the reporter should take.
mkdir -p "$tmp/bin"
cat >"$tmp/bin/gh" <<'STUB'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$GH_CALLS"
if [ "${1:-}" = "issue" ] && [ "${2:-}" = "list" ]; then
	cat "$GH_EXISTING"
fi
# `issue view <n>` answers with that issue's REAL title, which is what the
# reporter confirms a listing's candidate against. Keyed by number so a case can
# make the listing claim one thing and the issue say another.
if [ "${1:-}" = "issue" ] && [ "${2:-}" = "view" ]; then
	if [ -f "$GH_TITLES/${3:-}" ]; then cat "$GH_TITLES/${3:-}"; fi
fi
exit 0
STUB
chmod +x "$tmp/bin/gh"

# The one title every case files under, so a case can plant an issue that
# matches it exactly and one that only looks like it.
TITLE_UNDER_TEST="Scheduled turn-control e2e is failing"

# The real titles `gh issue view <n>` answers with, one file per issue number.
mkdir -p "$tmp/titles"

# $1 the `number<TAB>title` lines `gh issue list` should print, $2 description.
# By default every listed record tells the truth about its own issue, so a case
# that wants a lying listing overwrites $tmp/titles/<n> after calling this.
run_case() {
	printf '%s' "$1" >"$tmp/existing"
	rm -f "$tmp/titles"/*
	while IFS=$'\t' read -r number title; do
		[ -n "$number" ] && printf '%s\n' "$title" >"$tmp/titles/$number"
	done <<<"$1"
	: >"$tmp/calls"
	GH_CALLS="$tmp/calls" GH_EXISTING="$tmp/existing" GH_TITLES="$tmp/titles" \
		PATH="$tmp/bin:$PATH" \
		REPO="owner/repo" TITLE="$TITLE_UNDER_TEST" \
		BODY="the suite failed" RUN_URL="https://example.invalid/run/1" \
		bash "$root/scripts/report-workflow-failure.sh" >"$tmp/out" 2>&1 ||
		{
			cat "$tmp/out" >&2
			fail "$2: the reporter exited non-zero"
		}
}

# No open issue: one is created, and nothing is commented on.
run_case "" "no open issue"
grep -q '^issue create ' "$tmp/calls" || {
	cat "$tmp/calls" >&2
	fail "no open issue: expected an 'issue create' call"
}
grep -q '^issue comment ' "$tmp/calls" &&
	fail "no open issue: must not comment when there is nothing to comment on"
grep -q 'https://example.invalid/run/1' "$tmp/calls" ||
	fail "no open issue: the run URL must reach the issue body"

# An open issue: it is commented on, and no second issue is opened — or a week
# of nightly failures becomes seven issues nobody reads.
run_case "41	$TITLE_UNDER_TEST" "an open issue"
grep -q '^issue comment 41 ' "$tmp/calls" || {
	cat "$tmp/calls" >&2
	fail "an open issue: expected a comment on #41"
}
grep -q '^issue create ' "$tmp/calls" &&
	fail "an open issue: must not open a second one"

# `--search … in:title` is a fuzzy search, so it answers with issues whose title
# merely resembles this one. Commenting a turn-control failure onto someone
# else's issue is worse than opening a second one, so the title is matched
# exactly and a near miss goes to the create branch.
run_case "41	$TITLE_UNDER_TEST (macOS)" "a similar title"
grep -q '^issue create ' "$tmp/calls" || {
	cat "$tmp/calls" >&2
	fail "a similar title: an issue that is not this one must not be commented on"
}

# A FORGED record. The listing is `number<TAB>title` lines and an issue title is
# third-party text — anyone who can open an issue on the repository can put a
# newline in one and write a whole extra record, naming an issue number that is
# not the one the record claims. The digit check below does not catch it: a
# forged number is a perfectly good number. Here #77's real title is somebody
# else's, and a release failure commented onto it would be both lost and rude.
: >"$tmp/calls"
printf '%s' "77	$TITLE_UNDER_TEST" >"$tmp/existing"
rm -f "$tmp/titles"/*
printf '%s\n' "Crash on startup with an empty config" >"$tmp/titles/77"
GH_CALLS="$tmp/calls" GH_EXISTING="$tmp/existing" GH_TITLES="$tmp/titles" \
	PATH="$tmp/bin:$PATH" REPO="owner/repo" TITLE="$TITLE_UNDER_TEST" \
	BODY="the suite failed" \
	bash "$root/scripts/report-workflow-failure.sh" >"$tmp/out" 2>&1 || {
	cat "$tmp/out" >&2
	fail "a forged listing record: the reporter exited non-zero"
}
grep -q '^issue comment 77 ' "$tmp/calls" &&
	fail "a forged listing record must not be commented on"
grep -q '^issue create ' "$tmp/calls" || {
	cat "$tmp/calls" >&2
	fail "a forged listing record must still get the failure reported, as a new issue"
}
grep -qF "not \"$TITLE_UNDER_TEST\"" "$tmp/out" || {
	cat "$tmp/out" >&2
	fail "a forged listing record must say why it did not comment on it"
}

# An id that is not an issue number is drift in `gh issue list`, and addressing
# a comment at it would be a request to whatever it happens to name.
: >"$tmp/calls"
printf '%s' "not-a-number	$TITLE_UNDER_TEST" >"$tmp/existing"
if GH_CALLS="$tmp/calls" GH_EXISTING="$tmp/existing" GH_TITLES="$tmp/titles" PATH="$tmp/bin:$PATH" \
	REPO="owner/repo" TITLE="$TITLE_UNDER_TEST" BODY="x" \
	bash "$root/scripts/report-workflow-failure.sh" >"$tmp/out" 2>&1; then
	cat "$tmp/out" >&2
	fail "a non-numeric issue id must be refused, not addressed"
fi
grep -q '^issue comment ' "$tmp/calls" &&
	fail "a non-numeric issue id must not be commented on"

# A missing required input is refused rather than filing an empty issue — and
# says which one and how to supply it, since the caller is a workflow step.
: >"$tmp/calls"
missing_out="$tmp/missing.out"
if GH_CALLS="$tmp/calls" GH_EXISTING="$tmp/existing" GH_TITLES="$tmp/titles" PATH="$tmp/bin:$PATH" \
	REPO="owner/repo" TITLE="" BODY="x" \
	bash "$root/scripts/report-workflow-failure.sh" >"$missing_out" 2>&1; then
	fail "an empty title must be refused, not filed"
fi
grep -q '^issue ' "$tmp/calls" && fail "a refused run must not call gh at all"
grep -q 'TITLE' "$missing_out" || {
	cat "$missing_out" >&2
	fail "a refused run must name the variable that was missing"
}
grep -q 'Next:' "$missing_out" || {
	cat "$missing_out" >&2
	fail "a refused run must say what to do about it"
}

# A `gh` that fails. This is the path the whole script exists to survive being
# on: it runs only when something is already broken, so a `gh` failure here
# swallows a real finding. Each case must name what it was doing, repeat what
# `gh` said, and give the next action that particular answer calls for.
cat >"$tmp/bin/gh" <<'STUB'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$GH_CALLS"
if [ "${1:-}" = "issue" ] && [ "${2:-}" = "list" ] && [ -z "${GH_FAIL_LIST:-}" ]; then
	cat "$GH_EXISTING"
	exit 0
fi
if [ "${1:-}" = "issue" ] && [ "${2:-}" = "view" ] && [ -z "${GH_FAIL_VIEW:-}" ]; then
	if [ -f "$GH_TITLES/${3:-}" ]; then cat "$GH_TITLES/${3:-}"; fi
	exit 0
fi
printf '%s\n' "$GH_ERROR" >&2
exit 1
STUB

# $1 what `gh` writes to stderr, $2 `issue list` fails too when non-empty,
# $3 open-issue number (empty = the create branch), $4.. substrings required
gh_failure_case() {
	local error="$1" fail_list="$2" existing="$3" out="$tmp/failure.out" needle
	shift 3
	printf '%s' "$existing" >"$tmp/existing"
	: >"$tmp/calls"
	rm -f "$tmp/titles"/*
	while IFS=$'\t' read -r number title; do
		[ -n "$number" ] && printf '%s\n' "$title" >"$tmp/titles/$number"
	done <<<"$existing"
	if GH_CALLS="$tmp/calls" GH_EXISTING="$tmp/existing" GH_ERROR="$error" \
		GH_FAIL_LIST="$fail_list" GH_TITLES="$tmp/titles" \
		GH_FAIL_VIEW="${GH_FAIL_VIEW:-}" PATH="$tmp/bin:$PATH" \
		REPO="owner/repo" TITLE="$TITLE_UNDER_TEST" \
		BODY="the suite failed" RUN_URL="https://example.invalid/run/1" \
		bash "$root/scripts/report-workflow-failure.sh" >"$out" 2>&1; then
		cat "$out" >&2
		fail "a failing gh must not be reported as a filed issue"
	fi
	for needle in "$@"; do
		grep -qF "$needle" "$out" || {
			cat "$out" >&2
			fail "a failing gh must mention '$needle'"
		}
	done
}

# Authentication, permissions and a rejected query are three different problems
# with three different next actions, so the message has to tell them apart.
gh_failure_case "gh: To get started with GitHub CLI, please run: gh auth login" 1 "" \
	"looking for an open issue" "gh auth login" "Next:" "GH_TOKEN"
gh_failure_case "HTTP 403: Resource not accessible by integration" 1 "" \
	"HTTP 403" "issues: write"
gh_failure_case "HTTP 422: Validation Failed" 1 "" \
	"HTTP 422" "TITLE"
# An unclassifiable answer still gets what gh said and something to try.
gh_failure_case "something nobody predicted" 1 "" \
	"something nobody predicted" "Next:"
# The two write branches fail their own way, and neither may be silent.
gh_failure_case "HTTP 403: Resource not accessible by integration" "" "" \
	"opening an issue" "Next:"
gh_failure_case "HTTP 403: Resource not accessible by integration" "" "41	$TITLE_UNDER_TEST" \
	"commenting on #41" "Next:"
# Confirming the candidate's title is itself a `gh` call, so it fails its own way
# too — silently skipping the confirmation would put the forgery back.
GH_FAIL_VIEW=1 gh_failure_case "HTTP 404: Not Found" "" "41	$TITLE_UNDER_TEST" \
	"confirming the title of #41" "HTTP 404" "Next:"
# Whatever went wrong, the finding this was reporting must still be findable.
gh_failure_case "HTTP 500: Server Error" "" "41	$TITLE_UNDER_TEST" "https://example.invalid/run/1"

echo "report-workflow-failure-test: ok"
