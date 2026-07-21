#!/usr/bin/env bash
# Run the local llmlint tier, gracefully skipping unavailable model infrastructure.
set -euo pipefail

base=${1:?usage: local-llmlint-gate.sh <remote/base>}
root=$(cd "$(dirname "$0")/.." && pwd)
export PATH="$HOME/.local/bin:$PATH"

git rev-parse --verify --quiet "$base^{commit}" >/dev/null || {
  echo "llmlint: '$base' is not an existing commit; fetch it or pass a valid comparison ref" >&2
  exit 2
}

primary_harness=$(awk '
  /^[[:space:]]*harnesses[[:space:]]*=/ {
    value = $0
    sub(/^[^=]*=[[:space:]]*\[/, "", value)
    if (match(value, /"[^"]+"/)) {
      print substr(value, RSTART + 1, RLENGTH - 2)
      exit
    }
  }
' "$root/oneharness.toml")
if [[ ! $primary_harness =~ ^[[:alnum:]][[:alnum:]_.-]*$ ]]; then
  echo "llmlint: oneharness.toml must declare a valid first harness in 'harnesses'" >&2
  exit 2
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
