#!/usr/bin/env bash
# Live e2e: drive the real OpenAI Codex CLI through oneharness and assert the
# JSON contract. Auth: an existing codex login, else OPENAI_API_KEY. Model:
# $CODEX_E2E_MODEL (default: the CLI's own default).
set -euo pipefail
# shellcheck source=scripts/e2e-lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/e2e-lib.sh"

note "== oneharness live e2e: codex =="
need jq
# Codex authenticates from its own login (CI makes one from OPENAI_API_KEY with
# `codex login --with-api-key`), so an existing login is auth enough.
if [ ! -f "${CODEX_HOME:-$HOME/.codex}/auth.json" ]; then
    need_env "OpenAI auth" OPENAI_API_KEY
fi

export OH_MODEL="${CODEX_E2E_MODEL:-}"
marker="$(oh_marker)"
oh_run codex "$(oh_prompt "$marker")"
oh_assert_echoed codex "$marker"

# Synced permission rules, honored by Codex started DIRECTLY (not through
# `oneharness run`): `sync` writes .codex/rules/oneharness.rules into a trusted
# scratch project, and a synced deny must refuse a command that ran before it,
# and a synced allow must run a command that was refused before it.
# llmlint: ignore[tool_output_is_signal] Every phase in this script announces itself on one line before it runs, and that header is what attributes a later failure (or a hang) to a phase in the CI log; dropping it here alone would make this one phase the unlabelled one.
note "» synced rules: a directly started codex must honor .codex/rules/oneharness.rules"
oh_codex_rules_enforce
# Model-free: the synced rules match the argv codex checks on every platform —
# on Windows the words it lowers a `pwsh -Command` script into.
# llmlint: ignore[tool_output_is_signal] Every phase in this script announces itself on one line before it runs, and that header is what attributes a later failure to a phase in the CI log.
note "» synced rules: codex's own execpolicy must match them against the POSIX and Windows argv"
oh_codex_rules_match

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

# Resumed turns keep the mode's sandbox (issue #1372): `codex exec resume` has
# no `--sandbox`, so a continued turn carries it as `-c sandbox_mode=`. Two
# `--session` turns per mode; turn two must run, recall turn one, and write
# (auto) or be blocked (read-only / plan) exactly as a fresh turn would.
note "» resume under a sandbox mode: a continued turn must run and keep the sandbox"
oh_resume_mode_enforce codex auto
oh_resume_mode_enforce codex read-only
oh_resume_mode_enforce codex plan

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
# llmlint: ignore[tool_output_is_signal] Every phase in this script announces itself on one line before it runs, and that header is what attributes a later failure (or a hang) to a phase in the CI log; dropping it here alone would make this one phase the unlabelled one.
note "» usage: the zero-turn probe must get an answer out of the real app-server"
oh_usage_enforce codex

# Reasoning enforcement: `--reasoning high` maps to Codex's
# `-c model_reasoning_effort=high` and must round-trip; a bogus effort should be
# rejected (honoring evidence).
note "» reasoning: --reasoning must be accepted (and a bogus effort rejected)"
oh_reasoning_enforce codex high
