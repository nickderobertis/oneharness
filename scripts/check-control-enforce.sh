#!/usr/bin/env bash
# Hermetic contract check for the mechanism assertion used by live-control.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/e2e-lib.sh
source "$root/scripts/e2e-lib.sh"

fail() {
    echo "check-control-enforce: $1" >&2
    exit 1
}

while read -r harness mechanism; do
    _oh_control_mechanism_matches "$harness" "$mechanism" || \
        fail "$harness did not accept its declared mechanism $mechanism"
    _oh_control_mechanism_matches "$harness" "wrong-$mechanism" && \
        fail "$harness accepted a non-contract mechanism"
done <<'CASES'
claude-code claude-control-request
codex codex-app-server
opencode opencode-http
goose acp-cancel
copilot acp-cancel
crush crush-http
CASES

_oh_control_mechanism_matches cursor-agent claude-control-request && \
    fail "an undeclared harness accepted a mechanism"
_oh_control_mechanism_matches qwen acp-cancel && \
    fail "an undeclared harness accepted a mechanism"

echo "check-control-enforce: ok"
