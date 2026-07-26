#!/usr/bin/env bash
# Live identity/variant drift alarm. Secret values are never printed.
set -euo pipefail
# shellcheck source=scripts/e2e-lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/e2e-lib.sh"
need jq
need opencode
need qwen
need crush
need goose
if [ -z "${OH_E2E_VARIANTS_EXTENDED_ONLY:-}" ]; then
    need claude
    need codex
fi
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
evidence() {
    # Detailed proof is opt-in for an operator collecting sanitized evidence;
    # ordinary local/CI output remains the final one-line summary.
    [ -n "${OH_E2E_EVIDENCE:-}" ] && printf '%s\n' "$*"
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
mkdir -p "$tmp/qwen-home" "$tmp/crush-home" "$tmp/goose-root"
claude_a="${OH_CLAUDE_CONFIG_A:-$HOME/.claude}"
claude_b="${OH_CLAUDE_CONFIG_B:-$HOME/.claude-alt}"
opencode_bin="$(command -v opencode)"
opencode_wrapper="$tmp/opencode-isolation-wrapper"
cat >"$opencode_wrapper" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [ -n "${ANTHROPIC_API_KEY:-}" ] && [ -z "${OPENAI_API_KEY:-}" ]; then
    printf 'api_key=present ambient_openai=masked\n' >"$OH_VARIANT_ISOLATION_EVIDENCE"
else
    printf 'api_key_or_masking_check=failed\n' >"$OH_VARIANT_ISOLATION_EVIDENCE"
    exit 97
fi
exec "$OH_REAL_OPENCODE" "$@"
EOF
chmod 700 "$opencode_wrapper"
cat >"$config" <<EOF
[harness.opencode.variant.apikey]
bin = "$opencode_wrapper"
model = "anthropic/claude-haiku-4-5"
unset_env = ["OPENAI_API_KEY"]
[harness.opencode.variant.apikey.env_from]
ANTHROPIC_API_KEY = "OH_VARIANT_ANTHROPIC_KEY"
OH_REAL_OPENCODE = "OH_VARIANT_REAL_OPENCODE"
OH_VARIANT_ISOLATION_EVIDENCE = "OH_VARIANT_ISOLATION_EVIDENCE_FILE"

[harness.qwen.variant.apikey]
model = "gpt-4o-mini"
[harness.qwen.variant.apikey.env]
HOME = "$tmp/qwen-home"
OPENAI_BASE_URL = "https://api.openai.com/v1"
OPENAI_MODEL = "gpt-4o-mini"
[harness.qwen.variant.apikey.env_from]
OPENAI_API_KEY = "OH_VARIANT_OPENAI_KEY"

[harness.crush.variant.apikey]
model = "anthropic/claude-haiku-4-5-20251001"
[harness.crush.variant.apikey.env]
HOME = "$tmp/crush-home"
[harness.crush.variant.apikey.env_from]
ANTHROPIC_API_KEY = "OH_VARIANT_ANTHROPIC_KEY"

[harness.goose.variant.apikey.env]
GOOSE_PROVIDER = "openai"
GOOSE_MODEL = "gpt-4o-mini"
GOOSE_PATH_ROOT = "$tmp/goose-root"
[harness.goose.variant.apikey.env_from]
OPENAI_API_KEY = "OH_VARIANT_OPENAI_KEY"
EOF
cat >>"$config" <<'EOF'
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
isolation_file="$tmp/opencode-isolation.txt"
run_marker opencode:apikey "${marker}_oc" \
    OPENAI_API_KEY="$OPENAI_MATERIAL" \
    OH_VARIANT_ANTHROPIC_KEY="$ANTHROPIC_MATERIAL" \
    OH_VARIANT_REAL_OPENCODE="$opencode_bin" \
    OH_VARIANT_ISOLATION_EVIDENCE_FILE="$isolation_file"
[ "$(cat "$isolation_file")" = "api_key=present ambient_openai=masked" ] ||
    fail "OpenCode variant child did not receive only its selected API credential; inspect unset_env/env_from in the generated config with OH_E2E_EVIDENCE=1"
jq -e '.results[0].stdout | split("\n") | map(select(length > 0) | fromjson) |
    any(.type == "step_finish" and .part.cost > 0)' \
    "$tmp/opencode-apikey.json" >/dev/null ||
    fail "OpenCode API identity evidence missing; rerun with OH_E2E_EVIDENCE=1 and verify OpenCode still emits step_finish.part.cost"
evidence "IDENTITY opencode:apikey: provider=anthropic model=claude-haiku-4-5 api_key=present ambient_openai=masked completed_step_cost>0"

run_marker qwen:apikey "${marker}_qw" OH_VARIANT_OPENAI_KEY="$OPENAI_MATERIAL"
evidence "IDENTITY qwen:apikey: provider=openai base_url=api.openai.com model=gpt-4o-mini isolated_home=yes"

run_marker crush:apikey "${marker}_cr" OH_VARIANT_ANTHROPIC_KEY="$ANTHROPIC_MATERIAL"
evidence "IDENTITY crush:apikey: provider=anthropic model=claude-haiku-4-5-20251001 isolated_home=yes"

run_marker goose:apikey "${marker}_go" OH_VARIANT_OPENAI_KEY="$OPENAI_MATERIAL"
jq -e '.results[0].stdout | contains("new session · openai gpt-4o-mini")' \
    "$tmp/goose-apikey.json" >/dev/null ||
    fail "Goose API identity evidence missing; rerun with OH_E2E_EVIDENCE=1 and update the asserted banner only after confirming provider/model identity"
evidence "IDENTITY goose:apikey: session_banner='openai gpt-4o-mini' isolated_path_root=yes"

# These exact, sanitized evidence records are the executable drift gate for the
# auth reference and README matrix: changed live identity facts must update both.
for expected in \
    "IDENTITY opencode:apikey: provider=anthropic model=claude-haiku-4-5 api_key=present ambient_openai=masked completed_step_cost>0" \
    "IDENTITY qwen:apikey: provider=openai base_url=api.openai.com model=gpt-4o-mini isolated_home=yes" \
    "IDENTITY crush:apikey: provider=anthropic model=claude-haiku-4-5-20251001 isolated_home=yes" \
    "IDENTITY goose:apikey: session_banner='openai gpt-4o-mini' isolated_path_root=yes"; do
    grep -Fq "$expected" "$OH_REPO_ROOT/docs/harness-auth.md" ||
        fail "docs/harness-auth.md is stale; copy the sanitized live evidence record for ${expected%%:*}"
done
# Backticks below are literal Markdown delimiters, not shell substitutions.
# shellcheck disable=SC2016
for expected in \
    '`opencode` | OpenCode | `opencode` | `ANTHROPIC_API_KEY` (live-proven)' \
    '`goose` | Goose | `goose` | `GOOSE_PROVIDER` + `OPENAI_API_KEY` (live-proven)' \
    '`qwen` | Qwen Code | `qwen` | `OPENAI_API_KEY` + base URL (live-proven)' \
    '`crush` | Crush | `crush` | `ANTHROPIC_API_KEY` (live-proven)'; do
    grep -Fq "$expected" "$OH_REPO_ROOT/README.md" ||
        fail "README support matrix is stale for ${expected%% *}; update it from docs/harness-auth.md"
done

fallback="$tmp/fallback.json"
fallback_target="claude-code:apikey"
if [ -n "${OH_E2E_VARIANTS_EXTENDED_ONLY:-}" ]; then
    :
else
    run_marker claude-code:apikey "${marker}_ck" OH_VARIANT_ANTHROPIC_KEY="$ANTHROPIC_MATERIAL"
    run_marker codex:apikey "${marker}_ok" OH_VARIANT_CODEX_KEY="$OPENAI_MATERIAL" \
        OH_VARIANT_CODEX_API_HOME="$tmp/codex-api-home"
    jq -e '.results[0].stdout | fromjson |
        .usage.cache_creation.ephemeral_5m_input_tokens > 0' \
        "$tmp/claude-code-apikey.json" >/dev/null ||
        fail "Claude API-key cache evidence missing; verify the current Claude CLI still reports ephemeral_5m_input_tokens for API billing"
    evidence "IDENTITY claude-code:apikey: isolated API key source; ephemeral_5m_input_tokens>0"
fi
if [ -z "${OH_E2E_VARIANTS_API_ONLY:-}" ] && [ -z "${OH_E2E_VARIANTS_EXTENDED_ONLY:-}" ]; then
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
if [ -z "${OH_E2E_VARIANTS_EXTENDED_ONLY:-}" ]; then
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
fi
if [ -n "${OH_E2E_CODEX_SUBSCRIPTION:-}" ] && [ -z "${OH_E2E_VARIANTS_EXTENDED_ONLY:-}" ]; then
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
if [ -z "${OH_E2E_VARIANTS_EXTENDED_ONLY:-}" ]; then
    evidence "IDENTITY codex:apikey: empty CODEX_HOME plus per-process CODEX_API_KEY (sourced from OpenAI API auth material)"
fi
if [ -n "${OH_E2E_VARIANTS_EXTENDED_ONLY:-}" ]; then
    note "live variants: ok (extended adapter API identity and isolation evidence)"
elif [ -n "${OH_E2E_VARIANTS_API_ONLY:-}" ]; then
    note "live variants: ok (API identity evidence and fallback)"
else
    note "live variants: ok (API, subscription, masking, identity evidence, fallback)"
fi
