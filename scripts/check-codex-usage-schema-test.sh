#!/usr/bin/env bash
# Hermetic boundary coverage for check-codex-usage-schema.sh. No real codex is
# invoked and no network is touched: each case puts a scripted `codex` on PATH
# and drives the real gate script end to end.
#
# The gate itself is what stands between an upstream rename and `oneharness
# usage` silently reporting zeros, so its own branches need proving. In
# particular the skip branch: it exits 0, which is indistinguishable from a pass
# in CI output, so a bug that made *every* run skip would otherwise disable the
# gate without anyone noticing.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
gate="$root/scripts/check-codex-usage-schema.sh"
snapshot="$root/tests/fixtures/codex-rate-limits.schema.json"
token_snapshot="$root/tests/fixtures/codex-thread-token-usage.schema.json"
list_snapshot="$root/tests/fixtures/codex-thread-list-params.schema.json"
read_snapshot="$root/tests/fixtures/codex-thread-read-params.schema.json"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

fail() {
  echo "check-codex-usage-schema-test: $1" >&2
  echo "  rerun with 'bash -x scripts/check-codex-usage-schema-test.sh' to inspect the failing case" >&2
  exit 1
}

# A `codex` whose `app-server generate-json-schema --out DIR` behaves as
# `$mode` dictates. Written fresh per case so PATH holds exactly one.
write_fake_codex() {
  local mode="$1" bin="$tmp/bin"
  rm -rf "$bin"
  mkdir -p "$bin"
  cat >"$bin/codex" <<FAKE
#!/usr/bin/env bash
set -euo pipefail
if [ "\${1:-}" = "--version" ]; then
    echo "codex-cli 0.145.0-fake"
    exit 0
fi
# Expect: app-server generate-json-schema --out <dir>
out=""
while [ \$# -gt 0 ]; do
    case "\$1" in
        --out) out="\$2"; shift 2 ;;
        *) shift ;;
    esac
done
# A staging step that fails is the fake's own fault, never a gate verdict:
# exit 3, which no case below expects, naming what could not be written.
staging_failed() {
    echo "fake codex: could not stage \$1 under \$out" >&2
    exit 3
}
# The overlay's two requests, as codex declares them: \$1 the methods
# ClientRequest.json names.
stage_requests() {
    cp "$list_snapshot" "\$out/v2/ThreadListParams.json" || staging_failed ThreadListParams.json
    cp "$read_snapshot" "\$out/v2/ThreadReadParams.json" || staging_failed ThreadReadParams.json
    printf '{"oneOf": [%s]}\\n' "\$1" >"\$out/ClientRequest.json" || staging_failed ClientRequest.json
}
case "$mode" in
    match)
        mkdir -p "\$out/v2" || staging_failed "\$out/v2"
        cp "$snapshot" "\$out/v2/GetAccountRateLimitsResponse.json" || staging_failed GetAccountRateLimitsResponse.json
        cp "$token_snapshot" "\$out/v2/ThreadTokenUsageUpdatedNotification.json" || staging_failed ThreadTokenUsageUpdatedNotification.json
        stage_requests '{"enum": ["thread/list"]}, {"enum": ["thread/read"]}'
        ;;
    request-drift)
        mkdir -p "\$out/v2" || staging_failed "\$out/v2"
        cp "$snapshot" "\$out/v2/GetAccountRateLimitsResponse.json" || staging_failed GetAccountRateLimitsResponse.json
        cp "$token_snapshot" "\$out/v2/ThreadTokenUsageUpdatedNotification.json" || staging_failed ThreadTokenUsageUpdatedNotification.json
        stage_requests '{"enum": ["thread/list"]}, {"enum": ["thread/read"]}'
        sed 's/threadId/thread_id/g' "$read_snapshot" \\
            >"\$out/v2/ThreadReadParams.json" || staging_failed ThreadReadParams.json
        ;;
    requests-missing)
        mkdir -p "\$out/v2" || staging_failed "\$out/v2"
        cp "$snapshot" "\$out/v2/GetAccountRateLimitsResponse.json" || staging_failed GetAccountRateLimitsResponse.json
        cp "$token_snapshot" "\$out/v2/ThreadTokenUsageUpdatedNotification.json" || staging_failed ThreadTokenUsageUpdatedNotification.json
        stage_requests '{"enum": ["thread/list"]}, {"enum": ["thread/read"]}'
        rm "\$out/ClientRequest.json" || staging_failed ClientRequest.json
        ;;
    requests-unreadable)
        mkdir -p "\$out/v2" || staging_failed "\$out/v2"
        cp "$snapshot" "\$out/v2/GetAccountRateLimitsResponse.json" || staging_failed GetAccountRateLimitsResponse.json
        cp "$token_snapshot" "\$out/v2/ThreadTokenUsageUpdatedNotification.json" || staging_failed ThreadTokenUsageUpdatedNotification.json
        stage_requests '{"enum": ["thread/list"]}, {"enum": ["thread/read"]}'
        chmod 000 "\$out/ClientRequest.json" || staging_failed ClientRequest.json
        ;;
    method-missing)
        mkdir -p "\$out/v2" || staging_failed "\$out/v2"
        cp "$snapshot" "\$out/v2/GetAccountRateLimitsResponse.json" || staging_failed GetAccountRateLimitsResponse.json
        cp "$token_snapshot" "\$out/v2/ThreadTokenUsageUpdatedNotification.json" || staging_failed ThreadTokenUsageUpdatedNotification.json
        stage_requests '{"enum": ["thread/list"]}, {"enum": ["thread/fetch"]}'
        ;;
    drift)
        mkdir -p "\$out/v2" || staging_failed "\$out/v2"
        sed 's/usedPercent/pctUsed/g' "$snapshot" \\
            >"\$out/v2/GetAccountRateLimitsResponse.json" || staging_failed GetAccountRateLimitsResponse.json
        cp "$token_snapshot" "\$out/v2/ThreadTokenUsageUpdatedNotification.json" || staging_failed ThreadTokenUsageUpdatedNotification.json
        ;;
    token-drift)
        mkdir -p "\$out/v2" || staging_failed "\$out/v2"
        cp "$snapshot" "\$out/v2/GetAccountRateLimitsResponse.json" || staging_failed GetAccountRateLimitsResponse.json
        sed 's/outputTokens/completionTokens/g' "$token_snapshot" \\
            >"\$out/v2/ThreadTokenUsageUpdatedNotification.json" || staging_failed ThreadTokenUsageUpdatedNotification.json
        ;;
    token-missing)
        mkdir -p "\$out/v2" || staging_failed "\$out/v2"
        cp "$snapshot" "\$out/v2/GetAccountRateLimitsResponse.json" || staging_failed GetAccountRateLimitsResponse.json
        ;;
    missing-output)
        # Succeeds, but emits nothing where the rate-limits response used to be.
        mkdir -p "\$out/v2" || staging_failed "\$out/v2"
        echo '{}' >"\$out/v2/SomethingElse.json" || staging_failed SomethingElse.json
        ;;
    generator-failure)
        echo "codex: app-server: unrecognized subcommand 'generate-json-schema'" >&2
        exit 2
        ;;
