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

# $1 what `gh issue list --jq` should print, $2 description
run_case() {
	printf '%s' "$1" >"$tmp/existing"
	: >"$tmp/calls"
	GH_CALLS="$tmp/calls" GH_EXISTING="$tmp/existing" \
		PATH="$tmp/bin:$PATH" \
		REPO="owner/repo" TITLE="Scheduled turn-control e2e is failing" \
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
run_case "41" "an open issue"
grep -q '^issue comment 41 ' "$tmp/calls" || {
	cat "$tmp/calls" >&2
	fail "an open issue: expected a comment on #41"
}
grep -q '^issue create ' "$tmp/calls" &&
	fail "an open issue: must not open a second one"

# A missing required input is refused rather than filing an empty issue.
: >"$tmp/calls"
if GH_CALLS="$tmp/calls" GH_EXISTING="$tmp/existing" PATH="$tmp/bin:$PATH" \
	REPO="owner/repo" TITLE="" BODY="x" \
	bash "$root/scripts/report-scheduled-failure.sh" >/dev/null 2>&1; then
	fail "an empty title must be refused, not filed"
fi
grep -q '^issue ' "$tmp/calls" && fail "a refused run must call gh at all"

echo "report-scheduled-failure-test: ok"
