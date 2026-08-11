#!/usr/bin/env bash
#
# Keep `docs/sdk-parity.md` in step with what it audits.
#
# The document's capability and flag rows are the capability manifest formatted,
# and its Python/TypeScript columns are read from each client's own source — so
# an out-of-date file is an audit that says a method exists when it does not, or
# omits a flag someone added. Regenerate with `just parity-audit`.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

if ! command -v node >/dev/null 2>&1; then
  echo "check-parity-audit: node not found; the parity audit gate needs Node" >&2
  exit 1
fi

node scripts/parity-audit.mjs --check
echo "check-parity-audit: docs/sdk-parity.md matches the capability manifest"
