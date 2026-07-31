#!/usr/bin/env bash
# Hermetic behavioral check for oh_usage_enforce (scripts/e2e-lib.sh).
#
# That helper is the live drift alarm for the zero-turn `usage` probe, and its
# whole value is in which report it lets pass. The distinction it has to hold is
# between a harness that is ABSENT (nothing to probe — skip) and one that is
# INSTALLED BUT SILENT (the drift it exists to catch — fail). A helper that
# collapsed those would either go green on a box with no harness or turn every
# such box red. Only a live harness can exercise it in e2e, so the branches are
# pinned here against a stubbed `oneharness` that emits a chosen report.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if ! command -v jq >/dev/null 2>&1; then
    echo "check-usage-enforce: skipped (jq is not installed; oh_usage_enforce parses its report with it)"
    exit 0
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# The stub stands in for the real binary at the one boundary oh_usage_enforce
# uses: `oneharness usage` writing a report to stdout, diagnostics to stderr, and
# an exit code. Everything else in the helper is exercised for real.
cat >"$tmp/oneharness" <<'STUB'
#!/usr/bin/env bash
[ -n "${FAKE_STDOUT:-}" ] && printf '%s\n' "$FAKE_STDOUT"
[ -n "${FAKE_STDERR:-}" ] && printf '%s\n' "$FAKE_STDERR" >&2
exit "${FAKE_EXIT:-0}"
STUB
chmod +x "$tmp/oneharness"

fail() {
    echo "check-usage-enforce: $1" >&2
    echo "  Re-run the failing case on its own to see the helper's whole output:" >&2
    echo "    bash -x scripts/check-usage-enforce.sh" >&2
    exit 1
}

# Drive oh_usage_enforce once against the stubbed report, in a subshell because
# its skip/fail paths exit. Captures stdout+stderr in $out and the code in $rc.
drive() {
    local stdout="$1" exit_code="$2" no_skip="${3:-}"
    set +e
    out="$(
        FAKE_STDOUT="$stdout" FAKE_EXIT="$exit_code" \
            FAKE_STDERR="codex: the app-server went away" \
            ONEHARNESS_BIN="$tmp/oneharness" OH_E2E_NO_SKIP="$no_skip" \
            bash -c "set -euo pipefail; source '$root/scripts/e2e-lib.sh'; oh_usage_enforce codex" 2>&1
    )"
    rc=$?
    set -e
}

identity() {
    printf '{"schema_version":"0.1","identities":[{"harness":"codex","availability":%s}]}' "$1"
}

# 1. Absent: nothing to probe, so the phase steps aside rather than reporting
#    drift — the developer-box stance the library documents.
drive "$(identity '{"state":"unknown","reason":{"kind":"binary_missing","bin":"codex"}}')" 0
[ "$rc" -eq 0 ] || fail "an absent harness must exit 0, got $rc: $out"
case "$out" in
*"SKIP:"*"not installed"*"nothing to probe"*) ;;
*) fail "an absent harness must skip with a stated reason, got: $out" ;;
esac

# 2. Installed but silent: exactly the drift this phase exists to catch. It must
#    stay a hard failure, and say what to do about it.
drive "$(identity '{"state":"unknown","reason":{"kind":"probe_failed","message":"no answer"}}')" 0
[ "$rc" -eq 1 ] || fail "an unanswered probe must fail, got exit $rc: $out"
case "$out" in
*"Next, in order:"*"HoldUntilAnswered"*"FAIL:"*"no answer out of the harness"*) ;;
*) fail "an unanswered probe must fail with its next actions, got: $out" ;;
esac

# 3. The same absence, in CI. Every e2e workflow installs its harness up front,
#    so a skip there means install/detection broke and the job would go green
#    having probed nothing.
drive "$(identity '{"state":"unknown","reason":{"kind":"binary_missing","bin":"codex"}}')" 0 1
[ "$rc" -eq 1 ] || fail "OH_E2E_NO_SKIP must turn the absence into a failure, got exit $rc: $out"

# 4. No report at all: the helper cannot read a state, so it reports the exit
#    code and what to do next rather than only the symptom.
drive "" 2
[ "$rc" -eq 1 ] || fail "an empty report must fail, got exit $rc: $out"
case "$out" in
*"exited 2"*"Next, in order:"*"usage/config error"*"FAIL:"*"produced no report"*) ;;
*) fail "an empty report must name the exit code and a next action, got: $out" ;;
esac

# 5. An answer — either flavour, since CI's API-key identity honestly reports
#    `unavailable` while a subscription box reports windows — passes and logs the
#    reading, which is the phase's only evidence that it ran.
drive "$(identity '{"state":"unavailable","reason":"api_key_auth"}')" 0
[ "$rc" -eq 0 ] || fail "an answered probe must pass, got exit $rc: $out"
case "$out" in
*"PASS:"*"unavailable (api_key_auth)"*) ;;
*) fail "an answered probe must log the reading it got, got: $out" ;;
esac

drive "$(identity '{"state":"available","windows":[{"id":"codex","usage":{"used_percent":31}}]}')" 0
[ "$rc" -eq 0 ] || fail "a reported headroom must pass, got exit $rc: $out"
case "$out" in
*"PASS:"*"headroom codex 31"*) ;;
*) fail "a reported headroom must be logged, got: $out" ;;
esac

echo "check-usage-enforce: ok"
