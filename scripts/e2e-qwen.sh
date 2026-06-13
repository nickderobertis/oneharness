#!/usr/bin/env bash
# Live e2e: drive the real Qwen Code CLI through oneharness and assert the JSON
# contract. Qwen speaks an OpenAI-compatible API: set OPENAI_API_KEY (and
# typically OPENAI_BASE_URL + OPENAI_MODEL for the provider you point it at).
# Model: $QWEN_E2E_MODEL (default: the CLI's own default).
set -euo pipefail
# shellcheck source=scripts/e2e-lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/e2e-lib.sh"

note "== oneharness live e2e: qwen =="
need jq
need_env "Qwen (OpenAI-compatible) auth" OPENAI_API_KEY DASHSCOPE_API_KEY

export OH_MODEL="${QWEN_E2E_MODEL:-}"
marker="$(oh_marker)"
oh_run qwen "$(oh_prompt "$marker")"
oh_assert_echoed qwen "$marker"

# Sync enforcement: qwen's headless mode never auto-approves from
# permissions.allow (observed live: "requires user approval but cannot execute
# in non-interactive mode"), so the synced file carries
# permissions.defaultMode = "yolo" instead — the allow phase passing proves
# the synced file itself governs the run — and the deny rule (documented as
# highest priority: deny > ask > allow) must still override it. Rules use
# qwen's canonical run_shell_command(...) tool name.
ok="$(oh_enforce_file ok)"
blocked="$(oh_enforce_file blocked)"
policy="[harness.qwen]
denied_tools = [\"run_shell_command(touch $blocked)\"]
[harness.qwen.settings.permissions]
defaultMode = \"yolo\""
oh_sync_enforce qwen "$policy" "$ok" present allow
oh_sync_enforce qwen "$policy" "$blocked" absent deny
note "PASS: qwen sync enforcement"
