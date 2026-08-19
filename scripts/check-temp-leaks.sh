#!/usr/bin/env bash
#
# Run a command and refuse it if it left scratch directories behind.
#
# Nothing enforces the suites' scratch guards at the type level — a new test can
# still hand-roll a `create_dir_all` — and an abandoned directory is invisible
# until a host's root filesystem is full. So the suite's own run is the check.
#
# The prefix is read from `io::scratch::PREFIX` rather than copied, so the sweep
# still matches the day that constant changes.
#
# Directories only. The temp directory is shared with every other process on the
# host, including real `oneharness` runs, which write and clean up temp *files*
# of their own; reading one of those as a leak would fail the gate on someone
# else's work. No part of the product creates a temp *directory*.
#
# Silent on success: the wrapped command's output is captured and replayed only
# when that command fails or when it left scratch behind — where it is the only
# account of what the suite was doing at the time.
#
# Usage: scripts/check-temp-leaks.sh <command> [args...]
#   OH_SCRATCH_ROOTS  colon-separated roots to watch (default: "$TMPDIR:/tmp").
set -euo pipefail

if [ "$#" -eq 0 ]; then
  echo "check-temp-leaks: no command to run." >&2
  echo "  fix: pass the command to watch, as in 'scripts/check-temp-leaks.sh cargo test'." >&2
  exit 2
fi

# `/tmp` as well as `$TMPDIR`: the control tests root their sockets there
# deliberately, because a socket path is an address with a `sun_path` budget.
IFS=':' read -r -a scratch_roots <<< "${OH_SCRATCH_ROOTS:-${TMPDIR:-/tmp}:/tmp}"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
scratch_source="$repo_root/crates/oneharness-core/src/io/scratch.rs"
prefix=$(sed -n 's/^pub const PREFIX: &str = "\(.*\)";$/\1/p' "$scratch_source")
if [ -z "$prefix" ]; then
  echo "check-temp-leaks: could not read the scratch prefix from $scratch_source." >&2
  echo "  fix: restore the 'pub const PREFIX: &str = \"...\";' declaration, or point this gate at wherever it moved to." >&2
  exit 2
fi

snapshot() {
  local dir
  for dir in "${scratch_roots[@]}"; do
    if [ -z "$dir" ] || [ ! -d "$dir" ]; then continue; fi
    find "$dir" -maxdepth 1 -type d -name "$prefix*" 2>/dev/null || true
  done | sort -u
}

before=$(snapshot)

# Both streams into one file, so a replay preserves the order the command wrote
# them in rather than the order two buffers happened to flush.
transcript=$(mktemp)
trap 'rm -f "$transcript"' EXIT

status=0
"$@" >"$transcript" 2>&1 || status=$?

leaked=$(comm -13 <(printf '%s\n' "$before") <(snapshot))

# Everything the command said, verbatim, the moment anything is wrong with the
# run — including a leak after a clean exit, where it is the only account of
# what the suite was doing when it abandoned the directory.
if [ "$status" -ne 0 ] || [ -n "$leaked" ]; then
  cat "$transcript" >&2
fi

if [ -n "$leaked" ]; then
  echo "check-temp-leaks: '$1' left scratch directories behind:" >&2
  printf '%s\n' "$leaked" | sed 's/^/  /' >&2
  echo "  fix: own each one with oneharness_core::io::scratch::ScratchDir, which removes it when the test ends — including when the test panics." >&2
  # Named either way, but a command that failed keeps its status: the leak is
  # usually a consequence of the failure, and reporting it as the outcome would
  # hide the thing to fix first.
  [ "$status" -eq 0 ] && exit 1
fi

exit "$status"
