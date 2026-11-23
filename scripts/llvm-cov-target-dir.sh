#!/usr/bin/env bash
#
# Print the directory `cargo llvm-cov` builds its instrumented objects into and
# writes its raw profiles to, resolved the way cargo-llvm-cov resolves it
# (src/cargo.rs, 0.8): CARGO_LLVM_COV_TARGET_DIR, taken relative to the cwd
# when it is not absolute; otherwise `llvm-cov-target` under cargo's own target
# directory, which follows CARGO_TARGET_DIR and `build.target-dir`.
#
#   scripts/llvm-cov-target-dir.sh
#
# scripts/cargo-test.sh and scripts/rust-coverage.sh read the objects and
# profiles there, so a build moved off the checkout (CARGO_TARGET_DIR=/tmp/...)
# is measured where it was built rather than from whatever an older build left
# in target/. Both run it from the repository root, as cargo-llvm-cov runs.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

if [ -n "${CARGO_LLVM_COV_TARGET_DIR:-}" ]; then
  case "$CARGO_LLVM_COV_TARGET_DIR" in
    /*) printf '%s\n' "$CARGO_LLVM_COV_TARGET_DIR" ;;
    *) printf '%s/%s\n' "$root" "$CARGO_LLVM_COV_TARGET_DIR" ;;
  esac
  exit 0
fi

target="$(cargo metadata --no-deps --format-version 1 --locked --offline |
  node -e 'let s="";process.stdin.on("data",(d)=>{s+=d}).on("end",()=>{console.log(JSON.parse(s).target_directory ?? "")})')"
[ -n "$target" ] || {
  echo "llvm-cov-target-dir: cargo metadata named no target directory; check 'cargo metadata --no-deps --offline --locked' and CARGO_TARGET_DIR" >&2
  exit 1
}
printf '%s/llvm-cov-target\n' "$target"
