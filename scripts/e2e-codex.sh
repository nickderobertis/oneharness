#!/usr/bin/env bash
# Live e2e: drive the real OpenAI Codex CLI through oneharness and assert the
# JSON contract. Auth: OPENAI_API_KEY. Model: $CODEX_E2E_MODEL (default: the
# CLI's own default).
set -euo pipefail
# shellcheck source=scripts/e2e-lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/e2e-lib.sh"

note "== oneharness live e2e: codex =="
need jq
need_env "OpenAI auth" OPENAI_API_KEY

export OH_MODEL="${CODEX_E2E_MODEL:-}"
marker="$(oh_marker)"
oh_run codex "$(oh_prompt "$marker")"
oh_assert_echoed codex "$marker"

# Approval-mode enforcement: `read-only` is Codex's OS-enforced read-only
# sandbox, and `plan` is that same sandbox plus a prepended plan instruction —
# each must block a write that `--mode bypass` allows.
note "» read-only / plan enforcement: each must block a write"
oh_mode_enforce codex read-only
oh_mode_enforce codex plan

# Mock enforcement: codex's hooks engine loads project .codex/hooks.json under
# `exec` and honors the claude-nested `updatedInput` rewrite — but only when
# the invocation opts in with `-c features.hooks=true` plus the per-run hook
# trust bypass (probe-verified 2026-07-06; the `projects.<dir>.trust_level`
# config route loads no hooks). `run --mock-rules` appends those flags itself
# and restores the created .codex/hooks.json afterwards.
note "» mock enforcement: run --mock-rules must rewrite a marked command's input"
oh_mock_enforce codex

# Normalized tool events: Codex's default text has no transcript, so `--events`
# upgrades it to `exec --json`, whose `command_execution` items normalize to a
# `tool_call` via `json:codex-items` — the live drift alarm for the codex
# recognizer (sourced from a real `codex exec --json` transcript).
note "» events: a tool-using turn must surface normalized tool_call events (--events)"
oh_events_assert codex "json:codex-items" --events

# Streaming: the same events must arrive incrementally under --stream (with
# --events selecting exec --json), then a terminal result line.
note "» stream: events must arrive incrementally, then a terminal result line"
oh_stream_assert codex --events
