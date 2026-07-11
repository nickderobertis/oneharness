#!/usr/bin/env bash
# Live e2e: drive the real Cursor CLI (`cursor-agent`) through oneharness and
# assert the JSON contract. Auth: CURSOR_API_KEY (generate at
# https://cursor.com/dashboard/api). Model: $CURSOR_E2E_MODEL (default: the
# CLI's own default).
set -euo pipefail
# shellcheck source=scripts/e2e-lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/e2e-lib.sh"

note "== oneharness live e2e: cursor =="
need jq
need_env "Cursor auth" CURSOR_API_KEY

export OH_MODEL="${CURSOR_E2E_MODEL:-}"
marker="$(oh_marker)"
oh_run cursor "$(oh_prompt "$marker")"
oh_assert_echoed cursor "$marker"

# Large prompt + system (issue #1115): oneharness pipes a >128 KiB prompt to
# `cursor-agent -p`'s stdin (positional omitted), with the system prepended — so
# it never trips the argv ceiling. Verified live that cursor reads a stdin-only
# prompt (scripts/explore-cursor-stdin.sh); the marker must still round-trip.
note "» long prompt: a >128 KiB prompt+system must round-trip off the argv"
oh_long_prompt_enforce cursor

# Normalized tool events: Cursor's default `stream-json` carries its own
# `type:"tool_call"` transcript (nested `shellToolCall` payload), so a shell-tool
# turn must surface a normalized `tool_call` via `stream-json:cursor-tool-calls`
# — the live drift alarm for the cursor recognizer (sourced from a real
# cursor-agent transcript). No --events needed: its default already streams.
note "» events: a tool-using turn must surface normalized tool_call events"
oh_events_assert cursor "stream-json:cursor-tool-calls"

# Streaming: Cursor's default stream-json already carries the transcript, so
# --stream needs no --events; events must arrive incrementally, then a result line.
note "» stream: events must arrive incrementally, then a terminal result line"
oh_stream_assert cursor

# Sync enforcement: permissions synced into .cursor/cli.json must govern the
# real CLI without --force. Cursor's documented baseline rule is the command
# base token (Shell(touch)), so allow and deny get their own sandboxes.
ok="$(oh_enforce_file ok)"
blocked="$(oh_enforce_file blocked)"
oh_sync_enforce cursor "[harness.cursor]
allowed_tools = [\"Shell(touch)\"]" "$ok" present allow
oh_sync_enforce cursor "[harness.cursor]
denied_tools = [\"Shell(touch)\"]" "$blocked" absent deny
note "PASS: cursor sync enforcement"

# Hook enforcement is skipped on Windows: cursor-agent mis-runs the hook command
# there — it builds a PowerShell wrapper to pipe the payload, then executes it via
# bash (Git Bash on PATH), so the wrapper dies with a syntax error and cursor
# blocks every command. This is an acknowledged cursor-agent bug with no shell
# flag/config/env lever ($SHELL and $COMSPEC are both ignored); the only known
# workaround is WSL, which doesn't apply to a native-Windows runner. Echo and sync
# enforcement (above) DO work on Windows, and hooks are still proven on
# Linux/macOS. See the README support matrix.
# https://forum.cursor.com/t/agent-cli-on-windows-no-way-to-configure-shell-hardcoded-to-powershell-no-shell-flag-or-config-option/151858
if [ "${OS:-}" = "Windows_NT" ]; then
    note "» hook enforcement: SKIPPED on windows-latest (cursor-agent hook-shell bug; see README)"
    note "» mock enforcement: SKIPPED on windows-latest (same cursor-agent hook-shell bug)"
else
    note "» hook enforcement: the synced gate must block a marked command"
    oh_hook_enforce cursor

    # Mock enforcement: `run --mock-rules` installs .cursor/hooks.json
    # ephemerally (restored afterwards); cursor honors
    # `{"permission":"allow","updated_input":…}` on its `preToolUse` event
    # (probe-verified headlessly 2026-07-06), which the hook binding wires
    # alongside the three before* events.
    note "» mock enforcement: run --mock-rules must rewrite a marked command's input"
    oh_mock_enforce cursor
fi

# Approval-mode enforcement: `read-only` is Cursor's native `--mode ask` and
# `plan` is `--mode plan` — each must block a write that `--mode bypass` allows.
note "» read-only / plan enforcement: each must block a write"
oh_mode_enforce cursor read-only
oh_mode_enforce cursor plan

# Reasoning enforcement: Cursor's effort rides the model id
# (`--model 'MODEL[effort=high]'`), so it needs a model to attach to. A forum
# report says cursor-agent may reject the bracket syntax its own --help
# advertises — so this phase is exactly the honoring proof: if the syntax is not
# accepted, the run fails here. Only run when a model is configured (else there
# is nothing to attach the suffix to); note-and-continue rather than skip().
if [ -n "$OH_MODEL" ]; then
    note "» reasoning: --reasoning must ride the model id and be accepted"
    oh_reasoning_enforce cursor high
else
    note "» reasoning: SKIPPED (cursor effort rides the model id; set CURSOR_E2E_MODEL to test)"
fi
