#!/usr/bin/env bash
# Hermetic contract check for the mechanism assertion used by live-control.
set -euo pipefail

# The same treatment `check-local-gate.sh` and `check-js-install.sh` take, for
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

_oh_control_mechanism_matches "mechanism-from-frame" "mechanism-from-registry" &&
  fail "a response carrying a different mechanism was accepted"
_oh_control_mechanism_matches "mechanism-from-registry" "mechanism-from-registry" ||
  fail "a response carrying the registry mechanism was rejected"
_oh_control_mechanism_matches "mechanism-from-frame" "" &&
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

# Where a control phase makes its sandbox. Under a temp dir as long as macOS's
# resolved /private/var/folders/<..>/T, the socket the longest-named phase opens
# would pass the 104-byte sun_path (NUL included) every platform must meet, as
# every harness's --control refused on macOS CI; the sandbox moves to /tmp and
# the socket fits. Under a short temp dir it stays where the host put it.
long_tmp="$settled_tmp/var-folders-n6-hn37q6pj7sj_9dmqqc2113800000gn-T"
mkdir -p "$long_tmp" || fail "cannot create the long temp dir $long_tmp"
long_tmp="$(cd "$long_tmp" && pwd -P)" || fail "cannot resolve the long temp dir $long_tmp"
socket_len() {
  local socket
  local dir
  dir="$(cd "$1" && pwd -P)" || fail "cannot resolve $1"
  socket="$dir/store/control/model-claude-code.sock"
  printf '%s' "${#socket}"
}
moved="" short_tmp=""
trap 'rm -rf "$evidence_tmp" "$crlf_sort_dir" "$settled_tmp" "$moved" "$short_tmp" ||
  echo "check-control-enforce: could not remove its scratch ($evidence_tmp $crlf_sort_dir $settled_tmp $moved $short_tmp); delete them by hand" >&2' EXIT
in_place="$(mktemp -d "$long_tmp/tmp.XXXXXXXXXX")" || fail "cannot create a sandbox in place under $long_tmp"
moved="$(TMPDIR="$long_tmp" oh_socket_sandbox)" || fail "oh_socket_sandbox could not make a sandbox under a long temp dir"
short_tmp="$(mktemp -d /tmp/oh.XXXXXX)" || fail "cannot create a short temp dir under /tmp"
kept="$(TMPDIR="$short_tmp" oh_socket_sandbox)" || fail "oh_socket_sandbox could not make a sandbox under a short temp dir"
[ "$(socket_len "$in_place")" -ge 104 ] ||
  fail "the long temp dir case is vacuous: a sandbox made in place already fits ($(socket_len "$in_place") bytes)"
[ "$(socket_len "$moved")" -lt 104 ] ||
  fail "a control phase's socket under a long temp dir is $(socket_len "$moved") bytes, past the 104-byte sun_path: $moved"
