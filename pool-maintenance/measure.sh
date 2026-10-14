#!/usr/bin/env bash
# Measure oneharness-pool-maintenance-seconds (budgets.yaml beside this file):
# one steady-state pass of the declared pool maintenance (`just
# pool-maintenance`), the pass a regular post-run maintenance faces — one
# ordinary run's worth of superseded builds, never a first pass over history.
#
#   1. the ordinary gate build, then one untimed declared pass, which clears
#      any backlog while every unit's newest build is the ordinary one;
#   2. the same test targets built under an alternative feature selection
#      (each selection plus `sdk-schema`, which gives this repository's own
#      crates new metadata hashes), each entry that build created backdated
#      past the 2-day window so it stands in for a superseded run's builds;
#   3. the ordinary gate build again, so each unit's newest build is ordinary;
#   4. one timed declared pass, which removes the alternative build.
#
# Only pass 4's figure is reported: the pruner's own onebudgetspec report, its
# value unchanged, with the untimed backlog pass's seconds appended to its
# detail as separate evidence. The script refuses a state it did not establish:
# a timed pass that removed nothing, or one that removed any entry the
# alternative build did not create.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

result="${ONEBUDGETSPEC_RESULT:-}"
[ -n "$result" ] || {
  echo "measure: run me through 'onebudgetspec check pool-maintenance/budgets.yaml' (just budgets), which names ONEBUDGETSPEC_RESULT" >&2
  exit 2
}

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# Every instrumented Rust test selection the gate builds (each Rust project's
# `test` runs `scripts/cargo-test.sh` with these arguments), one per line; the
# alternative adds `sdk-schema` to each, directly or through the crate it
# depends on, so every one of this repository's crates gets a new hash. The
# uninstrumented e2e leg (target/debug) is the same selection uninstrumented.
ordinary='oneharness --features oneharness/mock-harness
oneharness --with oneharness-mock-harness --features oneharness/mock-harness
oneharness-core --features oneharness-core/mock-harness
oneharness-e2e --with oneharness --with oneharness-mock-harness --features oneharness/mock-harness
oneharness-history-compat --with oneharness --with oneharness-mock-harness --features oneharness/mock-harness
oneharness-mock-harness
oneharness-sdk-conformance
oneharness-sdk-contract'
alternative='oneharness --features oneharness/mock-harness,oneharness/sdk-schema
oneharness --with oneharness-mock-harness --features oneharness/mock-harness,oneharness/sdk-schema
oneharness-core --features oneharness-core/mock-harness,oneharness-core/sdk-schema
oneharness-e2e --with oneharness --with oneharness-mock-harness --features oneharness/mock-harness,oneharness/sdk-schema
oneharness-history-compat --with oneharness --with oneharness-mock-harness --features oneharness/mock-harness,oneharness/sdk-schema
oneharness-mock-harness --features oneharness/sdk-schema
oneharness-sdk-conformance --features oneharness-core/sdk-schema
oneharness-sdk-contract --features oneharness-sdk-contract/sdk-schema'

# Every top-level build entry of every profile directory under target/, the
# entries the pruner selects among, one path per line.
entries() {
  find target -type d -name .fingerprint -prune -print | sort | while IFS= read -r fingerprint; do
    profile="$(dirname "$fingerprint")"
    for kind in deps .fingerprint build incremental; do
      if [ -d "$profile/$kind" ]; then find "$profile/$kind" -mindepth 1 -maxdepth 1; fi
    done
  done | sort
}

# Build each selection of $1 (one per line) the way the gate's test does.
build() {
  local selection args
  while IFS= read -r selection; do
    read -ra args <<<"$selection"
    bash scripts/cargo-test.sh "${args[@]}" --build-only >/dev/null || {
      echo "measure: building 'scripts/cargo-test.sh $selection --build-only' failed (above)" >&2
      exit 1
    }
  done <<<"$1"
}

