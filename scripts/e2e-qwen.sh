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

# Sync enforcement: a policy synced into .qwen/settings.json (permissions
# allow/deny) must govern the real CLI without --yolo: the allowed command
# runs, the denied (and unapproved) one does not. Rules use qwen's canonical
# shell tool name run_shell_command(...) — in headless default mode the shell
# tool is excluded entirely unless a rule makes it available, and the Bash()
# alias did not do that (observed live).
ok="$(oh_enforce_file ok)"
blocked="$(oh_enforce_file blocked)"
policy="[harness.qwen]
allowed_tools = [\"run_shell_command(touch $ok)\"]
denied_tools = [\"run_shell_command(touch $blocked)\"]"
oh_sync_enforce qwen "$policy" "$ok" present allow
oh_sync_enforce qwen "$policy" "$blocked" absent deny
note "PASS: qwen sync enforcement"
