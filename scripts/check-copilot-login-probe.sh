#!/usr/bin/env bash
# Hermetic contract check for `oh_copilot_login_ready`, the credential detector
# the live-control copilot phase gates on.
#
# This detector is the fix for a phase that reported a working harness as
# unauthenticated: copilot's own login is stored by `copilot login` in the OS
# credential store, so a host with no GH_TOKEN/GITHUB_TOKEN in its environment
# still runs copilot fine — and the old env-token gate retired the phase there,
# shipping copilot's control path with no live alarm at all. The whole defect
# was the detection, so the detection is what this pins.
#
# Every case runs with GH_TOKEN, GITHUB_TOKEN, COPILOT_GITHUB_TOKEN and
# COPILOT_E2E_AUTH explicitly unset: a detector that quietly went back to
# reading them would pass its own test on a CI box that has one.
#
# A stub `copilot` speaks the two ACP frames the probe depends on, so the shapes
# here are the ones a real copilot answered (captured live):
#   authenticated: {"jsonrpc":"2.0","id":2,"result":{"sessionId":"…", …}}
#   logged out:    {"jsonrpc":"2.0","id":2,"error":{"code":-32000,"message":"Authentication required"}}
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/e2e-lib.sh
source "$root/scripts/e2e-lib.sh"

fail_check() {
    echo "check-copilot-login-probe: $1" >&2
    echo "  Re-run with: bash -x scripts/check-copilot-login-probe.sh" >&2
    exit 1
}

if ! command -v jq >/dev/null 2>&1; then
    echo "check-copilot-login-probe: skipped (jq is not installed; the probe it checks needs it)"
    exit 0
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# A stand-in copilot that answers `session/new` with whatever OH_STUB_REPLY
# holds — or, when it is empty, never answers at all (the hang case).
write_stub() {
    local path="$1"
    mkdir -p "$(dirname "$path")"
    cat >"$path" <<'STUB'
#!/usr/bin/env bash
[ "${1:-}" = "--acp" ] || exit 64
while IFS= read -r line; do
    case "$line" in
    *'"method":"initialize"'*)
        printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"authMethods":[]}}'
        ;;
    *'"method":"session/new"'*)
        [ -n "${OH_STUB_REPLY:-}" ] && printf '%s\n' "$OH_STUB_REPLY"
        ;;
    esac
done
STUB
    chmod +x "$path"
}

# Run the detector against a stub, with every copilot credential variable
# cleared — the host shape the fix exists for.
probe_with() {
    local reply="$1" bin="$work/bin/copilot"
    write_stub "$bin"
    # shellcheck disable=SC2016  # $1/$2 are the inner shell's arguments, not this one's
    env -u GH_TOKEN -u GITHUB_TOKEN -u COPILOT_GITHUB_TOKEN -u COPILOT_E2E_AUTH \
        OH_STUB_REPLY="$reply" OH_COPILOT_LOGIN_PROBE_SECONDS=5 \
        bash -c 'source "$1"; oh_copilot_login_ready "$2"' _ "$root/scripts/e2e-lib.sh" "$bin" \
        2>/dev/null
}

# 1. A session that opens is a usable login, with no token anywhere in the
#    environment. This is the case the old gate got wrong.
probe_with '{"jsonrpc":"2.0","id":2,"result":{"sessionId":"b321c23a-62cb-4b56-aa69-c831a7fd4efb","models":{"availableModels":[{"modelId":"auto"}]}}}' \
    || fail_check "a copilot that opened an ACP session was reported as having no login"

# 2. The logged-out answer must not read as a login. Otherwise the phase would
#    run and blame the control path for an auth failure.
probe_with '{"jsonrpc":"2.0","id":2,"error":{"code":-32000,"message":"Authentication required"}}' \
    && fail_check "a copilot that refused the session with 'Authentication required' was reported as logged in"

# 3. A result carrying no sessionId is not a session, whatever else it holds.
probe_with '{"jsonrpc":"2.0","id":2,"result":{"models":{"availableModels":[]}}}' \
    && fail_check "a session/new result with no sessionId was reported as a login"

