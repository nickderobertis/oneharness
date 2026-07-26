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

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
failure="$tmp/npm-failure.log"
invalid_prefix="$tmp/not-a-directory"
: >"$invalid_prefix"
if npm_config_prefix="$invalid_prefix" \
    just live-variants-tools >"$failure" 2>&1; then
    echo "live-variants-tools-e2e: npm unexpectedly installed into a regular-file prefix; verify the failure-path test isolation" >&2
    exit 1
fi
if ! grep -Fq "rerun 'just live-variants-tools'" "$failure"; then
    echo "live-variants-tools-e2e: failure output omitted the retry action; inspect the live-variants-tools error branch" >&2
    exit 1
fi
if ! grep -Fq "run 'npm config get registry'" "$failure"; then
    echo "live-variants-tools-e2e: failure output omitted the registry diagnostic; inspect the live-variants-tools error branch" >&2
    exit 1
fi