prune() {
  ONEBUDGETSPEC_RESULT="$1" just pool-maintenance >"$1.log" 2>&1 || {
    cat "$1.log" >&2
    echo "measure: the declared pool maintenance failed (above)" >&2
    exit 1
  }
  grep '^cargo-prune-superseded: ' "$1.log" >&2
}

echo "measure: ordinary gate build, then the untimed backlog pass" >&2
build "$ordinary"
prune "$work/backlog.json"

echo "measure: alternative-configuration build, backdated past the window" >&2
entries >"$work/before-alternative"
build "$alternative"
entries >"$work/after-alternative"
comm -13 "$work/before-alternative" "$work/after-alternative" >"$work/alternative"
[ -s "$work/alternative" ] || {
  echo "measure: the alternative build created no build entry, so there is nothing superseded to time" >&2
  exit 1
}
# Older than any real build, so the ordinary build stays each unit's newest
# even where step 3 rebuilds nothing.
while IFS= read -r entry; do
  find "$entry" -exec touch -h -t 200101010000 {} +
done <"$work/alternative"

echo "measure: ordinary gate build, then the timed pass" >&2
build "$ordinary"
entries >"$work/before-pass"
prune "$work/pass.json"
entries >"$work/after-pass"

comm -23 "$work/before-pass" "$work/after-pass" >"$work/removed"
comm -23 "$work/removed" "$work/alternative" >"$work/foreign"
if [ -s "$work/foreign" ]; then
  cat "$work/foreign" >&2
  echo "measure: the timed pass removed the entries above, which the alternative build did not create; the steady state was not established" >&2
  exit 1
fi
removed="$(wc -l <"$work/removed" | tr -d ' ')"
[ "$removed" -gt 0 ] || {
  echo "measure: the timed pass removed none of the alternative build's $(wc -l <"$work/alternative" | tr -d ' ') entries" >&2
  exit 1
}
kept="$(comm -23 "$work/before-pass" "$work/alternative" | wc -l | tr -d ' ')"
echo "measure: removed $removed entries, every one the alternative build's; all $kept other entries remain" >&2
# This repository's crates' entries, by unit name and metadata hash.
own() { grep -E '/(deps|\.fingerprint|build|incremental)/(lib)?oneharness[a-z0-9_-]*-[0-9a-f]{16}(\.rlib)?$' "$1" | sed 's#.*/##; s#^lib##; s#\.rlib$##' | sort -u; }
comm -23 "$work/before-pass" "$work/alternative" >"$work/ordinary"
own "$work/ordinary" >"$work/own-before"
own "$work/after-pass" >"$work/own-after"
own "$work/alternative" >"$work/own-alternative"
if ! cmp -s "$work/own-before" "$work/own-after"; then
  diff "$work/own-before" "$work/own-after" >&2 || true
  echo "measure: this repository's ordinary build hashes changed across the timed pass (above)" >&2
  exit 1
fi
echo "measure: this repository's crates, ordinary build: $(wc -l <"$work/own-before" | tr -d ' ') unit/hash entries before the pass, the same $(wc -l <"$work/own-after" | tr -d ' ') after; alternative build: $(wc -l <"$work/own-alternative" | tr -d ' '), of which $(comm -12 "$work/own-alternative" "$work/own-after" | wc -l | tr -d ' ') remain as their unit's only build" >&2
sed 's/^/measure:   ordinary kept: /' "$work/own-before" >&2

node -e '
  const fs = require("node:fs");
  const [pass, backlog, out] = process.argv.slice(1);
  const timed = JSON.parse(fs.readFileSync(pass, "utf8"));
  const untimed = JSON.parse(fs.readFileSync(backlog, "utf8"));
  const detail = timed.detail + "; untimed backlog pass beforehand (not counted): " + untimed.value.toFixed(3) + " s";
  fs.writeFileSync(out, JSON.stringify({ value: timed.value, detail }));
' "$work/pass.json" "$work/backlog.json" "$result"