esac
FAKE
  chmod +x "$bin/codex"
  printf '%s' "$bin"
}

# Trimmed to `$1` plus the system essentials so the scripted `codex` is the only
# one the gate can resolve, and returns the gate's status instead of aborting.
run_gate() {
  local extra_path="$1" out="$2"
  set +e
  PATH="$extra_path:/usr/bin:/bin" "$gate" >"$out" 2>&1
  local status=$?
  set -e
  printf '%s' "$status"
}

assert_contains() {
  local file="$1" pattern="$2" description="$3"
  grep -qF "$pattern" "$file" || {
    echo "--- captured output ---" >&2
    cat "$file" >&2
    fail "$description"
  }
}

empty="$tmp/empty-bin"
mkdir -p "$empty"
status="$(run_gate "$empty" "$tmp/skip.log")"
[ "$status" = "0" ] || fail "a missing codex must skip (exit 0), got exit $status"
assert_contains "$tmp/skip.log" "skipped" \
  "the skip must announce itself rather than passing silently"
assert_contains "$tmp/skip.log" "codex is not installed" \
  "the skip must name why it skipped"

status="$(run_gate "$(write_fake_codex match)" "$tmp/ok.log")"
[ "$status" = "0" ] || {
  cat "$tmp/ok.log" >&2
  fail "a matching schema must pass, got exit $status"
}
assert_contains "$tmp/ok.log" "check-codex-usage-schema: ok" \
  "a pass must report a pass, not a skip"
grep -qF "skipped" "$tmp/ok.log" &&
  fail "a real run must not report itself as skipped"

status="$(run_gate "$(write_fake_codex generator-failure)" "$tmp/genfail.log")"
[ "$status" = "1" ] || fail "a failing generator must fail the gate, got exit $status"
assert_contains "$tmp/genfail.log" "unrecognized subcommand" \
  "the generator's own error must be preserved"
assert_contains "$tmp/genfail.log" "update this script to the new command" \
  "a generator failure must name a concrete next action"

status="$(run_gate "$(write_fake_codex missing-output)" "$tmp/missing.log")"
[ "$status" = "1" ] || fail "a missing generated file must fail the gate, got exit $status"
assert_contains "$tmp/missing.log" "no longer emits v2/GetAccountRateLimitsResponse.json" \
  "a moved response must be reported as moved, not as a diff"
