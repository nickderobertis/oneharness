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
# A harness that ran and broke its contract is the regression this suite exists
# to catch, so it fails the run even where everything else was proven — and even
# on a developer box, where a missing credential would have been forgiven.
OH_E2E_NO_SKIP='' outcome_case "a harness that broke its contract" 1 "codex" "claude-code" "" "1m00s" "codex"
OH_E2E_NO_SKIP='' outcome_case "a failure outranking a partial run" 1 "codex" "claude-code" "goose" "1m00s" "codex"

# The evidence an inconclusive phase leaves behind. Three attempts retry that
# outcome and then fail naming only the symptom, so whatever the harness did has
# to reach the log — a CI-only suite has nothing else to look at.
evidence_case() {
    local description="$1" needle="$2" sandbox="$3" report="$4" out
    out="$(_oh_control_evidence "$sandbox" "$report" 2>&1)"
    case "$out" in
    *"$needle"*) ;;
    *)
        printf '%s\n' "$out" >&2
        fail "$description: evidence did not mention '$needle'"
        ;;
    esac
}

evidence_tmp="$(mktemp -d)"
trap 'rm -rf "$evidence_tmp"' EXIT
printf 'codex: stream error\n' >"$evidence_tmp/run.err"
printf '{"results":[{"status":"nonzero","exit_code":2,"failure_kind":"auth","error":"the provider refused","text":"starting now"}]}\n' \
    >"$evidence_tmp/report.json"
evidence_case "a run that failed loudly" "codex: stream error" "$evidence_tmp" "$evidence_tmp/report.json"
evidence_case "a partial report" "status=nonzero" "$evidence_tmp" "$evidence_tmp/report.json"
# `error` is the field that says WHY a run did not succeed; stderr does not.
evidence_case "the run's own error" "the provider refused" "$evidence_tmp" "$evidence_tmp/report.json"
evidence_case "the classified failure" "failure_kind=auth" "$evidence_tmp" "$evidence_tmp/report.json"

# A harness that failed on its own is not a verdict on the redirection, and the
# status is what tells the two apart.
[ "$(_oh_result_status "$evidence_tmp/report.json")" = "nonzero" ] ||
    fail "a failed run's status was not read back from its report"
[ -z "$(_oh_result_status "$evidence_tmp/absent.json")" ] ||
    fail "a missing report must yield no status rather than a made-up one"
# Silence is itself the finding, so it must be stated rather than left blank.
: >"$evidence_tmp/run.err"
evidence_case "a run that said nothing" "wrote nothing to stderr" "$evidence_tmp" "$evidence_tmp/absent.json"
evidence_case "a run with no report" "no parseable report" "$evidence_tmp" "$evidence_tmp/absent.json"

echo "check-control-enforce: ok"
