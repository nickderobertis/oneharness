#!/usr/bin/env bash
# Hermetic boundary coverage for check-e2e-matrix.sh's turn-control checks. No
# network and no real workflow run: each case copies the workflows and scripts
# into a scratch tree, drifts exactly one thing, and drives the real gate there.
#
# Those checks are what stands between a silent gap in the turn-control suite
# and a control regression merging unnoticed — a moved source stops triggering
# the workflow, an unsupplied provider key drops that harness out of a run
# `OH_E2E_NO_SKIP` only turns red minutes into paid model calls, and a reporting
# job with no checkout exits 127 before it can announce anything. All of those
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

# A key the up-front check verifies but the live step never exports. The
# preflight passes, then every phase that reads it drops its harness out — the
# expensive failure the preflight exists to prevent, arriving anyway.
build_fixture
sed -i.bak '/name: Live turn-control e2e/,$ { /ANTHROPIC_API_KEY: /d; }' \
    "$tmp/repo/$workflow"
grep -q 'ANTHROPIC_API_KEY: ' "$tmp/repo/$workflow" ||
    fail "fixture setup: the live-step case removed ANTHROPIC_API_KEY from the whole file, not just the live step"
run_case "a credential missing from the live step" 1 "never reaches the live step"

# The reverse drift: the live step exports it, but the up-front check no longer
# looks for it, so a run with that secret unset fails minutes in and names the
# harness rather than the secret to set.
build_fixture
sed -i.bak '/name: Verify every controllable harness/,/uses: actions-rust-lang/ { /ANTHROPIC_API_KEY: /d; }' \
    "$tmp/repo/$workflow"
grep -q 'ANTHROPIC_API_KEY: ' "$tmp/repo/$workflow" ||
    fail "fixture setup: the preflight case removed ANTHROPIC_API_KEY from the whole file, not just the credential check"
run_case "a credential missing from the up-front check" 1 "not checked up front"

# The env KEY is the contract, not the secret it is drawn from: a live step that
# renames the variable still references the right secret, but the phase reading
# $ANTHROPIC_API_KEY finds nothing.
build_fixture
sed -i.bak '/name: Live turn-control e2e/,$ { s/^          ANTHROPIC_API_KEY: /          ANTHROPIC_KEY: /; }' \
    "$tmp/repo/$workflow"
grep -q 'ANTHROPIC_KEY: .*secrets\.ANTHROPIC_API_KEY' "$tmp/repo/$workflow" ||
    fail "fixture setup: the renamed-key case did not leave the live step referencing secrets.ANTHROPIC_API_KEY"
run_case "a live-step env key renamed away from what the phase reads" 1 "never reaches the live step"

# The up-front check deleted outright. Every phase's key would then be verified
# nowhere, so the gate must refuse rather than silently compare against nothing.
build_fixture
sed -i.bak '/- name: Verify every controllable harness/,/^      - uses: actions-rust-lang/{/^      - uses: actions-rust-lang/!d;}' \
    "$tmp/repo/$workflow"
grep -q '::error::missing secret' "$tmp/repo/$workflow" &&
    fail "fixture setup: the removed-preflight case left the credential-check step in place"
run_case "the up-front credential check removed" 1 "up-front credential check"

# The live step left with no secret-backed env at all: every phase reads an unset
# credential, which is the same silent gap seen from the other end.
build_fixture
sed -i.bak '/name: Live turn-control e2e/,$ { /secrets\./d; }' "$tmp/repo/$workflow"
run_case "the live step stripped of its credentials" 1 "secret-backed variable for \`just live-control\`"

# The schedule removed: macOS then runs the suite nowhere, and this feature has
# already broken there three times in ways Linux cannot show.
build_fixture
sed -i.bak '/^  schedule:$/,/^    - cron:/d' "$tmp/repo/$workflow"
grep -q '^  schedule:$' "$tmp/repo/$workflow" &&
    fail "fixture setup: the removed-schedule case left the schedule trigger in place"
run_case "the schedule removed" 1 "no schedule trigger"

# The schedule kept, but narrowed to Linux: the daily run would repeat the pull
# request's leg, and macOS would still never run.
build_fixture
sed -i.bak "s/github.event_name == 'schedule' && '\[\"ubuntu-latest\",\"macos-latest\"\]'/github.event_name == 'schedule' \&\& '[\"ubuntu-latest\"]'/" \
    "$tmp/repo/$workflow"
grep -qF "'schedule' && '[\"ubuntu-latest\"]'" "$tmp/repo/$workflow" ||
    fail "fixture setup: the linux-only-schedule case did not narrow the schedule matrix"
run_case "a schedule narrowed to Linux" 1 "does not run its schedule across"

# A schedule declared with no cron under it: the trigger is there, so a grep for
# `schedule:` alone would pass, and the workflow would never actually fire.
build_fixture
sed -i.bak '/^    - cron:/d' "$tmp/repo/$workflow"
grep -q '^    - cron:' "$tmp/repo/$workflow" &&
    fail "fixture setup: the no-cron case left a cron entry in place"
