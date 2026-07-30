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

# Large prompt + system (issue #1115): oneharness pipes a >128 KiB prompt to
# `codex exec -` (stdin sentinel), with the system prepended into that stream —
# so it never trips the argv ceiling. The marker must still round-trip.
note "» long prompt: a >128 KiB prompt+system must round-trip off the argv"
oh_long_prompt_enforce codex

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

# Usage enforcement: the app-server answers `account/rateLimits/read`
# asynchronously and shuts down on stdin EOF, so the probe holds stdin open until
# its answer lands. Whatever that answer is — headroom on a ChatGPT login, an
# auth error under this suite's API key — it must be an answer.
note "» usage: the zero-turn probe must get an answer out of the real app-server"
oh_usage_enforce codex

# Reasoning enforcement: `--reasoning high` maps to Codex's
# `-c model_reasoning_effort=high` and must round-trip; a bogus effort should be
# rejected (honoring evidence).
note "» reasoning: --reasoning must be accepted (and a bogus effort rejected)"
oh_reasoning_enforce codex high
