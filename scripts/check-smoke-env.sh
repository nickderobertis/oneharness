#!/usr/bin/env bash
# Exercise smoke's public recipe with polluted config and binary overrides.
set -euo pipefail

cd "$(dirname "$0")/.."

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
if ! just smoke >"$work/recipe-out" 2>&1; then
  cat "$work/recipe-out" >&2
  echo "check-smoke-env: 'just smoke' did not scrub its planted undeclared harness override; fix scripts/smoke.sh and rerun 'bash scripts/check-smoke-env.sh'" >&2
  exit 1
fi

if ONEHARNESS_BIN="$work/not-executable" scripts/smoke.sh >"$work/out" 2>&1; then
  echo "check-smoke-env: invalid ONEHARNESS_BIN unexpectedly passed; fix scripts/smoke.sh executable validation and rerun 'bash scripts/check-smoke-env.sh'" >&2
  exit 1
fi
if ! grep -Fq 'ONEHARNESS_BIN is not an executable file' "$work/out"; then
  cat "$work/out" >&2
  echo "check-smoke-env: invalid ONEHARNESS_BIN lacked an actionable diagnostic; fix scripts/smoke.sh and rerun 'bash scripts/check-smoke-env.sh'" >&2
  exit 1
fi

echo "check-smoke-env: ok"
