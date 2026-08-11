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
# For a turn driven over a protocol, the frames the harness sent are the only
# place the SERVER says anything — the stderr and the `error` above are both
# oneharness's account of it.
# `%s`, not a format with escapes in it: `printf` interprets `\n` and `\"` and
# does not agree across platforms about how many backslashes it takes to survive
# that, which left this fixture one frame long on macOS and Windows.
printf '%s\n' '{"results":[{"status":"nonzero","stdout":"{\"type\":\"session.error\",\"error\":\"no model configured\"}\n{\"type\":\"session.idle\"}"}]}' \
    >"$evidence_tmp/frames.json"
evidence_case "the frames the harness sent" "no model configured" "$evidence_tmp" "$evidence_tmp/frames.json"
# A turn that never ends is usually a recognizer that stopped matching, and the
# `type` nobody handled is invisible in a tail of the ones that were.
evidence_case "the frame vocabulary" "session.error session.idle" "$evidence_tmp" "$evidence_tmp/frames.json"
# The same vocabulary against a `sort` that ends its lines CRLF, which is what
# the Windows runner's does. Joining those with `tr '\n' ' '` alone leaves a CR
# between every pair, and a CR is a line break to every log viewer — so the one
# line this evidence exists to print came out as one type per line and the
# assertion above could never match two adjacent ones. Stubbed rather than
# skipped: the behavior is the platform's, but the pipeline's tolerance of it is
# ours, and it is only checkable where a CRLF `sort` can be arranged.
crlf_sort_dir="$(mktemp -d)"
trap 'rm -rf "$evidence_tmp" "$crlf_sort_dir"' EXIT
cat >"$crlf_sort_dir/sort" <<'STUB'
#!/usr/bin/env bash
# Stands in for the Windows runner's `sort`: same ordering, CRLF line endings.
command sort "$@" | sed 's/$/\r/'
STUB
chmod +x "$crlf_sort_dir/sort"
PATH="$crlf_sort_dir:$PATH" \
    evidence_case "the frame vocabulary from a CRLF sort" "session.error session.idle" \
    "$evidence_tmp" "$evidence_tmp/frames.json"
# The error a frame carried, in full: the tail above truncates lines, and where
# it cuts is the difference between a credential and a rate limit.
printf '%s\n' '{"results":[{"status":"timeout","stdout":"{\"type\":\"session.next.step.failed\",\"data\":{\"error\":{\"message\":\"Provider request failed with HTTP 429: rate limited\"}}}"}]}' \
    >"$evidence_tmp/errors.json"
evidence_case "the error a frame carried" "HTTP 429: rate limited" "$evidence_tmp" "$evidence_tmp/errors.json"

# Silence is itself the finding, so it must be stated rather than left blank.
: >"$evidence_tmp/run.err"
evidence_case "a run that said nothing" "wrote nothing to stderr" "$evidence_tmp" "$evidence_tmp/absent.json"
evidence_case "a run with no report" "no parseable report" "$evidence_tmp" "$evidence_tmp/absent.json"

# Telling a turn the harness's own provider refused from a turn that ran. Every
# phase leans on this before calling a turn that did not end a contract
# violation, and reading a 401 as one is how the suite blames this feature for
# another party's outage.
[ -n "$(_oh_harness_errors "$evidence_tmp/errors.json")" ] ||
    fail "an error the harness reported was not read back from its frames"
# Both shapes count: an object with a message, and the bare string
# `session.error` carries.
[ -n "$(_oh_harness_errors "$evidence_tmp/frames.json")" ] ||
    fail "an error the harness reported as a bare string was not read back"
printf '%s\n' '{"results":[{"status":"timeout","stdout":"{\"type\":\"session.idle\",\"data\":{}}"}]}' \
    >"$evidence_tmp/clean.json"
[ -z "$(_oh_harness_errors "$evidence_tmp/clean.json")" ] ||
    fail "a transcript with no error in it must not be read as the harness failing"
[ -z "$(_oh_harness_errors "$evidence_tmp/absent.json")" ] ||
    fail "a missing report must yield no harness error rather than a made-up one"

# The wait that decides an attempt has settled. It must come back for EITHER
# answer — the work started, or the run that was doing it is gone — because a
# wait that only watched the step files spent its whole window on a run that had
# already exited, three times per harness.
settled_tmp="$(mktemp -d)"
trap 'rm -rf "$evidence_tmp" "$crlf_sort_dir" "$settled_tmp"' EXIT
sleep 30 &
alive=$!
_oh_control_wait_settled "$settled_tmp" "$alive" &&
    fail "a live run with no steps must keep the wait going"
touch "$settled_tmp/step-001.txt" "$settled_tmp/step-002.txt"
_oh_control_wait_settled "$settled_tmp" "$alive" ||
    fail "two steps under a live run must end the wait"
rm -f "$settled_tmp"/step-*.txt
kill "$alive" 2>/dev/null || true
wait "$alive" 2>/dev/null || true
_oh_control_wait_settled "$settled_tmp" "$alive" ||
    fail "a run that has exited must end the wait rather than being waited out"

echo "check-control-enforce: ok"
