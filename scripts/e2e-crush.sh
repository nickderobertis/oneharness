#!/usr/bin/env bash
# Live e2e: drive the real Crush CLI through oneharness and assert the JSON
# contract. Auth: a provider key (ANTHROPIC_API_KEY or OPENAI_API_KEY). Model:
# $CRUSH_E2E_MODEL (default: the CLI's own default for the detected provider).
set -euo pipefail
# shellcheck source=scripts/e2e-lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/e2e-lib.sh"

note "== oneharness live e2e: crush =="
need jq
need_env "Crush provider key" ANTHROPIC_API_KEY OPENAI_API_KEY

export OH_MODEL="${CRUSH_E2E_MODEL:-}"
marker="$(oh_marker)"
oh_run crush "$(oh_prompt "$marker")"
oh_assert_echoed crush "$marker"

# Sync enforcement: crush's permissions are tool-coarse — allowed_tools admits
# the bash tool (crush.json permissions.allowed_tools), denied_tools disables
# it entirely (options.disabled_tools) — so each phase gets its own sandbox.
ok="$(oh_enforce_file ok)"
blocked="$(oh_enforce_file blocked)"
oh_sync_enforce crush "[harness.crush]
allowed_tools = [\"bash\"]" "$ok" present allow
oh_sync_enforce crush "[harness.crush]
denied_tools = [\"bash\"]" "$blocked" absent deny
note "PASS: crush sync enforcement"

note "» hook enforcement: the synced gate must block a marked command"
oh_hook_enforce crush

# Mock enforcement: crush's PreToolUse stdout `updated_input` (a shallow-merge
# patch of the tool input) must substitute the marked command — the live proof
# of the crush-flat mock_rewrite shape, and that the spy log preserves the
# original (pre-rewrite) event.
note "» mock enforcement: the synced mock must rewrite a marked command's input"
oh_mock_enforce crush
