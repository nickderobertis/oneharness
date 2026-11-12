#!/usr/bin/env bash
# Hermetic check for scripts/chat-capture-endpoint.py's Anthropic Messages
# route, driven over HTTP the way claude reaches it in
# oh_claude_multiline_system_enforce (scripts/e2e-lib.sh): every shape `system`
# may take is recorded to system.jsonl as the text the harness sent, an invalid
# one is refused unrecorded, a request it cannot record is answered with an
# error rather than a reply, and both the whole and the streamed reply carry
# the marker back.
set -euo pipefail

fail() {
  echo "check-chat-capture-endpoint: $*; rerun bash -x scripts/check-chat-capture-endpoint.sh" >&2
  exit 1
}
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)" || fail "cannot resolve repository root; run this script from a readable checkout"
py="$(command -v python3 || command -v python)" || fail "neither python3 nor python is on PATH; the endpoint is a Python script"
command -v curl >/dev/null || fail "curl is not on PATH; install it to drive the endpoint over HTTP"
command -v jq >/dev/null || fail "jq is not on PATH; install it to read the endpoint's records"
tmp="$(mktemp -d "${TMPDIR:-/tmp}/tmp.XXXXXXXXXX")" || fail "cannot create scratch space; check TMPDIR and free disk space"
pid=""
cleanup() {
  [ -z "$pid" ] || kill "$pid" 2>/dev/null || true
  [ -z "$pid" ] || wait "$pid" 2>/dev/null || true
  rm -rf "$tmp" || echo "check-chat-capture-endpoint: remove abandoned scratch directory $tmp by hand" >&2
}
trap cleanup EXIT
dir="$tmp/endpoint"
mkdir "$dir" || fail "cannot create $dir; check permissions and disk space"
"$py" "$root/scripts/chat-capture-endpoint.py" "$dir" 2>"$tmp/endpoint.log" &
pid=$!
for ((tries = 0; tries < 100; tries++)); do
  [ ! -s "$dir/port" ] || break
  kill -0 "$pid" 2>/dev/null || fail "the endpoint exited before reporting its port: $(cat "$tmp/endpoint.log")"
  sleep 0.1
done
port="$(cat "$dir/port" 2>/dev/null)" || fail "the endpoint never wrote its port within 10s: $(cat "$tmp/endpoint.log")"
url="http://127.0.0.1:$port/v1/messages?beta=true"

# post BODY: the HTTP status, with the response body left in $tmp/response.
post() {
  curl -sS --max-time 10 -o "$tmp/response" -w '%{http_code}' -H 'Content-Type: application/json' \
    --data-binary "$1" "$url" || fail "could not reach the endpoint at $url"
}
# request SYSTEM STREAM: a Messages body whose `system` is the JSON SYSTEM
# (`absent` leaves it out) and whose user turn carries a marker.
request() {
  jq -cn --arg system "$1" --argjson stream "$2" '
    {model: "capture", max_tokens: 8, stream: $stream,
     messages: [{role: "user", content: [{type: "text", text: "say ONEHARNESS-LIVE-42"}]}]}
    + (if $system == "absent" then {} else {system: ($system | fromjson)} end)'
}
# last_system: the newest system.jsonl record's text list, as compact JSON.
last_system() {
  tail -n 1 "$dir/system.jsonl" | jq -c .system ||
    fail "cannot read the last record of $dir/system.jsonl: $(cat "$dir/system.jsonl")"
}
expect_recorded() { # SYSTEM EXPECTED
  local status got
  status="$(post "$(request "$1" false)")"
  [ "$status" = 200 ] || fail "system $1 was answered $status: $(cat "$tmp/response")"
  got="$(last_system)"
  [ "$got" = "$2" ] || fail "system $1 was recorded as $got, not $2"
}

expect_recorded absent '[]'
expect_recorded '"one\ntwo\r\nthree"' '["one\ntwo\r\nthree"]'
expect_recorded '[{"type":"text","text":"first"},{"type":"image","source":{}},{"type":"text","text":"line 1\nline 2"},"stray"]' \
  '["first","line 1\nline 2"]'
grep -qxF 'line 2' "$dir/requests.txt" || fail "requests.txt does not hold the system text: $(cat "$dir/requests.txt")"

# The whole reply (no stream): one Messages response carrying the marker.
jq -e '.type == "message" and .role == "assistant" and .stop_reason == "end_turn"
  and .stop_sequence == null and .content == [{type: "text", text: "ONEHARNESS-LIVE-42"}]
  and .usage.output_tokens == 1' "$tmp/response" >/dev/null ||
  fail "the whole Messages reply is not one marker-carrying message: $(cat "$tmp/response")"

# The streamed reply: Anthropic's event sequence, the marker in its one delta.
status="$(post "$(request '"streamed"' true)")"
[ "$status" = 200 ] || fail "a streamed request was answered $status: $(cat "$tmp/response")"
events="$(sed -n 's/^event: //p' "$tmp/response" | tr '\n' ' ')" ||
  fail "cannot read the stream's event names from $tmp/response"
[ "$events" = 'message_start content_block_start content_block_delta content_block_stop message_delta message_stop ' ] ||
  fail "the stream's events are $events"
sed -n 's/^data: //p' "$tmp/response" | jq -se '
  (map(.type) | join(" ")) == "message_start content_block_start content_block_delta content_block_stop message_delta message_stop"
  and .[0].message.content == [] and .[0].message.stop_reason == null
  and .[2].delta == {type: "text_delta", text: "ONEHARNESS-LIVE-42"}
  and .[4].delta.stop_reason == "end_turn"' >/dev/null ||
  fail "the stream's data does not carry the marker in Anthropic's shape: $(cat "$tmp/response")"

# An invalid system is refused, and nothing of it is recorded.
before="$(wc -l <"$dir/system.jsonl")" || fail "cannot count the records in $dir/system.jsonl"
status="$(post "$(request 5 false)")"
[ "$status" = 400 ] || fail "a numeric system was answered $status, not 400: $(cat "$tmp/response")"
grep -qF 'system is neither text nor a list of blocks' "$tmp/response" || fail "the 400 does not say why: $(cat "$tmp/response")"
after="$(wc -l <"$dir/system.jsonl")" || fail "cannot count the records in $dir/system.jsonl"
[ "$after" = "$before" ] || fail "a refused request was recorded"

# A system record it cannot write fails the request rather than answering it.
mv "$dir/system.jsonl" "$tmp/system.jsonl" || fail "cannot move $dir/system.jsonl aside"
mkdir "$dir/system.jsonl" || fail "cannot create $dir/system.jsonl as a directory"
status="$(post "$(request '"unrecordable"' false)")"
[ "$status" = 500 ] || fail "an unrecordable request was answered $status, not 500: $(cat "$tmp/response")"
grep -qF 'system.jsonl' "$tmp/response" || fail "the 500 does not name the file it could not write: $(cat "$tmp/response")"
grep -qF "could not append to $dir/system.jsonl" "$tmp/endpoint.log" ||
  fail "the endpoint's stderr does not name the file it could not write: $(cat "$tmp/endpoint.log")"
echo 'check-chat-capture-endpoint: ok'
