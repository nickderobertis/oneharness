#!/usr/bin/env bash
#
# Run ONE workspace crate's tests — the body of every Rust project's Nx `test`
# target — and leave its coverage contribution where the repo-level `coverage`
# target reads it.
#
#   scripts/cargo-test.sh <package> [--with <package>]... [--features <list>]
#                         [--uninstrumented]
#
# `--with` names a crate whose BINARIES the suite spawns (`oneharness`,
# `oneharness-mock-harness`). They are selected beside the package so cargo
# builds them into the same profile directory the suite reads them from, under
# the same instrumentation; the nextest filter still runs only <package>'s tests.
#
# On Linux and macOS the run is instrumented (`cargo llvm-cov`) and its line
# data exported to target/coverage/<package>.lcov — a self-contained record of
# which lines this run executed, so a cache-replayed run still carries its
# contribution without the instrumented objects that produced it. On Windows
# llvm-cov does not attribute the coverage of subprocess-spawned binaries (the
# binary crate reads as ~0% there), so the suite runs uninstrumented and the
# floor is enforced on the other two platforms. `--uninstrumented` asks for that
# plain run on any platform (the symlinked-TMPDIR replay of the e2e journeys,
# which re-runs a suite already measured).
#
# Quiet on success apart from nextest's summary; a failure prints in full.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

usage() {
  echo "cargo-test: $1" >&2
  echo "  usage: scripts/cargo-test.sh <package> [--with <package>]... [--features <list>] [--uninstrumented]" >&2
  exit 2
}

[ $# -ge 1 ] || usage "no package named"
package="$1"
shift
[[ "$package" =~ ^[a-z0-9_-]+$ ]] || usage "'$package' is not a cargo package name"
packages=(-p "$package")
features=()
instrumented=1
while [ $# -gt 0 ]; do
  case "$1" in
    --with)
      [ $# -ge 2 ] && [[ "$2" =~ ^[a-z0-9_-]+$ ]] || usage "--with needs a cargo package name"
      packages+=(-p "$2")
      shift 2 ;;
    --features)
      [ $# -ge 2 ] && [[ "$2" =~ ^[a-z0-9_/,-]+$ ]] || usage "--features needs a comma-separated feature list"
      features=(--features "$2")
      shift 2 ;;
    --uninstrumented)
      instrumented=0
      shift ;;
    *) usage "unknown argument '$1'" ;;
  esac
done

# Only <package>'s own tests run; the companions are there for their binaries.
filter=(-E "package($package)")
nextest_flags=(--locked --status-level fail --final-status-level fail)

if [[ "${OS:-}" == "Windows_NT" ]] || [ "$instrumented" -eq 0 ]; then
  exec bash scripts/check-temp-leaks.sh cargo nextest run "${packages[@]}" "${features[@]}" "${filter[@]}" "${nextest_flags[@]}"
fi

lcov="target/coverage/$package.lcov"
mkdir -p target/coverage
rm -f "$lcov"
# Profiles are named after the workspace, not the crate, so a previous crate's
# are cleared first; Nx runs these targets one at a time (`parallelism: false`)
# so no other run's profiles are in flight while this one is cleared.
cargo llvm-cov clean --profraw-only
RUSTFLAGS="${RUSTFLAGS:-} -C linker=$root/scripts/coverage-linker.sh" \
  bash scripts/check-temp-leaks.sh cargo llvm-cov --no-report nextest "${packages[@]}" "${features[@]}" "${filter[@]}" "${nextest_flags[@]}"
# Report over every workspace member, as the single `--workspace` run this
# replaces did: `report` otherwise keeps only the root package's files.
members="$(cargo metadata --no-deps --format-version 1 --locked --offline |
  node -e 'let s="";process.stdin.on("data",(d)=>{s+=d}).on("end",()=>{for(const p of JSON.parse(s).packages)console.log(p.name)})')"
report=()
while IFS= read -r member; do report+=(-p "$member"); done <<<"$members"
cargo llvm-cov report "${report[@]}" --lcov --output-path "$lcov" >/dev/null
