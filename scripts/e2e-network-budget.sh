#!/usr/bin/env bash
# Hold a live suite's recorded network telemetry to its budgets, after the suite.
#
#   scripts/e2e-network-budget.sh <suite>
#
# `just live-codex` runs this after scripts/e2e-codex.sh: it waits for the
# recorder to finish any launch still being measured, then runs `onebudgetspec
# check` (the uv workspace's dev tool) over the suite's budgets file, whose
# commands read $OH_E2E_TELEMETRY_DIR/<suite>.jsonl. Its exit status is the
# check's. The recorder measures on Linux alone (live/AGENTS.md), so on any
# other leg — OH_E2E_OS, default `uname -s` — it says so and checks nothing.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

suite="${1:-}"
case "$suite" in
  codex) budgets="$root/live/pr-matrix/harness/codex/budgets.yaml" ;;
  *)
    echo "e2e-network-budget: usage: scripts/e2e-network-budget.sh codex (the one suite with network budgets)" >&2
    exit 2
    ;;
esac

os="${OH_E2E_OS:-$(uname -s)}"
case "$os" in
  Linux*) ;;
  *)
    echo "network budget ($suite): not checked on $os — the recorder measures only on Linux, so this leg's launches carry no bytes (live/AGENTS.md)"
    exit 0
    ;;
esac

dir="${OH_E2E_TELEMETRY_DIR:-$root/target/e2e-telemetry/network}"
case "$dir" in /*) ;; *) dir="$PWD/$dir" ;; esac

# A launch oneharness stopped is still being measured while its namespace
# winds down (at most the 5 s kill grace); its marker goes once its line is in.
wait_seconds="${OH_E2E_PENDING_WAIT:-30}"
case "$wait_seconds" in "" | *[!0-9]*)
  echo "e2e-network-budget: OH_E2E_PENDING_WAIT must be a whole number of seconds, got '$wait_seconds'" >&2
  exit 2
  ;;
esac
waited=0
while [ -n "$(ls -A "$dir/.pending" 2>/dev/null)" ] && [ "$waited" -lt "$((wait_seconds * 5))" ]; do
  sleep 0.2
  waited=$((waited + 1))
done

uv sync --project "$root/python" --frozen --no-install-workspace --quiet || {
  echo "e2e-network-budget: could not install the uv workspace's dev tools (onebudgetspec); run 'just bootstrap', then rerun" >&2
  exit 1
}
OH_E2E_TELEMETRY_DIR="$dir" uv run --project "$root/python" --frozen --no-sync onebudgetspec check "$budgets"