case "$moved" in
  "$long_tmp"/*) fail "a sandbox under a long temp dir stayed there: $moved" ;;
esac
case "$kept" in
  "$short_tmp"/*) ;;
  *) fail "a sandbox under a short temp dir left it: $kept" ;;
esac

# The bound the control phases put on a run. macOS has no `timeout`, so with it
# hidden from PATH the perl fallback runs — as on macOS CI, where every phase
# calling bare `timeout` failed before its harness ran. On either branch a run
# that ends in time keeps its own status and environment, and one that outlives
# its deadline is stopped there and ends with 124, or, ignoring the TERM, is
# KILLed after the grace and ends with 137.
no_timeout="$settled_tmp/no-timeout-bin"
mkdir "$no_timeout" || fail "cannot create $no_timeout"
for tool in perl sh sleep; do
  path="$(command -v "$tool")" || fail "$tool is not on PATH, so the deadline cases cannot run"
  ln -s "$path" "$no_timeout/$tool" || fail "cannot link $tool into $no_timeout"
done
deadline_case() {
  local branch="$1" status=0 out started elapsed
  # shellcheck disable=SC2016 # expanded by the child shell, from the environment it was given
  out="$(OH_DEADLINE_SEEN=kept oh_deadline 10 sh -c 'printf %s "$OH_DEADLINE_SEEN"; exit 3')" || status=$?
  [ "$status" -eq 3 ] || fail "$branch: a run that ended in time reported $status, not its own 3"
  [ "$out" = kept ] || fail "$branch: a run that ended in time saw '$out' for the variable it was given"
  status=0
  started=$SECONDS
  oh_deadline 1 sleep 20 || status=$?
  elapsed=$((SECONDS - started))
  [ "$status" -eq 124 ] || fail "$branch: a run past its deadline ended $status, not 124"
  [ "$elapsed" -le 5 ] || fail "$branch: a 1 s deadline let the run go on for ${elapsed}s"
  # One that ignores the TERM is KILLed once the grace has passed.
  status=0
  started=$SECONDS
  # shellcheck disable=SC2016 # the program is perl's, expanded by perl
  OH_DEADLINE_KILL_AFTER=1 oh_deadline 1 perl -e '$SIG{TERM} = "IGNORE"; sleep 20' 2>/dev/null || status=$?
  elapsed=$((SECONDS - started))
  [ "$status" -eq 137 ] || fail "$branch: a run that ignored the TERM ended $status, not 137"
  [ "$elapsed" -le 6 ] || fail "$branch: a run that ignored the TERM went on for ${elapsed}s past a 1 s deadline and 1 s grace"
  # A grace that is not whole seconds from 1 is refused before anything runs.
  for grace in "" 0 00 05 3s -1; do
    status=0
    OH_DEADLINE_KILL_AFTER="$grace" oh_deadline 1 sh -c 'echo ran' 2>/dev/null || status=$?
    [ "$status" -eq 2 ] || fail "$branch: a kill grace of '$grace' was not refused (ended $status)"
  done
}
(
  PATH="$no_timeout"
  ! command -v timeout >/dev/null 2>&1 || fail "timeout is still on the hidden PATH"
  deadline_case "without timeout"
) || exit 1
if command -v timeout >/dev/null 2>&1; then
  deadline_case "with timeout"
fi

# What a resumed turn that ran into its bound was doing. The live resume phase
# once spent 300 s on a turn that said nothing, and a timeout alone cannot tell a
# stalled model stream from a resume codex never took — so each reading below is
# the frames such a turn leaves, read back into the one line the phase prints.
silent_case() {
  local description="$1" needle="$2" frames="$3" reading
  jq -n --arg out "$frames" '{results: [{status: "timeout", stdout: $out}]}' >"$evidence_tmp/silent.json" ||
    fail "$description: could not write the fixture report under $evidence_tmp (is jq installed and the temp dir writable?)"
  reading="$(_oh_silent_turn_reading "$evidence_tmp/silent.json")"
  case "$reading" in
    *"$needle"*) ;;
    *) fail "$description: the reading was '$reading', not one naming '$needle'" ;;
  esac
}
silent_case "a turn that sent nothing" "never announced the thread" ""
silent_case "a resume that never started a turn" "never started the turn" \
  '{"type":"thread.started","thread_id":"t"}'
silent_case "a stalled model stream" "a stalled model stream" \
  "$(printf '%s\n' '{"type":"thread.started","thread_id":"t"}' '{"type":"turn.started"}')"
silent_case "a command that never finished" "never finished: touch resume-auto.txt" \
  "$(printf '%s\n' '{"type":"thread.started","thread_id":"t"}' '{"type":"turn.started"}' \
    '{"type":"item.started","item":{"id":"i0","type":"command_execution","command":"touch resume-auto.txt"}}')"
silent_case "a command that finished before the stall" "a stalled model stream" \
  "$(printf '%s\n' '{"type":"thread.started","thread_id":"t"}' '{"type":"turn.started"}' \
    '{"type":"item.started","item":{"id":"i0","type":"command_execution","command":"touch x"}}' \
    '{"type":"item.completed","item":{"id":"i0","type":"command_execution","command":"touch x"}}')"

# The control phase end to end, against an agent that works as fast as a shell
# can and would batch everything it could: goose finished all 60 steps of a
# paced workload in 15 s, three attempts running, so the phase never had a turn
# in flight to interrupt. Driven through `oh_control_enforce` with a
# stand-in `oneharness` whose agent runs whatever the prompt says — the chain of
# held steps when the prompt names one, otherwise every step file at once, the
# way that goose turn did. One agent stops at the interrupt; one carries on.
agent_dir="$(mktemp -d /tmp/oh-ctl.XXXXXX)" || fail "cannot create a scratch directory under /tmp"
trap 'rm -rf "$evidence_tmp" "$crlf_sort_dir" "$settled_tmp" "$moved" "$short_tmp" "$agent_dir" ||
  echo "check-control-enforce: could not remove its scratch; delete it by hand" >&2' EXIT
cat >"$agent_dir/oneharness" <<'AGENT' || fail "could not write the fake agent into $agent_dir; check that /tmp is writable"
#!/usr/bin/env bash
# Stands in for `oneharness run --control` driving an agent, and for
# `oneharness interrupt`. FAKE_AGENT_HONORS=1: an interrupt stops the agent and
# kills its in-flight tool call; anything else: the agent ignores it.
set -euo pipefail
cmd="$1"
shift
name="" store="" cwd="" prompt=""
while [ $# -gt 0 ]; do
  case "$1" in
    --session) name="$2" && shift 2 ;;
    --session-dir) store="$2" && shift 2 ;;
    --cwd) cwd="$2" && shift 2 ;;
    --prompt) prompt="$2" && shift 2 ;;
    *) shift ;;
  esac
done
# The fixture's own failures, named, so the check that drove it can say why.
die() {
  echo "fake agent: $*; check that /tmp is writable and rerun scripts/check-control-enforce.sh" >&2
  exit 1
}
flag="$store/control/$name.interrupted"
if [ "$cmd" = interrupt ]; then
  : >"$flag" || die "cannot record the interrupt at $flag"
  printf '{"ok":true,"mechanism":"fake-mechanism"}\n'
  exit 0
fi
mkdir -p "$store/control" || die "cannot create $store/control"
socket="$store/control/$name.sock"
perl -MIO::Socket::UNIX -e 'IO::Socket::UNIX->new(Type => SOCK_STREAM(), Local => $ARGV[0], Listen => 1) or die "$!\n"' "$socket" ||
  die "cannot open a control socket at $socket"
cd "$cwd" || die "cannot enter $cwd"
# The first step command a text names, or nothing.
token() { sed -n 's/.*sh ohstep\.sh \([0-9a-f]*\).*/\1/p' | sed -n 1p; }
tok="$(printf '%s' "$prompt" | token)"
if [ -z "$tok" ]; then
  for ((n = 1; n <= 60; n++)); do
    : >"$(printf 'step-%03d.txt' "$n")" || die "cannot write step file $n in $cwd"
  done
  sleep 2
