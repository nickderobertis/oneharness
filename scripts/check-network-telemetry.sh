#!/usr/bin/env bash
# The network-telemetry project's test step: every hermetic test of the live
# tier's network recorder and of live-codex's network budgets
# (tools/network-telemetry/test_*.py). Run through scripts/shell-test.sh, so the
# bash the tests drive — the shim, e2e-lib.sh's helpers, the suites and the
# budget check step — counts toward the shell coverage floor.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
exec bash scripts/check-temp-leaks.sh uv run --project python --frozen --no-sync python -I -m unittest \
  discover -s tools/network-telemetry -t tools/network-telemetry -p 'test_*.py' "$@"
