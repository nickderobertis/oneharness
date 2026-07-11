#!/usr/bin/env bash
# Live e2e: drive the real GitHub Copilot CLI through oneharness and assert the
# JSON contract. Auth: COPILOT_GITHUB_TOKEN (a fine-grained PAT with the
# "Copilot Requests" permission), or an existing `copilot` login. Model:
# $COPILOT_E2E_MODEL (default: the CLI's own default).
set -euo pipefail
# shellcheck source=scripts/e2e-lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/e2e-lib.sh"

note "== oneharness live e2e: copilot =="
need jq
need_env "Copilot auth" COPILOT_GITHUB_TOKEN GH_TOKEN GITHUB_TOKEN

export OH_MODEL="${COPILOT_E2E_MODEL:-}"
marker="$(oh_marker)"
oh_run copilot "$(oh_prompt "$marker")"
oh_assert_echoed copilot "$marker"

# Large prompt + system (issue #1115): oneharness pipes a >128 KiB prompt to
# copilot's stdin (`-p` dropped — a `-p` value makes it ignore the pipe), with
# the system prepended — so it never trips the argv ceiling. The marker must
# still round-trip.
note "» long prompt: a >128 KiB prompt+system must round-trip off the argv"
oh_long_prompt_enforce copilot

# Approval-mode enforcement: `read-only` denies the `shell`/`write` tools (deny
# beats allow-all) and `plan` is `--mode plan` — each must block a write that
# `--mode bypass` allows.
note "» read-only / plan enforcement: each must block a write"
oh_mode_enforce copilot read-only
oh_mode_enforce copilot plan

# Edit enforcement: `--mode edit` allows the `write` tool, so a file edit that
# bare `-p` would auto-deny must apply.
note "» edit enforcement: a file edit is auto-approved under --mode edit"
oh_edit_enforce copilot

# Reasoning enforcement: `--reasoning high` maps to Copilot's `--reasoning-effort`.
# This is the honoring proof that matters most for Copilot — it has a history of
# headless features silently not firing under `-p` (its hooks were probe-refuted),
# so a bogus effort SHOULD be rejected if the flag is really honored.
note "» reasoning: --reasoning must be accepted (and a bogus effort rejected)"
oh_reasoning_enforce copilot high
