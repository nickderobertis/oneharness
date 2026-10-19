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
#
# Every build and pass uses the measurement's own target directory, never the
# checkout's target/, which `just budgets`'s concurrent Nx tasks write
# (pool-maintenance/AGENTS.md).
#
# Silent on success. POOL_MAINTENANCE_EVIDENCE, when set, names a file this run
# writes its evidence to: the selections it built, each pass's pruner lines,
# and this repository's unit/hash entries before and after the timed pass.
set -euo pipefail

# A path onebudgetspec (or the caller) hands in: an existing, writable regular
# file, made absolute before the cd below. $1 names it in messages.
regular_file() {
  local name="$1" path="$2"
  if [ ! -f "$path" ] || [ ! -w "$path" ]; then
    echo "measure: $name='$path' is not an existing, writable regular file; create it (touch) or point $name at a writable file, then re-run" >&2
    exit 2
  fi
  printf '%s/%s\n' "$(cd "$(dirname "$path")" && pwd)" "$(basename "$path")"
}

[ -n "${ONEBUDGETSPEC_RESULT:-}" ] || {
  echo "measure: run me through 'onebudgetspec check pool-maintenance/budgets.yaml' (just budgets), which names ONEBUDGETSPEC_RESULT" >&2
  exit 2
}
result="$(regular_file ONEBUDGETSPEC_RESULT "$ONEBUDGETSPEC_RESULT")"
[ ! -s "$result" ] || {
  echo "measure: ONEBUDGETSPEC_RESULT='$result' is not the fresh, empty file onebudgetspec hands a measurement; run me through 'just budgets' rather than reusing a result file" >&2
  exit 2
}
evidence=/dev/null
if [ -n "${POOL_MAINTENANCE_EVIDENCE:-}" ]; then
  evidence="$(regular_file POOL_MAINTENANCE_EVIDENCE "$POOL_MAINTENANCE_EVIDENCE")"
  : >"$evidence"
fi
# One evidence line; nothing reaches the terminal on success.
note() { printf 'measure: %s\n' "$*" >>"$evidence"; }

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
# The measurement's own checkout-relative layout: `just pool-maintenance` runs in
# $measured, whose `target` every build below writes to (and whose .just-tmp is
# the justfile's relative `tempdir` there).
measured="$root/target/pool-maintenance"
target="$measured/target"
mkdir -p "$target" "$measured/.just-tmp" || {
  echo "measure: could not create the measurement's target directory $target (above); check that target/ is writable and has room, then re-run" >&2
  exit 1
}
export CARGO_TARGET_DIR="$target"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# Every Rust test selection the gate builds, read from each project's `test`
# target (`bash scripts/cargo-test.sh <selection>`, bare or wrapped in
# scripts/with-symlinked-tmp.sh, which changes only the run's TMPDIR) so the
# measurement cannot drift from the gate: the instrumented ones build into
# target/llvm-cov-target, the e2e project's `--uninstrumented` one into
# target/debug (both under $target here), each with its own run's flags (`--build-only`). The alternative adds `sdk-schema` through
# oneharness-core (or, where a selection does not depend on it directly, the
# binary crate that enables it), and every crate here depends on that, so every
# one of them gets a new metadata hash. One selection per line in each.
cargo metadata --no-deps --format-version 1 --offline --locked >"$work/metadata.json" || {
  echo "measure: cargo metadata failed (above); run 'cargo fetch --locked', then re-run" >&2
  exit 1
}
node -e '
  const fs = require("node:fs");
  const [metadata, out] = process.argv.slice(1);
  const files = require("node:child_process").execFileSync("git", ["ls-files", "-z", "--", "*project.json"], { encoding: "utf8" }).split("\0").filter(Boolean);
  const packages = JSON.parse(fs.readFileSync(metadata, "utf8")).packages;
  const deps = new Map(packages.map((p) => [p.name, new Set(p.dependencies.map((d) => d.name))]));
  const ordinary = [];
  const alternative = [];
  for (const file of files.sort()) {
    const test = JSON.parse(fs.readFileSync(file, "utf8")).targets?.test?.options ?? {};
    for (const command of test.commands ?? [test.command ?? ""]) {
      const match = /^(?:bash scripts\/with-symlinked-tmp\.sh )?bash scripts\/cargo-test\.sh (.*)$/u.exec(command);
      if (!match) continue;
      const args = match[1].replace(/ --filter "[^"]*"/gu, "").replace(/ --record \S+/gu, "").trim().split(/\s+/u);
      const selected = [args[0], ...args.flatMap((a, i) => (args[i - 1] === "--with" ? [a] : []))];
      const via = ["oneharness-core", "oneharness"].find((c) => selected.some((p) => p === c || deps.get(p)?.has(c)));
      if (via === undefined) throw new Error(file + ": " + command + " selects nothing that reaches oneharness-core");
      const at = args.indexOf("--features");
      const alt = [...args];
      if (at < 0) alt.push("--features", via + "/sdk-schema");
      else alt[at + 1] += "," + via + "/sdk-schema";
      ordinary.push(args.join(" "));
      alternative.push(alt.join(" "));
    }
  }
  if (ordinary.length === 0) throw new Error("no project test target runs scripts/cargo-test.sh");
  fs.writeFileSync(out + "/selections-ordinary", ordinary.join("\n") + "\n");
  fs.writeFileSync(out + "/selections-alternative", alternative.join("\n") + "\n");
