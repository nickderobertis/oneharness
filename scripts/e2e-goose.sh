#!/usr/bin/env bash
# Live e2e: drive the real Goose CLI through oneharness and assert the JSON
# contract. Goose selects its model from its own environment, not a CLI flag, so
# oneharness does not pass --model here; set GOOSE_PROVIDER / GOOSE_MODEL and the
# matching provider key (e.g. OPENAI_API_KEY) instead.
set -euo pipefail
# shellcheck source=scripts/e2e-lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/e2e-lib.sh"

note "== oneharness live e2e: goose =="
need jq

# Goose reads its provider/model from the environment. Default to OpenAI; allow
# any of Goose's documented provider keys to satisfy the auth preflight.
export GOOSE_PROVIDER="${GOOSE_PROVIDER:-openai}"
export GOOSE_MODEL="${GOOSE_MODEL:-${GOOSE_E2E_MODEL:-gpt-4o-mini}}"
need_env "Goose provider key" OPENAI_API_KEY ANTHROPIC_API_KEY GOOGLE_API_KEY

export OH_MODEL=""  # oneharness intentionally does not map --model for goose
marker="$(oh_marker)"
oh_run goose "$(oh_prompt "$marker")"
oh_assert_echoed goose "$marker"

# Hook enforcement: a synced `[[hooks]]` gate, installed as a Goose plugin, must
# block a marked command and let an unmarked one through (Goose fires PreToolUse
# hooks even under GOOSE_MODE=auto, which oneharness's bypass requests).
note "» hook enforcement: the synced plugin gate must block a marked command"
oh_hook_enforce goose

# Mock enforcement (deny+spy — goose's mock ceiling: its hook protocol has no
# rewrite verdict): `run --mock-rules` installs the plugin pair ephemerally
# (restored afterwards), the deny rule must block the marked command, and the
# spy log must record the original call — the live proof of goose's one-shot
# delivery and of the deny action through `oneharness mock`.
note "» mock enforcement (deny): run --mock-rules must block a marked command"
oh_mock_deny_enforce goose
