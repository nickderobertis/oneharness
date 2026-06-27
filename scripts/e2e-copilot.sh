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

# Approval-mode enforcement: `--mode read-only` denies the `shell`/`write` tools
# (deny beats allow-all) — a write must be blocked that `--mode bypass` allows.
note "» read-only enforcement: --mode read-only (--deny-tool) must block a write"
oh_mode_enforce copilot
