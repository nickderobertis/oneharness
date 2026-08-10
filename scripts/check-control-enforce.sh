#!/usr/bin/env bash
# Hermetic contract check for the mechanism assertion used by live-control.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/e2e-lib.sh
source "$root/scripts/e2e-lib.sh"

fail() {
    echo "check-control-enforce: $1" >&2
    echo "  Re-run with: bash -x scripts/check-control-enforce.sh" >&2
    exit 1
}

_oh_control_mechanism_matches "mechanism-from-frame" "mechanism-from-registry" && \
    fail "a response carrying a different mechanism was accepted"
_oh_control_mechanism_matches "mechanism-from-registry" "mechanism-from-registry" || \
    fail "a response carrying the registry mechanism was rejected"
_oh_control_mechanism_matches "mechanism-from-frame" "" && \
    fail "an absent registry mechanism was accepted"

# The partial-run verdict. Reaching it for real needs credentials for some
# controllable harnesses and not others, which no hermetic run can arrange — but
# it is the branch the suite's honesty rests on, so it is driven directly. Each
# case runs in a subshell because both verdicts exit.
#
# $1 description, $2 expected exit, $3 substring the output must carry, $4.. args
outcome_case() {
    local description="$1" expected="$2" needle="$3" out status=0
    shift 3
    out="$(oh_control_report_outcome "$@" 2>&1)" || status=$?
    [ "$status" -eq "$expected" ] || {
        printf '%s\n' "$out" >&2
        fail "$description: exited $status, expected $expected"
    }
    case "$out" in
    *"$needle"*) ;;
    *)
        printf '%s\n' "$out" >&2
        fail "$description: output did not mention '$needle'"
        ;;
    esac
}

# In CI a harness that dropped out for want of a credential must be RED and must
# name itself — a green there would report the whole feature honored having never
# exercised goose at all.
OH_E2E_NO_SKIP=1 outcome_case "a partial run in CI" 1 "goose" "claude-code" "goose" "1m00s"
# On a developer box the same partial run is the point: one unauthenticated
# harness cannot retire the phases that did run.
OH_E2E_NO_SKIP='' outcome_case "a partial run locally" 0 "NOT PROVEN THIS RUN" "claude-code" "goose" "1m00s"
# Nothing proven at all is an absence, not a pass — and in CI not even that.
OH_E2E_NO_SKIP='' outcome_case "a run that proved nothing" 0 "SKIP" "" "goose crush" "1m00s"
OH_E2E_NO_SKIP=1 outcome_case "a run that proved nothing in CI" 1 "skip disallowed" "" "goose crush" "1m00s"
# A fully credentialed run reports the harnesses it proved and stays green.
OH_E2E_NO_SKIP=1 outcome_case "a complete run" 0 "PASS" "claude-code goose" "" "1m00s"

echo "check-control-enforce: ok"
