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

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
python="${1:-python3}"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/oh-net-preflight-XXXXXX")"
trap 'rm -rf "$scratch"' EXIT

"$python" -I "$root/scripts/e2e-netmeasure.py" \
  --line-file "$scratch/preflight.jsonl" --launch-id preflight -- true
"$python" -I - "$scratch/preflight.jsonl" <<'PY'
import json, sys

path = sys.argv[1]
try:
    with open(path) as file:
        (line,) = [json.loads(text) for text in file if text.strip()]
except (OSError, ValueError) as error:
    sys.exit(f"network-recorder-preflight: the recorder wrote no single line to {path} ({error}); run scripts/e2e-netmeasure.py by hand to see why")
if line.get("method") != "netns-pasta":
    sys.exit(
        "network-recorder-preflight: the network recorder cannot measure here: "
        f"{line.get('reason', 'no reason recorded')}"
    )
print("network-recorder-preflight: the recorder measured a launch in pasta's namespace")
PY
