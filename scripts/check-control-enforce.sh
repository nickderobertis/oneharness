#!/usr/bin/env bash
# Hermetic contract check for the mechanism assertion used by live-control.
set -euo pipefail

# The same treatment `check-local-gate.sh` and `check-sdk-install.sh` take, for
# the same reason: the cases below stand a `sort` of their own on the PATH to
# drive the CRLF branch, and an extensionless stub is not a program on Windows.
# The suite this checks is itself unix-only (`e2e-control.sh` exits early there —
# control sockets are unix domain sockets), so nothing that runs on Windows is
# left unchecked by skipping here.
case $(uname -s) in
MINGW* | MSYS* | CYGWIN*)
    echo "check-control-enforce: skipped on Windows because this Unix behavioral harness relies on extensionless executable stubs" >&2
    exit 0
    ;;
esac

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

# A harness whose own PROVIDER refused the turn. This is the one absence
# OH_E2E_NO_SKIP must NOT turn red: it exists to catch a harness that dropped out
# for want of a credential, and no credential CI can supply makes a provider stop
# refusing — so a red here reports someone else's billing state as this feature
# breaking. It must still be SAID, in the same "not run" category, or a green
# would claim coverage the run does not have.
OH_E2E_NO_SKIP=1 outcome_case "a provider refusal in CI" 0 \
    "NOT RUN (the provider refused the turn): copilot (provider refused: usage limit)" \
    "claude-code codex" "" "1m00s" "" "copilot (provider refused: usage limit)"
# And it is not a contract violation, so it cannot make the run red on its own.
OH_E2E_NO_SKIP=1 outcome_case "a provider refusal is still a pass" 0 "PASS" \
    "claude-code" "" "1m00s" "" "copilot (provider refused: usage limit)"
# A harness that RAN and broke its contract still outranks it — and the refused
# one is named in that verdict's "not run" list rather than dropped from it.
OH_E2E_NO_SKIP=1 outcome_case "a refusal named beside a real failure" 1 \
    "not run: copilot (provider refused: usage limit)" \
    "claude-code" "" "1m00s" "codex" "copilot (provider refused: usage limit)"

# A phase nobody has made pass is REPORTED every run, with its reason. Dropping
# it would be indistinguishable from coverage — the mistake this category exists
# to make impossible — and it is not red, because a known gap is a decision, not
# a regression.
OH_E2E_NO_SKIP=1 outcome_case "a known gap in CI" 0 \
    "KNOWN GAP (reported, not proven): opencode (control-mode: …)" \
    "claude-code" "" "1m00s" "" "" "opencode (control-mode: …)"

# Where BOTH of those leniencies stop: a run that proved NOTHING. Neither a
# refusal nor a gap is red on its own, because each sits beside harnesses that
# did prove something — but a run where nothing did is vacuous, and reporting it
# green in CI is precisely the false pass OH_E2E_NO_SKIP exists for. So the
# no-proven branch stays loud whatever the reason, and says what the reason was.
OH_E2E_NO_SKIP=1 outcome_case "nothing proven, all refused, in CI" 1 \
    "copilot (provider refused: usage limit)" \
    "" "" "1m00s" "" "copilot (provider refused: usage limit)"
OH_E2E_NO_SKIP=1 outcome_case "nothing proven, only a known gap, in CI" 1 \
    "skip disallowed" "" "" "1m00s" "" "" "opencode (control-mode: …)"
# On a developer box the same run is an absence rather than a failure, and it
# still has to NAME what was not run — a bare SKIP would read as "nothing to do".
OH_E2E_NO_SKIP='' outcome_case "nothing proven, all refused, locally" 0 \
    "copilot (provider refused: usage limit)" \
    "" "" "1m00s" "" "copilot (provider refused: usage limit)"

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
# The real `sort`, resolved BEFORE the stub joins the PATH and baked in as an
# absolute path. `command sort` would not do: `command` bypasses functions, not
# the PATH, so the stub would find itself and fork until the box ran out.
real_sort="$(command -v sort)"
cat >"$crlf_sort_dir/sort" <<STUB
#!/usr/bin/env bash
# Stands in for the Windows runner's \`sort\`: same ordering, CRLF line endings.
"$real_sort" "\$@" | sed 's/\$/\r/'
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

