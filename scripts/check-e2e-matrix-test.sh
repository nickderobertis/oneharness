#!/usr/bin/env bash
# Hermetic boundary coverage for check-e2e-matrix.sh's turn-control checks. No
# network and no real workflow run: each case copies the workflows and scripts
# into a scratch tree, drifts exactly one thing, and drives the real gate there.
#
# Those two checks are what stands between a silent gap in the turn-control
# suite and a control regression merging unnoticed — a moved source stops
# triggering the workflow, and an unsupplied provider key drops that harness out
# of a run `OH_E2E_NO_SKIP` only turns red minutes into paid model calls. Both
# failure modes are invisible in a green CI log, so the gate that catches them
# needs its own proof that it still fires.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
workflow=".github/workflows/e2e-control.yml"
suite="scripts/e2e-control.sh"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

fail() {
    echo "check-e2e-matrix-test: $1" >&2
    echo "  rerun with 'bash -x scripts/check-e2e-matrix-test.sh' to inspect the failing case" >&2
    exit 1
}

# A scratch repo holding everything the gate reads, rebuilt per case so one
# case's drift can never leak into the next. The control filter names crate
# sources the gate only checks the existence of, so empty placeholders stand in
# for them rather than copying the tree.
build_fixture() {
    rm -rf "$tmp/repo"
    mkdir -p "$tmp/repo"
    cp -R "$root/scripts" "$tmp/repo/scripts"
    cp -R "$root/.github" "$tmp/repo/.github"
    local p
    while IFS= read -r p; do
        [ -e "$tmp/repo/$p" ] && continue
        mkdir -p "$tmp/repo/$(dirname "$p")"
        : >"$tmp/repo/$p"
    done < <(control_paths "$tmp/repo/$workflow")
}

control_paths() {
    awk '
        /^  pull_request:$/ { in_pr = 1; next }
        in_pr && /^    paths:$/ { in_paths = 1; next }
        in_paths && /^      - / { sub(/^      - /, ""); print; next }
        in_paths { exit }
        in_pr && /^  [^ ]/ { exit }
    ' "$1"
}

# $1 description, $2 expected exit (0|1), $3 substring the output must carry
run_case() {
    local description="$1" expected="$2" needle="$3" out status=0
    out="$(bash "$tmp/repo/scripts/check-e2e-matrix.sh" 2>&1)" || status=$?
    if [ "$status" -ne "$expected" ]; then
        printf '%s\n' "$out" >&2
        fail "$description: gate exited $status, expected $expected"
    fi
    case "$out" in
    *"$needle"*) ;;
    *)
        printf '%s\n' "$out" >&2
        fail "$description: gate output did not mention '$needle'"
        ;;
    esac
}

# The undrifted tree must pass, or every case below would "catch" drift that was
# never introduced.
build_fixture
run_case "an undrifted tree" 0 "all e2e workflows match the matrix contract"

# A control source that moved: the filter still names the old path, so a change
# to the feature would no longer trigger the suite.
build_fixture
sed -i.bak 's#- crates/oneharness-core/src/io/control.rs#- crates/oneharness-core/src/io/control-moved.rs#' \
    "$tmp/repo/$workflow"
run_case "a control source that moved" 1 "control-moved.rs"

# A phase whose provider key CI does not supply: that harness drops out mid-run.
build_fixture
sed -i.bak 's#have_env "Crush auth" CRUSH_E2E_AUTH ANTHROPIC_API_KEY#have_env "Crush auth" CRUSH_E2E_AUTH XAI_API_KEY#' \
    "$tmp/repo/$suite"
run_case "a provider key CI does not supply" 1 "XAI_API_KEY"

# A phase left with only its developer-host sentinel, which the workflow does
# supply. The sentinel means "this host is ready", never a credential, so it
# must NOT satisfy the check — otherwise wiring one into CI would silently
# retire a harness's coverage.
build_fixture
sed -i.bak 's#have_env "Crush auth" CRUSH_E2E_AUTH ANTHROPIC_API_KEY#have_env "Crush auth" CRUSH_E2E_AUTH#' \
    "$tmp/repo/$suite"
sed -i.bak 's#          CRUSH_E2E_MODEL: .*#          CRUSH_E2E_AUTH: ${{ secrets.CRUSH_E2E_AUTH }}#' \
    "$tmp/repo/$workflow"
grep -q 'secrets.CRUSH_E2E_AUTH' "$tmp/repo/$workflow" ||
    fail "fixture setup: the sentinel case did not wire secrets.CRUSH_E2E_AUTH into the workflow"
run_case "a sentinel standing in for a credential" 1 "CRUSH_E2E_AUTH"

echo "check-e2e-matrix-test: ok"
