#!/usr/bin/env bash
# Prove the pool maintenance budget is measured in isolation from the checkout's
# own target/ (`just pool-maintenance-isolation`; pool-maintenance/AGENTS.md).
#
# While `onebudgetspec check pool-maintenance/budgets.yaml --label host` runs,
# as `just budgets` runs it beside the gate's other Nx tasks, a second cargo
# build keeps writing into the checkout's ordinary target/debug: a scratch
# crate rebuilt under a new feature each round, every earlier round's entries
# backdated past the 2-day window, so the checkout always holds superseded
# builds a pass over it would remove. The budget must report a figure, and every
# one of those superseded entries must still be in target/debug afterwards: the
# timed pass never reached the checkout's builds.
#
# Really runs the measurement (it builds the repository's test selections into
# target/pool-maintenance); run it only in your own worktree. Silent on success
# apart from one line.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

fail() {
  echo "measure-isolation-test: $1" >&2
  [ -z "${2:-}" ] || echo "  fix: $2" >&2
  exit 1
}

scratch="$(mktemp -d)"
disruptor=""
cleanup() {
  [ -z "$disruptor" ] || kill "$disruptor" 2>/dev/null || true
  [ -z "$disruptor" ] || wait "$disruptor" 2>/dev/null || true
  # This test's entries in the checkout, and nothing else of it.
  find target/debug/deps target/debug/.fingerprint target/debug/incremental -mindepth 1 -maxdepth 1 \
    -name '*pool_maintenance_disruptor-*' -exec rm -rf {} + 2>/dev/null || true
  rm -rf "$scratch"
}
trap cleanup EXIT

# The other build: a dependency-free crate outside the checkout, built into the
# checkout's target/ with a different feature (so a new metadata hash) each round.
crate="$scratch/disruptor"
mkdir -p "$crate/src"
{
  printf '[package]\nname = "pool_maintenance_disruptor"\nversion = "0.0.0"\nedition = "2021"\npublish = false\n\n[workspace]\n\n[features]\n'
  for i in $(seq 0 999); do printf 'f%d = []\n' "$i"; done
} >"$crate/Cargo.toml"
echo 'pub fn round() {}' >"$crate/src/lib.rs"
mkdir -p target/debug
own_entries() {
  find target/debug/deps target/debug/.fingerprint -mindepth 1 -maxdepth 1 -name '*pool_maintenance_disruptor-*' 2>/dev/null | sort
}
(
  i=0
  while [ ! -e "$scratch/stop" ] && [ "$i" -lt 1000 ]; do
    # Every earlier round's builds are superseded once this round's lands.
    own_entries | while IFS= read -r entry; do find "$entry" -exec touch -h -t 200101010000 {} +; done
    cargo build --quiet --offline --manifest-path "$crate/Cargo.toml" --target-dir "$root/target" --features "f$i" \
      >>"$scratch/disruptor.log" 2>&1 || exit 1
    echo "$i" >"$scratch/rounds"
    i=$((i + 1))
    sleep 1
  done
) &
disruptor=$!

uv sync --project python --frozen --no-install-workspace --quiet
status=0
uv run --project python --frozen --no-sync onebudgetspec check pool-maintenance/budgets.yaml --label host --json \
  >"$scratch/report.json" 2>"$scratch/report.err" || status=$?

touch "$scratch/stop"
wait "$disruptor" || {
  cat "$scratch/disruptor.log" >&2
  disruptor=""
  fail "the concurrent cargo build failed (above), so the measurement was not exercised beside one" "fix the error it names, then re-run"
}
disruptor=""
rounds="$(cat "$scratch/rounds" 2>/dev/null || echo -1)"
[ "$rounds" -ge 2 ] || fail "the concurrent cargo build finished only $((rounds + 1)) round(s) during the measurement, too few to leave a superseded build in target/debug" "check 'cargo build --offline' works here, then re-run"

# The budget reported a figure for its own id, unit and threshold.
node -e '
  const fs = require("node:fs");
  const [file, status, err] = process.argv.slice(1);
  const report = JSON.parse(fs.readFileSync(file, "utf8"));
  const result = (report.results ?? []).find((r) => r.id === "oneharness-pool-maintenance-seconds");
  const problem = result === undefined
    ? "reported no oneharness-pool-maintenance-seconds result"
    : result.verdict === "error" || !Number.isFinite(result.actual)
      ? "errored instead of reporting a figure: " + result.error
      : result.unit !== "seconds" || result.direction !== "max" || result.threshold !== 20 || JSON.stringify(result.labels) !== "[\"host\"]"
        ? "reported a different budget shape: " + JSON.stringify(result)
        : "";
  if (problem !== "") {
    process.stderr.write(fs.readFileSync(err, "utf8"));
    console.error("measure-isolation-test: beside a concurrent cargo build into target/debug (onebudgetspec exit " + status + "), the budget " + problem);
    process.exit(1);
  }
  console.log("measure-isolation-test: " + result.actual.toFixed(3) + " s (" + result.verdict + ") beside a concurrent cargo build into target/debug");
' "$scratch/report.json" "$status" "$scratch/report.err" ||
  fail "the measurement was disturbed by another build in the checkout (above)" "make pool-maintenance/measure.sh build and prune only its own target directory, then re-run"

# Every superseded, backdated round of the other build is still there: the
# timed pass removed nothing of the checkout's target/.
own_entries >"$scratch/left"
[ "$(wc -l <"$scratch/left")" -ge $((2 * (rounds + 1))) ] ||
  fail "only $(wc -l <"$scratch/left") of the concurrent build's $((rounds + 1)) rounds' entries are left in target/debug; the measurement pruned the checkout's own target/" \
    "make pool-maintenance/measure.sh build and prune only its own target directory, then re-run"
