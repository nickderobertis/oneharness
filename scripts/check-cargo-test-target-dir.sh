#!/usr/bin/env bash
# An instrumented Rust test run whose build is moved off the checkout
# (CARGO_TARGET_DIR) must record ITS OWN profile: scripts/cargo-test.sh reads
# the raw profiles where cargo-llvm-cov wrote them, under that directory, not a
# fixed target/llvm-cov-target that holds nothing, or an older run's. Drives the
# real cargo-test.sh over the smallest crate, then reads the recorded profile
# with the toolchain's llvm-profdata. Coverage is not measured on Windows.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ "${OS:-}" == "Windows_NT" ]]; then
  echo "check-cargo-test-target-dir: skipped on Windows (coverage is measured on Linux/macOS)"
  exit 0
fi

tmp="$(mktemp -d)"
record="target-dir-probe-$$"
profile="$root/target/coverage/$record.profdata"
trap 'rm -rf "$tmp"; rm -f "$profile"' EXIT

fail() {
  echo "check-cargo-test-target-dir: $1" >&2
  exit 1
}

# The resolver follows cargo-llvm-cov's own rule.
dir="$(CARGO_TARGET_DIR="$tmp/moved" bash "$root/scripts/llvm-cov-target-dir.sh")"
[ "$dir" = "$tmp/moved/llvm-cov-target" ] ||
  fail "under CARGO_TARGET_DIR=$tmp/moved the llvm-cov directory resolved to '$dir', not $tmp/moved/llvm-cov-target; fix scripts/llvm-cov-target-dir.sh"
dir="$(CARGO_TARGET_DIR="$tmp/moved" CARGO_LLVM_COV_TARGET_DIR=cov bash "$root/scripts/llvm-cov-target-dir.sh")"
[ "$dir" = "$root/cov" ] ||
  fail "a relative CARGO_LLVM_COV_TARGET_DIR resolved to '$dir', not $root/cov (cargo-llvm-cov takes it from the cwd, the repository root); fix scripts/llvm-cov-target-dir.sh"

# The real run, built under the moved target directory.
if ! CARGO_TARGET_DIR="$tmp/moved" bash "$root/scripts/cargo-test.sh" oneharness-sdk-contract \
  --record "$record" >"$tmp/run.log" 2>&1; then
  cat "$tmp/run.log" >&2
  fail "the instrumented run under CARGO_TARGET_DIR=$tmp/moved failed (above); scripts/cargo-test.sh must read the raw profiles from scripts/llvm-cov-target-dir.sh"
fi
[ -s "$profile" ] || fail "the run recorded no profile at $profile"
compgen -G "$tmp/moved/llvm-cov-target/*.profraw" >/dev/null ||
  fail "cargo-llvm-cov wrote no raw profile under $tmp/moved/llvm-cov-target, so this check proves nothing about where cargo-test.sh reads them; check cargo-llvm-cov's target-directory rule"

# The recorded profile is this run's: it counts the probed crate's own test
# binary (sdk_contract_matrix), and nothing of a suite it never ran (the e2e
# `cli` suite, whose leftover raw profiles a fixed path would pick up).
llvm_profdata="$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/^host: //p')/bin/llvm-profdata"
"$llvm_profdata" show --all-functions "$profile" >"$tmp/functions.txt" ||
  fail "llvm-profdata could not read $profile; run 'rustup component add llvm-tools-preview'"
grep -q 'sdk_contract_matrix' "$tmp/functions.txt" ||
  fail "the profile recorded under CARGO_TARGET_DIR=$tmp/moved counts none of oneharness-sdk-contract's functions, so cargo-test.sh merged some other run's profiles; it must read them from scripts/llvm-cov-target-dir.sh"
if grep -q '_3cli' "$tmp/functions.txt"; then
  fail "the profile recorded under CARGO_TARGET_DIR=$tmp/moved counts the e2e cli suite's functions, which this run never ran, so cargo-test.sh merged some other run's profiles; it must read them from scripts/llvm-cov-target-dir.sh"
fi

echo "check-cargo-test-target-dir: ok"