fi
while [ -n "$tok" ]; do
  sh ohstep.sh "$tok" >agent.out 2>&1 &
  call=$!
  while kill -0 "$call" 2>/dev/null; do
    if [ -e "$flag" ] && [ "${FAKE_AGENT_HONORS:-}" = 1 ]; then
      kill "$call" 2>/dev/null || true
      # Killed, so it ends non-zero: that is the point.
      wait "$call" 2>/dev/null || true
      break 2
    fi
    sleep 0.1
  done
  if ! wait "$call"; then
    die "step $tok failed, saying: $(cat agent.out 2>/dev/null || echo '<nothing readable>')"
  fi
  tok="$(token <agent.out)"
done
rm -f "$socket" || die "cannot remove $socket"
outcome=none
if [ -e "$flag" ]; then
  outcome=served
fi
printf '{"results":[{"status":"ok"}],"control":{"interrupts":[{"outcome":"%s"}]},"session":{"token":"t"}}\n' "$outcome"
AGENT
chmod +x "$agent_dir/oneharness" || fail "could not make $agent_dir/oneharness executable; check that /tmp allows it"
agent_case() {
  local honors="$1"
  status=0
  out="$(TMPDIR=/tmp ONEHARNESS_BIN="$agent_dir/oneharness" FAKE_AGENT_HONORS="$honors" \
    _OH_CONTROL_FREEZE_SECONDS=2 oh_control_enforce fake fake-mechanism 2>&1)" || status=$?
}
out=""
agent_case 1
[ "$status" -eq 0 ] || {
  printf '%s\n' "$out" >&2
  fail "a fast agent that stops at the interrupt did not pass the control phase (returned $status)"
}
case "$out" in
  *"work frozen at 2 step files"*) ;;
  *)
    printf '%s\n' "$out" >&2
    fail "a fast agent that stops at the interrupt was not held at step 2"
    ;;
esac
agent_case 0
[ "$status" -eq 1 ] || {
  printf '%s\n' "$out" >&2
  fail "an agent that ignores the interrupt was not failed (returned $status)"
}
case "$out" in
  *"work did NOT stop"*) ;;
  *)
    printf '%s\n' "$out" >&2
    fail "an agent that ignores the interrupt failed for some other reason"
    ;;
esac

