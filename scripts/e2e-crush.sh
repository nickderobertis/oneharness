#!/usr/bin/env bash
# Live e2e: drive the real Crush CLI through oneharness and assert the JSON
# contract. Auth: a provider key (ANTHROPIC_API_KEY or OPENAI_API_KEY). Model:
# $CRUSH_E2E_MODEL (default: the CLI's own default for the detected provider).
set -euo pipefail
# shellcheck source=scripts/e2e-lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/e2e-lib.sh"

note "== oneharness live e2e: crush =="
need jq
need_env "Crush provider key" ANTHROPIC_API_KEY OPENAI_API_KEY

export OH_MODEL="${CRUSH_E2E_MODEL:-}"
marker="$(oh_marker)"
oh_run crush "$(oh_prompt "$marker")"
oh_assert_echoed crush "$marker"
