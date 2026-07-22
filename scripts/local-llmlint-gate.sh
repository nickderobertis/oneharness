#!/usr/bin/env bash
# Run the local llmlint tier, gracefully skipping unavailable model infrastructure.
set -euo pipefail

base=${1:?usage: local-llmlint-gate.sh <remote/base>}
root=$(cd "$(dirname "$0")/.." && pwd)
source "$root/scripts/local-llmlint-gate-lib.sh"
export PATH="$HOME/.local/bin:$PATH"

git rev-parse --verify --quiet "$base^{commit}" >/dev/null || {
  echo "llmlint: '$base' is not an existing commit; fetch it or pass a valid comparison ref" >&2
  exit 2
}

primary_harness=$(llmlint_primary_harness "$root/oneharness.toml")

if ! command -v llmlint >/dev/null 2>&1; then
  echo "llmlint: skipped locally (llmlint unavailable; run 'just setup-llmlint')" >&2
  exit 0
fi

llmlint validate --diff-base "$base"

if [[ -n ${OPENAI_API_KEY:-} ]]; then
  if ! command -v "$primary_harness" >/dev/null 2>&1; then
    echo "llmlint: judge skipped locally (committed primary harness '$primary_harness' unavailable)" >&2
    exit 0
  fi
  printenv OPENAI_API_KEY | "$primary_harness" login --with-api-key >/dev/null
elif llmlint_judge_available "$root/oneharness.toml"; then
  :
else
  status=$?
  [[ $status -eq 75 ]] && exit 0
  exit "$status"
fi

llmlint --diff --diff-base "$base"