' "$work/metadata.json" "$work" || {
  echo "measure: could not read the gate's Rust test selections from the project.json files (above); make each Rust project's test target call 'bash scripts/cargo-test.sh <selection>' (optionally via scripts/with-symlinked-tmp.sh), or teach this parser its new shape" >&2
  exit 1
}
ordinary="$(cat "$work/selections-ordinary")"
alternative="$(cat "$work/selections-alternative")"
note "$(wc -l <"$work/selections-ordinary" | tr -d ' ') gate Rust test selections:"
paste -d '|' "$work/selections-ordinary" "$work/selections-alternative" | sed 's/^/measure:   ordinary | alternative: /' >>"$evidence"

# Every top-level build entry of every profile directory under $target, the
# entries the pruner selects among, one path per line.
entries() {
  find "$target" -type d -name .fingerprint -prune -print | sort | while IFS= read -r fingerprint; do
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
    bash scripts/cargo-test.sh "${args[@]}" --build-only >"$work/build.log" 2>&1 || {
      cat "$work/build.log" >&2
      echo "measure: building 'scripts/cargo-test.sh $selection --build-only' failed (above); fix the error it names, check 'just test' passes, then re-run" >&2
      exit 1
    }
  done <<<"$1"
}

prune() {
  ONEBUDGETSPEC_RESULT="$1" just --justfile "$root/justfile" --working-directory "$measured" pool-maintenance >"$1.log" 2>&1 || {
    cat "$1.log" >&2
    echo "measure: the declared pool maintenance failed (above); run 'just bootstrap' to install the pinned pruner, check 'just pool-maintenance' passes alone, then re-run" >&2
    exit 1
  }
  grep '^cargo-prune-superseded: ' "$1.log" >>"$evidence" || true
}

note "ordinary gate build, then the untimed backlog pass"
build "$ordinary"
prune "$work/backlog.json"

note "alternative-configuration build, backdated past the window"
entries >"$work/before-alternative"
build "$alternative"
entries >"$work/after-alternative"
comm -13 "$work/before-alternative" "$work/after-alternative" >"$work/alternative"
[ -s "$work/alternative" ] || {
  echo "measure: the alternative build created no build entry, so there is nothing superseded to time; check the selections in POOL_MAINTENANCE_EVIDENCE still reach oneharness-core's sdk-schema feature, then re-run" >&2
  exit 1
}
# Older than any real build, so the ordinary build stays each unit's newest
# even where step 3 rebuilds nothing.
while IFS= read -r entry; do
  find "$entry" -exec touch -h -t 200101010000 {} +
done <"$work/alternative"

note "ordinary gate build, then the timed pass"
build "$ordinary"
entries >"$work/before-pass"
prune "$work/pass.json"
entries >"$work/after-pass"

