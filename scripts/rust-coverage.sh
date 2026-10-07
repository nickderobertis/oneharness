#!/usr/bin/env bash
#
# The Rust line-coverage floor, enforced once over every crate's test run.
#
#   scripts/rust-coverage.sh <record>...
#
# Each Rust project's `test` target (scripts/cargo-test.sh) leaves its run's
# merged profile in target/coverage/<record>.profdata. This hands all of them to
# `cargo llvm-cov report --fail-under-lines`, over every workspace member — the
# same metric and the same files the single `--workspace` run it replaces
# measured, with the counts of every run combined. Every named record must exist:
# a missing one is a run that never happened, and leaving it out could only move
# the number.
#
# The report maps the profiles onto the instrumented objects those runs built in
# target/llvm-cov-target, so it needs them there. A profile whose objects were
# rebuilt since can only lose counts — llvm-cov drops records whose hash no longer
# matches — which fails the floor rather than passing it; `cargo llvm-cov clean
# --workspace` clears a stale instrumented build.
#
# Windows is skipped with its reason: llvm-cov does not attribute the coverage of
# subprocess-spawned binaries there, so the binary crate reads as ~0%. The floor
# is a property of the suite, measured on Linux and macOS.
#
# Quiet on success: one line. Writes the report's summary table to
# target/coverage/rust-coverage.txt and the merged profile to
# target/coverage/rust.profdata, the target's declared outputs.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

floor="${COVERAGE_MIN:-95}"

if [[ "${OS:-}" == "Windows_NT" ]]; then
  echo "coverage: skipped on Windows (llvm-cov subprocess attribution under-reports; measured on Linux/macOS — see scripts/rust-coverage.sh)"
  exit 0
fi
[ $# -ge 1 ] || { echo "usage: scripts/rust-coverage.sh <record>...  (COVERAGE_MIN, default 95)" >&2; exit 2; }
[[ "$floor" =~ ^[0-9]+(\.[0-9]+)?$ ]] || { echo "rust-coverage: COVERAGE_MIN '$floor' is not a percentage" >&2; exit 2; }

# The report collects every profile in target/llvm-cov-target. A record is an
# indexed profile, which llvm-profdata merges exactly as it does a raw one, so
# each is placed there under the extension the report collects.
cargo llvm-cov clean --profraw-only
for record in "$@"; do
  [[ "$record" =~ ^[a-z0-9_-]+$ ]] || { echo "rust-coverage: '$record' is not a record name" >&2; exit 2; }
  if [ ! -s "target/coverage/$record.profdata" ]; then
    echo "coverage: no profile for $record at target/coverage/$record.profdata; its test target never ran instrumented. Run 'just coverage' (it runs every Rust test target first)." >&2
    exit 1
  fi
  cp "target/coverage/$record.profdata" "target/llvm-cov-target/$record.profraw"
done

# Every workspace member, as `--workspace` reported: `report` otherwise keeps
# only the root package's files.
# Captured before use, so a failed metadata read stops the run rather than
# leaving the report on its default package selection.
names="$(cargo metadata --no-deps --format-version 1 --locked --offline |
  node -e 'let s="";process.stdin.on("data",(d)=>{s+=d}).on("end",()=>{for(const p of JSON.parse(s).packages)console.log(p.name)})')"
[ -n "$names" ] || { echo "rust-coverage: cargo metadata named no workspace member to report on" >&2; exit 1; }
members=()
while IFS= read -r member; do members+=(-p "$member"); done <<<"$names"

status=0
cargo llvm-cov report "${members[@]}" --summary-only --fail-under-lines "$floor" \
  >target/coverage/rust-coverage.txt 2>target/coverage/rust-coverage.err || status=$?
merged=(target/llvm-cov-target/*.profdata)
if [ -s "${merged[0]}" ]; then
  cp "${merged[0]}" target/coverage/rust.profdata
elif [ "$status" -eq 0 ]; then
  echo "rust-coverage: the report wrote no merged profile under target/llvm-cov-target" >&2
  exit 1
fi
total="$(awk '/^TOTAL/ { print $(NF-3) " lines (" $(NF-4) " missed of " $(NF-5) ")" }' target/coverage/rust-coverage.txt)"
if [ "$status" -ne 0 ]; then
  cat target/coverage/rust-coverage.txt target/coverage/rust-coverage.err >&2
  echo "coverage: ${total:-no total} over $# crate runs is below the ${floor}% floor." >&2
  echo "  Cover the new behavior with a test (never lower the floor); 'just coverage-html' shows the uncovered lines." >&2
  exit 1
fi
echo "coverage: $total over $# crate runs (floor ${floor}%)"
