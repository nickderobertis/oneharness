#!/usr/bin/env bash
# Run the local llmlint tier, gracefully skipping unavailable model infrastructure.
set -euo pipefail

base=${1:?usage: local-llmlint-gate.sh <remote/base>}
root=$(cd "$(dirname "$0")/.." && pwd)
export PATH="$HOME/.local/bin:$PATH"

primary_harness=$(sed -nE 's/^harnesses = \["([a-z0-9][a-z0-9-]*)".*$/\1/p' "$root/oneharness.toml")
if [[ -z $primary_harness ]]; then
  echo "llmlint: cannot determine the primary harness from $root/oneharness.toml; fix its harnesses = [\"<primary>\", ...] entry" >&2
  exit 1
fi

if ! command -v llmlint >/dev/null 2>&1; then
  echo "llmlint: skipped locally (llmlint unavailable; run 'just setup-llmlint')" >&2
  exit 0
fi

llmlint validate --diff-base "$base"

if [[ -z ${OPENAI_API_KEY:-} ]]; then
  echo "llmlint: judge skipped locally (OPENAI_API_KEY unavailable)" >&2
  exit 0
fi
if ! command -v "$primary_harness" >/dev/null 2>&1; then
  echo "llmlint: judge skipped locally (committed primary harness '$primary_harness' unavailable)" >&2
  exit 0
fi

printenv OPENAI_API_KEY | "$primary_harness" login --with-api-key >/dev/null
llmlint --diff --diff-base "$base"
