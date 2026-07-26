#!/usr/bin/env bash
# Live identity/variant drift alarm. Secret values are never printed.
set -euo pipefail
# The runtime path is anchored to this script; this directive gives ShellCheck
# the equivalent repository-relative source for cross-file analysis.
# shellcheck source=scripts/e2e-lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/e2e-lib.sh"
need jq
need claude
need codex
OH="$(oh_bin)"
[ -n "$OH" ] || skip "oneharness binary not found; run 'just live-variants' or set ONEHARNESS_BIN"
AUTH_FILE="${OH_LIVE_AUTH_FILE:-$HOME/.config/oneharness/live-auth.env}"
if [ -f "$AUTH_FILE" ]; then
    mode="$(stat -c %a "$AUTH_FILE" 2>/dev/null || stat -f %Lp "$AUTH_FILE")"
    [ "$mode" = 600 ] || fail "$AUTH_FILE must have mode 0600; run: chmod 600 '$AUTH_FILE'"
    invalid_line="$(
        awk '
            /^[[:space:]]*($|#)/ { next }
            /^[A-Za-z_][A-Za-z0-9_]*=/ { next }
            { print NR; exit }
        ' "$AUTH_FILE"
    )"
    [ -z "$invalid_line" ] ||
        fail "$AUTH_FILE has invalid KEY=VALUE syntax at line $invalid_line; fix or comment that line without printing its secret value"
fi
auth_value() {
    local name="$1"
    [ -f "$AUTH_FILE" ] || return 1
    awk -F= -v key="$name" 'index($0, "=") > 0 && $1 == key { sub(/^[^=]*=/, ""); print; exit }' "$AUTH_FILE"
}
EVIDENCE_FILE="${OH_E2E_EVIDENCE_FILE:-}"
if [ -n "$EVIDENCE_FILE" ]; then
    case "$EVIDENCE_FILE" in
        *$'\n'*) fail "OH_E2E_EVIDENCE_FILE must be a single-line file path; choose a path without newline characters and retry" ;;
    esac
    evidence_dir="$(dirname -- "$EVIDENCE_FILE")"
    if [ -e "$EVIDENCE_FILE" ]; then
        [ -f "$EVIDENCE_FILE" ] ||
            fail "OH_E2E_EVIDENCE_FILE must name a regular file; remove the non-file target or choose a new file path"
        [ -w "$EVIDENCE_FILE" ] ||
            fail "OH_E2E_EVIDENCE_FILE must name a writable file; run chmod u+w on it or choose another path"
    else
        if [ ! -d "$evidence_dir" ] || [ ! -w "$evidence_dir" ]; then
            fail "OH_E2E_EVIDENCE_FILE must have a writable parent directory; create it with mkdir -p and grant the current user write access"
        fi
    fi
fi
evidence() {
    if [ -n "$EVIDENCE_FILE" ]; then
        printf '%s\n' "$*" >>"$EVIDENCE_FILE"
    fi
    if [ -n "${OH_E2E_EVIDENCE:-}" ]; then
        printf '%s\n' "$*"
    fi
    return 0
}
ANTHROPIC_MATERIAL="${ANTHROPIC_API_KEY:-$(auth_value ANTHROPIC_API_KEY || true)}"
OPENAI_MATERIAL="${CODEX_API_KEY:-${OPENAI_API_KEY:-$(auth_value OPENAI_API_KEY || true)}}"
[ -n "$ANTHROPIC_MATERIAL" ] ||
    skip "Anthropic API key unavailable; set ANTHROPIC_API_KEY or add it to OH_LIVE_AUTH_FILE"
[ -n "$OPENAI_MATERIAL" ] ||
    skip "OpenAI API key unavailable; set CODEX_API_KEY/OPENAI_API_KEY or add OPENAI_API_KEY to OH_LIVE_AUTH_FILE"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
