#!/usr/bin/env bash
# Live e2e: drive the real Crush CLI through oneharness and assert the JSON
# contract. Auth: a provider key (ANTHROPIC_API_KEY or OPENAI_API_KEY). Model:
# $CRUSH_E2E_MODEL (default: the CLI's own default for the detected provider).
set -euo pipefail
# shellcheck source=scripts/e2e-lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/e2e-lib.sh"

note "== oneharness live e2e: crush =="
need jq
# Every harness launch below is measured into target/e2e-telemetry/network/crush.jsonl.
oh_telemetry_setup crush
need_env "Crush provider key" ANTHROPIC_API_KEY OPENAI_API_KEY

export OH_MODEL="${CRUSH_E2E_MODEL:-}"
marker="$(oh_marker)"
oh_run crush "$(oh_prompt "$marker")"
oh_assert_echoed crush "$marker"

# The run's text is read back from crush's own session store when crush exits
# before printing its last deltas, so the store reader is held to the real CLI
# here: the marker must be in `.text`, and the store must have been read.
note "» session store: the run must be readable back from crush's store"
case "$(oh_field '.results[0].text_source')" in
  raw | crush-session) ;;
  *)
    oh_dump
    fail "crush: text_source=$(oh_field '.results[0].text_source') — oneharness could not read the run back from crush's session store; compare \`crush session list --json\` / \`crush session show <id> --json\` with domain::crush_store and re-capture tests/fixtures/crush-*-session-*.json"
    ;;
esac
if ! printf '%s' "$OH_REPORT" | jq -e --arg m "$marker" '(.results[0].text // "") | contains($m)' >/dev/null; then
  oh_dump
  fail "crush: the marker is absent from .text (only in stdout) — the reply was cut short and not recovered from the session store"
fi
note "PASS: crush session store read back"

# Large prompt + system (issue #1115): oneharness pipes a >128 KiB prompt to
# `crush run`'s stdin (positional omitted), with the system prepended — so it
# never trips the argv ceiling. The marker must still round-trip.
note "» long prompt: a >128 KiB prompt+system must round-trip off the argv"
oh_long_prompt_enforce crush

# Sync enforcement: crush's permissions are tool-coarse — allowed_tools admits
# the bash tool (crush.json permissions.allowed_tools), denied_tools disables
# it entirely (options.disabled_tools) — so each phase gets its own sandbox.
ok="$(oh_enforce_file ok)"
blocked="$(oh_enforce_file blocked)"
oh_sync_enforce crush "[sync.harness.crush]
allowed_tools = [\"bash\"]" "$ok" present allow
oh_sync_enforce crush "[sync.harness.crush]
denied_tools = [\"bash\"]" "$blocked" absent deny
note "PASS: crush sync enforcement"

note "» hook enforcement: the synced gate must block a marked command"
oh_hook_enforce crush

# Mock enforcement: `run --mock-rules` installs the crush hook ephemerally
# (restored afterwards) and its PreToolUse stdout `updated_input` (a
# shallow-merge patch of the tool input) must substitute the marked command —
# the live proof of the crush-flat mock_rewrite shape, and that the spy log
# preserves the original (pre-rewrite) event.
note "» mock enforcement: run --mock-rules must rewrite a marked command's input"
oh_mock_enforce crush
