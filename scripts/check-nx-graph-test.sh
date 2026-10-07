#!/usr/bin/env bash
#
# Behavioral test of the project-graph boundary check (scripts/check-nx-graph.mjs).
#
# A green run of the real check proves nothing about what it refuses, so this
# copies the working tree to a scratch workspace, runs the real check there once
# warm, then gives one project at a time each edge the boundaries forbid —
# a dependency on a live suite, on an exploration probe, on the binary e2e
# journeys, and from the SDK contract to an SDK that consumes it — plus a crate
# edge Cargo has and the graph lost, and holds the check to rejecting each with
# that edge named — and the AGENTS.md project record and the Rust coverage
# floor's list of test runs falling out of step with the graph. Nx computes every
# graph here for real; nothing is stubbed.
#
# Quiet on success, one line.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

if [ ! -d node_modules/nx ]; then
  echo "check-nx-graph-test: Nx is not installed in this checkout; run 'just bootstrap' (or 'bun install --frozen-lockfile') and re-run." >&2
  exit 1
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
ws="$tmp/ws"
mkdir -p "$ws"

# The working tree as it stands — tracked and new files, never ignored build
# output — so the check is exercised against exactly what is about to be pushed.
git ls-files -z --cached --others --exclude-standard |
  while IFS= read -r -d '' f; do [ -e "$f" ] && printf '%s\0' "$f"; done |
  tar --null -T - -cf - | tar -xf - -C "$ws"
ln -s "$root/node_modules" "$ws/node_modules"

fail() {
  echo "check-nx-graph-test: $1" >&2
  [ -s "$tmp/err" ] && sed 's/^/  check said: /' "$tmp/err" >&2
  exit 1
}

check() {
  status=0
  (cd "$ws" && NX_DAEMON=false NX_NO_CLOUD=true node scripts/check-nx-graph.mjs) >"$tmp/out" 2>"$tmp/err" || status=$?
}

# $1 project.json, $2 the dependency to add to its implicitDependencies.
add_edge() {
  node -e '
    const fs = require("fs");
    const [file, dependency] = process.argv.slice(1);
    const project = JSON.parse(fs.readFileSync(file, "utf8"));
    project.implicitDependencies = [...(project.implicitDependencies ?? []), dependency];
    fs.writeFileSync(file, JSON.stringify(project, null, 2) + "\n");
  ' "$ws/$1" "$2"
}

check
[ "$status" -eq 0 ] || fail "the unmodified tree should pass the boundary check"
grep -q '^check-nx-graph: ok' "$tmp/out" || fail "a passing check should say so in one line"

# $1 project.json, $2 dependency, $3 the edge the refusal must name.
expect_refused() {
  cp "$ws/$1" "$tmp/saved.json"
  add_edge "$1" "$2"
  check
  cp "$tmp/saved.json" "$ws/$1"
  [ "$status" -ne 0 ] || fail "a dependency $3 should have failed the boundary check"
  grep -qF -- "$3" "$tmp/err" || fail "the check failed but did not name the edge '$3'"
}

expect_refused npm/oneharness-sdk/project.json live-claude "node-sdk (type:sdk) -> live-claude (type:live)"
expect_refused src/project.json explore-events "oneharness (type:app) -> explore-events (type:explore)"
expect_refused tools/release-tooling/project.json oneharness-e2e "release-tooling (type:tooling) -> oneharness-e2e (type:e2e)"
expect_refused crates/sdk-contract/project.json node-sdk "sdk-contract (type:contract) -> node-sdk (type:sdk)"
expect_refused crates/sdk-contract/project.json python-sdk "sdk-contract (type:contract) -> python-sdk (type:sdk)"

# A crate edge Cargo resolves but the graph does not carry: affected selection
# would skip the dependent crate when its dependency changes.
cp "$ws/crates/history-compat/project.json" "$tmp/saved.json"
node -e '
  const fs = require("fs");
  const file = process.argv[1];
  const project = JSON.parse(fs.readFileSync(file, "utf8"));
  project.implicitDependencies = project.implicitDependencies.filter((d) => d !== "oneharness-core");
  fs.writeFileSync(file, JSON.stringify(project, null, 2) + "\n");
' "$ws/crates/history-compat/project.json"
check
cp "$tmp/saved.json" "$ws/crates/history-compat/project.json"
[ "$status" -ne 0 ] || fail "a Cargo path dependency missing from the graph should have failed the check"
grep -qF 'no history-compat -> oneharness-core edge' "$tmp/err" ||
  fail "the check failed but did not name the missing history-compat -> oneharness-core edge"

# The two restatements of the project set: a project the AGENTS.md record does
# not name, and a Rust test run the coverage floor does not read.
cp "$ws/AGENTS.md" "$tmp/saved.md"
sed 's/.harness-captures., //' "$tmp/saved.md" >"$ws/AGENTS.md"
check
cp "$tmp/saved.md" "$ws/AGENTS.md"
[ "$status" -ne 0 ] || fail "an AGENTS.md project record missing harness-captures should have failed the check"
grep -qF 'record does not name harness-captures' "$tmp/err" ||
  fail "the check failed but did not name the project the AGENTS.md record leaves out"

cp "$ws/tools/rust-coverage/project.json" "$tmp/saved.json"
node -e '
  const fs = require("fs");
  const file = process.argv[1];
  const project = JSON.parse(fs.readFileSync(file, "utf8"));
  const options = project.targets.coverage.options;
  options.commands = options.commands.map((c) => c.replace(" oneharness-e2e", ""));
  fs.writeFileSync(file, JSON.stringify(project, null, 2) + "\n");
' "$ws/tools/rust-coverage/project.json"
check
cp "$tmp/saved.json" "$ws/tools/rust-coverage/project.json"
[ "$status" -ne 0 ] || fail "a coverage floor that no longer reads oneharness-e2e's profile should have failed the check"
grep -qF "does not read oneharness-e2e's profile (oneharness-e2e)" "$tmp/err" ||
  fail "the check failed but did not name the Rust test run the floor leaves out"

echo "check-nx-graph-test: ok"