config="$tmp/oneharness.toml"
mkdir -p "$tmp/codex-api-home"
claude_a="${OH_CLAUDE_CONFIG_A:-$HOME/.claude}"
claude_b="${OH_CLAUDE_CONFIG_B:-$HOME/.claude-alt}"
validate_claude_config_dir() {
    local name="$1" value="$2"
    case "$value" in
        "" | *$'\n'*)
            fail "$name must be a non-empty single-line directory path; unset it to use the default or set it to one Claude config home"
            ;;
    esac
    [ -d "$value" ] ||
        fail "$name must name an existing directory; create/authenticate that Claude config home or correct the override"
}
cat >"$config" <<'EOF'
[harness.claude-code.variant.subscription-a]
unset_env = ["ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"]
[harness.claude-code.variant.subscription-a.env_from]
CLAUDE_CONFIG_DIR = "OH_VARIANT_CLAUDE_A"
[harness.claude-code.variant.subscription-b]
unset_env = ["ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"]
[harness.claude-code.variant.subscription-b.env_from]
CLAUDE_CONFIG_DIR = "OH_VARIANT_CLAUDE_B"
[harness.claude-code.variant.apikey.env_from]
ANTHROPIC_API_KEY = "OH_VARIANT_ANTHROPIC_KEY"
[harness.claude-code.variant.invalid]
env = { ANTHROPIC_API_KEY = "deliberately-invalid" }
unset_env = ["CLAUDE_CODE_OAUTH_TOKEN"]
[harness.codex.variant.apikey.env_from]
CODEX_API_KEY = "OH_VARIANT_CODEX_KEY"
CODEX_HOME = "OH_VARIANT_CODEX_API_HOME"
[harness.codex.variant.subscription]
unset_env = ["CODEX_API_KEY", "OPENAI_API_KEY"]
[harness.codex.variant.subscription.env_from]
CODEX_HOME = "OH_VARIANT_CODEX_SUBSCRIPTION_HOME"
EOF
run_marker() {
    local id="$1" marker="$2"
    local report="$tmp/${id//:/-}.json"
    local command_stderr="$tmp/${id//:/-}.stderr"
    shift 2
    if env "$@" "$OH" run --config "$config" --harness "$id" \
        --prompt "Reply exactly $marker" --compact >"$report" 2>"$command_stderr"; then
        :
    else
        run_exit=$?
        if jq -e '.results[0]' "$report" >/dev/null 2>&1; then
            diagnostic="$(jq -r \
                '.results[0] | "status=\(.status) exit_code=\(.exit_code) failure_kind=\(.failure_kind) stderr_present=\((.stderr // "") | length > 0)"' \
                "$report")"
            fail "$id exited nonzero ($diagnostic); verify that variant's selected credential source"
        else
            stderr_tail="$(
                tail -5 "$command_stderr" |
                    sed -E \
                        -e 's/(sk-ant-|sk-proj-|sk-)[A-Za-z0-9_-]+/<redacted>/g' \
                        -e 's/(Bearer )[A-Za-z0-9._-]+/\1<redacted>/g'
            )"
            fail "$id exited $run_exit before producing a valid report; stderr tail: ${stderr_tail:-<empty>}; verify the harness installation, auth source, and config"
        fi
    fi
    jq -e --arg marker "$marker" \
        '.results[0].status == "ok" and (.results[0].stdout | contains($marker))' \
        "$report" >/dev/null || {
        diagnostic="$(jq -r --arg marker "$marker" \
            '.results[0] | "status=\(.status) exit_code=\(.exit_code) failure_kind=\(.failure_kind) stderr_present=\((.stderr // "") | length > 0) marker_present=\((.stdout // "") | contains($marker))"' \
            "$report")"
        fail "$id did not complete with marker ($diagnostic); verify that variant's selected credential source"
    }
    evidence "COMMAND $id: oneharness run --config <temporary> --harness $id --prompt 'Reply exactly <marker>' --compact"
    evidence "ASSERT $id: status=ok marker=exact harness_id=$id"
}
marker="OH_VARIANT_$(date +%s)_$RANDOM"
run_marker claude-code:apikey "${marker}_ck" OH_VARIANT_ANTHROPIC_KEY="$ANTHROPIC_MATERIAL"
run_marker codex:apikey "${marker}_ok" OH_VARIANT_CODEX_KEY="$OPENAI_MATERIAL" \
    OH_VARIANT_CODEX_API_HOME="$tmp/codex-api-home"
jq -e '.results[0].stdout | fromjson |
    .usage.cache_creation.ephemeral_5m_input_tokens > 0' \
    "$tmp/claude-code-apikey.json" >/dev/null ||
    fail "Claude API-key cache evidence missing; verify the current Claude CLI still reports ephemeral_5m_input_tokens for API billing"
evidence "IDENTITY claude-code:apikey: isolated API key source; ephemeral_5m_input_tokens>0"
fallback="$tmp/fallback.json"
fallback_target="claude-code:apikey"
if [ -z "${OH_E2E_VARIANTS_API_ONLY:-}" ]; then
    validate_claude_config_dir OH_CLAUDE_CONFIG_A "$claude_a"
    validate_claude_config_dir OH_CLAUDE_CONFIG_B "$claude_b"
    for config_dir in "$claude_a" "$claude_b"; do
        auth_method="$(
            CLAUDE_CONFIG_DIR="$config_dir" env -u ANTHROPIC_API_KEY -u CLAUDE_CODE_OAUTH_TOKEN \
                claude auth status --json |
                jq -er 'select(.loggedIn == true) | .authMethod'
        )" ||
            fail "Claude subscription preflight failed; run 'CLAUDE_CONFIG_DIR=<dir> claude auth login' for the missing identity"
        [ "$auth_method" = "claude.ai" ] ||
            fail "Claude subscription preflight used authMethod='$auth_method', expected 'claude.ai'; remove API auth from that config home and run 'claude auth login'"
    done
    run_marker claude-code:subscription-a "${marker}_ca" \
        ANTHROPIC_API_KEY="$ANTHROPIC_MATERIAL" OH_VARIANT_CLAUDE_A="$claude_a"
    run_marker claude-code:subscription-b "${marker}_cb" \
        ANTHROPIC_API_KEY="$ANTHROPIC_MATERIAL" OH_VARIANT_CLAUDE_B="$claude_b"
    jq -e '.results[0].stdout | fromjson |
        .usage.cache_creation.ephemeral_1h_input_tokens > 0' \
        "$tmp/claude-code-subscription-a.json" >/dev/null ||
        fail "Claude subscription A lacked subscription cache evidence; run 'CLAUDE_CONFIG_DIR=<subscription-a-dir> claude auth status --json', confirm a Max subscription, then rerun"
    evidence "IDENTITY claude-code:subscription-a: authMethod=claude.ai alternate_config=no ambient_api_key=present child_api_key=masked ephemeral_1h_input_tokens>0"
    jq -e '.results[0].stdout | fromjson |
        .usage.cache_creation.ephemeral_1h_input_tokens > 0' \
        "$tmp/claude-code-subscription-b.json" >/dev/null ||
        fail "Claude subscription B lacked subscription cache evidence; run 'CLAUDE_CONFIG_DIR=<subscription-b-dir> claude auth status --json', confirm a Max subscription, then rerun"
    evidence "IDENTITY claude-code:subscription-b: authMethod=claude.ai alternate_config=yes ambient_api_key=present child_api_key=masked ephemeral_1h_input_tokens>0"
    fallback_target="claude-code:subscription-a"
