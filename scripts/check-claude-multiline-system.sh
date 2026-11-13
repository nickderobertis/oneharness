#!/usr/bin/env bash
# Exercise oh_claude_multiline_system_enforce through the real oneharness
# subprocess boundary and the real capture endpoint. The claude double sends
# the Messages request claude would — its own system block, then the
# --append-system-prompt-file it was given — and prints claude's JSON result;
# the paid live-claude suite runs the same phase against real claude.
# llmlint: ignore-file[e2e_not_mocked] This hermetic helper test replaces only the paid CLI, as check-opencode-context-free.sh does. The live-claude suite drives real claude through this phase.
set -euo pipefail
case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*)
    echo "check-claude-multiline-system: skipped on Windows (extensionless executable double)" >&2
    exit 0
    ;;
esac
fail() {
  echo "check-claude-multiline-system: $*; rerun bash -x scripts/check-claude-multiline-system.sh" >&2
  exit 1
}
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)" || fail "cannot resolve repository root; run this script from a readable checkout"
[ -x "$root/target/debug/oneharness" ] || fail "run just build first"
tmp="$(mktemp -d "${TMPDIR:-/tmp}/tmp.XXXXXXXXXX")" || fail "cannot create scratch space; check TMPDIR and free disk space"
trap 'rm -rf "$tmp" || echo "check-claude-multiline-system: remove abandoned scratch directory $tmp by hand" >&2' EXIT
mkdir -p "$tmp/bin" "$tmp/sandboxes" || fail "cannot create fixtures under $tmp; check permissions and disk space"
cat >"$tmp/bin/claude" <<'DOUBLE' || fail "cannot write $tmp/bin/claude; check permissions and disk space"
#!/usr/bin/env bash
set -euo pipefail
if [ "${1:-}" = --version ]; then echo '0.0.0 (Claude Code double)'; exit 0; fi
system_file=""
while [ "$#" -gt 0 ]; do
  [ "$1" != --append-system-prompt-file ] || system_file="$2"
  shift
done
[ -n "$system_file" ] || { echo "claude double: no --append-system-prompt-file; oneharness changed how it delivers --system" >&2; exit 1; }
# The phase's local key must be the only credential the turn carries.
[ -z "${CLAUDE_CODE_OAUTH_TOKEN:-}" ] || { echo "claude double: an OAuth token reached the local-endpoint turn" >&2; exit 1; }
# Silent: the reply still carries the marker, so only the missing request differs.
if [ "${PROBE_MODE:-}" = silent ]; then
  jq -cn --arg text "$(grep -o 'ONEHARNESS-LIVE-[0-9]*' | head -n 1)" '{type:"result",subtype:"success",is_error:false,result:$text}'
  exit 0
fi
SYSTEM_FILE="$system_file" python3 -c '
import json, os, sys, urllib.request
system = open(os.environ["SYSTEM_FILE"], encoding="utf-8", newline="").read()
if os.environ.get("PROBE_MODE") == "fold":
    system = system.replace("\n", " ")
blocks = [{"type": "text", "text": "You are the claude double."}]
# Drop: the request is sent, but without the --system it was given.
if os.environ.get("PROBE_MODE") != "drop":
    blocks.append({"type": "text", "text": system})
body = json.dumps({"model": "capture", "stream": True, "max_tokens": 64,
    "system": blocks,
    "messages": [{"role": "user", "content": [{"type": "text", "text": sys.stdin.read()}]}]}).encode()
request = urllib.request.Request(os.environ["ANTHROPIC_BASE_URL"] + "/v1/messages?beta=true", data=body,
    headers={"Content-Type": "application/json", "x-api-key": os.environ["ANTHROPIC_API_KEY"]})
text = ""
with urllib.request.urlopen(request, timeout=10) as response:
    for raw in response:
        line = raw.decode().strip()
        if line.startswith("data: "):
            event = json.loads(line[6:])
            if event.get("type") == "content_block_delta":
                text += event["delta"]["text"]
if os.environ.get("PROBE_MODE") == "mute":
    text = "a reply that drops the marker"
print(json.dumps({"type": "result", "subtype": "success", "is_error": False, "result": text}))
' || { echo "claude double: the Messages request failed; read the Python error above" >&2; exit 1; }
DOUBLE
chmod +x "$tmp/bin/claude" || fail "cannot make $tmp/bin/claude executable; check permissions"

for mode in intact fold drop silent mute; do
  rc=0
  # shellcheck disable=SC2016 # expanded by the driven shell
  env PATH="$tmp/bin:$PATH" TMPDIR="$tmp/sandboxes" ONEHARNESS_BIN="$root/target/debug/oneharness" \
    PROBE_MODE="$mode" CLAUDE_CODE_OAUTH_TOKEN=ambient-token-must-not-reach-the-turn \
    bash -c 'set -euo pipefail; source "$1/scripts/e2e-lib.sh"; oh_claude_multiline_system_enforce' _ "$root" >"$tmp/output" 2>&1 || rc=$?
  case "$mode" in
    intact)
      [ "$rc" -eq 0 ] || fail "an intact system prompt failed the phase: $(cat "$tmp/output")"
      expected='PASS: claude-code delivered a three-line --system'
      ;;
    mute) expected='unique marker never surfaced' ;;
    fold | drop) expected='the three-line --system did not reach the model request intact' ;;
    silent) expected='sent no Messages request to the capture endpoint' ;;
  esac
  [ "$mode" = intact ] || [ "$rc" -ne 0 ] || fail "$mode passed the phase: $(cat "$tmp/output")"
  grep -qF "$expected" "$tmp/output" || fail "$mode failed for the wrong reason: $(cat "$tmp/output")"
  remaining="$(ls -A "$tmp/sandboxes")" || fail "cannot inspect $tmp/sandboxes; check permissions"
  [ -z "$remaining" ] || fail "$mode left scratch space behind: $remaining"
done
grep -qx 'oh_claude_multiline_system_enforce' "$root/scripts/e2e-claude.sh" || fail "the live suite no longer runs the phase"
echo 'check-claude-multiline-system: ok'
