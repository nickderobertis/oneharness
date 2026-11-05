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
export OH_LONG_RECORD="$tmp/cases.jsonl" OH_LONG_WIRE="$tmp/acp.jsonl"
cat >"$tmp/driver" <<'DRIVER' || fail "cannot write $tmp/driver; check disk space and directory permissions"
#!/usr/bin/env bash
set -euo pipefail
[ "$#" -gt 0 ] || { echo 'long helper wrapper: missing verb; inspect oh_long_prompt_enforce invocation' >&2; exit 2; }
if [ "$1" = list ]; then exec "$OH_REAL_BIN" "$@"; fi
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
done
[ -n "$id" ] || { echo 'long helper wrapper: missing harness id; inspect the --harness argument in oh_long_prompt_enforce' >&2; exit 2; }
exec "$OH_REAL_BIN" "$verb" --bin "$id=$OH_MOCK_BIN" "$@"
DRIVER
chmod +x "$tmp/driver" || fail "cannot make $tmp/driver executable; check filesystem permissions"
export ONEHARNESS_BIN="$tmp/driver" MOCK_LONG_RECEIPT=1 OH_MODEL=""
bash -c 'set -euo pipefail; source "$1/scripts/e2e-lib.sh"; oh_long_prompt_enforce goose' _ "$root" >"$tmp/pass.log" 2>&1 || {
  show "$tmp/pass.log"
  fail 'Goose helper rejected valid marker replies; inspect its diagnostic above'
}
jq -se 'length == 5 and any(.[]; index("--resume")) and any(.[]; index("--batch-strategy")) and any(.[]; index("--control")) and any(.[]; index("--schema")) and all(.[]; index("--system-file"))' "$OH_LONG_RECORD" >/dev/null || {
  show "$OH_LONG_RECORD"
  fail 'expected five Goose invocations covering resume, batch, control and schema, each with system input; inspect helper case selection'
}
acp_prompts="$(grep -c '"method":"session/prompt"' "$OH_LONG_WIRE")" || [ "$acp_prompts" = 0 ] || fail "cannot read $OH_LONG_WIRE; inspect its permissions"
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
for omitted in MOCK_LONG_OMIT_SYSTEM MOCK_LONG_OMIT_USER; do
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
done
echo 'check-long-prompt-enforce: all journeys selected; missing user/system receipt rejected'
