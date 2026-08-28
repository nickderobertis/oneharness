#!/usr/bin/env bash
#
# Live check of the release probe against the real public registries.
#
# Two of its three answers can only come from a registry, so this is the only
# place they are proven: the version answer, driven against every declared
# target, and the empty answer, driven against a name no registry has ever
# served. Network-touching and therefore opt-in — out of `just check` and CI,
# like `just smoke-live`. scripts/check-release-probe.sh holds the offline half
# (every refusal) inside the gate.
#
# It also holds the two conditions the probe promises a caller: it runs under an
# environment carrying nothing but PATH and HOME — no credential, nothing the
# caller happened to be holding — and every answer lands well inside sixty
# seconds.
#
# Quiet on success, one line. On failure it names the target and what to do.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

BOUND_SECONDS=60
DECLARATION_SCHEMA_VERSION=1
probe=scripts/release-probe.sh

fail() {
  echo "release-probe-live: $1" >&2
  [ -s "$work/err" ] && { echo "  the probe's stderr:" >&2; cat "$work/err" >&2; }
  exit 1
}

# Runs a probe with only PATH and HOME, timing it. $1 = script, $2 = identifier.
# Sets `status` and `elapsed`, and leaves stdout in $work/out.
#
# llmlint: ignore-block[live_tier_compiles_and_requires_credential] This live
# tier must NOT require a credential, and asserting its absence is half of what
# it exists to prove: the probe's contract is that a consumer may spawn it with
# an environment carrying only PATH and HOME, holding no credential of any
# kind, because every target is on a public registry an unauthenticated read
# reaches. A fail-fast credential check here would assert the opposite of the
# contract, and `env -i` below is what holds the real one in place.
probe_run() {
  local started=$SECONDS
  status=0
  env -i PATH="$PATH" HOME="$HOME" "$1" "$2" >"$work/out" 2>"$work/err" || status=$?
  elapsed=$((SECONDS - started))
}
# llmlint: ignore-end[live_tier_compiles_and_requires_credential]

assert_within_bound() {
  [ "$elapsed" -lt "$BOUND_SECONDS" ] ||
    fail "'$1' took ${elapsed}s, past the ${BOUND_SECONDS}s a caller may assume; lower the --max-time/--retry budget in $probe so it refuses instead of overrunning"
}

declared_version="$(sed -n 's/^schema_version = \([0-9]*\)$/\1/p' release-targets.toml)"
[ "$declared_version" = "$DECLARATION_SCHEMA_VERSION" ] ||
  fail "release-targets.toml declares schema_version '$declared_version' and this suite reads exactly one, version $DECLARATION_SCHEMA_VERSION; leave a single schema_version line, then bring whichever side is behind up to it"
declared="$(awk '
  /^\[\[target\]\]$/ { inside = 1; next }
  inside && match($0, /^id = "[^"]+"$/) {
    entry = $0; sub(/^id = "/, "", entry); sub(/"$/, "", entry)
    print entry; inside = 0
  }
' release-targets.toml)"
[ -n "$declared" ] || fail "release-targets.toml declares no targets to probe; restore its [[target]] entries"

answered=0
slowest=0
while read -r id; do
  [ -n "$id" ] || continue
  probe_run "$probe" "$id"
  [ "$status" -eq 0 ] ||
    fail "'$id' was not answered; every declared target is published, so read the reason below and fix whichever it names — the registry, or this target's name in release-targets.toml"
  version="$(cat "$work/out")"
  [ -n "$version" ] ||
    fail "'$id' answered 'no release yet', but this repository has published it; check that name on its registry, then correct release-targets.toml if the registry serves it under a different one"
  assert_within_bound "$id"
  [ "$elapsed" -le "$slowest" ] || slowest=$elapsed
  answered=$((answered + 1))
done < <(printf '%s\n' "$declared")

# The empty answer, one per registry. It needs a target the registry has never
# served, and every target this repository declares has been published — so the
# real script is driven from a staged checkout whose declaration names one
# never-published artifact per registry. Nothing about the probe is stubbed;
# only what it is asked about changes.
fixture="$work/fixture"
mkdir -p "$fixture/scripts"
cp "$probe" "$fixture/scripts/release-probe.sh"
cat >"$fixture/release-targets.toml" <<'FIXTURE'
schema_version = 1

[[target]]
id = "crate:oneharness-release-probe-absent-fixture"
manifest = "Cargo.toml"

[[target]]
id = "pypi:oneharness-release-probe-absent-fixture"
manifest = "pyproject.toml"

[[target]]
id = "npm:@oneharness/release-probe-absent-fixture"
manifest = "npm/oneharness/package.json"

[[target]]
id = "gem:oneharness-release-probe-unreadable-fixture"
manifest = "Cargo.toml"
FIXTURE

unserved=0
for id in \
  crate:oneharness-release-probe-absent-fixture \
  pypi:oneharness-release-probe-absent-fixture \
  npm:@oneharness/release-probe-absent-fixture; do
  probe_run "$fixture/scripts/release-probe.sh" "$id"
  [ "$status" -eq 0 ] ||
    fail "'$id' was refused; a registry answering 'this does not exist' is the empty answer, so teach $probe to read that registry's 404 as HTTP 404 rather than as a failure"
  [ ! -s "$work/out" ] ||
    fail "'$id' answered '$(cat "$work/out")'; that name was chosen because no registry serves it, so either somebody published it — pick another — or $probe is reading the wrong name"
  assert_within_bound "$id"
  [ "$elapsed" -le "$slowest" ] || slowest=$elapsed
  unserved=$((unserved + 1))
done

# A declared target on a registry this probe cannot read is refused rather than
# reported as unreleased — the same rule, reached from the declaration side.
probe_run "$fixture/scripts/release-probe.sh" gem:oneharness-release-probe-unreadable-fixture
[ "$status" -ne 0 ] ||
  fail "a target on an unreadable registry was answered; restore the '*)' arm of the registry dispatch in $probe so an unsupported registry is refused instead of guessed at"
[ ! -s "$work/out" ] ||
  fail "a refusal wrote '$(cat "$work/out")' to stdout; a refusal says nothing there — in $probe, make that branch end through unanswered/usage_error before anything is printed"

printf 'release-probe-live: %d declared target(s) answer a version, %d unserved name(s) answer nothing, an unreadable registry is refused (slowest %ds of %ds)\n' \
  "$answered" "$unserved" "$slowest" "$BOUND_SECONDS"
