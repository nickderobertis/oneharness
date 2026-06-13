#!/usr/bin/env bash
# Live e2e: drive the real OpenCode CLI through oneharness and assert the JSON
# contract. Auth: a provider key (ANTHROPIC_API_KEY or OPENAI_API_KEY). Model:
# $OPENCODE_E2E_MODEL (default: anthropic/claude-haiku-4-5) — OpenCode needs a
# fully-qualified provider/model id.
set -euo pipefail
# shellcheck source=scripts/e2e-lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/e2e-lib.sh"

note "== oneharness live e2e: opencode =="
need jq
need_env "OpenCode provider key" ANTHROPIC_API_KEY OPENAI_API_KEY

export OH_MODEL="${OPENCODE_E2E_MODEL:-anthropic/claude-haiku-4-5}"
marker="$(oh_marker)"
oh_run opencode "$(oh_prompt "$marker")"
# OpenCode streams JSONL under `--format json`; oneharness must reconstruct the
# answer from its `text` parts, so require that exact extraction method here.
oh_assert_echoed opencode "$marker" "json:opencode-parts"

# Sync enforcement: OpenCode has no list-shaped rules, so the policy goes via
# the raw settings table into opencode.json's permission map. Without
# --dangerously-skip-permissions the deny pattern must block the command, and
# the explicit allow pattern is the positive control.
ok="$(oh_enforce_file ok)"
blocked="$(oh_enforce_file blocked)"
policy="[harness.opencode.settings.permission.bash]
\"touch $ok\" = \"allow\"
\"touch $blocked\" = \"deny\""
oh_sync_enforce opencode "$policy" "$ok" present allow
oh_sync_enforce opencode "$policy" "$blocked" absent deny
note "PASS: opencode sync enforcement"

note "» hook enforcement: the synced plugin gate must block a marked command"
oh_hook_enforce opencode