# 3b. Nor is a sessionId that is not a usable one. The frame is a protocol
#     response from another process, so "present" is not the same as "a session
#     the phase could go on to drive".
probe_with '{"jsonrpc":"2.0","id":2,"result":{"sessionId":""}}' \
    && fail_check "an empty sessionId was reported as a login"
probe_with '{"jsonrpc":"2.0","id":2,"result":{"sessionId":{"id":7}}}' \
    && fail_check "a non-string sessionId was reported as a login"

# 4. A copilot that never answers must lose to the probe's own deadline rather
#    than hanging the suite before any phase has run.
started=$SECONDS
probe_with '' && fail_check "a copilot that never answered session/new was reported as logged in"
elapsed=$((SECONDS - started))
[ "$elapsed" -le 20 ] || fail_check "the probe took ${elapsed}s to give up on a silent copilot; it is meant to be bounded"

# 5. No copilot at all is not a login (and must not be an error).
# shellcheck disable=SC2016  # $1/$2 are the inner shell's arguments, not this one's
env -u GH_TOKEN -u GITHUB_TOKEN -u COPILOT_GITHUB_TOKEN -u COPILOT_E2E_AUTH \
    bash -c 'source "$1"; oh_copilot_login_ready "$2"' _ "$root/scripts/e2e-lib.sh" "$work/bin/absent-copilot" \
    2>/dev/null \
    && fail_check "a missing copilot binary was reported as logged in"

# 6. The live-control copilot phase must gate on this detector and NOT on the
#    environment tokens it replaced — that substitution IS the fix, and a revert
#    to the token gate would otherwise re-skip an authenticated host silently.
phase="$(sed -n '/^    copilot)$/,/^        ;;$/p' "$root/scripts/e2e-control.sh")"
[ -n "$phase" ] || fail_check "no copilot phase found in scripts/e2e-control.sh"
printf '%s' "$phase" | grep -q 'oh_copilot_login_ready' \
    || fail_check "the copilot phase in scripts/e2e-control.sh does not gate on oh_copilot_login_ready"
printf '%s' "$phase" | grep -qE '(have_env|need_env).*(GH_TOKEN|GITHUB_TOKEN|COPILOT_E2E_AUTH)' \
    && fail_check "the copilot phase in scripts/e2e-control.sh is gating on an environment token again"

# 7. Polling starts before the agent's transcript exists — the `--acp` redirect
#    only opens it once the probe's fifo has a reader. "Not there yet" is not an
#    answer and is not an error either: a stray `No such file or directory` in a
#    live log is one more thing to rule out when a phase goes wrong.
noise="$(_oh_acp_answer "$work/never-written.jsonl" 2>&1)" && \
    fail_check "a transcript that does not exist yet was read as an answer"
[ -z "$noise" ] || fail_check "polling a not-yet-created transcript printed: $noise"

# 8. A deadline that is not a positive whole number is a broken probe, and a
#    broken probe fails the same way an unauthenticated copilot does — the
#    phase retires for a reason that has nothing to do with copilot. Loud, then.
for bad in "thirty" "5s" "-5" "0"; do
    # shellcheck disable=SC2016  # $1/$2 are the inner shell's arguments, not this one's
    complaint="$(env -u GH_TOKEN -u GITHUB_TOKEN -u COPILOT_GITHUB_TOKEN -u COPILOT_E2E_AUTH \
        OH_COPILOT_LOGIN_PROBE_SECONDS="$bad" \
        bash -c 'source "$1"; oh_copilot_login_ready "$2"' _ "$root/scripts/e2e-lib.sh" "$work/bin/copilot" 2>&1)" \
        && fail_check "OH_COPILOT_LOGIN_PROBE_SECONDS='$bad' was accepted as a deadline"
    printf '%s' "$complaint" | grep -q 'OH_COPILOT_LOGIN_PROBE_SECONDS' \
        || fail_check "OH_COPILOT_LOGIN_PROBE_SECONDS='$bad' was rejected without saying so: $complaint"
done

echo "check-copilot-login-probe: ok"
