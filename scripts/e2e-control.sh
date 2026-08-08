#!/usr/bin/env bash
# Live e2e: out-of-band turn control (`run --control` + `oneharness interrupt`).
#
# Drives a REAL harness through a turn that produces one file per step,
# interrupts it from a separate process mid-flight, and proves the file count is
# frozen for 15 seconds afterwards. The filesystem is the assertion on purpose:
# some harnesses report a normal `end_turn` after a real cancellation, so a
# check that trusted the harness's own stop reason would pass for the wrong
# reason.
#
# This is a per-FEATURE live suite (like `e2e-schema.sh`), deliberately separate
# from the per-harness `e2e-<id>.sh` scripts: each phase drives a real
# multi-step turn and then waits out a 15-second freeze window, which is far too
# slow to add to a suite that runs on every PR. Run it on demand
# (`just live-control`) and after any change to the control path.
#
# One phase per harness that DECLARES a control mechanism, read from
# `oneharness list` — so a newly declared harness is exercised here without
# editing this script, and a harness whose capability was never proven cannot
# quietly skip.
#
# Auth and model mirror the per-harness scripts. A missing CLI or auth is a
# SKIP, never a failure.
#
# llmlint: ignore-file[tool_output_is_signal] Every live e2e script in this repo
# announces its phases (see e2e-claude.sh, e2e-schema.sh): the transcript is what
# attributes a later failure — or a hang inside a 15s freeze window — to a phase
# in a CI log where nobody can attach a debugger. Silence here would make a
# timeout indistinguishable from a harness that never started.
set -euo pipefail
# shellcheck source=scripts/e2e-lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/e2e-lib.sh"

note "== oneharness live e2e: out-of-band turn control (--control / interrupt) =="
need jq

case "$(uname -s 2>/dev/null || echo unknown)" in
MINGW* | MSYS* | CYGWIN*)
    note "control sockets are unix-only; nothing to verify on Windows"
    exit 0
    ;;
esac

BIN="$(oh_bin)"
[ -n "$BIN" ] || skip "oneharness binary not found (build it: \`just build-release\`, or set ONEHARNESS_BIN)"

# The capability matrix is the script's input: every harness declaring a
# mechanism must prove it here.
mapfile -t CONTROLLABLE < <(ONEHARNESS_NO_CONFIG=1 "$BIN" list --compact \
    | jq -r '.harnesses[] | select(.control != null) | .id')

if [ "${#CONTROLLABLE[@]}" -eq 0 ]; then
    fail "no harness declares a control mechanism, so this suite would prove nothing"
fi
note "harnesses declaring turn control: ${CONTROLLABLE[*]}"

for id in "${CONTROLLABLE[@]}"; do
    case "$id" in
    claude-code)
        need_env "Claude auth" CLAUDE_CODE_OAUTH_TOKEN ANTHROPIC_API_KEY
        export OH_MODEL="${CLAUDE_E2E_MODEL:-haiku}"
        ;;
    codex)
        need_env "Codex auth" CODEX_E2E_AUTH OPENAI_API_KEY
        export OH_MODEL="${CODEX_E2E_MODEL:-}"
        ;;
    opencode)
        need_env "OpenCode auth" OPENCODE_E2E_AUTH ANTHROPIC_API_KEY
        export OH_MODEL="${OPENCODE_E2E_MODEL:-}"
        ;;
    goose)
        need_env "Goose auth" GOOSE_E2E_AUTH ANTHROPIC_API_KEY
        export OH_MODEL="${GOOSE_E2E_MODEL:-}"
        ;;
    crush)
        need_env "Crush auth" CRUSH_E2E_AUTH ANTHROPIC_API_KEY
        export OH_MODEL="${CRUSH_E2E_MODEL:-}"
        ;;
    copilot)
        need_env "Copilot auth" COPILOT_E2E_AUTH GH_TOKEN GITHUB_TOKEN
        export OH_MODEL="${COPILOT_E2E_MODEL:-}"
        ;;
    *)
        export OH_MODEL="${OH_MODEL:-}"
        ;;
    esac
    note "» $id: a real turn must actually STOP when interrupted"
    oh_control_enforce "$id"
done

note "PASS: turn control honored by every harness that declares it"