# The resume phase's answer to a silent turn, end to end: a resumed turn that
# runs into its bound fails naming what its frames say it was doing — never
# blamed on the resume argv, which is what this phase said of the macOS turn
# that spent all 300 s on a stalled stream. A stand-in `oneharness` answers the
# phase's turns: turn one creates the session, and the resumed turn reports a
# timeout (a thread and a turn started, then nothing) the first
# FAKE_RESUME_SILENT times, then runs — touching the file it was asked to and
# recalling the word.
resume_dir="$(mktemp -d /tmp/oh-resume.XXXXXX)" || fail "cannot create a scratch directory under /tmp"
trap 'rm -rf "$evidence_tmp" "$crlf_sort_dir" "$settled_tmp" "$moved" "$short_tmp" "$agent_dir" "$resume_dir" ||
  echo "check-control-enforce: could not remove its scratch; delete it by hand" >&2' EXIT
cat >"$resume_dir/oneharness" <<'RESUME' || fail "could not write the fake oneharness into $resume_dir; check that /tmp is writable"
#!/usr/bin/env bash
set -euo pipefail
shift
prompt="" session="" store="" cwd=""
while [ $# -gt 0 ]; do
  case "$1" in
    --prompt) prompt="$2" && shift 2 ;;
    --session) session="$2" && shift 2 ;;
    --session-dir) store="$2" && shift 2 ;;
    --cwd) cwd="$2" && shift 2 ;;
    *) shift ;;
  esac
done
die() {
  echo "fake oneharness: $*; check that /tmp is writable and rerun scripts/check-control-enforce.sh" >&2
  exit 3
}
mkdir -p "$store" || die "cannot create $store"
word="$store/$session.word"
if [ ! -e "$word" ]; then
  printf '%s' "$prompt" | sed -n 's/.*for the rest of our conversation: \([A-Z0-9]*\)\..*/\1/p' >"$word" ||
    die "cannot store the word turn one established in $word"
  jq -n '{results: [{status: "ok", command: ["codex", "exec"], text: "OK"}], session: {phase: "create", token: "th-1"}}'
  exit 0
fi
count="$store/$session.resumes"
n=0
if [ -e "$count" ]; then
  n="$(cat "$count")" || die "cannot read the resume count in $count"
fi
n=$((n + 1))
echo "$n" >"$count" || die "cannot write the resume count to $count"
if [ "$n" -le "${FAKE_RESUME_SILENT:-0}" ]; then
  jq -n '{results: [{status: "timeout", command: ["codex", "exec", "resume"], stdout: "{\"type\":\"thread.started\",\"thread_id\":\"th-1\"}\n{\"type\":\"turn.started\"}"}], session: {phase: "continue", token: "th-1"}}'
  exit 1
fi
file="$(printf '%s' "$prompt" | sed -n 's/.*in the current directory: touch \([^;]*\);.*/\1/p')"
: >"$cwd/$file" || die "cannot touch $cwd/$file"
recalled="$(cat "$word")" || die "cannot read $word"
jq -n --arg word "$recalled" '{results: [{status: "ok", command: ["codex", "exec", "resume"], text: $word}], session: {phase: "continue", token: "th-1"}}'
RESUME
chmod +x "$resume_dir/oneharness" || fail "could not make $resume_dir/oneharness executable; check that /tmp allows it"
resume_case() {
  local silent="$1"
  status=0
  out="$(ONEHARNESS_BIN="$resume_dir/oneharness" FAKE_RESUME_SILENT="$silent" TMPDIR=/tmp OH_TIMEOUT='' \
    oh_resume_mode_enforce fakecodex auto 2>&1)" || status=$?
}
resume_case 0
[ "$status" -eq 0 ] || {
  printf '%s\n' "$out" >&2
  fail "a resumed turn that ran was not passed (returned $status)"
}
resume_case 1
[ "$status" -eq 1 ] || {
  printf '%s\n' "$out" >&2
  fail "a resumed turn that ran into its bound was not failed (returned $status)"
}
case "$out" in
  *"ran into its 420s bound: the turn started and the model sent nothing more"*) ;;
  *)
    printf '%s\n' "$out" >&2
    fail "a resumed turn that ran into its bound failed without naming what it was doing"
    ;;
esac
case "$out" in
  *argv_codex*)
    printf '%s\n' "$out" >&2
    fail "a resumed turn that ran into its bound was blamed on the resume argv"
    ;;
esac

# The stalling endpoint `oh_resume_stall_enforce` points real codex at. Its
# verdict rests on the endpoint stalling exactly one model request and serving
# the retry — never a websocket warmup, which codex abandons and carries on —
# and on its websocket handshake being one codex accepts (RFC 6455's own
# example key and accept value).
endpoint_dir="$(mktemp -d /tmp/oh-stall.XXXXXX)" || fail "cannot create a scratch directory under /tmp"
trap 'kill "${endpoint_pid:-}" 2>/dev/null || true; rm -rf "$evidence_tmp" "$crlf_sort_dir" "$settled_tmp" "$moved" "$short_tmp" "$agent_dir" "$resume_dir" "$endpoint_dir" ||
  echo "check-control-enforce: could not remove its scratch; delete it by hand" >&2' EXIT
