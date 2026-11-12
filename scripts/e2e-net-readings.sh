#!/usr/bin/env bash
# e2e-net-readings.sh FILE — which launches of a live suite the network
# recorder measured in its namespace (`netns-pasta`) and which it ran directly
# (`unavailable`, with why), counted from the suite's telemetry FILE
# (scripts/e2e-netshim.sh writes it): a turn that passes unmeasured would
# otherwise look like one that passed under the recorder. One line,
# `network recorder: COUNT METHOD[: REASON]; ...`, most frequent first, a
# reason's control characters shown as spaces; a missing FILE (the suite
# stopped before its first launch) says so and passes. The live workflows run
# it after their suite, pass or fail.
set -euo pipefail

fail() {
  echo "e2e-net-readings: $*" >&2
  exit 1
}
[ "$#" -eq 1 ] || {
  echo "usage: scripts/e2e-net-readings.sh FILE (a suite's target/e2e-telemetry/network/<suite>.jsonl)" >&2
  exit 2
}
file="$1"
if [ ! -e "$file" ]; then
  echo "network recorder: no telemetry file at $file; the suite recorded no launch"
  exit 0
fi
command -v jq >/dev/null || fail "jq is not on PATH; install it to read $file"
# Start lines carry the launch's argv and no method; each launch's one
# measurement line carries its method, and an unmeasured one its reason.
summary="$(jq -rs '
  [.[] | select(has("method"))
   | if (.method | type) != "string" or ((.reason // "") | type) != "string"
     then error("a measurement line whose method or reason is not a string: \(tojson)")
     else .method + (if .reason then ": " + (.reason | gsub("[[:cntrl:]]"; " ")) else "" end)
     end]
  | group_by(.) | map({reading: .[0], count: length})
  | sort_by(-.count, .reading)
  | if length == 0 then "none of its launches has a measurement line"
    else map("\(.count) \(.reading)") | join("; ") end' "$file" 2>&1)" ||
  fail "cannot read $file ($summary); the recorder writes one whole JSON object per line, so inspect it for another writer"
echo "network recorder: $summary"
