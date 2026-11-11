#!/usr/bin/env bash
# A runner's proof that the live tier's network recorder can measure here: one
# `true` launch through scripts/e2e-netmeasure.py, started the way the shim and
# the network-telemetry tests start it, must come back measured in pasta's
# namespace ("method": "netns-pasta"). Otherwise it exits 1 with the recorder's
# own reason. A bare `pasta -- true` is no such proof: Ubuntu 26.04's
# `usr.bin.pasta` AppArmor profile lets pasta start /usr/bin/true but refused
# the recorder's python.
#
#   network-recorder-preflight.sh [PYTHON]   (default: python3 on PATH)
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)" || {
  echo "network-recorder-preflight: cannot resolve the repository root from ${BASH_SOURCE[0]}; run it as scripts/network-recorder-preflight.sh from a readable checkout" >&2
  exit 1
}
python="${1:-python3}"
by_hand="run \`$python -I scripts/e2e-netmeasure.py --line-file /tmp/line.jsonl -- true\` by hand and read its stderr"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/oh-net-preflight-XXXXXX")" || {
  echo "network-recorder-preflight: cannot create scratch space under ${TMPDIR:-/tmp}; check that it exists and is writable" >&2
  exit 1
}
trap 'rm -rf "$scratch" || echo "network-recorder-preflight: could not remove $scratch; delete it by hand" >&2' EXIT

"$python" -I -c '' || {
  echo "network-recorder-preflight: cannot run the interpreter \`$python\` (its error is above); install python3 or pass a working one as the first argument" >&2
  exit 1
}
status=0
"$python" -I "$root/scripts/e2e-netmeasure.py" \
  --line-file "$scratch/preflight.jsonl" --launch-id preflight -- true || status=$?
if [ "$status" -ne 0 ]; then
  echo "network-recorder-preflight: the recorder launch of \`true\` exited $status (its stderr is above); $by_hand" >&2
  exit 1
fi
"$python" -I - "$scratch/preflight.jsonl" "$by_hand" <<'PY'
import json, sys

path, by_hand = sys.argv[1:3]
try:
    with open(path) as file:
        records = [json.loads(text) for text in file if text.strip()]
except (OSError, ValueError) as error:
    sys.exit(f"network-recorder-preflight: the recorder left no readable JSON lines in {path} ({error}); {by_hand}")
if len(records) != 1:
    sys.exit(f"network-recorder-preflight: the recorder wrote {len(records)} lines for one launch, not 1; {by_hand}")
(line,) = records
if not isinstance(line, dict):
    sys.exit(f"network-recorder-preflight: the recorder's line is not a JSON object: {line!r}; {by_hand}")
if line.get("method") != "netns-pasta":
    reason = line.get("reason") or f"it recorded method {line.get('method')!r} without a reason; {by_hand}"
    sys.exit(f"network-recorder-preflight: the network recorder cannot measure here: {reason}")
print("network-recorder-preflight: the recorder measured a launch in pasta's namespace")
PY
