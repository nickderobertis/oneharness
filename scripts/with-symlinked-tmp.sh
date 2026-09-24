#!/usr/bin/env bash
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
case "$platform" in
  Linux | Darwin | MINGW* | MSYS* | CYGWIN*) ;;
  *)
    echo "with-symlinked-tmp: unrecognized platform '$platform'." >&2
    echo "  fix: set OH_SYMLINKED_TMP_UNAME to a 'uname -s' value (Linux, Darwin, MINGW*, MSYS*, CYGWIN*), or unset it." >&2
    exit 2
    ;;
esac
if [ "$platform" != "Linux" ]; then
  echo "with-symlinked-tmp: skipped on $platform (macOS spells every temp path through /tmp -> /private/tmp already; Windows has no such root)"
  exit 0
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

root=$(mktemp -d "${TMPDIR:-/tmp}/symlinked-tmp.XXXXXX") || {
  echo "with-symlinked-tmp: could not create a scratch directory under ${TMPDIR:-/tmp}." >&2
  echo "  fix: make that directory writable (or point TMPDIR at one that is) and re-run." >&2
  exit 1
}
trap 'rm -rf "$root"' EXIT
if ! mkdir "$root/real" || ! ln -s "$root/real" "$root/link"; then
  echo "with-symlinked-tmp: could not build the symlinked temp root under $root." >&2
  echo "  fix: this lane needs a filesystem that supports symlinks; point TMPDIR at one and re-run." >&2
  exit 1
fi

# Only `$TMPDIR` moves: the leak gate's default roots are what must keep
# watching the scratch space behind the symlink.
export TMPDIR="$root/link"
bash "$repo_root/scripts/check-temp-leaks.sh" "$@"
