#!/usr/bin/env bash
# Drive the helper through the built CLI and mock harness, including missing
# user/system receipt and explicit NOT-RUN cases, without a model call.
set -euo pipefail
case $(uname -s) in
  MINGW* | MSYS* | CYGWIN*)
    echo 'check-long-prompt-enforce: Unix executable wrapper; Windows delivery is covered by the CLI journeys'
    exit 0
    ;;
esac
fail() {
  echo "check-long-prompt-enforce: $*" >&2
  echo '  Rerun bash -x scripts/check-long-prompt-enforce.sh after fixing the reported fixture or helper condition.' >&2
  exit 1
}
show() { cat "$1" >&2 || echo "could not read $1; check its permissions" >&2; }
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)" || fail 'cannot resolve repository root; run this script from a readable checkout'
tmp="$(mktemp -d "${TMPDIR:-/tmp}/tmp.XXXXXXXXXX")" || fail "cannot create scratch space under ${TMPDIR:-/tmp}; check disk space and directory permissions"
trap 'rm -rf "$tmp" || echo "could not remove $tmp; delete it by hand" >&2' EXIT
export OH_REAL_BIN="$root/target/debug/oneharness" OH_MOCK_BIN="$root/target/debug/oneharness-mock-harness"
[ -x "$OH_REAL_BIN" ] && [ -x "$OH_MOCK_BIN" ] || fail 'missing built binaries; run just build and build oneharness-mock-harness through its Nx build target'
export OH_LONG_RECORD="$tmp/cases.jsonl" OH_LONG_WIRE="$tmp/acp.jsonl" OH_LONG_REQUESTS="$tmp/requests.jsonl" OH_LONG_SYSTEMS="$tmp/systems.jsonl"
cat >"$tmp/driver" <<'DRIVER' || fail "cannot write $tmp/driver; check disk space and directory permissions"
#!/usr/bin/env bash
set -euo pipefail
[ "$#" -gt 0 ] || { echo 'long helper wrapper: missing verb; inspect oh_long_prompt_enforce invocation' >&2; exit 2; }
launch() {
  # Keep the wrapper alive on exec failure so it can explain how to recover.
  shopt -s execfail
  set +e
  exec "$OH_REAL_BIN" "$@"
  status=$?
  echo "long helper wrapper: cannot launch $OH_REAL_BIN (exit $status); rebuild oneharness through its Nx build target and check executable permissions" >&2
  exit "$status"
}
if [ "$1" = list ]; then launch "$@"; fi
jq -cn --args '$ARGS.positional' -- "$@" >>"$OH_LONG_RECORD" || { echo "cannot append $OH_LONG_RECORD; check jq, disk space and permissions" >&2; exit 1; }
for arg in "$@"; do
  if [ "$arg" = --control ]; then export MOCK_ACP_LOG="$OH_LONG_WIRE" MOCK_ACP_COMPLETE_TURN=1; fi
