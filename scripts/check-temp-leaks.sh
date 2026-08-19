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
# The prefix below is `io::scratch::PREFIX`, which the guard mints rather than
# each caller spelling its own — so a guarded directory is always in reach of
# this sweep and an unguarded one is what gets reported.
#
# It wraps `just test` — the Rust suite. The Node and Python SDK suites leak
# scratch directories of their own (`oneharness-sdk-*`, `oneharness-python-*`,
# from `mkdtemp`/`mkdtempSync` calls with no teardown); they need the same
# treatment in their own idiom before `sdk-check`/`python-sdk-check` can run
# under this too.
#
# Directories only. Files are excluded on purpose: the temp directory is shared
# with every other process on the host, including real `oneharness` runs, which
# write (and clean up) temp *files* of their own — reading one of those as this
# suite's leak would fail the gate on someone else's work. No part of the
# product creates a temp *directory*, so a new one is always a test's.
#
# Quiet on success. On failure it names every directory left behind and what to
# do about it. The command's own exit status wins when it fails.
#
# Usage: scripts/check-temp-leaks.sh <command> [args...]
#   OH_SCRATCH_ROOTS  colon-separated roots to watch (default: "$TMPDIR:/tmp").
set -euo pipefail

if [ "$#" -eq 0 ]; then
  echo "check-temp-leaks: usage: scripts/check-temp-leaks.sh <command> [args...]" >&2
  exit 2
fi

# `/tmp` as well as `$TMPDIR`: the control tests root their sockets there
# deliberately, because a socket path is an address with a `sun_path` budget.
IFS=':' read -r -a scratch_roots <<< "${OH_SCRATCH_ROOTS:-${TMPDIR:-/tmp}:/tmp}"

snapshot() {
  local dir
  for dir in "${scratch_roots[@]}"; do
    if [ -z "$dir" ] || [ ! -d "$dir" ]; then continue; fi
    find "$dir" -maxdepth 1 -type d -name 'oneharness-*' 2>/dev/null || true
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
  exit 1
fi

exit "$status"
