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