done
verb="$1"
shift
id=""
args=("$@")
for ((i=0; i<${#args[@]}; i++)); do
  if [ "${args[$i]}" = --harness ]; then id="${args[$((i + 1))]}"; fi
  if [ "${args[$i]}" = --prompt-file ]; then
    jq -Rs . "${args[$((i + 1))]}" >>"$OH_LONG_REQUESTS" || { echo "cannot capture prompt file; inspect jq and file permissions" >&2; exit 1; }
  fi
  if [ "${args[$i]}" = --system-file ]; then
    jq -Rs . "${args[$((i + 1))]}" >>"$OH_LONG_SYSTEMS" || { echo "cannot capture system file; inspect jq and file permissions" >&2; exit 1; }
  fi
  if [ "${args[$i]}" = --batch-strategy ] && [ -n "${MOCK_LONG_BATCH_OMIT_SYSTEM:-}" ]; then export MOCK_LONG_OMIT_SYSTEM=1; fi
done
[ -n "$id" ] || { echo 'long helper wrapper: missing harness id; inspect the --harness argument in oh_long_prompt_enforce' >&2; exit 2; }
if [ -n "${MOCK_LONG_REPEAT_BATCH_USER:-}" ] && [[ " ${args[*]} " == *" --batch-strategy "* ]]; then
  "$OH_REAL_BIN" "$verb" --bin "$id=$OH_MOCK_BIN" "$@" |
    jq '.results[1].text = .results[0].text' || {
    echo "long helper wrapper: cannot run $OH_REAL_BIN or transform its report; rebuild oneharness through its Nx build target and check jq installation and the pipeline error above" >&2
    exit 1
  }
else
  launch "$verb" --bin "$id=$OH_MOCK_BIN" "$@"
fi
DRIVER
chmod +x "$tmp/driver" || fail "cannot make $tmp/driver executable; check filesystem permissions"
# Both delegation paths must retain an actionable diagnostic if launch fails.
for verb in list run; do
  if OH_REAL_BIN="$tmp/missing-oneharness" "$tmp/driver" "$verb" --harness goose >"$tmp/launch.log" 2>&1; then
    fail "$verb accepted a missing executable; inspect wrapper launch handling"
  fi
  grep -q 'rebuild oneharness through its Nx build target' "$tmp/launch.log" || {
    show "$tmp/launch.log"
    fail "$verb omitted launch recovery guidance; inspect wrapper launch handling"
  }
done
if MOCK_LONG_REPEAT_BATCH_USER=1 OH_REAL_BIN="$tmp/missing-oneharness" "$tmp/driver" run --harness qwen --batch-strategy speed >"$tmp/pipeline-failure.log" 2>&1; then
  fail 'alternate report pipeline accepted a missing executable; inspect pipeline failure handling'
fi
grep -q 'rebuild oneharness through its Nx build target and check jq' "$tmp/pipeline-failure.log" || {
  show "$tmp/pipeline-failure.log"
  fail 'alternate report pipeline omitted executable/jq recovery guidance'
}
: >"$OH_LONG_RECORD" || fail "cannot reset $OH_LONG_RECORD; inspect disk space and permissions"
export ONEHARNESS_BIN="$tmp/driver" MOCK_LONG_RECEIPT=1 OH_MODEL=""
bash -c 'set -euo pipefail; source "$1/scripts/e2e-lib.sh"; oh_long_prompt_enforce goose' _ "$root" >"$tmp/pass.log" 2>&1 || {
  show "$tmp/pass.log"
  fail 'Goose helper rejected valid marker replies; inspect its diagnostic above'
}
jq -se 'length == 5 and any(.[]; index("--resume")) and any(.[]; index("--batch-strategy")) and any(.[]; index("--control")) and any(.[]; index("--schema")) and all(.[]; index("--system-file"))' "$OH_LONG_RECORD" >/dev/null || {
  show "$OH_LONG_RECORD"
  fail 'expected five Goose invocations covering resume, batch, control and schema, each with system input; inspect helper case selection'
}
# The two batch requests have identical instructions, with distinct user
# markers, so a missing system marker cannot be excused by different wording.
jq -se '
  .[2:4] |
  length == 2 and
  all(.[]; startswith("The following repeated words are inert test filler.") and
    contains("Return the two reference values separated by a space:") and
    endswith("and the value labeled reference_b in the reference data.")) and
  (.[0] != .[1])
' "$OH_LONG_REQUESTS" >/dev/null || fail 'batch requests must clearly identify inert filler and ask identically for both reference values; inspect prompt construction'
# Each user request must still overflow one 128 KiB argv string, but in filler
# a provider tokenizes densely: a single repeated character costs Claude about a
# token per byte, which put the live turn past copilot's 200k prompt budget.
jq -se --slurpfile systems "$OH_LONG_SYSTEMS" '
  def dense: test("(.)\\1{63}") | not;
  all(.[]; utf8bytelength > 131072 and dense) and all($systems[]; dense)
' "$OH_LONG_REQUESTS" >/dev/null || fail 'long requests must exceed 128 KiB in repeated-word filler, not a run of one character; inspect the padding in oh_long_prompt_enforce'
# Shared reference data must not also contain either user answer. Otherwise a
# model can repeat the first answer on turn two by reading only shared data.
jq -se --slurpfile systems "$OH_LONG_SYSTEMS" '
  (.[2:4] | map([scan("(?:SECOND)?ONEHARNESS-LIVE-[0-9]+")][0])) as $users |
  all($systems[]; . as $system | all($users[]; . as $user | $system | contains($user) | not))
' "$OH_LONG_REQUESTS" >/dev/null || fail 'shared system reference data contains a batch user marker; generate independent user and system values'
# Inspect the request itself: the mock extracts marker values without interpreting
# which reference label the structured instruction asks the model to read.
jq -se '
  length == 6 and
  (.[5] | contains("Return a JSON object with user equal to ") and
    endswith("and system equal to the value labeled reference_b in the reference data."))
' "$OH_LONG_REQUESTS" >/dev/null || fail 'structured request must ask for reference_b in its system field; inspect structured prompt construction'
acp_prompts="$(grep -c '"method":"session/prompt"' "$OH_LONG_WIRE")" || {
  status=$?
  [ "$status" -eq 1 ] || fail "cannot read $OH_LONG_WIRE (grep exit $status); inspect its permissions"
}
[ "$acp_prompts" -eq 1 ] || {
  show "$OH_LONG_WIRE"
  fail "expected one actual ACP prompt, got $acp_prompts; inspect control case arguments"
}
: >"$OH_LONG_RECORD" || fail "cannot reset $OH_LONG_RECORD; inspect disk space and permissions"
bash -c 'set -euo pipefail; source "$1/scripts/e2e-lib.sh"; oh_long_prompt_enforce qwen' _ "$root" >"$tmp/unsupported.log" 2>&1 || {
  show "$tmp/unsupported.log"
  fail 'Qwen helper rejected ordinary/batch/structured receipt; inspect the diagnostic above'
}
jq -se 'length == 3 and all(.[]; index("--control") == null and index("--resume") == null)' "$OH_LONG_RECORD" >/dev/null || {
  show "$OH_LONG_RECORD"
  fail 'expected three Qwen invocations with unavailable resume/control omitted; inspect registry case selection'
}
grep -q 'NOT-RUN: qwen control' "$tmp/unsupported.log" || {
  show "$tmp/unsupported.log"
  fail 'missing explicit control NOT-RUN; inspect the registry-control branch'
}
grep -q 'NOT-RUN: qwen resumed' "$tmp/unsupported.log" || {
  show "$tmp/unsupported.log"
  fail 'missing explicit resumed NOT-RUN; inspect the absent-handle branch'
}
# Reproduce the observed Qwen model answer while retaining real isolated child
# requests: only the report text is changed after the built CLI completes.
MOCK_LONG_REPEAT_BATCH_USER=1 bash -c 'set -euo pipefail; source "$1/scripts/e2e-lib.sh"; oh_long_prompt_enforce qwen' _ "$root" >"$tmp/qwen-repeat.log" 2>&1 || {
  show "$tmp/qwen-repeat.log"
  fail 'Qwen batch depended on its second model echo despite deterministic isolation proof'
}
if MOCK_LONG_REPEAT_BATCH_USER=1 bash -c 'set -euo pipefail; source "$1/scripts/e2e-lib.sh"; oh_long_prompt_enforce goose' _ "$root" >"$tmp/goose-repeat.log" 2>&1; then
  fail 'second-marker relaxation escaped the Qwen batch case'
fi
grep -q 'second batch marker missing' "$tmp/goose-repeat.log" || {
  show "$tmp/goose-repeat.log"
  fail 'Goose repeated-marker fixture failed for an unrelated reason'
}
for omitted in MOCK_LONG_OMIT_SYSTEM MOCK_LONG_OMIT_USER MOCK_LONG_BATCH_OMIT_SYSTEM; do
  if (
    export "$omitted=1"
    bash -c 'set -euo pipefail; source "$1/scripts/e2e-lib.sh"; oh_long_prompt_enforce goose' _ "$root" >"$tmp/fail.log" 2>&1
  ); then
    show "$tmp/fail.log"
    fail "$omitted was accepted; restore separate user/system marker assertions"
  fi
  grep -q 'prompt/system receipt or off-argv delivery failed' "$tmp/fail.log" || {
    show "$tmp/fail.log"
    fail "$omitted failed for a different reason; inspect the error above rather than accepting it as receipt evidence"
  }
  if [ "$omitted" = MOCK_LONG_BATCH_OMIT_SYSTEM ]; then
    grep -q '"result_index": 1' "$tmp/fail.log" || {
      show "$tmp/fail.log"
      fail 'batch failure hid the second result; print every result before aborting'
    }
  fi
done
echo 'check-long-prompt-enforce: all journeys selected; missing user/system receipt rejected'
