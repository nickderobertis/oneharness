#!/usr/bin/env bash
# The shared Codex app-server's sustained-reuse workload: one pooled server
# serving sequential leases through oneharness-core's own pool (the
# app_server_reuse example around `PooledSession`, the lease, connection and
# release a `--control` dispatch takes) — each lease starting a thread, running
# one `thread/shellCommand` of `true` on it, resuming it once, holding it 1 s
# and releasing it, with no model turn. The shell turn makes no model call: it
# is there because codex-cli 0.159.2 writes a thread's rollout only after a
# turn and refuses to resume a thread without one. It
# writes the server's whole-tree resident memory after the first and the last
# release to target/e2e-telemetry/app-server-reuse/reuse.json (or
# $OH_E2E_REUSE_FILE), which live/control/budgets.yaml's
# codex-shared-server-rss-growth reads.
#
#   live/control/app-server-reuse.sh
#
# Each reading waits until the server holds none of the threads released so
# far (`thread/loaded/list`), for at most OH_REUSE_SETTLE_MS (default 90000):
# codex-cli 0.159.2 unloads a released thread 60 s after its last subscriber
# leaves, so an earlier reading would count threads already released. A thread
# still loaded at that bound fails the workload, named beside both readings.
#
# OH_REUSE_CODEX names the Codex executable (default: `codex` on PATH);
# OH_REUSE_LEASES the lease count (default 5). Linux only: the reading is /proc's.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
out="${OH_E2E_REUSE_FILE:-$root/target/e2e-telemetry/app-server-reuse/reuse.json}"
codex="${OH_REUSE_CODEX:-codex}"
leases="${OH_REUSE_LEASES:-5}"
settle="${OH_REUSE_SETTLE_MS:-90000}"
case "$settle" in [0-9] | [1-9][0-9] | [1-9][0-9][0-9] | [1-9][0-9][0-9][0-9] | [1-9][0-9][0-9][0-9][0-9] | [1-9][0-9][0-9][0-9][0-9][0-9]) ;; *)
  echo "app-server-reuse: OH_REUSE_SETTLE_MS must be whole milliseconds below 1000000, got '$settle'" >&2
  exit 2
  ;;
esac
case "$leases" in [1-9] | [1-9][0-9]) ;; *)
  echo "app-server-reuse: OH_REUSE_LEASES must be a whole number from 1 to 99, got '$leases'" >&2
  exit 2
  ;;
esac
command -v "$codex" >/dev/null 2>&1 || {
  echo "app-server-reuse: no Codex executable '$codex' on PATH; install codex or set OH_REUSE_CODEX" >&2
  exit 1
}
(cd "$root" && cargo build --quiet --locked -p oneharness-core --example app_server_reuse) || {
  echo "app-server-reuse: could not build the app_server_reuse example; fix cargo's error above (run 'just bootstrap' if the toolchain is missing), then rerun" >&2
  exit 1
}
# The server must outlive each settle: the pool's own linger (60 s) is shorter
# than Codex's unload grace, so the workload's server lingers longer.
export ONEHARNESS_INTERNAL_APP_SERVER_LINGER_MS="${ONEHARNESS_INTERNAL_APP_SERVER_LINGER_MS:-$((settle + 60000))}"
work="$(mktemp -d)" || {
  echo "app-server-reuse: cannot create a scratch directory; check that the host temp dir is writable" >&2
  exit 1
}
trap 'rm -rf "$work" || echo "app-server-reuse: could not remove $work; delete it by hand" >&2' EXIT
rm -f "$out" || {
  echo "app-server-reuse: cannot remove the previous telemetry $out; check its permissions" >&2
  exit 1
}
"$root/target/debug/examples/app_server_reuse" --codex "$codex" --leases "$leases" \
  --hold-ms 1000 --settle-ms "$settle" --cwd "$work" --out "$out"
echo "app-server reuse: wrote $out"
