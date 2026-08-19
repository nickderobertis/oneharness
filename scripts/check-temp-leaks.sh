#!/usr/bin/env bash
#
# Run a command and refuse it if it left scratch directories behind.
#
# Every scratch directory this repository's suites make is owned by an
# `io::scratch::ScratchDir` guard that removes it on the way out, panic or not.
# Nothing enforces that at the type level, though — a new test can still
# hand-roll a `create_dir_all` in the host temp directory — and the failure is
# invisible until a host's root filesystem is full: one accumulated 108,234 of
# them and stopped every program on it, twice. So the suite's own run is the
# check.
#
# The prefix is read from `io::scratch::PREFIX`, not copied: the guard mints it
# rather than each caller spelling its own, so a guarded directory is always in
# reach of this sweep — and reading the declaration is what keeps that true the
# day the constant changes.
#
# It wraps every suite that takes scratch space — `just test`, and the Node and
# Python halves of `sdk-check`/`python-sdk-check`, whose own guards spell the
# same prefix in their own idiom (`scripts/check-scratch-prefixes.sh` keeps all
# three in step).
#
# Directories only. Files are excluded on purpose: the temp directory is shared
# with every other process on the host, including real `oneharness` runs, which
# write (and clean up) temp *files* of their own — reading one of those as this
# suite's leak would fail the gate on someone else's work. No part of the
# product creates a temp *directory*, so a new one is always a test's.
#
# Quiet of its own accord, and transparent about the command's: what the wrapped
# command prints goes straight through, because that output IS the exact error a
# failing gate has to preserve, and nothing here diagnosed it well enough to add
# a next action to it. A caller that wants a quiet success captures this
# invocation and replays it on failure, as `sdk-check` does.
#
# On a leak it names every directory left behind and what to do about that. A
# command that failed keeps its own exit status, because the failure explains
# more than the scratch abandoned on the way to it.
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

status=0
"$@" || status=$?

leaked=$(comm -13 <(printf '%s\n' "$before") <(snapshot))
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
