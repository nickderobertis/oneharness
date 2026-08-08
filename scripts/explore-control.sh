#!/usr/bin/env bash
# Investigative turn-control probe (the `--control` capability's drift alarm;
# see AGENTS.md and the control matrix in README.md).
#
# NOT part of the gate. Per harness, it stands up that harness's OWN control
# path directly (bypassing oneharness), drives a real turn that creates one file
# per step, interrupts it mid-flight, and reports whether the work actually
# stopped — so a `ControlShape` is sourced from real behavior, never guessed, and
# a mechanism that silently stops working is caught rather than believed.
#
# The verdict is read off the filesystem on purpose: goose and copilot both
# report a normal `end_turn` after a real cancellation, so a probe that trusted
# the harness's own stop reason would report success for the wrong reason.
#
# Usage: scripts/explore-control.sh <harness-id|all>
# Auth/model come from the environment (same as the e2e jobs). A missing binary
# or a provider refusal is reported as BLOCKED — never as support.
set -uo pipefail

ID="${1:?usage: explore-control.sh <harness-id|all>}"

if ! command -v python3 >/dev/null 2>&1; then
    echo "explore-control: python3 is required to drive the JSON-RPC/HTTP control protocols" >&2
    exit 2
fi

case "$(uname -s 2>/dev/null || echo unknown)" in
MINGW* | MSYS* | CYGWIN*)
    echo "explore-control: control sockets and these stdio/unix-socket protocols are unix-only" >&2
    exit 0
    ;;
esac

exec python3 "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/explore_control.py" "$ID"
