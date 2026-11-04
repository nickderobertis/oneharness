#!/usr/bin/env bash
# Exercise the live echo helper through the real oneharness subprocess boundary.
# The OpenCode double records the launch environment and emits native JSONL;
# the paid live suite proves the same setup against real OpenCode.
# llmlint: ignore-file[e2e_not_mocked] This hermetic helper test replaces only the paid CLI, as check-qwen-context-free.sh does. The live-opencode suite drives real OpenCode with this helper.
set -euo pipefail
case $(uname -s) in
  MINGW* | MSYS* | CYGWIN*)
    echo "check-opencode-context-free: skipped on Windows (extensionless executable double)" >&2
    exit 0
    ;;
esac
fail() {
  echo "check-opencode-context-free: $*; rerun bash -x scripts/check-opencode-context-free.sh" >&2
  exit 1
}
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)" || fail "cannot resolve repository root; run this script from a readable checkout"
[ -x "$root/target/debug/oneharness" ] || fail "run just build first"
tmp="$(mktemp -d "${TMPDIR:-/tmp}/tmp.XXXXXXXXXX")" || fail "cannot create scratch space; check TMPDIR and free disk space"
trap 'rm -rf "$tmp" || echo "check-opencode-context-free: remove abandoned scratch directory $tmp by hand" >&2' EXIT
mkdir -p "$tmp/bin" "$tmp/sandboxes" "$tmp/ambient" || fail "cannot create fixtures under $tmp; check permissions and disk space"
cat >"$tmp/bin/opencode" <<'DOUBLE' || fail "cannot write $tmp/bin/opencode; check permissions and disk space"
#!/usr/bin/env bash
set -euo pipefail
if [ "${1:-}" = --version ]; then echo 0.0.0-double; exit 0; fi
jq -cn --arg cwd "$PWD" --arg home "$HOME" --arg config "${XDG_CONFIG_HOME:-}" \
  --arg data "${XDG_DATA_HOME:-}" --arg cache "${XDG_CACHE_HOME:-}" --arg state "${XDG_STATE_HOME:-}" \
  --arg custom "${OPENCODE_CONFIG:-}" --arg dir "${OPENCODE_CONFIG_DIR:-}" \
  --arg content "${OPENCODE_CONFIG_CONTENT:-}" --arg claude "${OPENCODE_DISABLE_CLAUDE_CODE:-}" \
  '{cwd:$cwd,home:$home,config:$config,data:$data,cache:$cache,state:$state,custom:$custom,dir:$dir,content:$content,claude:$claude}' >>"$PROBE_RECORD" || { echo "opencode double: cannot write $PROBE_RECORD; check permissions and disk space" >&2; exit 1; }
text="$(printf '%s\n' "$@" | grep -o 'ONEHARNESS-LIVE-[0-9]*' | head -n 1)" || { echo "opencode double: no marker in argv; inspect the live echo prompt" >&2; exit 1; }
if [ -n "${PROBE_CONTEXT_MODE:-}" ]; then
  # Stand in for the paid CLI only: requests still reach the real local HTTP
  # endpoint. The live alarm runs these discovery observations on real OpenCode.
  export PROBE_MARKER="$text"
  python3 - <<'CAPTURE'
import json, os, pathlib, urllib.request
cfg = json.loads(os.environ["OPENCODE_CONFIG_CONTENT"])
base = cfg["provider"]["capture"]["options"]["baseURL"]
mode = os.environ["PROBE_CONTEXT_MODE"]
project = pathlib.Path.cwd() / "AGENTS.md"
global_rules = pathlib.Path(os.environ["HOME"]) / ".config/opencode/AGENTS.md"
fallback = pathlib.Path(os.environ["HOME"]) / ".claude/CLAUDE.md"
context = ""
if mode != "blind":
    for path in [project, global_rules if global_rules.exists() else fallback]:
        if path.exists():
            context += path.read_text()
if mode == "leak" and not project.exists():
    context += pathlib.Path(os.environ["PROBE_PROJECT"]).read_text()
if mode != "empty" or project.exists():
    data = json.dumps({"messages": [{"role": "system", "content": context},
        {"role": "user", "content": os.environ["PROBE_MARKER"]}]}).encode()
    with urllib.request.urlopen(urllib.request.Request(base + "/chat/completions", data=data,
        headers={"Content-Type": "application/json"}), timeout=10) as response:
        response.read()
