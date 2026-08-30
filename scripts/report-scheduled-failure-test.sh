#!/usr/bin/env bash
# Hermetic behavioral test for scripts/report-scheduled-failure.sh.
#
# The reporter is the only thing that makes a nightly turn-control failure
# visible, and it runs exactly when something is already broken — so the one
# time it matters is the one time nobody is watching it work. It is also the
# rare path CI cannot rehearse: a real run would file real issues.
#
# So `gh` is stubbed, and both branches are driven: no open issue (must CREATE
# one) and an open issue already there (must COMMENT, never open a second).
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

fail() {
	echo "report-scheduled-failure-test: $1" >&2
	echo "  Next: re-run with \`bash -x scripts/report-scheduled-failure-test.sh\` to see the reporter's own commands. The stubbed \`gh\` records every call it was given in \$tmp/calls, and each case's output is above — a broken assertion here means the reporter files, comments, or refuses differently than a nightly failure needs it to." >&2
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
exit 0
STUB
chmod +x "$tmp/bin/gh"

# The one title every case files under, so a case can plant an issue that
# matches it exactly and one that only looks like it.
TITLE_UNDER_TEST="Scheduled turn-control e2e is failing"

# $1 the `number<TAB>title` lines `gh issue list` should print, $2 description
run_case() {
	printf '%s' "$1" >"$tmp/existing"
	: >"$tmp/calls"
	GH_CALLS="$tmp/calls" GH_EXISTING="$tmp/existing" \
		PATH="$tmp/bin:$PATH" \
		REPO="owner/repo" TITLE="$TITLE_UNDER_TEST" \
		BODY="the suite failed" RUN_URL="https://example.invalid/run/1" \
		bash "$root/scripts/report-scheduled-failure.sh" >"$tmp/out" 2>&1 ||
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

# An id that is not an issue number is drift in `gh issue list`, and addressing
# a comment at it would be a request to whatever it happens to name.
: >"$tmp/calls"
printf '%s' "not-a-number	$TITLE_UNDER_TEST" >"$tmp/existing"
if GH_CALLS="$tmp/calls" GH_EXISTING="$tmp/existing" PATH="$tmp/bin:$PATH" \
	REPO="owner/repo" TITLE="$TITLE_UNDER_TEST" BODY="x" \
	bash "$root/scripts/report-scheduled-failure.sh" >"$tmp/out" 2>&1; then
	cat "$tmp/out" >&2
	fail "a non-numeric issue id must be refused, not addressed"
fi
grep -q '^issue comment ' "$tmp/calls" &&
	fail "a non-numeric issue id must not be commented on"

# A missing required input is refused rather than filing an empty issue — and
# says which one and how to supply it, since the caller is a workflow step.
: >"$tmp/calls"
missing_out="$tmp/missing.out"
if GH_CALLS="$tmp/calls" GH_EXISTING="$tmp/existing" PATH="$tmp/bin:$PATH" \
	REPO="owner/repo" TITLE="" BODY="x" \
	bash "$root/scripts/report-scheduled-failure.sh" >"$missing_out" 2>&1; then
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
	if GH_CALLS="$tmp/calls" GH_EXISTING="$tmp/existing" GH_ERROR="$error" \
		GH_FAIL_LIST="$fail_list" PATH="$tmp/bin:$PATH" \
		REPO="owner/repo" TITLE="$TITLE_UNDER_TEST" \
		BODY="the suite failed" RUN_URL="https://example.invalid/run/1" \
		bash "$root/scripts/report-scheduled-failure.sh" >"$out" 2>&1; then
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
# Whatever went wrong, the finding this was reporting must still be findable.
gh_failure_case "HTTP 500: Server Error" "" "41	$TITLE_UNDER_TEST" "https://example.invalid/run/1"

echo "report-scheduled-failure-test: ok"
