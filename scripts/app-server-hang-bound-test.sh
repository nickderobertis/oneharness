#!/usr/bin/env bash
#
# Behavioral test of the per-test bound .config/nextest.toml sets on the shared
# app-server journeys (crates/oneharness-e2e/tests/app_server.rs). It runs the
# journey that hangs on purpose — `a_journey_that_never_ends_is_ended_by_the_suite_s_bound`,
# ignored in every ordinary run — through cargo nextest under the repository's
# config, as the gate's `test` target runs the suite, and holds the run to what
# the bound promises: it ends at the bound and no later, as a failure naming
# the journey, and leaves none of the processes the journey started running.
#
# Linux only: the bound is the same configuration on every platform, and this
# proof waits the bound out, which is cheapest here.
set -euo pipefail

if ! repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)" || ! cd "$repo_root"; then
  echo "app-server-hang-bound-test: could not enter the repository above ${BASH_SOURCE[0]}." >&2
  echo "  fix: run it from a checkout, as 'bash scripts/app-server-hang-bound-test.sh'." >&2
  exit 1
fi

probe="a_journey_that_never_ends_is_ended_by_the_suite_s_bound"
config=".config/nextest.toml"
rerun="rerun 'bash scripts/app-server-hang-bound-test.sh'"

if [ "$(uname -s)" != Linux ]; then
  echo "app-server-hang-bound-test: skipped: the bound is proven on Linux only"
  exit 0
fi

if ! work="$(mktemp -d)"; then
  echo "app-server-hang-bound-test: could not create its scratch directory under ${TMPDIR:-/tmp}." >&2
  echo "  fix: point TMPDIR at a writable directory with free space, then $rerun." >&2
  exit 1
fi
world=""
cleanup() {
  local status=$?
  # A run that failed may have left the journey's processes behind: each still
  # carrying its world in its environment is stopped before the world goes.
  local role pid
  if [ -n "$world" ] && [ -r "$work/manifest" ]; then
    while IFS='=' read -r role pid; do
      # Only a process the journey recorded, by a strictly positive decimal
      # pid, is one this may stop; any other manifest line is not acted on.
      case "$role" in test | dispatch | server | sweeper) ;; *) continue ;; esac
      [[ "$pid" =~ ^[1-9][0-9]*$ ]] || continue
      if grep -qaF "$world" "/proc/$pid/environ" 2>/dev/null; then
        kill -KILL "$pid" 2>/dev/null || true
      fi
    done <"$work/manifest"
  fi
  if ! rm -rf "$work" ${world:+"$world"}; then
    echo "app-server-hang-bound-test: could not remove its scratch; fix: rm -rf $work $world" >&2
    [ "$status" -ne 0 ] || status=1
  fi
  exit "$status"
}
trap cleanup EXIT

# fail <what went wrong> <the next action that case calls for>
fail() {
  echo "app-server-hang-bound-test: $1" >&2
  if [ -s "$work/out" ] && ! cat "$work/out" >&2; then
    echo "  (its captured output, $work/out, could not be read)" >&2
  fi
  echo "  fix: $2, then $rerun." >&2
  exit 1
}
config_fix="state the bound in $config as one [[profile.default.overrides]] for binary_id(oneharness-e2e::app_server) with slow-timeout = { period = \"<n>s\", terminate-after = <n> }"
bound_fix="make $config end the app-server journeys at its stated bound, as a TIMEOUT that names the journey"
probe_fix="make $probe (crates/oneharness-e2e/tests/app_server.rs) write dir= and its test, dispatch, server and sweeper pids to OH_HANG_PROBE_MANIFEST before it waits"

# The bound under test is the one the config states, period times
# terminate-after, read from the override for the app-server binary.
override="$(sed -n '/binary_id(oneharness-e2e::app_server)/,/^\[/p' "$config")" ||
  fail "could not read $config" "make $config a readable file in this checkout"
period="$(sed -n 's/.*period = "\([0-9]*\)s".*/\1/p' <<<"$override")" ||
  fail "could not parse the slow-timeout period out of $config" "$config_fix"
times="$(sed -n 's/.*terminate-after = \([0-9]*\).*/\1/p' <<<"$override")" ||
  fail "could not parse terminate-after out of $config" "$config_fix"
[ -n "$period" ] && [ -n "$times" ] ||
  fail "$config states no slow-timeout period and terminate-after for binary_id(oneharness-e2e::app_server)" "$config_fix"
