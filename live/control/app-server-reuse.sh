#!/usr/bin/env bash
# The shared Codex app-server's sustained-reuse workload: one pooled server
# serving sequential leases through oneharness-core's own pool (the
# app_server_reuse example around `PooledSession`, the lease, connection and
# release a `--control` dispatch takes) — each lease starting a thread, running
# one `thread/shellCommand` of `true` on it, resuming it once, holding it 1 s
# and releasing it, with no model turn. The shell turn makes no model call: it
# is there because codex-cli 0.159.2 writes a thread's rollout only after a
# turn and refuses to resume a thread without one. An idle lease of the
# workload's own holds the server from the first lease to the last, so however
# short the pool's linger, no gap between two leases stops it. It
# writes the server's whole-tree resident memory after the first and the last
# release to target/e2e-telemetry/app-server-reuse/reuse.json (or
# $OH_E2E_REUSE_FILE), which live/control/budgets.yaml's
# codex-shared-server-rss-growth reads.
#
#   live/control/app-server-reuse.sh
#
# Each reading waits until the server holds none of the threads released so
# far (`thread/loaded/list`), for at most OH_REUSE_SETTLE_MS (default 90000),
# so it never counts a thread already released. A thread still loaded at that
# bound fails the workload, named beside both readings.
#
# Codex unloads a thread once it has had no subscriber and no activity for
# `thread_unload_delay_secs`: by default 60 s on codex-cli 0.159.2 and 1800 s
# (30 minutes) on codex-cli 0.163.0, which answers the release's
# `thread/unsubscribe` with `unsubscribed` and then keeps the thread loaded for
# those 30 minutes. The workload's server is launched with
# `-c thread_unload_delay_secs=1`, so Codex's unload of a released thread lands
# within the bound; a release that leaves its thread subscribed still fails
# here, since Codex never unloads a subscribed thread whatever the delay. A
# dispatch's own server keeps Codex's default, so on 0.163.0 it holds each
# released thread for 30 minutes: retention bounded by that delay, not grown
# for the server's whole life.
#
# OH_REUSE_CODEX names the Codex executable (default: `codex` on PATH);
# OH_REUSE_LEASES the lease count (default 5). The reading is one `ps` listing's.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)" || {
  echo "app-server-reuse: cannot resolve the repository root from ${BASH_SOURCE[0]}; run it from a checkout" >&2
  exit 1
}
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
# The example runs from wherever this build put it, which cargo's own artifact
# message names: a caller's CARGO_TARGET_DIR (cargo llvm-cov's
# target/llvm-cov-target, say) moves it out of target/debug. Only those JSON
# messages reach stdout: `json-render-diagnostics` has cargo render compiler
# errors as text on stderr, which this capture leaves on the terminal.
artifacts="$(cd "$root" && cargo build --quiet --locked -p oneharness-core --example app_server_reuse \
  --message-format json-render-diagnostics)" || {
  echo "app-server-reuse: could not build the app_server_reuse example; fix cargo's error above (run 'just bootstrap' if the toolchain is missing), then rerun" >&2
  exit 1
}
command -v node >/dev/null 2>&1 || {
  echo "app-server-reuse: no node on PATH to read cargo's artifact message; run 'just bootstrap', then rerun" >&2
  exit 1
}
example="$(printf '%s\n' "$artifacts" | node -e '
  let input = "";
  process.stdin.on("data", (d) => { input += d; }).on("end", () => {
    let executable = "";
    for (const line of input.split("\n")) {
      if (!line.startsWith("{")) continue;
      const message = JSON.parse(line);
      if (message.reason === "compiler-artifact" && message.target?.name === "app_server_reuse" && typeof message.executable === "string" && message.executable !== "") executable = message.executable;
    }
    process.stdout.write(executable);
  });
')" || {
  echo "app-server-reuse: cargo's artifact message for the app_server_reuse example is not JSON node can read; its error is above. Run 'cargo build -p oneharness-core --example app_server_reuse --message-format json-render-diagnostics' from $root to see the line it rejected, and check 'cargo --version' against rust-toolchain.toml" >&2
  exit 1
}
[ -n "$example" ] || {
  echo "app-server-reuse: cargo built the app_server_reuse example but its output named no executable for it; check 'cargo --version' against rust-toolchain.toml, then rerun" >&2
  exit 1
}
work="$(mktemp -d)" || {
  echo "app-server-reuse: cannot create a scratch directory; check that the host temp dir is writable" >&2
  exit 1
}
trap 'rm -rf "$work" || echo "app-server-reuse: could not remove $work; delete it by hand" >&2' EXIT
# Run a copy of its own: the path cargo names is shared by every build in that
# target directory, and one that re-links it — removing the file, then linking
# it again — leaves it missing for a moment, which a run that looked once (as
# on macOS CI, beside other builds of the same checkout) took for gone. A file
# missing for longer than the wait is reported as gone.
copied="$work/app_server_reuse"
copy_error=""
for ((tries = 0; tries < 50; tries++)); do
  if [ -x "$example" ]; then
    copy_error="$(cp "$example" "$copied" 2>&1)" && break
  fi
  rm -f "$copied" || {
    echo "app-server-reuse: cannot remove the partial copy $copied; check its permissions" >&2
    exit 1
  }
  sleep 0.1
done
[ -x "$copied" ] || {
  if [ -n "$copy_error" ]; then
    echo "app-server-reuse: cannot copy the app_server_reuse example $example into $work: $copy_error; check both paths' permissions and free space, then rerun" >&2
  else
    echo "app-server-reuse: cargo named $example as the app_server_reuse example, but no executable is there; check that nothing removed it from its target directory, then rerun" >&2
  fi
  exit 1
}
example="$copied"
rm -f "$out" || {
  echo "app-server-reuse: cannot remove the previous telemetry $out; check its permissions" >&2
  exit 1
}
"$example" --codex "$codex" --leases "$leases" \
  --hold-ms 1000 --settle-ms "$settle" --cwd "$work" --out "$out" || {
  echo "app-server-reuse: the workload failed; its error is above. Threads still loaded at the bound: the server kept released threads past OH_REUSE_SETTLE_MS (${settle} ms) though its unload delay is 1 s — a release left them subscribed, or this Codex ignores thread_unload_delay_secs; report a retention. Anything else: check '$codex --version' and 'codex login status', then rerun" >&2
  exit 1
}
echo "app-server reuse: wrote $out"
