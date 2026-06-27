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

# Approval-mode enforcement: `--mode read-only` is Codex's OS-enforced read-only
# sandbox — a write must be blocked that `--mode bypass` allows.
note "» read-only enforcement: --mode read-only (sandbox) must block a write"
oh_mode_enforce codex
