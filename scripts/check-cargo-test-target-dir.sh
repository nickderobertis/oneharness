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

fail() {
  echo "check-cargo-test-target-dir: $1" >&2
  exit 1
}

tmp="$(mktemp -d "${TMPDIR:-/tmp}/tmp.XXXXXXXXXX")" || fail "could not create a scratch directory under ${TMPDIR:-/tmp}; check that it exists and is writable, or set TMPDIR to one that is"
record="target-dir-probe-$$"
profile="$root/target/coverage/$record.profdata"
cleanup() {
  local status=$?
  rm -rf "$tmp" || {
    echo "check-cargo-test-target-dir: could not remove the scratch directory $tmp; delete it by hand" >&2
    status=1
  }
  rm -f "$profile" || {
    echo "check-cargo-test-target-dir: could not remove the probe profile $profile; delete it by hand" >&2
    status=1
  }
  exit "$status"
}
trap cleanup EXIT

if CARGO_LLVM_COV_TARGET_DIR=cov bash "$root/scripts/llvm-cov-target-dir.sh" >/dev/null 2>"$tmp/refused.err" ||
  ! grep -q 'move the build with CARGO_TARGET_DIR' "$tmp/refused.err"; then
  fail "scripts/llvm-cov-target-dir.sh did not refuse CARGO_LLVM_COV_TARGET_DIR with its recovery action; restore the refusal"
fi

if ! CARGO_TARGET_DIR="$tmp/moved" bash "$root/scripts/cargo-test.sh" oneharness-sdk-contract \
  --record "$record" >"$tmp/run.log" 2>&1; then
  cat "$tmp/run.log" >&2
  fail "the instrumented run under CARGO_TARGET_DIR=$tmp/moved failed (above); scripts/cargo-test.sh must read the raw profiles from scripts/llvm-cov-target-dir.sh"
fi
[ -s "$profile" ] || fail "the run exited 0 but recorded no profile at $profile; read scripts/cargo-test.sh's merge step (llvm-profdata into target/coverage/<record>.profdata) and rerun this check"
compgen -G "$tmp/moved/llvm-cov-target/*.profraw" >/dev/null ||
  fail "cargo-llvm-cov wrote no raw profile under $tmp/moved/llvm-cov-target, so this check proves nothing about where cargo-test.sh reads them; check cargo-llvm-cov's target-directory rule"

# The recorded profile is this run's: it counts the probed crate's own test
# binary (sdk_contract_matrix), and nothing of a suite it never ran (the e2e
# `cli` suite, whose leftover raw profiles a fixed path would pick up).
sysroot="$(rustc --print sysroot)" && [ -n "$sysroot" ] ||
  fail "rustc --print sysroot failed; check the pinned toolchain (just bootstrap)"
host="$(rustc -vV | sed -n 's/^host: //p')" && [ -n "$host" ] ||
  fail "rustc -vV named no host triple; check the pinned toolchain (just bootstrap)"
llvm_profdata="$sysroot/lib/rustlib/$host/bin/llvm-profdata"
"$llvm_profdata" show --all-functions "$profile" >"$tmp/functions.txt" ||
  fail "llvm-profdata could not read $profile; run 'rustup component add llvm-tools-preview'"
grep -q 'sdk_contract_matrix' "$tmp/functions.txt" ||
  fail "the profile recorded under CARGO_TARGET_DIR=$tmp/moved counts none of oneharness-sdk-contract's functions, so cargo-test.sh merged some other run's profiles; it must read them from scripts/llvm-cov-target-dir.sh"
foreign=0
grep -q '_3cli' "$tmp/functions.txt" || foreign=$?
case "$foreign" in
  0) fail "the profile recorded under CARGO_TARGET_DIR=$tmp/moved counts the e2e cli suite's functions, which this run never ran, so cargo-test.sh merged some other run's profiles; it must read them from scripts/llvm-cov-target-dir.sh" ;;
  1) ;;
  *) fail "could not read $tmp/functions.txt (grep exited $foreign), so nothing was proven about which run the profile counts; check the scratch directory and rerun" ;;
esac

echo "check-cargo-test-target-dir: ok"