# Telling a turn the provider REFUSED from a turn that failed. The two look
# nothing alike on the wire and mean opposite things to this suite: a failure is
# retried, a refusal is not run. The fixture below is the shape CI actually read
# — an ACP `agent_message_chunk` carrying the quota message, then a clean
# `end_turn` — so the report is `ok`, the turn lasted three seconds, and the TEXT
# is the only place the refusal is stated at all.
printf '%s\n' '{"results":[{"status":"ok","text":"","stdout":"{\"sessionUpdate\":\"agent_message_chunk\",\"content\":{\"type\":\"text\",\"text\":\"Error: You'"'"'ve reached your additional usage limit for your plan. Go to https://github.com/settings/copilot/features for more details.\"}}\n{\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{\"stopReason\":\"end_turn\"}}"}]}' \
    >"$evidence_tmp/refused.json"
refusal="$(_oh_provider_refusal <"$evidence_tmp/refused.json")"
case "$refusal" in
*"reached your additional usage limit"*) ;;
*)
    printf '%s\n' "$refusal" >&2
    fail "a quota refusal carried in a message chunk was not read back in the provider's own words"
    ;;
esac
# The two fixtures every other case in this file uses are NOT refusals: an HTTP
# 429 is a rate limit the next attempt may well clear, and `no model configured`
# is this side's problem. Classifying either as "not run" would retire a phase
# that should have been retried or failed.
[ -z "$(_oh_provider_refusal <"$evidence_tmp/errors.json")" ] ||
    fail "a rate limit must stay a retryable failure rather than becoming a refusal"
[ -z "$(_oh_provider_refusal <"$evidence_tmp/frames.json")" ] ||
    fail "a harness's own error must not be read as its provider refusing"
[ -z "$(_oh_provider_refusal <"$evidence_tmp/clean.json")" ] ||
    fail "a transcript with nothing wrong in it must not be read as a refusal"
# A refusal in the normalized `text` alone, which is where it lands for a harness
# whose headless output is plain text (the per-harness suites' shape).
printf '%s\n' '{"results":[{"status":"ok","text":"Error: You have reached your usage limit for this plan."}]}' \
    >"$evidence_tmp/refused-text.json"
[ -n "$(_oh_provider_refusal <"$evidence_tmp/refused-text.json")" ] ||
    fail "a refusal stated only in the normalized text was not recognized"
# And in `stderr` ALONE, which is the shape copilot's ordinary `-p` run takes: it
# exits 1 with `text`, `error` and `stdout` all empty and the refusal printed
# only there. A reader that skipped stderr called that a broken harness, which is
# how this suite went red on someone else's quota one phase earlier than the
# control suite did.
printf '%s\n' '{"results":[{"status":"nonzero","exit_code":1,"text":null,"error":null,"stdout":"","stderr":"\nYou'"'"'ve reached your additional usage limit for your plan. Go to https://github.com/settings/copilot/features for more details. (Request ID: B84A:2256BB:3AD438:448202:6A7B0ECA)\n\nChanges    +0 -0\nAI Credits 0 (2s)\n"}]}' \
    >"$evidence_tmp/refused-stderr.json"
case "$(_oh_provider_refusal <"$evidence_tmp/refused-stderr.json")" in
*"additional usage limit"*) ;;
*)
    fail "a refusal printed only on the CLI's stderr was not recognized"
    ;;
esac

# And what the phases actually call: the reason must reach the caller, because
# every phase runs in a subshell whose variables die with it.
OH_NOT_RUN_FILE="$evidence_tmp/not-run" \
    _oh_note_provider_refusal copilot "$evidence_tmp/refused.json" >/dev/null 2>&1 ||
    fail "a report carrying a quota refusal was not recognized as one"
[ "$(cat "$evidence_tmp/not-run")" = "copilot (provider refused: usage limit)" ] ||
    fail "the refusal handed to the suite's verdict was '$(cat "$evidence_tmp/not-run")'"
OH_NOT_RUN_FILE="$evidence_tmp/not-run-clean" \
    _oh_note_provider_refusal opencode "$evidence_tmp/frames.json" >/dev/null 2>&1 &&
    fail "a harness that failed on its own must not be reported as its provider refusing"
[ ! -e "$evidence_tmp/not-run-clean" ] ||
    fail "a run nobody refused must leave no refusal behind"

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
