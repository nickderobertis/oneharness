#!/usr/bin/env bash
#
# Print the directory `cargo llvm-cov` builds its instrumented objects into and
# writes its raw profiles to, resolved the way cargo-llvm-cov resolves it
# (src/cargo.rs, 0.8): `llvm-cov-target` under cargo's own target directory,
# which follows CARGO_TARGET_DIR and `build.target-dir`. cargo-llvm-cov's own
# override, CARGO_LLVM_COV_TARGET_DIR, is refused rather than re-implemented:
# nothing here sets it, and a copy of its rule would have no drift check.
#
#   scripts/llvm-cov-target-dir.sh
#
# scripts/cargo-test.sh and scripts/rust-coverage.sh read the objects and
# profiles there, so a build moved off the checkout (CARGO_TARGET_DIR=/tmp/...)
# is measured where it was built rather than from whatever an older build left
# in target/. Both run it from the repository root, as cargo-llvm-cov runs.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)" || {
  echo "llvm-cov-target-dir: cannot resolve the repository root from ${BASH_SOURCE[0]}; run this script from a readable checkout" >&2
  exit 1
}
cd "$root" || {
  echo "llvm-cov-target-dir: cannot enter the repository root $root; check its permissions, or run this script from a readable checkout" >&2
  exit 1
}

if [ -n "${CARGO_LLVM_COV_TARGET_DIR:-}" ]; then
  echo "llvm-cov-target-dir: CARGO_LLVM_COV_TARGET_DIR is set ('$CARGO_LLVM_COV_TARGET_DIR'), which the coverage scripts do not follow; unset it and move the build with CARGO_TARGET_DIR instead" >&2
  exit 2
fi

if ! target="$(cargo metadata --no-deps --format-version 1 --locked --offline |
  node -e 'let s="";process.stdin.on("data",(d)=>{s+=d}).on("end",()=>{console.log(JSON.parse(s).target_directory ?? "")})')"; then
  echo "llvm-cov-target-dir: reading cargo's target directory failed (cargo metadata or node, above); run 'cargo metadata --no-deps --offline --locked' to see why, fix it, and rerun" >&2
  exit 1
fi
[ -n "$target" ] || {
  echo "llvm-cov-target-dir: cargo metadata named no target directory; check 'cargo metadata --no-deps --offline --locked' and CARGO_TARGET_DIR" >&2
  exit 1
}
printf '%s/llvm-cov-target\n' "$target"
