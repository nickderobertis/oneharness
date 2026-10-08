#!/usr/bin/env bash
#
# The Rust line-coverage floor, enforced once over every crate's test run.
#
#   scripts/rust-coverage.sh
#
# Each Rust project's `test` target (scripts/cargo-test.sh) leaves its run's
# merged profile in target/coverage/<record>.profdata. This hands all of them to
# `cargo llvm-cov report --fail-under-lines`, over every workspace member — the
# same metric and the same files the single `--workspace` run it replaces
# measured, with the counts of every run combined. The runs are read from the
# project definitions themselves (every `lang:rust` project whose `test` runs
# scripts/cargo-test.sh), so a new crate's run cannot be left out by a list, and
# every one must have left its profile: a missing one is a run that never
# happened, and leaving it out could only move the number.
#
# The report maps the profiles onto the instrumented objects in
# target/llvm-cov-target, so those must be this tree's. A profile the cache
# replayed was produced by objects built from these same inputs, but the objects
# on disk may be a later build's (an edit since reverted), which could drop the
# replayed counts or carry code the tree no longer has — failing the floor or,
# worse, passing it. So each run's selection is first rebuilt, build-only, for
# this tree: a no-op when nothing changed, and otherwise exactly the objects the
# replayed profile came from.
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
[[ "$floor" =~ ^[0-9]+(\.[0-9]+)?$ ]] || { echo "rust-coverage: COVERAGE_MIN '$floor' is not a percentage; unset it for the 95 default or set a number such as 95" >&2; exit 2; }

# Each Rust test run's cargo-test.sh arguments, one run per line and one
# argument per unit-separator-delimited field, from the project definitions
# (tracked and new, never ignored).
specs="$(git ls-files --cached --others --exclude-standard '*project.json' | node -e '
  const fs = require("fs");
  let input = "";
  process.stdin.on("data", (d) => { input += d; }).on("end", () => {
    for (const file of input.split("\n").filter((f) => f && fs.existsSync(f))) {
      const project = JSON.parse(fs.readFileSync(file, "utf8"));
      if (!(project.tags ?? []).includes("lang:rust") || !project.targets?.test) continue;
      const options = project.targets.test.options ?? {};
      const first = (options.commands ?? [options.command ?? ""])[0] ?? "";
      const match = first.match(/^bash scripts\/cargo-test\.sh (.+)$/u);
      if (!match) continue;
      // The arguments as the shell would split them: words, or "quoted text".
      const args = [...match[1].matchAll(/"([^"]*)"|(\S+)/gu)].map((m) => m[1] ?? m[2]);
      console.log(args.join("\x1f"));
    }
  });
')"
[ -n "$specs" ] || { echo "rust-coverage: no Rust project's test runs scripts/cargo-test.sh; there is nothing to measure; restore a Rust project's test target that runs scripts/cargo-test.sh in its project.json" >&2; exit 1; }

# The objects first, for this tree; then the profiles, each run's own.
records=()
build_log="$(mktemp)"
trap 'rm -f "$build_log"' EXIT
while IFS=$'\x1f' read -r -a args; do
  if ! bash scripts/cargo-test.sh "${args[@]}" --build-only >"$build_log" 2>&1; then
    cat "$build_log" >&2
    echo "rust-coverage: rebuilding the instrumented objects for '${args[*]}' failed (above); fix the build and re-run 'just coverage'" >&2
    exit 1
  fi
  record="${args[0]}"
  for ((i = 1; i < ${#args[@]}; i++)); do
    [ "${args[$i]}" = --record ] && record="${args[$((i + 1))]}"
  done
  records+=("$record")
done <<<"$specs"

# The report collects every profile in target/llvm-cov-target. A record is an
# indexed profile, which llvm-profdata merges exactly as it does a raw one, so
# each is placed there under the extension the report collects.
cargo llvm-cov clean --profraw-only
for record in "${records[@]}"; do
  [[ "$record" =~ ^[a-z0-9_-]+$ ]] || { echo "rust-coverage: '$record' is not a record name; fix the --record argument of the Rust project.json test target that passes it (lowercase letters, digits, '_' and '-')" >&2; exit 2; }
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
[ -n "$names" ] || { echo "rust-coverage: cargo metadata named no workspace member to report on; check the [workspace] members in the root Cargo.toml with 'cargo metadata --no-deps --offline --locked'" >&2; exit 1; }
members=()
while IFS= read -r member; do members+=(-p "$member"); done <<<"$names"

status=0
cargo llvm-cov report "${members[@]}" --summary-only --fail-under-lines "$floor" \
  >target/coverage/rust-coverage.txt 2>target/coverage/rust-coverage.err || status=$?
merged=(target/llvm-cov-target/*.profdata)
if [ -s "${merged[0]}" ]; then
  cp "${merged[0]}" target/coverage/rust.profdata
elif [ "$status" -eq 0 ]; then
  echo "rust-coverage: the report wrote no merged profile under target/llvm-cov-target; re-run the Rust test targets with --skip-nx-cache so their profiles are rebuilt, then this coverage target" >&2
  exit 1
fi
total="$(awk '/^TOTAL/ { print $(NF-3) " lines (" $(NF-4) " missed of " $(NF-5) ")" }' target/coverage/rust-coverage.txt)"
if [ "$status" -ne 0 ]; then
  cat target/coverage/rust-coverage.txt target/coverage/rust-coverage.err >&2
  echo "coverage: ${total:-no total} over ${#records[@]} crate runs is below the ${floor}% floor." >&2
  echo "  Cover the new behavior with a test (never lower the floor); 'just coverage-html' shows the uncovered lines." >&2
  exit 1
fi
echo "coverage: $total over ${#records[@]} crate runs (floor ${floor}%)"