CAPTURE
fi
[ "${PROBE_OMIT:-0}" = 0 ] || text='Please provide more context.'
jq -cn --arg text "$text" '{type:"text",sessionID:"ses_probe",part:{type:"text",text:$text}}'
DOUBLE
chmod +x "$tmp/bin/opencode" || fail "cannot make $tmp/bin/opencode executable; check permissions"
probe() {
  rm -f "$tmp/record" || fail "cannot clear $tmp/record; check directory permissions"
  local rc=0
  # shellcheck disable=SC2016 # the driven shell expands its own positional argument
  env PATH="$tmp/bin:$PATH" HOME="$tmp/ambient" TMPDIR="$tmp/sandboxes" \
    ONEHARNESS_BIN="$root/target/debug/oneharness" PROBE_RECORD="$tmp/record" PROBE_OMIT="$1" \
    OPENCODE_CONFIG=/ambient/config OPENCODE_CONFIG_DIR=/ambient/dir OPENCODE_CONFIG_CONTENT='{"instructions":["/ambient/rules"]}' \
    bash -c 'set -euo pipefail; cd "$1"; source scripts/e2e-lib.sh; oh_echo_without_context opencode' _ "$root" >"$tmp/output" 2>&1 || rc=$?
  [ "$rc" -eq "$2" ] || fail "unexpected exit $rc: $(cat "$tmp/output")"
  [ "$(wc -l <"$tmp/record")" -eq 1 ] || fail "echo was retried"
  jq -e --arg prefix "$tmp/sandboxes/" '
    (.cwd | startswith($prefix)) and (.cwd | endswith("/work")) and
    (.home | startswith($prefix)) and
    .config == (.home + "/.config") and .data == (.home + "/.local/share") and
    .cache == (.home + "/.cache") and .state == (.home + "/.local/state") and
    .custom == "" and .dir == (.config + "/opencode") and
    .content == "{}" and .claude == "1"' "$tmp/record" >/dev/null || fail "ambient context leaked: $(cat "$tmp/record")"
  remaining="$(ls -A "$tmp/sandboxes")" || fail "cannot inspect $tmp/sandboxes; check permissions"
  [ -z "$remaining" ] || fail "sandbox was left behind"
}
probe 0 0
grep -q "extracted .text via 'json:opencode-parts'" "$tmp/output" || fail "extraction assertion was lost"
probe 1 1
grep -q 'unique marker never surfaced' "$tmp/output" || fail "missing marker failed for the wrong reason: $(cat "$tmp/output")"
grep -qx 'oh_echo_without_context opencode' "$root/scripts/e2e-opencode.sh" || fail "live suite bypasses isolation"
# Native conversion failure must stop before the CLI launches and clean up.
for converter_status in 0 1; do
  rm -f "$tmp/record" || fail "cannot clear $tmp/record; check directory permissions"
  rc=0
  # shellcheck disable=SC2016 # expanded by the driven shell
  PROBE_RECORD="$tmp/record" TMPDIR="$tmp/sandboxes" \
    bash -c 'set -euo pipefail; source "$1/scripts/e2e-lib.sh"; converter_status="$2"; cygpath() { return "$converter_status"; }; oh_echo_without_context opencode' _ "$root" "$converter_status" >"$tmp/output" 2>&1 || rc=$?
  [ "$rc" -ne 0 ] || fail "empty/failed conversion passed"
  grep -q 'could not convert .*native context home' "$tmp/output" || fail "conversion failed without diagnostic: $(cat "$tmp/output")"
  [ ! -e "$tmp/record" ] || fail "CLI launched after conversion failure"
  remaining="$(ls -A "$tmp/sandboxes")" || fail "cannot inspect $tmp/sandboxes; check permissions"
  [ -z "$remaining" ] || fail "conversion failure leaked sandbox"
done
# Prove the alarm's own failure paths against the real endpoint and CLI double;
# the paid live suite holds that double's discovery model to real OpenCode.
for mode in normal blind empty leak; do
  rm -f "$tmp/record" || fail "cannot clear record; check temp permissions"
  rc=0
  # shellcheck disable=SC2016 # expanded in the driven shell
  env PATH="$tmp/bin:$PATH" ONEHARNESS_BIN="$root/target/debug/oneharness" \
    PROBE_CONTEXT_MODE="$mode" PROBE_PROJECT="$root/AGENTS.md" PROBE_RECORD="$tmp/record" TMPDIR="$tmp/sandboxes" \
    bash -c 'set -euo pipefail; source "$1/scripts/e2e-lib.sh"; oh_opencode_context_free_enforce' _ "$root" >"$tmp/output" 2>&1 || rc=$?
  case "$mode" in
    normal)
      expected='PASS: opencode real CLI excludes'
      [ "$rc" -eq 0 ] || fail "normal alarm failed: $(cat "$tmp/output")"
      ;;
    blind) expected='checkout control did not send project/global rules' ;;
    empty) expected='isolated turn sent no request' ;;
    leak) expected='isolated turn leaked project/global/Claude rules' ;;
  esac
  [ "$mode" = normal ] || [ "$rc" -ne 0 ] || fail "$mode alarm incorrectly passed"
  grep -qF "$expected" "$tmp/output" || fail "$mode alarm failed for the wrong reason: $(cat "$tmp/output")"
  remaining="$(ls -A "$tmp/sandboxes")" || fail "cannot inspect scratch space; check temp permissions"
  [ -z "$remaining" ] || fail "$mode alarm leaked scratch space"
done
echo 'check-opencode-context-free: ok'
