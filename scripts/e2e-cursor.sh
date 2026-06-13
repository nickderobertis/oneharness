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

note "» hook enforcement: the synced gate must block a marked command"
oh_hook_enforce cursor
