#!/usr/bin/env bash
# Live identity/variant drift alarm. Secret values are never printed.
set -euo pipefail
# shellcheck source=scripts/e2e-lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/e2e-lib.sh"
need jq
need claude
need codex
OH="$(oh_bin)"
[ -n "$OH" ] || skip "oneharness binary not found"
AUTH_FILE="${OH_LIVE_AUTH_FILE:-$HOME/.config/oneharness/live-auth.env}"
if [ -f "$AUTH_FILE" ]; then
    mode="$(stat -c %a "$AUTH_FILE" 2>/dev/null || stat -f %Lp "$AUTH_FILE")"
    [ "$mode" = 600 ] || fail "$AUTH_FILE must have mode 0600"
fi
auth_value() {
    local name="$1"
    [ -f "$AUTH_FILE" ] || return 1
    awk -F= -v key="$name" '$1 == key { sub(/^[^=]*=/, ""); print; exit }' "$AUTH_FILE"
}
ANTHROPIC_MATERIAL="${ANTHROPIC_API_KEY:-$(auth_value ANTHROPIC_API_KEY || true)}"
OPENAI_MATERIAL="${CODEX_API_KEY:-${OPENAI_API_KEY:-$(auth_value OPENAI_API_KEY || true)}}"
[ -n "$ANTHROPIC_MATERIAL" ] || skip "Anthropic API key unavailable"
[ -n "$OPENAI_MATERIAL" ] || skip "OpenAI API key unavailable"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
config="$tmp/oneharness.toml"
mkdir -p "$tmp/codex-api-home"
claude_a="${OH_CLAUDE_CONFIG_A:-$HOME/.claude}"
claude_b="${OH_CLAUDE_CONFIG_B:-$HOME/.claude-alt}"
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
    shift 2
    env "$@" "$OH" run --config "$config" --harness "$id" \
        --prompt "Reply exactly $marker" --compact >"$report"
    jq -e --arg marker "$marker" \
        '.results[0].status == "ok" and (.results[0].stdout | contains($marker))' \
        "$report" >/dev/null || fail "$id did not complete with marker"
}
marker="OH_VARIANT_$(date +%s)_$RANDOM"
run_marker claude-code:apikey "${marker}_ck" OH_VARIANT_ANTHROPIC_KEY="$ANTHROPIC_MATERIAL"
run_marker codex:apikey "${marker}_ok" OH_VARIANT_CODEX_KEY="$OPENAI_MATERIAL" \
    OH_VARIANT_CODEX_API_HOME="$tmp/codex-api-home"
jq -e '.results[0].stdout | fromjson |
    .usage.cache_creation.ephemeral_5m_input_tokens > 0' \
    "$tmp/claude-code-apikey.json" >/dev/null ||
    fail "Claude API-key cache evidence missing; verify the current Claude CLI still reports ephemeral_5m_input_tokens for API billing"
fallback="$tmp/fallback.json"
fallback_target="claude-code:apikey"
if [ -z "${OH_E2E_VARIANTS_API_ONLY:-}" ]; then
    for config_dir in "$claude_a" "$claude_b"; do
        CLAUDE_CONFIG_DIR="$config_dir" env -u ANTHROPIC_API_KEY -u CLAUDE_CODE_OAUTH_TOKEN \
            claude auth status --json |
            jq -e '.loggedIn == true and .authMethod == "claude.ai"' >/dev/null ||
            fail "Claude subscription preflight failed"
    done
    run_marker claude-code:subscription-a "${marker}_ca" \
        ANTHROPIC_API_KEY="$ANTHROPIC_MATERIAL" OH_VARIANT_CLAUDE_A="$claude_a"
    run_marker claude-code:subscription-b "${marker}_cb" \
        ANTHROPIC_API_KEY="$ANTHROPIC_MATERIAL" OH_VARIANT_CLAUDE_B="$claude_b"
    jq -e '.results[0].stdout | fromjson |
        .usage.cache_creation.ephemeral_1h_input_tokens > 0' \
        "$tmp/claude-code-subscription-a.json" >/dev/null ||
        fail "Claude subscription A lacked subscription cache evidence"
    jq -e '.results[0].stdout | fromjson |
        .usage.cache_creation.ephemeral_1h_input_tokens > 0' \
        "$tmp/claude-code-subscription-b.json" >/dev/null ||
        fail "Claude subscription B lacked subscription cache evidence"
    fallback_target="claude-code:subscription-a"
fi
OH_VARIANT_ANTHROPIC_KEY="$ANTHROPIC_MATERIAL" OH_VARIANT_CLAUDE_A="$claude_a" \
    "$OH" run --config "$config" \
    --harness claude-code:invalid --harness "$fallback_target" \
    --run-mode fallback --prompt "Reply exactly ${marker}_fb" --compact >"$fallback"
jq -e --arg marker "${marker}_fb" \
    '.results[0].failure_kind == "auth" and
     (.results[-1].stdout | contains($marker))' "$fallback" >/dev/null ||
    fail "same-harness auth fallback failed; inspect failure_kind for the invalid candidate and fallback ordering in the report"
if [ -n "${OH_E2E_CODEX_SUBSCRIPTION:-}" ]; then
    env -u CODEX_API_KEY -u OPENAI_API_KEY codex login status 2>&1 |
        grep -q "Logged in using ChatGPT" || fail "Codex ChatGPT login unavailable"
    run_marker codex:subscription "${marker}_cs" \
        OH_VARIANT_CODEX_SUBSCRIPTION_HOME="$HOME/.codex"
else
    note "Codex subscription phase not requested (set OH_E2E_CODEX_SUBSCRIPTION=1)"
fi
note "live variants: ok (API, subscription, masking, identity evidence, fallback)"