python3 -I "$root/scripts/codex-stall-endpoint.py" "$endpoint_dir" >"$endpoint_dir/endpoint.log" 2>&1 &
endpoint_pid=$!
_oh_wait_for 15 test -s "$endpoint_dir/port" ||
  fail "the stalling endpoint never wrote its port: $(cat "$endpoint_dir/endpoint.log" 2>/dev/null)"
endpoint_port="$(_oh_port_in "$endpoint_dir/port")" ||
  fail "the stalling endpoint's port file does not hold one TCP port (said above)"
# The port reader the phase dials with refuses anything but one TCP port.
for bad in "" 0 080 65536 "12 34" "1x"; do
  printf '%s\n' "$bad" >"$endpoint_dir/bad-port" || fail "could not write $endpoint_dir/bad-port"
  ! _oh_port_in "$endpoint_dir/bad-port" >/dev/null 2>&1 || fail "the port reader accepted '$bad'"
done
endpoint="http://127.0.0.1:$endpoint_port/v1/responses"
printf 'stall-next\n' >"$endpoint_dir/mode" || fail "could not arm the endpoint's stall in $endpoint_dir/mode"
# One POST, failing the check unless curl ends as expected: 0 when the endpoint
# serves, 28 (its time limit) when it is meant to be holding the response.
# Anything else is a connection or protocol failure, reported as curl's own.
#   $1 time limit, $2 body, $3 the exit curl must end with
post() {
  local status=0 out
  out="$(curl -sS --max-time "$1" -H 'content-type: application/json' -d "$2" "$endpoint" 2>&1)" || status=$?
  [ "$status" -eq "$3" ] ||
    fail "curl ended $status, not $3, posting $2 to the stalling endpoint: $out (its log: $endpoint_dir/endpoint.log)"
  printf '%s' "$out"
}
# A body that is not a Responses request is refused, and does not spend the
# armed stall: only a model request may consume it.
case "$(post 5 '[]' 0)" in
  *"a Responses request is a JSON object"*) ;;
  *) fail "the endpoint answered a body that is not a JSON object as if it were a model request" ;;
esac
case "$(post 5 '{"generate":false}' 0)" in
  *response.completed*) ;;
  *) fail "the endpoint stalled a warmup, which codex abandons, so the stall would prove nothing" ;;
esac
case "$(post 2 '{"input":[]}' 28)" in
  *response.completed*) fail "the endpoint served the model request it was armed to stall" ;;
  *response.created*) ;;
  *) fail "the endpoint's stalled request did not even announce its response" ;;
esac
case "$(post 5 '{"input":[]}' 0)" in
  *response.completed*) ;;
  *) fail "the endpoint did not serve the request after its one stall, so codex's retry would stall too" ;;
esac
[ "$(awk '{print $2, $3, $4}' "$endpoint_dir/requests.log" | tr '\n' ';')" = "sse warmup serve;sse request stall;sse request serve;" ] ||
  fail "the endpoint's request log was not warmup served, request stalled, request served: $(cat "$endpoint_dir/requests.log")"
# The upgraded socket stays open, so curl ends on its time limit (28) having
# read the handshake; any other ending is the probe failing, not the endpoint.
handshake_status=0
handshake="$(curl -sS -i --max-time 2 -H 'Connection: Upgrade' -H 'Upgrade: websocket' -H 'Sec-WebSocket-Version: 13' \
  -H 'Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==' "$endpoint" 2>&1)" || handshake_status=$?
[ "$handshake_status" -eq 28 ] ||
  fail "the websocket probe's curl ended $handshake_status, not 28 (its time limit on the open socket): $handshake"
accept="$(printf '%s\n' "$handshake" | tr -d '\r' | awk -F': ' 'tolower($1) == "sec-websocket-accept" { print $2 }')"
[ "$accept" = "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=" ] ||
  fail "the endpoint's websocket accept was '$accept', not RFC 6455's s3pPLMBiTxaQ9kYGzzhZRbK+xOo=, so codex would refuse the transport it uses by default"
kill "$endpoint_pid" 2>/dev/null || true
wait "$endpoint_pid" 2>/dev/null || true

echo "check-control-enforce: ok"
