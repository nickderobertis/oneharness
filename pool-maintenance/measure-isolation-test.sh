#!/usr/bin/env bash
# Prove the pool maintenance budget is measured in isolation from the checkout's
# own target/ (`just pool-maintenance-isolation`; pool-maintenance/AGENTS.md).
#
# Runs the budget while another cargo build keeps leaving superseded, backdated
# builds in the checkout's target/debug; the budget must report a figure and
# leave every one of them there.
#
# Really runs the measurement (it builds the repository's test selections into
# target/pool-maintenance); run it only in your own worktree. Silent on success
# apart from one line.
set -euo pipefail

fail() {
  echo "measure-isolation-test: $1" >&2
  [ -z "${2:-}" ] || echo "  fix: $2" >&2
  exit 1
}

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)" ||
  fail "could not resolve the repository root from ${BASH_SOURCE[0]}" "run it from a checkout as 'just pool-maintenance-isolation'"
cd "$root" || fail "could not enter the repository root $root" "check that it exists and is readable, then re-run"

scratch="$(mktemp -d)" || fail "could not create a scratch directory under ${TMPDIR:-/tmp}" "check that it is writable and has room, then re-run"
disruptor=""
# This test's entries in the checkout's target/debug, one path per line.
# Fails when any listing does, so a partial list is never taken for the whole.
own_entries() {
  (
    for kind in deps .fingerprint incremental; do
      [ ! -d "target/debug/$kind" ] || find "target/debug/$kind" -mindepth 1 -maxdepth 1 -name '*pool_maintenance_disruptor-*' || exit 1
    done
  ) | sort
}
# A cleanup that leaves anything behind fails a run that had otherwise passed.
cleanup() {
  local status=$? clean=1
  if [ -n "$disruptor" ]; then
    kill "$disruptor" 2>/dev/null || true
    wait "$disruptor" 2>/dev/null || true
  fi
  # Judged by what is left, so no single failed removal can hide behind a later one.
  local entries left
  if entries="$(own_entries)"; then
    while IFS= read -r entry; do [ -z "$entry" ] || rm -rf "$entry" || true; done <<<"$entries"
  fi
  if ! left="$(own_entries)" || [ -n "$left" ]; then
    echo "measure-isolation-test: could not remove this test's pool_maintenance_disruptor entries from target/debug (above); delete them by hand" >&2
    clean=0
  fi
  rm -rf "$scratch" || {
    echo "measure-isolation-test: could not remove the scratch directory $scratch (above); delete it by hand" >&2
    clean=0
  }
  if [ "$clean" -eq 0 ] && [ "$status" -eq 0 ]; then exit 1; fi
}
trap cleanup EXIT

# The other build: a dependency-free crate outside the checkout, built into the
# checkout's target/ with a different feature (so a new metadata hash) each round.
crate="$scratch/disruptor"
{
  mkdir -p "$crate/src" target/debug &&
    {
      printf '[package]\nname = "pool_maintenance_disruptor"\nversion = "0.0.0"\nedition = "2021"\npublish = false\n\n[workspace]\n\n[features]\n'
      for ((i = 0; i < 1000; i++)); do printf 'f%d = []\n' "$i"; done
    } >"$crate/Cargo.toml" &&
    echo 'pub fn round() {}' >"$crate/src/lib.rs"
} || fail "could not write the scratch crate under $scratch (above)" "check that ${TMPDIR:-/tmp} and target/ are writable, then re-run"
(
  i=0
  while [ ! -e "$scratch/stop" ] && [ "$i" -lt 1000 ]; do
    # Every earlier round's builds are superseded once this round's lands. One
    # removed meanwhile is for the survival check at the end, not an error here.
    if ! {
      own_entries >"$scratch/earlier" &&
        while IFS= read -r entry; do find "$entry" -exec touch -h -t 200101010000 {} + || [ ! -e "$entry" ] || {
          echo "backdating $entry for round $i failed (above)" >&2
          exit 1
        }; done <"$scratch/earlier"
    } 2>>"$scratch/disruptor.log"; then
      echo "backdating round $i's predecessors in target/debug failed (above)" >>"$scratch/disruptor.log"
      exit 1
    fi
    cargo build --quiet --offline --manifest-path "$crate/Cargo.toml" --target-dir "$root/target" --features "f$i" >>"$scratch/disruptor.log" 2>&1 || {
      echo "cargo build of round $i failed (above)" >>"$scratch/disruptor.log"
      exit 1
    }
    # What exists now is what the checkout must still hold at the end.
    if ! { own_entries >>"$scratch/created" && echo "$i" >"$scratch/rounds"; } 2>>"$scratch/disruptor.log"; then
      echo "recording round $i's entries failed (above)" >>"$scratch/disruptor.log"
      exit 1
    fi
    i=$((i + 1))
    sleep 1
  done
) &
disruptor=$!

