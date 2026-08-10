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
		fail "$f pull_request paths differ from the authoritative list; update its paths block to match scripts/check-e2e-matrix.sh (expected: $(printf '%s' "$expected" | tr '\n' ','); got: $(printf '%s' "$actual" | tr '\n' ','))"
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

# The turn-control feature suite is outside the shared PR matrix — its own
# feature-scoped paths, its own unix-only platform set — so it is checked for the
# two contracts it does restate rather than skipped outright:
#
#   - Its pull_request filter names the control feature's sources one path at a
#     time, which Actions cannot glob from the feature's module graph. A source
#     that moves silently stops triggering the suite, and the symptom is a
#     control regression merging unnoticed — so every listed path must exist.
#   - Its up-front credential step restates which provider keys the phases in
#     e2e-control.sh actually read. A phase whose key nobody supplied drops that
#     harness out, which OH_E2E_NO_SKIP turns RED only after minutes of paid
#     model calls — so every `have_env` phase must have at least one accepted key
#     supplied to BOTH steps: the up-front check, or the run fails late and names
#     the wrong thing; and the live step, or the phase reads an unset variable
#     however loudly the preflight passed. Accepting a name that appears anywhere
#     in the file lets those two drift apart in either direction, so each is read
#     from its own step's `env:` block. `*_E2E_AUTH` entries are developer-host
#     sentinels rather than credentials (see e2e-control.sh), so they cannot
#     satisfy it.
#
# Copilot's phase is deliberately absent from this second check: it asks copilot
# itself rather than the environment, so it declares no `have_env` to align to.

# Print the secret names one step's `env:` block puts in that step's
# environment. $1 workflow file, $2 a regex identifying the step by what it runs.
step_env_secrets() {
	awk -v marker="$2" '
		function flush() {
			if (buf ~ marker) printf "%s", names
			buf = ""; names = ""; in_env = 0
		}
		/^      - / { flush() }
		{ buf = buf $0 "\n" }
		/^        env:$/ { in_env = 1; next }
		in_env && /^          / {
			if (match($0, /secrets\.[A-Za-z_][A-Za-z0-9_]*/))
				names = names substr($0, RSTART + 8, RLENGTH - 8) "\n"
			next
		}
		in_env && /^[[:space:]]*$/ { next }
		in_env { in_env = 0 }
		END { flush() }
	' "$1"
}

check_control() {
	local f=".github/workflows/e2e-control.yml" s="scripts/e2e-control.sh"
	local p line accepted v satisfied detail preflight live
	check_common "$f"
	while IFS= read -r p; do
		[ -e "$p" ] || fail "$f triggers on '$p', which no longer exists, so a change to the control path would not run this suite; point that entry at the source's new location (or drop it if the source is gone)"
	done < <(awk '
		/^  pull_request:$/ { in_pr = 1; next }
		in_pr && /^    paths:$/ { in_paths = 1; next }
		in_paths && /^      - / { sub(/^      - /, ""); print; next }
		in_paths { exit }
		in_pr && /^  [^ ]/ { exit }
	' "$f")

	# Identified by what each step does rather than by its name, so renaming a
	# step cannot quietly retire the check.
	preflight="$(step_env_secrets "$f" "::error::missing secret")"
	live="$(step_env_secrets "$f" "just live-control")"
	[ -n "$preflight" ] ||
		fail "$f has no step whose env supplies secrets to an up-front credential check (one emitting '::error::missing secret'), so a phase's key going missing would only surface minutes into paid model calls"
	[ -n "$live" ] ||
		fail "$f has no step whose env supplies secrets to \`just live-control\`, so every control phase would read an unset credential"

	while IFS= read -r line; do
		accepted="$(printf '%s' "$line" | sed -E 's/.*have_env[[:space:]]+"[^"]*"[[:space:]]*//; s/\|\|.*//')"
		satisfied=0
		detail=""
		for v in $accepted; do
			case "$v" in *_E2E_AUTH) continue ;; esac
			if printf '%s\n' "$preflight" | grep -qx "$v"; then
				if printf '%s\n' "$live" | grep -qx "$v"; then
					satisfied=1
					break
				fi
				detail="$detail; $v is checked up front but never reaches the live step"
			elif printf '%s\n' "$live" | grep -qx "$v"; then
				detail="$detail; $v reaches the live step but is not checked up front"
			fi
		done
		[ "$satisfied" -eq 1 ] ||
			fail "$f supplies none of [$accepted] to both its credential-check step and its live step, and $s accepts them for one of its phases, so that harness would drop out mid-run$detail; add one of them as a \`secrets.<NAME>\` env entry to both steps (sync it first with 'just secrets-sync'), or narrow that phase's accepted list to a key CI already has"
	done < <(grep -E '^[[:space:]]*have_env ' "$s")
}
check_control

if [ "$fails" -ne 0 ]; then
	printf '\ncheck-e2e-matrix: %d drift(s) from the contract in scripts/check-e2e-matrix.sh\n' "$fails" >&2
	exit 1
fi
echo "check-e2e-matrix: all e2e workflows match the matrix contract"
