#!/usr/bin/env bash
# Run the local llmlint tier, gracefully skipping unavailable model infrastructure.
#
# The judge is non-deterministic, so this REPLAYS a green it already reached
# instead of rolling again: one workspace content plus one resolved base commit
# plus one judge configuration is judged exactly once, and the pre-push hook
# replays what the working tree's own gate already cleared. Any of those three
# moving invalidates. Only a green is recorded — a finding or a toolchain error
# judges again next run. Set ONEHARNESS_LLMLINT_REJUDGE=1 to force a fresh roll
# that neither reads nor records a verdict.
set -euo pipefail

base=${1:?usage: local-llmlint-gate.sh <remote/base>}
root=$(cd "$(dirname "$0")/.." && pwd)
source "$root/scripts/local-llmlint-gate-lib.sh"
export PATH="$HOME/.local/bin:$PATH"

base_commit=$(git rev-parse --verify --quiet "$base^{commit}") || {
  echo "llmlint: '$base' is not an existing commit; fetch it or pass a valid comparison ref" >&2
  exit 2
}

primary_harness=$(llmlint_primary_harness "$root/oneharness.toml")

if ! command -v llmlint >/dev/null 2>&1; then
  echo "llmlint: skipped locally (llmlint unavailable; run 'just setup-llmlint')" >&2
  exit 0
fi

# Deterministic and model-free, so it runs on every invocation; only the judge
# below is worth replaying.
llmlint validate --diff-base "$base"

verdict_key=""
if [[ ${ONEHARNESS_LLMLINT_REJUDGE:-0} == 1 ]]; then
  echo "llmlint: ONEHARNESS_LLMLINT_REJUDGE=1; rolling the judge without reading or recording a verdict" >&2
elif verdict_key=$(llmlint_verdict_key "$root" "$base_commit"); then
  if recorded_at=$(llmlint_recorded_verdict "$verdict_key" "$base_commit"); then
    echo "llmlint: replaying the green verdict recorded $recorded_at for this tree, base ${base_commit:0:12}, and judge config; set ONEHARNESS_LLMLINT_REJUDGE=1 to roll again" >&2
    exit 0
  fi
else
  verdict_key=""
  echo "llmlint: cannot identify this verdict because the judge would not report its version or effective config; rolling without recording — run 'llmlint config' from $root to see why" >&2
fi

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

# Reached only on a green; `set -e` has already carried a finding out.
if [[ -n $verdict_key ]]; then
  llmlint_record_verdict "$verdict_key" "$base_commit" ||
    echo "llmlint: judged green but could not record the verdict; the next run judges again" >&2
fi
