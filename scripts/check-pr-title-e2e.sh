#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

PR_TITLE='fix: validate release manifests' scripts/check-pr-title.sh
PR_TITLE='feat(sdk)!: change the public contract' scripts/check-pr-title.sh
if PR_TITLE='misc changes' scripts/check-pr-title.sh > /dev/null 2>&1; then
  echo "check-pr-title-e2e: accepted a non-conventional title" >&2
  exit 1
fi
echo "check-pr-title-e2e: ok"
