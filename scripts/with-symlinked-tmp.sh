#!/usr/bin/env bash
# llmlint: ignore-file[new_code_lands_in_a_project] The rule presumes an Nx project graph; this repository has none by a recorded decision (`AGENTS.md`: root `just` delegates to Cargo/Bun without Nx because the two-package graph is static), so no project definition can cover this file and `just test-symlinked-tmp` is what runs it.
#
# Run a command with `$TMPDIR` reached through a symlink, under the scratch-leak
# gate — the temp-path spelling macOS gives every run (`/tmp` is a symlink to
# `/private/tmp`) and Linux never does. `just test-symlinked-tmp` runs the CLI
# journeys through it.
#
# Linux only: off Linux it says so and runs nothing, since macOS already spells
# every temp path this way and Windows has no such root.
#
# Usage: scripts/with-symlinked-tmp.sh <command> [args...]
#   OH_SYMLINKED_TMP_UNAME  the platform to act as, a `uname -s` value (default:
#                           `uname -s`); anything else is a usage error.
set -euo pipefail

if [ "$#" -eq 0 ]; then
  echo "with-symlinked-tmp: no command to run." >&2
  echo "  fix: pass the command to run, as in 'scripts/with-symlinked-tmp.sh cargo test'." >&2
  exit 2
fi

platform="${OH_SYMLINKED_TMP_UNAME:-$(uname -s)}"
known_platforms=(Linux Darwin 'MINGW*' 'MSYS*' 'CYGWIN*')
recognized=0
for pattern in "${known_platforms[@]}"; do
  # shellcheck disable=SC2254 # the pattern must glob, so it stays unquoted.
  case "$platform" in $pattern) recognized=1 ;; esac
done
if [ "$recognized" -eq 0 ]; then
  echo "with-symlinked-tmp: unrecognized platform '$platform'." >&2
  listed=$(printf '%s, ' "${known_platforms[@]}")
  echo "  fix: set OH_SYMLINKED_TMP_UNAME to a 'uname -s' value (${listed%, }), or unset it." >&2
  exit 2
fi
if [ "$platform" != "Linux" ]; then
  echo "with-symlinked-tmp: skipped on $platform (macOS spells every temp path through /tmp -> /private/tmp already; Windows has no such root)"
  exit 0
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

root=""
cleanup() {
  local status=$?
  if [ -z "$root" ] || rm -rf "$root"; then return; fi
  echo "with-symlinked-tmp: could not remove its symlinked temp root $root." >&2
  echo "  fix: remove it by hand with 'rm -rf $root'." >&2
  [ "$status" -ne 0 ] || exit 1
}
trap cleanup EXIT
if ! root=$(mktemp -d "${TMPDIR:-/tmp}/symlinked-tmp.XXXXXX") ||
  ! mkdir "$root/real" || ! ln -s "$root/real" "$root/link"; then
  echo "with-symlinked-tmp: could not build a symlinked temp root under ${TMPDIR:-/tmp}." >&2
  echo "  fix: point TMPDIR at a writable directory on a filesystem that supports symlinks, then re-run." >&2
  exit 1
fi

# Only `$TMPDIR` moves: the leak gate's default roots are what must keep
# watching the scratch space behind the symlink. An inherited root list would
# replace those defaults, so this lane's root joins it rather than going unwatched.
export TMPDIR="$root/link"
if [ -n "${OH_SCRATCH_ROOTS-}" ]; then
  export OH_SCRATCH_ROOTS="$TMPDIR:$OH_SCRATCH_ROOTS"
fi
bash "$repo_root/scripts/check-temp-leaks.sh" "$@"