comm -23 "$work/before-pass" "$work/after-pass" >"$work/removed"
comm -23 "$work/removed" "$work/alternative" >"$work/foreign"
if [ -s "$work/foreign" ]; then
  cat "$work/foreign" >&2
  echo "measure: the timed pass removed the entries above, which the alternative build did not create; the steady state was not established; stop any other cargo build writing to $target (the measurement's own target directory), then re-run" >&2
  exit 1
fi
removed="$(wc -l <"$work/removed" | tr -d ' ')"
[ "$removed" -gt 0 ] || {
  echo "measure: the timed pass removed none of the alternative build's $(wc -l <"$work/alternative" | tr -d ' ') entries; check 'cargo-prune-superseded --version' names the release the justfile pins (else run 'just bootstrap'), then re-run" >&2
  exit 1
}
kept="$(comm -23 "$work/before-pass" "$work/alternative" | wc -l | tr -d ' ')"
note "removed $removed entries, every one the alternative build's; all $kept other entries remain"
# This repository's crates' entries, as "<profile> <unit>-<metadata hash>".
own() {
  grep -E '/(deps|\.fingerprint|build|incremental)/(lib)?oneharness[a-z0-9_-]*-[0-9a-f]{16}(\.rlib)?$' "$1" |
    sed -E 's#^(.*)/(deps|\.fingerprint|build|incremental)/(lib)?([^/]*)$#\1 \4#; s#\.rlib$##' | sort -u || true
}
comm -23 "$work/before-pass" "$work/alternative" >"$work/ordinary"
own "$work/ordinary" >"$work/own-before"
comm -23 "$work/after-pass" "$work/alternative" | own /dev/stdin >"$work/own-after"
own "$work/alternative" >"$work/own-alternative"
own "$work/after-pass" >"$work/own-survivors"
if ! cmp -s "$work/own-before" "$work/own-after"; then
  diff "$work/own-before" "$work/own-after" >&2 || true
  echo "measure: this repository's ordinary build hashes changed across the timed pass (above); stop any other cargo build writing to $target (the measurement's own target directory), then re-run" >&2
  exit 1
fi
count() { awk -v p="$1" '$1 == p' "$2" | wc -l | tr -d ' '; }
cut -d' ' -f1 "$work/own-before" "$work/own-alternative" | sort -u >"$work/profiles"
while IFS= read -r profile; do
  left="$(awk -v p="$profile" '$1 == p' "$work/own-alternative" | comm -12 - "$work/own-survivors" | wc -l | tr -d ' ')"
  note "$profile, this repository's crates: ordinary $(count "$profile" "$work/own-before") unit/hash entries before the pass, $(count "$profile" "$work/own-after") after (identical); alternative $(count "$profile" "$work/own-alternative"), $left left"
done <"$work/profiles"
sed 's/^/measure:   ordinary kept: /' "$work/own-before" >>"$evidence"

# The pruner's two reports are subprocess output: each must be the
# onebudgetspec shape, a finite non-negative `value` and a string `detail`.
node -e '
  const fs = require("node:fs");
  const [pass, backlog, out] = process.argv.slice(1);
  const read = (file, name) => {
    let report;
    try {
      report = JSON.parse(fs.readFileSync(file, "utf8"));
    } catch (error) {
      throw new Error(name + " pass report " + file + " is not JSON: " + error.message);
    }
    if (report === null || typeof report !== "object" || !Number.isFinite(report.value) || report.value < 0 || typeof report.detail !== "string") {
      throw new Error(name + " pass report " + file + " lacks a finite, non-negative value and a string detail: " + JSON.stringify(report));
    }
    return report;
  };
  const timed = read(pass, "timed");
  const untimed = read(backlog, "untimed backlog");
  const detail = timed.detail + "; untimed backlog pass beforehand (not counted): " + untimed.value.toFixed(3) + " s";
  fs.writeFileSync(out, JSON.stringify({ value: timed.value, detail }));
' "$work/pass.json" "$work/backlog.json" "$result" || {
  echo "measure: the pruner did not report through onebudgetspec as cargo-prune-superseded $(cargo-prune-superseded --version 2>/dev/null || echo '(absent)') should (above); run 'just bootstrap' to install the pinned release, then re-run" >&2
  exit 1
}