uv sync --project python --frozen --no-install-workspace --quiet ||
  fail "uv could not sync the Python toolchain onebudgetspec runs from (above)" "run 'just bootstrap', then re-run"
status=0
uv run --project python --frozen --no-sync onebudgetspec check pool-maintenance/budgets.yaml --label host --json \
  >"$scratch/report.json" 2>"$scratch/report.err" || status=$?

touch "$scratch/stop" || fail "could not write the stop marker in $scratch (above)" "check that ${TMPDIR:-/tmp} is writable and has room, then re-run"
wait "$disruptor" || {
  cat "$scratch/disruptor.log" >&2 || echo "measure-isolation-test: could not read the concurrent build's log $scratch/disruptor.log (above)" >&2
  disruptor=""
  fail "the concurrent build into target/debug failed (above), so the measurement was not exercised beside one" "fix the error it names, then re-run"
}
disruptor=""
rounds="$(cat "$scratch/rounds" 2>/dev/null || echo -1)"
[ "$rounds" -ge 2 ] || fail "the concurrent cargo build finished only $((rounds + 1)) round(s) during the measurement, too few to leave a superseded build in target/debug" "check 'cargo build --offline' works here, then re-run"

# The budget's one host result is a figure: a measured value, never an error.
# llmlint: ignore-block[contracts_have_one_source_or_a_drift_gate] Reading onebudgetspec's own --json report (the pinned python/uv.lock release) is this test's subject, not a restated copy: `actual` and `error` are the two fields that say whether the budget produced a figure, and a release that renamed them fails this test loudly rather than drifting silently.
node -e '
  const fs = require("node:fs");
  const [file, status, err] = process.argv.slice(1);
  const read = (path) => {
    try {
      return fs.readFileSync(path, "utf8");
    } catch (error) {
      return "(could not read " + path + ": " + error.message + ")";
    }
  };
  let problem = "";
  let result;
  try {
    const results = JSON.parse(read(file)).results;
    if (!Array.isArray(results) || results.length !== 1) problem = "reported " + JSON.stringify(results) + " rather than its one host result";
    else if (results[0] === null || typeof results[0] !== "object") problem = "reported a result that is not an object: " + JSON.stringify(results[0]);
    else result = results[0];
  } catch (error) {
    problem = "printed no JSON report: " + error.message;
  }
  if (problem === "" && (result.error !== null || !Number.isFinite(result.actual) || typeof result.id !== "string" || typeof result.unit !== "string")) {
    problem = "reported no figure: " + result.error;
  }
  if (problem !== "") {
    process.stderr.write(read(err));
    console.error("measure-isolation-test: beside a concurrent cargo build into target/debug (onebudgetspec exit " + status + "), the budget " + problem);
    process.exit(1);
  }
  console.log("measure-isolation-test: " + result.id + " " + result.actual.toFixed(3) + " " + result.unit + " (" + result.verdict + ") beside a concurrent cargo build into target/debug");
' "$scratch/report.json" "$status" "$scratch/report.err" ||
  fail "the measurement was disturbed by another build in the checkout (above)" "make pool-maintenance/measure.sh build and prune only its own target directory, then re-run"
# llmlint: ignore-end[contracts_have_one_source_or_a_drift_gate]

# Every entry the other build created, superseded and backdated or not, is
# still there: the timed pass removed nothing of the checkout's target/.
if ! { sort -u "$scratch/created" >"$scratch/expected" && own_entries >"$scratch/left"; }; then
  fail "could not list this test's entries in target/debug (above)" "check that target/debug and ${TMPDIR:-/tmp} are readable and writable, then re-run"
fi
missing="$(comm -23 "$scratch/expected" "$scratch/left")" || fail "could not compare this test's entries (above)" "check that comm is on PATH, then re-run"
[ -z "$missing" ] || {
  printf '%s\n' "$missing" >&2
  fail "the measurement removed the entries above, which the concurrent build created in the checkout's target/debug" \
    "make pool-maintenance/measure.sh build and prune only its own target directory, then re-run"
}