fi
OH_VARIANT_ANTHROPIC_KEY="$ANTHROPIC_MATERIAL" OH_VARIANT_CLAUDE_A="$claude_a" \
    "$OH" run --config "$config" \
    --harness claude-code:invalid --harness "$fallback_target" \
    --run-mode fallback --prompt "Reply exactly ${marker}_fb" --compact >"$fallback"
jq -e --arg marker "${marker}_fb" \
    '.results[0].failure_kind == "auth" and
     (.results[-1].stdout | contains($marker))' "$fallback" >/dev/null || {
    diagnostic="$(jq -r --arg marker "${marker}_fb" \
        '"first_status=\(.results[0].status) first_failure_kind=\(.results[0].failure_kind) next_harness_id=\(.results[-1].harness_id) next_status=\(.results[-1].status) marker_present=\((.results[-1].stdout // "") | contains($marker))"' \
        "$fallback")"
    fail "same-harness auth fallback failed ($diagnostic); verify invalid-key classification and candidate ordering"
}
evidence "COMMAND fallback: oneharness run --config <temporary> --harness claude-code:invalid --harness $fallback_target --run-mode fallback --prompt 'Reply exactly <marker>' --compact"
evidence "ASSERT fallback: first_failure_kind=auth next_harness_id=$fallback_target status=ok marker=exact"
if [ -n "${OH_E2E_CODEX_SUBSCRIPTION:-}" ]; then
    codex_login_status="$(
        env -u CODEX_API_KEY -u OPENAI_API_KEY codex login status 2>&1
    )" ||
        fail "Codex ChatGPT login preflight failed with status: $codex_login_status"
    printf '%s\n' "$codex_login_status" | grep -q "Logged in using ChatGPT" ||
        fail "Codex ChatGPT login unavailable; observed status: $codex_login_status; run 'CODEX_HOME=<host-login-home> codex login' interactively"
    run_marker codex:subscription "${marker}_cs" \
        OH_VARIANT_CODEX_SUBSCRIPTION_HOME="$HOME/.codex"
    evidence "IDENTITY codex:subscription: login_status='Logged in using ChatGPT' CODEX_HOME=host-login API keys masked"
else
    :
fi
evidence "IDENTITY codex:apikey: empty CODEX_HOME plus per-process CODEX_API_KEY (sourced from OpenAI API auth material)"
if [ -n "${OH_E2E_VARIANTS_API_ONLY:-}" ]; then
    note "live variants: ok (API identity evidence and fallback)"
else
    note "live variants: ok (API, subscription, masking, identity evidence, fallback)"
fi
