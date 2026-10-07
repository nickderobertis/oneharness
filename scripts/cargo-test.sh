#!/usr/bin/env bash
#
# Run ONE workspace crate's tests — the body of every Rust project's Nx `test`
# target — and leave its coverage contribution where the repo-level `coverage`
# target reads it.
#
#   scripts/cargo-test.sh <package> [--with <package>]... [--features <list>]
#                         [--filter <nextest filterset>] [--record <name>]
#                         [--uninstrumented | --build-only]
#
# `--with` names a crate whose BINARIES the suite spawns (`oneharness`,
# `oneharness-mock-harness`). They are selected beside the package so cargo
# builds them into the same profile directory the suite reads them from, under
# the same instrumentation; the nextest filter still runs only <package>'s tests.
#
# On Linux and macOS the run is instrumented (`cargo llvm-cov`) and its raw
# profiles are merged into one indexed profile, target/coverage/<record>.profdata
# — this run's contribution to the Rust floor, which rust-coverage reads (with
# every other crate's) through `cargo llvm-cov report`. One small file rather
# than the run's raw profiles, which number in the thousands for a suite that
# spawns the binary, so the test target can cache and replay it. On Windows
# llvm-cov does not attribute the coverage of subprocess-spawned binaries (the
# binary crate reads as ~0% there), so the suite runs uninstrumented and the
# floor is enforced on the other two platforms.
#
# `--filter` narrows which of <package>'s tests run (ANDed with the package), so
# one crate's unit and integration tiers can be two projects; `--record` names
# the profile (default: the package), so each tier leaves its own.
# `--uninstrumented` asks for that plain run on any platform (the
# symlinked-TMPDIR replay of the e2e journeys, which re-runs a suite already
# measured). `--build-only` builds that same instrumented selection and runs
# nothing: scripts/rust-coverage.sh uses it so the objects a report reads are
# this tree's, whichever profiles the cache replayed.
#
# Quiet on success apart from nextest's summary; a failure prints in full.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

usage() {
  echo "cargo-test: $1" >&2
  echo "  usage: scripts/cargo-test.sh <package> [--with <package>]... [--features <list>] [--filter <filterset>] [--record <name>] [--uninstrumented | --build-only]" >&2
  exit 2
}

[ "$#" -ge 1 ] || usage "no package named"
package="$1"
shift
[[ "$package" =~ ^[a-z0-9_-]+$ ]] || usage "'$package' is not a cargo package name"
packages=(-p "$package")
features=()
filterset=""
record="$package"
instrumented=1
build_only=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --with)
      [ "$#" -ge 2 ] && [[ "$2" =~ ^[a-z0-9_-]+$ ]] || usage "--with needs a cargo package name"
      packages+=(-p "$2")
      shift 2 ;;
    --features)
      [ "$#" -ge 2 ] && [[ "$2" =~ ^[a-z0-9_/,-]+$ ]] || usage "--features needs a comma-separated feature list"
      features=(--features "$2")
      shift 2 ;;
    --filter)
      [ "$#" -ge 2 ] && [ -n "$2" ] || usage "--filter needs a nextest filterset"
      filterset="$2"
      shift 2 ;;
    --record)
      [ "$#" -ge 2 ] && [[ "$2" =~ ^[a-z0-9_-]+$ ]] || usage "--record needs a record name"
      record="$2"
      shift 2 ;;
    --uninstrumented)
      instrumented=0
      shift ;;
    --build-only)
      build_only=1
      shift ;;
    *) usage "unknown argument '$1'" ;;
  esac
done

# Only <package>'s own tests run; the companions are there for their binaries.
filter=(-E "package($package)${filterset:+ & ($filterset)}")
nextest_flags=(--locked --status-level fail --final-status-level fail)

# A warning in test code fails the run, as it does every other compile the gate
# makes; the same flags as the `build` targets, so target/debug is not rebuilt
# between them.
export RUSTFLAGS="${RUSTFLAGS:-} -D warnings"

if [ "$build_only" -eq 1 ]; then
  [ "$instrumented" -eq 1 ] || usage "--build-only builds the instrumented selection; it cannot be --uninstrumented"
  # Coverage is not measured on Windows, so there is nothing to keep current.
  [[ "${OS:-}" == "Windows_NT" ]] && exit 0
  RUSTFLAGS="${RUSTFLAGS} -C linker=$root/scripts/coverage-linker.sh" \
    cargo llvm-cov --no-report nextest "${packages[@]}" ${features[@]+"${features[@]}"} \
    -E 'none()' --no-tests=pass --locked --status-level none --final-status-level none
  exit 0
fi

if [[ "${OS:-}" == "Windows_NT" ]] || [ "$instrumented" -eq 0 ]; then
  exec bash scripts/check-temp-leaks.sh cargo nextest run "${packages[@]}" ${features[@]+"${features[@]}"} "${filter[@]}" "${nextest_flags[@]}"
fi

profile="target/coverage/$record.profdata"
mkdir -p target/coverage
rm -f "$profile"
# Profiles are named after the workspace, not the crate, so a previous crate's
# are cleared first; Nx runs these targets one at a time (`parallelism: false`)
# so no other run's profiles are in flight while this one is cleared.
cargo llvm-cov clean --profraw-only
RUSTFLAGS="${RUSTFLAGS:-} -C linker=$root/scripts/coverage-linker.sh" \
  bash scripts/check-temp-leaks.sh cargo llvm-cov --no-report nextest "${packages[@]}" ${features[@]+"${features[@]}"} "${filter[@]}" "${nextest_flags[@]}"
# The toolchain's own llvm-profdata (rustup's llvm-tools-preview), the one
# cargo-llvm-cov merges with.
llvm_profdata="$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/^host: //p')/bin/llvm-profdata"
[ -x "$llvm_profdata" ] || [ -x "$llvm_profdata.exe" ] || {
  echo "cargo-test: no llvm-profdata at $llvm_profdata; run 'rustup component add llvm-tools-preview' (just bootstrap does)" >&2
  exit 1
}
inputs="$(mktemp)"
trap 'rm -f "$inputs"' EXIT
find target/llvm-cov-target -maxdepth 1 -name '*.profraw' >"$inputs"
[ -s "$inputs" ] || {
  echo "cargo-test: the instrumented run of $package wrote no profile under target/llvm-cov-target; its coverage cannot be recorded" >&2
  exit 1
}
"$llvm_profdata" merge -sparse --input-files="$inputs" -o "$profile"
