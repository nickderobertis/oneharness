#!/usr/bin/env bash
#
# Hold every CLI capability to a typed method in every language SDK.
#
# The sibling `sdk-check` / `python-sdk-check` gates compare schemas and types,
# so they can only catch a method that DRIFTED. This one catches the method that
# was never written — the failure that let five verbs ship with no SDK
# counterpart at all.
#
# Quiet on success, one line. On failure `sdk-coverage.mjs` names each missing
# method and the file to add it to.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

if ! command -v node >/dev/null 2>&1; then
  echo "check-sdk-coverage: node not found; the SDK coverage gate needs Node" >&2
  exit 1
fi

node scripts/sdk-coverage.mjs
