#!/usr/bin/env bash
# Run the local llmlint tier, gracefully skipping unavailable model infrastructure.
set -euo pipefail

if [[ $# -ne 1 || -z $1 ]]; then
  echo "usage: local-llmlint-gate.sh <remote/base>" >&2
  exit 2
fi
base=$1
if [[ $base != */* ]] || ! git check-ref-format --allow-onelevel "refs/remotes/$base" >/dev/null 2>&1; then
  echo "local-llmlint-gate: '$base' is not a valid remote/base ref" >&2
  exit 2
fi
export PATH="$HOME/.local/bin:$PATH"

if ! command -v llmlint >/dev/null 2>&1; then
  echo "llmlint: skipped locally (llmlint unavailable; run 'just setup-llmlint')" >&2
  exit 0
fi

llmlint validate --diff-base "$base"

if [[ -z ${OPENAI_API_KEY:-} ]]; then
  echo "llmlint: judge skipped locally (OPENAI_API_KEY unavailable)" >&2
  exit 0
fi
if ! command -v codex >/dev/null 2>&1; then
  echo "llmlint: judge skipped locally (committed primary harness 'codex' unavailable)" >&2
  exit 0
fi

printenv OPENAI_API_KEY | codex login --with-api-key >/dev/null
llmlint --diff --diff-base "$base"