run_case "a schedule with no cron entry" 1 "no cron entry"

# The reporter job left without permission to report: it runs, and the `gh issue`
# call fails against the workflow's top-level read-only token.
build_fixture
sed -i.bak '/^      issues: write$/d' "$tmp/repo/$workflow"
grep -q 'issues: write' "$tmp/repo/$workflow" &&
    fail "fixture setup: the no-permission case left 'issues: write' in place"
run_case "the reporter without issues: write" 1 "issues: write"

# The reporting job removed: the nightly run still happens, and a failure is a
# red square nobody is looking at.
build_fixture
sed -i.bak "/failure() && github.event_name == 'schedule'/d" "$tmp/repo/$workflow"
run_case "the scheduled-failure report removed" 1 "nightly red"

# The checkout contract, proven against the job the defect was found in: the
# scheduled-failure reporter runs scripts/report-scheduled-failure.sh, and
# without a checkout its workspace holds no such file. This is the shape that
# swallowed ten consecutive nightly failures, so it is checked as itself rather
# than only through the synthetic job below.
build_fixture
sed -i.bak '/^  report:$/,$ { /uses: actions\/checkout@/d; }' "$tmp/repo/$workflow"
grep -q 'uses: actions/checkout@' "$tmp/repo/$workflow" ||
    fail "fixture setup: the reporter case removed every checkout in the file, not just the reporter's"
run_case "the reporter without a checkout" 1 "report-scheduled-failure.sh out of this repository"

# And both halves of that contract against one definition: the same job, once
# missing the step and once carrying it. `just` is the second way a job reaches
# into this repository (it needs the justfile), so the synthetic job runs that
# rather than a script path, leaving neither detection half assumed.
append_probe_job() {
    # $1 = workflow file, $2 = where the checkout goes relative to the command:
    # "before" (correct), "after" (too late to help), or "none". $3 = the command
    # the job runs, defaulting to a recipe of this repository's own justfile.
    {
        printf '\n  probe:\n    runs-on: ubuntu-latest\n    steps:\n'
        if [ "$2" = before ]; then printf '      - uses: actions/checkout@v4\n'; fi
        printf '      - run: %s\n' "${3:-just --list}"
        if [ "$2" = after ]; then printf '      - uses: actions/checkout@v4\n'; fi
    } >>"$1"
}

build_fixture
append_probe_job "$tmp/repo/$workflow" none
run_case "a job running just with no checkout" 1 "job 'probe' runs justfile"

# Steps run in order, so a checkout further down the job is a workspace the
# command has already needed and not found — reading the job as a whole would
# call this one fixed.
build_fixture
append_probe_job "$tmp/repo/$workflow" after
run_case "a checkout that comes after the command" 1 "job 'probe' runs justfile"

build_fixture
append_probe_job "$tmp/repo/$workflow" before
run_case "the same job carrying its checkout" 0 "all e2e workflows match the matrix contract"

# Text that merely says `uses: actions/checkout@` is not a checkout step: a
# commented-out one, and one printed by the job's own command. Both leave the
# workspace as empty as no checkout at all, so neither may satisfy the contract.
append_impostor_job() {
    # $1 = workflow file, $2 = where the checkout-looking text sits: "comment"
    # (a commented-out step) or "run" (inside the command the job runs).
    {
        printf '\n  probe:\n    runs-on: ubuntu-latest\n    steps:\n'
        if [ "$2" = comment ]; then
            printf '      # - uses: actions/checkout@v4\n'
            printf '      - run: just --list\n'
        else
            printf '      - run: |\n'
            printf '          echo "uses: actions/checkout@v4"\n'
            printf '          just --list\n'
        fi
    } >>"$1"
}

build_fixture
append_impostor_job "$tmp/repo/$workflow" comment
run_case "a commented-out checkout step" 1 "job 'probe' runs justfile"

build_fixture
append_impostor_job "$tmp/repo/$workflow" run
run_case "a command that prints a checkout step" 1 "job 'probe' runs justfile"

# The checkout contract is the one check here that reads every workflow rather
# than the turn-control one, and a traversal that stopped at e2e-control.yml
# would still pass every case above. So the same violation is placed in the
# repository's own CI workflow, which no other check in this gate reads.
build_fixture
append_probe_job "$tmp/repo/.github/workflows/ci.yml" none
run_case "a checkout-less job in a workflow other than the control suite" 1 "ci.yml job 'probe' runs justfile"

# A script path this repository does not have is some other tree's file — a
# runner-local helper, a dependency's — and no checkout of THIS repository would
# put it in the workspace, so it is not this check's business to demand one.
build_fixture
append_probe_job "$tmp/repo/$workflow" none 'bash scripts/belongs-to-another-tree.sh'
[ -e "$tmp/repo/scripts/belongs-to-another-tree.sh" ] &&
    fail "fixture setup: the foreign-script case named a script this repository actually has"
run_case "a checkout-less job running a script from elsewhere" 0 "all e2e workflows match the matrix contract"

echo "check-e2e-matrix-test: ok"
