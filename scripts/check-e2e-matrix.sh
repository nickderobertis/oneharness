#!/usr/bin/env bash
# Drift gate for the live-e2e CI matrix contract.
#
# The per-harness/-feature e2e workflows (.github/workflows/e2e-*.yml) encode one
# shared contract in several places GitHub Actions cannot centralize: the
# workflow_dispatch `os` choice list and pull-request path filter must be
# literals per workflow, and each job's matrix restates the PR-default platform
# set. This script is the single source of that contract and fails if any
# workflow drifts from it — so the duplication is *checked*, not free-floating
# (AGENTS.md > "Live e2e in CI").
#
# The contract (change it HERE, then update the workflows to match):
#   - No workflow may trigger on `push` (the release-plz release PR re-runs the
#     suite as the pre-release gate; an on-main run was redundant paid calls).
#   - Every workflow exposes a workflow_dispatch `os` input for on-demand single
#     harness/platform runs.
#   - Every pull_request path filter contains the shared build/e2e inputs plus
#     exactly that workflow's script and workflow file.
#   - claude-code and codex run the full cross-platform PR matrix; every other
#     harness — and the schema feature — runs Linux-only on PRs.
#   - schema never offers/uses windows (its quote-heavy --json-schema argv is
#     mangled by the Windows .cmd shim).
set -euo pipefail

cd "$(dirname "$0")/.."

FULL='["ubuntu-latest","macos-latest","windows-latest"]'
LINUX='["ubuntu-latest"]'
SCHEMA_ALL='["ubuntu-latest","macos-latest"]'
SHARED_PATHS='src/**
crates/**
Cargo.toml
Cargo.lock
rust-toolchain.toml
scripts/e2e-lib.sh
scripts/install-e2e.sh'

# harness id -> PR-default matrix (the `|| '<default>'` tail of the os expression)
CROSS_PLATFORM="claude codex"
LINUX_ONLY="copilot crush cursor goose opencode qwen"

fails=0
fail() {
	printf 'e2e-matrix drift: %s\n' "$1" >&2
	fails=$((fails + 1))
}

check_common() {
	# $1 = workflow file
	local f="$1"
	# The `on:` trigger block ends at `permissions:`; no `push` may appear in it.
	if awk '/^on:/{a=1} /^permissions:/{a=0} a' "$f" | grep -qE '^\s*push:'; then
		fail "$f still triggers on push (must be pull_request + workflow_dispatch only)"
	fi
	grep -qE '^\s*workflow_dispatch:' "$f" || fail "$f missing workflow_dispatch trigger"
	grep -qE '^\s+os:$' "$f" || fail "$f missing the workflow_dispatch 'os' input"
}

# $1 = workflow file, $2 = workflow id
check_paths() {
	local f="$1" id="$2" actual expected
	expected="${SHARED_PATHS}
scripts/e2e-${id}.sh
.github/workflows/e2e-${id}.yml"
	actual="$(awk '
		/^  pull_request:$/ { in_pr = 1; next }
		in_pr && /^    paths:$/ { in_paths = 1; next }
		in_paths && /^      - / { sub(/^      - /, ""); print; next }
		in_paths { exit }
		in_pr && /^  [^ ]/ { exit }
	' "$f")"
	if [ "$actual" != "$expected" ]; then
		fail "$f pull_request paths differ from the authoritative list (expected: $(printf '%s' "$expected" | tr '\n' ','); got: $(printf '%s' "$actual" | tr '\n' ','))"
	fi
}

# $1 = file, $2 = expected PR-default JSON, $3 = expected 'all' JSON
check_matrix() {
	local f="$1" prd="$2" all="$3"
	local line
	# The GitHub expression is a literal to match, not a shell expansion.
	# shellcheck disable=SC2016
	line="$(grep -F 'os: ${{ fromJSON(' "$f" || true)"
	[ -n "$line" ] || { fail "$f has no fromJSON matrix expression"; return; }
	printf '%s' "$line" | grep -qF "|| '$prd') }}" ||
		fail "$f PR-default matrix is not $prd"
	printf '%s' "$line" | grep -qF "'all' && '$all'" ||
		fail "$f dispatch 'all' matrix is not $all"
}

for id in $CROSS_PLATFORM; do
	f=".github/workflows/e2e-${id}.yml"
	check_common "$f"
	check_paths "$f" "$id"
	check_matrix "$f" "$FULL" "$FULL"
	# A cross-platform harness must offer windows on demand.
	grep -qE '^\s+- windows-latest$' "$f" || fail "$f missing windows-latest dispatch option"
done

for id in $LINUX_ONLY; do
	f=".github/workflows/e2e-${id}.yml"
	check_common "$f"
	check_paths "$f" "$id"
	check_matrix "$f" "$LINUX" "$FULL"
done

# schema is Linux-only on PR and never windows (not even on demand).
f=".github/workflows/e2e-schema.yml"
check_common "$f"
check_paths "$f" schema
check_matrix "$f" "$LINUX" "$SCHEMA_ALL"
if grep -qE '^\s+- windows-latest$' "$f"; then
	fail "$f must not offer windows-latest (native --json-schema argv is .cmd-shim-mangled)"
fi

if [ "$fails" -ne 0 ]; then
	printf '\ncheck-e2e-matrix: %d drift(s) from the contract in scripts/check-e2e-matrix.sh\n' "$fails" >&2
	exit 1
fi
echo "check-e2e-matrix: all e2e workflows match the matrix contract"
