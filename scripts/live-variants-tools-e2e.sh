#!/usr/bin/env bash
# Live command-boundary test for the variant CLI installer. This intentionally
# reaches npm and is run only by the credentialed live workflow.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

just live-variants-tools

for binary in claude codex opencode qwen crush; do
    if ! command -v "$binary" >/dev/null 2>&1; then
        echo "live-variants-tools-e2e: $binary is absent after installation; inspect the npm output and global npm bin directory" >&2
        exit 1
    fi
done