bound=$((period * times))

# The plain run's selection, as scripts/cargo-test.sh runs it uninstrumented:
# built first, so the timed run below is the journey alone.
export RUSTFLAGS="${RUSTFLAGS:-} -D warnings"
select=(-p oneharness-e2e -p oneharness -p oneharness-mock-harness --features oneharness/mock-harness --locked)
cargo nextest run "${select[@]}" --no-run >"$work/out" 2>&1 ||
  fail "the app-server suite did not build" "fix the compile error above"

export OH_HANG_PROBE_MANIFEST="$work/manifest"
started=$SECONDS
# Its own bound too, past the one under test, so a config that lost the bound
# fails this test rather than hanging it.
if timeout --kill-after=10 "$((bound + 120))" cargo nextest run "${select[@]}" --run-ignored only \
  -E "binary_id(oneharness-e2e::app_server) & test(=$probe)" \
  --status-level fail --final-status-level fail >"$work/out" 2>&1; then
  fail "the journey that never ends passed" "restore $probe's endless wait in crates/oneharness-e2e/tests/app_server.rs: it must never return"
fi
took=$((SECONDS - started))

[ -s "$work/manifest" ] || fail "the journey never recorded what it started, so it did not reach its wait" "$probe_fix"
# The world is cleanup's to delete, so it is held only once it is validated:
# one directory directly under /tmp with the journey's prefix, and no other.
recorded="$(sed -n 's/^dir=//p' "$work/manifest")" ||
  fail "could not read the journey's manifest $work/manifest" "make TMPDIR (${TMPDIR:-/tmp}) readable and writable"
prefix="/tmp/oneharness-pool-hang-probe-"
suffix="${recorded#"$prefix"}"
case "$suffix" in
  "$recorded" | "" | */* | *..* | *$'\n'*) ;;
  *) world="$recorded" ;;
esac
[ -n "$world" ] ||
  fail "the journey recorded '$recorded' as its world, which is no scratch directory of its own" "$probe_fix, where dir= is its own ${prefix}* scratch directory"

grep -Eq "TIMEOUT .*app_server $probe\$" "$work/out" ||
  fail "the run did not fail as a TIMEOUT naming $probe" "$bound_fix"
# Ended at the bound: not before it, by the runner's own clock (so it is the
# bound that ended it), and within a minute after it by this one (nextest's
# grace period, and its own start).
ran="$(sed -nE "s/.*TIMEOUT \[ *([0-9]+)\.[0-9]+s\].*app_server $probe\$/\1/p" "$work/out" | head -n 1)" ||
  fail "could not read how long the runner ran $probe from its output" "$bound_fix"
[ -n "$ran" ] && [ "$ran" -ge "$bound" ] ||
  fail "the runner timed the journey out after ${ran:-an unread number of }s, before the ${bound}s bound" "$bound_fix"
[ "$took" -le "$((bound + 60))" ] || fail "the run ended after ${took}s, long past the ${bound}s bound" "$bound_fix"

# Nothing the journey started survives it: the journey itself, its dispatch,
# and the shared server and sweeper that dispatch left detached, which reclaim
# themselves once its lease is gone. A zombie counts as gone.
alive() {
  local stat
  stat="$(cat "/proc/$1/stat" 2>/dev/null)" || return 1
  stat="${stat##*) }"
  [ "${stat%% *}" != Z ]
}
pids="$(sed -nE 's/^(test|dispatch|server|sweeper)=([1-9][0-9]*)$/\1 \2/p' "$work/manifest")" ||
  fail "could not read the processes the journey recorded in $work/manifest" "make TMPDIR (${TMPDIR:-/tmp}) readable and writable"
for role in test dispatch server sweeper; do
  count="$(grep -c "^$role " <<<"$pids" || true)"
  [ "$count" -eq 1 ] || fail "the journey recorded $count pids for its $role, not one: $pids" "$probe_fix"
done
deadline=$((SECONDS + 30))
while :; do
  left=""
  while read -r role pid; do
    if alive "$pid"; then left+="${left:+, }$role $pid"; fi
  done <<<"$pids"
  [ -z "$left" ] && break
  [ "$SECONDS" -lt "$deadline" ] || fail "still running 30s after the run ended: $left" "make each process named above end with the journey: nextest kills the test's process group, and the detached server and sweeper reclaim themselves once the dispatch's lease is gone (crates/oneharness-core/src/io/server_pool.rs)"
  sleep 0.2
done
