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


# --- oh_usage_cwd_enforce ----------------------------------------------------
#
# The sibling phase holds a different distinction: whether the probe's ANSWER
# still depends on the directory it was pointed at (#1279). Only a real Claude
# Code can exercise that live, so its branches are pinned here against a stubbed
# harness whose "session start" is a sleep the driver chooses.

# The stub harness. With no argument to sleep for it returns at once, which is
# the platform-cannot-run-the-fixture case; with one it stands in for a project
# whose session-start work costs that long.
cat >"$tmp/claude" <<'HARNESS'
#!/usr/bin/env bash
[ -n "${FAKE_HOOK_SLEEP:-}" ] && sleep "$FAKE_HOOK_SLEEP"
exit 0
HARNESS
chmod +x "$tmp/claude"

# The stub `oneharness`, which answers both verbs this phase drives: `detect`
# names the harness binary above, and `usage` returns a chosen report — after
# sleeping, when the probe is being made to pay the hook's cost.
cat >"$tmp/oneharness-cwd" <<'STUB'
#!/usr/bin/env bash
case "$1" in
detect)
    if [ -n "${FAKE_HARNESS_BIN:-}" ]; then
        printf '{"detected":[{"id":"claude-code","available":true,"path":"%s"}]}\n' "$FAKE_HARNESS_BIN"
    else
        printf '{"detected":[{"id":"claude-code","available":false,"path":null}]}\n'
    fi
    ;;
usage)
    case " $* " in
    *"/hooked "*)
        [ -n "${FAKE_PROBE_SLEEP:-}" ] && sleep "$FAKE_PROBE_SLEEP"
        printf '%s\n' "${FAKE_HOOKED:-$FAKE_PLAIN}"
        ;;
    *) printf '%s\n' "$FAKE_PLAIN" ;;
    esac
    ;;
esac
exit 0
STUB
chmod +x "$tmp/oneharness-cwd"

# Drive oh_usage_cwd_enforce once, under a throwaway HOME so the workspace-trust
# preparation it runs cannot touch the developer's own ~/.claude.json.
drive_cwd() {
    local harness_bin="$1" hook_sleep="$2" probe_sleep="$3" hooked="$4" plain="$5"
    local home
    home="$(mktemp -d)"
    set +e
    out="$(
        FAKE_HARNESS_BIN="$harness_bin" FAKE_HOOK_SLEEP="$hook_sleep" \
            FAKE_PROBE_SLEEP="$probe_sleep" FAKE_HOOKED="$hooked" FAKE_PLAIN="$plain" \
            HOME="$home" ONEHARNESS_BIN="$tmp/oneharness-cwd" \
            bash -c "set -euo pipefail; source '$root/scripts/e2e-lib.sh'
                     OH_USAGE_HOOK_MARGIN=2
                     oh_usage_cwd_enforce claude-code" 2>&1
    )"
    rc=$?
    set -e
    rm -rf "$home"
}

reading() {
    printf '{"identities":[{"harness":"claude-code","plan":"%s","auth_mode":"subscription","selector":{"kind":"env_path","env":"CLAUDE_CONFIG_DIR","path":"/h/.claude"},"availability":{"state":"available","windows":[{"id":"five_hour"}]}}]}' "$1"
}

# 6. Absent harness: nothing to point at a directory, so the phase steps aside.
drive_cwd "" "" "" "$(reading max)" "$(reading max)"
[ "$rc" -eq 0 ] || fail "an absent harness must exit 0 for the cwd phase, got $rc: $out"
case "$out" in
*"SKIP:"*"not installed"*"nothing to probe"*) ;;
*) fail "an absent harness must skip with a stated reason, got: $out" ;;
esac

# 7. A fixture that costs nothing: the platform could not run the hook command,
#    so there is no working-directory dependence to be free of and a PASS here
#    would be a pass nobody established.
drive_cwd "$tmp/claude" "" "" "$(reading max)" "$(reading max)"
[ "$rc" -eq 0 ] || fail "an unfired fixture must exit 0, got $rc: $out"
case "$out" in
*"SKIP:"*"did not cost anything here"*) ;;
*) fail "an unfired fixture must skip naming what did not happen, got: $out" ;;
esac

# 8. The regression itself: the probe pays the directory's session-start cost.
#    This is the whole reason the phase exists, so it must be a hard failure that
#    names the issue and says what to check.
drive_cwd "$tmp/claude" 3 3 "$(reading max)" "$(reading max)"
[ "$rc" -eq 1 ] || fail "a probe that waited out the hook must fail, got exit $rc: $out"
case "$out" in
*"Next, in order:"*"setting-sources"*"FAIL:"*"depends on its working directory"*) ;;
*) fail "the regression must fail with its next actions, got: $out" ;;
esac

# 9. The fix in place: the probe answers without paying, and answers the same
#    thing it answers where nothing is registered.
drive_cwd "$tmp/claude" 3 "" "$(reading max)" "$(reading max)"
[ "$rc" -eq 0 ] || fail "an independent probe must pass, got exit $rc: $out"
case "$out" in
*"PASS:"*"independent of its working directory"*) ;;
*) fail "an independent probe must log its readings, got: $out" ;;
esac

# 10. Fast but different: dropping the directory's settings must change what the
#     probe WAITS ON, never what it reports. A phase that only timed the probe
#     would go green on a flag that silently changed the answer.
drive_cwd "$tmp/claude" 3 "" "$(reading pro)" "$(reading max)"
[ "$rc" -eq 1 ] || fail "a changed reading must fail even when fast, got exit $rc: $out"
case "$out" in
*"FAIL:"*"different identity from the hooked directory"*) ;;
*) fail "a changed reading must say what differed, got: $out" ;;
esac

echo "check-usage-enforce: ok"
