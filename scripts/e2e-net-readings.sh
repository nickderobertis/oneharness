#!/usr/bin/env bash
# e2e-net-readings.sh FILE — which launches of a live suite the network
# recorder measured in its namespace (`netns-pasta`) and which it ran directly
# (`unavailable`, with why), counted from the suite's telemetry FILE
# (scripts/e2e-netshim.sh writes it): a turn that passes unmeasured would
# otherwise look like one that passed under the recorder. One
# `COUNT<TAB>METHOD<TAB>REASON` line per distinct reading, most frequent first;
# a missing FILE (the suite stopped before its first launch) says so and
# passes. The live workflows run it after their suite, pass or fail.
set -euo pipefail

[ "$#" -eq 1 ] || {
  echo "usage: scripts/e2e-net-readings.sh FILE (a suite's target/e2e-telemetry/network/<suite>.jsonl)" >&2
  exit 2
}
file="$1"
if [ ! -e "$file" ]; then
  echo "no telemetry file at $file: the suite recorded no launch"
  exit 0
fi
command -v jq >/dev/null || {
  echo "e2e-net-readings: jq is not on PATH; install it to read $file" >&2
  exit 1
}
# Start lines carry the launch's argv and no method; each launch's one
# measurement line carries its method, and an unmeasured one its reason.
readings="$(jq -r 'select(.method) | "\(.method)\t\(.reason // "")"' "$file")" || {
  echo "e2e-net-readings: $file holds a line that is not JSON; the recorder writes whole lines, so inspect it for another writer" >&2
  exit 1
}
if [ -z "$readings" ]; then
  echo "no launch in $file has a measurement line"
  exit 0
fi
printf '%s\n' "$readings" | sort | uniq -c | sort -rn |
  awk '{ count = $1; sub(/^ *[0-9]+ /, ""); print count "\t" $0 }'
