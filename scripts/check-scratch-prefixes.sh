#!/usr/bin/env bash
#
# Hold every suite's scratch-directory prefix to the one the leak gate sweeps for.
#
# `scripts/check-temp-leaks.sh` reads `oneharness_core::io::scratch::PREFIX` and
# reports directories that start with it. The Node and Python suites cannot read
# a Rust constant, so each spells its own prefix — and a prefix that drifted out
# of the sweep would leave the gate passing while the directories piled up, which
# is the one failure a leak gate cannot report on itself.
#
# So the constants are checked against the Rust one here, and the count is
# checked too: a rename that removed them all would otherwise pass an empty
# comparison.
#
# Quiet on success, one line. On failure it names each prefix and what it must
# start with.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

scratch_source="crates/oneharness-core/src/io/scratch.rs"
rust_prefix=$(sed -n 's/^pub const PREFIX: &str = "\(.*\)";$/\1/p' "$scratch_source")
if [ -z "$rust_prefix" ]; then
  echo "check-scratch-prefixes: could not read the scratch prefix from $scratch_source." >&2
  echo "  fix: restore the 'pub const PREFIX: &str = \"...\";' declaration, or point this check at wherever it moved to." >&2
  exit 1
fi

# One declaration per suite, each named `PREFIX` so this stays a fixed list
# rather than a scan for string literals that a new spelling could slip past.
declarations=(
  "npm/oneharness-sdk/test/scratch.mjs:export const PREFIX = "
  "python/oneharness-sdk/test/scratch.py:PREFIX = "
  "python/oneharness-sdk/test/package_e2e.py:PREFIX = "
)

failed=0
for declaration in "${declarations[@]}"; do
  file=${declaration%%:*}
  assignment=${declaration#*:}
  # `|| true`: a declaration that is gone is a finding to report, not a reason
  # for `set -e` to end the run before it is printed.
  prefix=$(grep -F "$assignment" "$file" | head -n 1 | sed 's/.*"\(.*\)".*/\1/' || true)
  if [ -z "$prefix" ]; then
    echo "check-scratch-prefixes: $file declares no scratch prefix ('$assignment')." >&2
    echo "  fix: restore the declaration, or update this check with where it moved to." >&2
    failed=1
    continue
  fi
  case $prefix in
    "$rust_prefix"*) ;;
    *)
      echo "check-scratch-prefixes: $file uses '$prefix', which the leak gate would never see." >&2
      echo "  fix: start it with '$rust_prefix' (io::scratch::PREFIX), which scripts/check-temp-leaks.sh sweeps for." >&2
      failed=1
      ;;
  esac
done

[ "$failed" -eq 0 ] || exit 1
echo "check-scratch-prefixes: every suite's scratch prefix is inside the leak gate's sweep"
