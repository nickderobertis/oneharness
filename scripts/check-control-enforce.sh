#!/usr/bin/env bash
# Hermetic contract check for the mechanism assertion used by live-control.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/e2e-lib.sh
source "$root/scripts/e2e-lib.sh"

fail() {
    echo "check-control-enforce: $1" >&2
    echo "  Re-run with: bash -x scripts/check-control-enforce.sh" >&2
    exit 1
}

_oh_control_mechanism_matches "mechanism-from-frame" "mechanism-from-registry" && \
    fail "a response carrying a different mechanism was accepted"
_oh_control_mechanism_matches "mechanism-from-registry" "mechanism-from-registry" || \
    fail "a response carrying the registry mechanism was rejected"
_oh_control_mechanism_matches "mechanism-from-frame" "" && \
    fail "an absent registry mechanism was accepted"

echo "check-control-enforce: ok"