assert_contains "$tmp/missing.log" "parse_codex_rate_limits" \
  "the failure must point at the parser that depends on the shape"

status="$(run_gate "$(write_fake_codex drift)" "$tmp/drift.log")"
[ "$status" = "1" ] || fail "a drifted schema must fail the gate, got exit $status"
assert_contains "$tmp/drift.log" "pctUsed" \
  "the diff must show what actually changed"
assert_contains "$tmp/drift.log" "the codex rate-limits contract drifted" \
  "drift must be named as drift"
assert_contains "$tmp/drift.log" "would otherwise become a silent zero" \
  "the failure must say why the drift matters"

status="$(run_gate "$(write_fake_codex token-drift)" "$tmp/token-drift.log")"
[ "$status" = "1" ] || fail "a drifted token-usage schema must fail the gate, got exit $status"
assert_contains "$tmp/token-drift.log" "completionTokens" \
  "the token-usage diff must show what actually changed"
assert_contains "$tmp/token-drift.log" "the codex thread token-usage notification contract drifted" \
  "token-usage drift must be named as its own contract's drift"
assert_contains "$tmp/token-drift.log" "absorb_codex_token_usage" \
  "the failure must point at the reader that depends on the shape"

status="$(run_gate "$(write_fake_codex token-missing)" "$tmp/token-missing.log")"
[ "$status" = "1" ] || fail "a missing token-usage schema must fail the gate, got exit $status"
assert_contains "$tmp/token-missing.log" "no longer emits v2/ThreadTokenUsageUpdatedNotification.json" \
  "a moved notification must be reported as moved, not as a diff"

status="$(run_gate "$(write_fake_codex request-drift)" "$tmp/request-drift.log")"
[ "$status" = "1" ] || fail "a drifted thread/read request schema must fail the gate, got exit $status"
assert_contains "$tmp/request-drift.log" "thread_id" \
  "the request diff must show what actually changed"
assert_contains "$tmp/request-drift.log" "the codex thread/read request contract drifted" \
  "request drift must be named as its own contract's drift"
assert_contains "$tmp/request-drift.log" "RunOverlay::repair_threads" \
  "the failure must point at the overlay code that sends the request"

status="$(run_gate "$(write_fake_codex method-missing)" "$tmp/method-missing.log")"
[ "$status" = "1" ] || fail "a request method codex no longer accepts must fail the gate, got exit $status"
assert_contains "$tmp/method-missing.log" "no longer accepts the \`thread/read\` request" \
  "a removed method must be named, not reported as a pass"

status="$(run_gate "$(write_fake_codex requests-missing)" "$tmp/requests-missing.log")"
[ "$status" = "1" ] || fail "a missing request catalog must fail the gate, got exit $status"
assert_contains "$tmp/requests-missing.log" "no longer emits ClientRequest.json" \
  "a moved request catalog must be named as moved, not as a dropped method"
grep -qF "no longer accepts" "$tmp/requests-missing.log" &&
  fail "a missing request catalog must not be reported as a dropped method"

# A catalog grep cannot read keeps grep's own cause. Where a mode cannot deny
# its owner a read (root, Git Bash) there is no such file to make.
probe="$tmp/unreadable-probe"
: >"$probe" || fail "could not create the permission probe $probe; check that the scratch dir $tmp is writable"
chmod 000 "$probe" || fail "could not chmod the permission probe $probe to 000"
if ! cat "$probe" 2>/dev/null; then
  status="$(run_gate "$(write_fake_codex requests-unreadable)" "$tmp/requests-unreadable.log")"
  [ "$status" = "1" ] || fail "an unreadable request catalog must fail the gate, got exit $status"
  assert_contains "$tmp/requests-unreadable.log" "could not read" \
    "an unreadable request catalog must be named as unreadable"
  assert_contains "$tmp/requests-unreadable.log" "Permission denied" \
    "grep's own error must be preserved"
  grep -qF "no longer accepts" "$tmp/requests-unreadable.log" &&
    fail "an unreadable request catalog must not be reported as a dropped method"
fi
chmod 600 "$probe" || fail "could not restore the permission probe $probe to 600, so the scratch dir may not clean up"

# The diff must survive the way `just lint-workflows` invokes the gate — stdout
# suppressed. A drift report whose diff went to /dev/null tells a reader the
# schema moved without saying what moved.
drift_path="$(write_fake_codex drift)"
set +e
PATH="$drift_path:/usr/bin:/bin" "$gate" >/dev/null 2>"$tmp/drift-stderr.log"
status=$?
set -e
[ "$status" = "1" ] || fail "drift must still fail with stdout suppressed, got exit $status"
assert_contains "$tmp/drift-stderr.log" "pctUsed" \
  "the diff must reach stderr, so 'just lint-workflows' still shows what changed"

echo "check-codex-usage-schema-test: ok"
