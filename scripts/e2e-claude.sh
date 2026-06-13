#!/usr/bin/env bash
# Live e2e: drive the real Claude Code CLI through oneharness and assert the
# JSON contract. Auth: CLAUDE_CODE_OAUTH_TOKEN (mint with `claude setup-token`)
# or ANTHROPIC_API_KEY. Model: $CLAUDE_E2E_MODEL (default: haiku).
set -euo pipefail
# shellcheck source=scripts/e2e-lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/e2e-lib.sh"

note "== oneharness live e2e: claude-code =="
need jq
need_env "Claude auth" CLAUDE_CODE_OAUTH_TOKEN ANTHROPIC_API_KEY

export OH_MODEL="${CLAUDE_E2E_MODEL:-haiku}"
marker="$(oh_marker)"
oh_run claude-code "$(oh_prompt "$marker")"
oh_assert_echoed claude-code "$marker"

# Sync enforcement: a policy synced into .claude/settings.json must govern the
# real CLI under --no-bypass — the allow rule lets the exact command run, the
# deny rule (and headless default-deny) keeps the other from running.
ok="$(oh_enforce_file ok)"
blocked="$(oh_enforce_file blocked)"
policy="[harness.claude-code]
allowed_tools = [\"Bash(touch $ok)\"]
denied_tools = [\"Bash(touch $blocked)\"]"
oh_sync_enforce claude-code "$policy" "$ok" present allow
oh_sync_enforce claude-code "$policy" "$blocked" absent deny
note "PASS: claude-code sync enforcement"
