#!/usr/bin/env bash
# PATH shim for measurement 7: installed as `codex` ahead of the real one, it
# runs every Codex launch under netmeasure.py with `--disable plugins`, appending
# one report line per launch to $NETMEASURE_REPORT. NETMEASURE_REAL_CODEX names
# the real binary. The suite pins ONEHARNESS_NO_CONFIG=1, so a
# `[harness.codex] args` config entry would never reach its launches.
set -euo pipefail
here="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")" && pwd)"
real="${NETMEASURE_REAL_CODEX:?set NETMEASURE_REAL_CODEX to the real codex}"
if [ "$(readlink -f "$real")" = "$(readlink -f "${BASH_SOURCE[0]}")" ]; then
  echo "codex-shim: NETMEASURE_REAL_CODEX resolves to this shim; resolve it before prepending the shim to PATH" >&2
  exit 2
fi
exec "$here/netmeasure.py" --label "codex $*" --report "$NETMEASURE_REPORT" \
  -- "$real" --disable plugins "$@"
