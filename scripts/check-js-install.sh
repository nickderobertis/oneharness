#!/usr/bin/env bash
# Regression gate: `just check` must be self-sufficient for the checkout it is
# verifying, not just for a checkout somebody remembered to bootstrap.
#
# `node_modules` — the root Bun workspace's install, which carries the Node SDK's
# dependencies AND Nx itself — is the one per-checkout artifact `bootstrap`
# creates that every gate recipe needs: every other step writes machine-global
# state a new checkout inherits for free (rustup components, the cargo registry
# cache, the uv-installed llmlint, and `core.hooksPath` in the shared common git
# dir). It is also gitignored, so a fresh clone or `git worktree add` starts
# without it. When the SDK gate merely *used* those dependencies, `just gate`
# from the pre-push hook — which runs the gate directly, never `bootstrap` —
# died in every fresh worktree with ERR_MODULE_NOT_FOUND. CI never caught it
# because CI runs `just bootstrap` first.
#
# Every gate recipe reaches Nx through scripts/nx, so that is where the install
# happens. This asserts it hermetically: in a fresh checkout `just check`
# installs the locked workspace before Nx runs, a second run does not install
# again, a changed lockfile does, the install is quiet on success and keeps
# bun's own reason on failure, `bootstrap` reaches the same install, and the
# wrapper keeps Nx's stdout and exit status while surfacing failed-task logs. Only
# bun (and the Nx it would install) are stubbed; the real justfile, the real
# scripts/nx and the real node are what run.
set -euo pipefail

case $(uname -s) in
  MINGW* | MSYS* | CYGWIN*)
    echo "check-js-install: skipped on Windows because this Unix behavioral harness relies on extensionless executable stubs" >&2
    exit 0
    ;;
esac

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

install_line='bun install --frozen-lockfile'

fail() {
  echo "check-js-install: $1" >&2
  echo "  Restore the contract: scripts/nx installs the root Bun workspace ('$install_line')" >&2
  echo "  before Nx runs whenever the install is missing or older than bun.lock, quiet on" >&2
  echo "  success and loud on failure, and bootstrap reaches 'just js-install'; and it" >&2
  echo "  keeps Nx's stdout and exit status its own while replaying failed-task logs to stderr." >&2
  exit 1
}

# An isolated checkout holding just enough for the gate recipes and `bootstrap`
# to run: the real justfile, the real Nx wrapper and llmlint installer, and the
# workspace manifests the wrapper compares its install against.
fixture="$tmp/checkout"
mkdir -p "$fixture/scripts" "$fixture/.just-tmp"
cp "$root/justfile" "$root/package.json" "$root/bun.lock" "$fixture/"
cp "$root/scripts/nx" "$root/scripts/nx-base.sh" "$root/scripts/setup-llmlint.sh" "$fixture/scripts/"
# The pinned shell toolchain's installer downloads; scripts/check-shell-tools.sh
# drives the real one, so here it only records that bootstrap reached it.
cat >"$fixture/scripts/shell-tools.sh" <<'STUB'
printf 'shell-tools %s\n' "$*" >> "$CALL_LOG"
STUB
git -C "$fixture" init -q

bin="$tmp/bin"
mkdir -p "$bin"
# A bun whose `install` lays down an Nx that only records how it was called.
# Chatty on success, like the real one: that is exactly the noise the
# quiet-on-success assertion below would catch leaking into every gate run.
cat >"$bin/bun" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
printf 'bun %s\n' "$*" >> "$CALL_LOG"
if [ "${1:-}" = install ]; then
  mkdir -p node_modules/nx/dist/bin
  printf '{"name":"nx","bin":{"nx":"./dist/bin/nx.js"}}\n' > node_modules/nx/package.json
  printf 'require("fs").appendFileSync(process.env.CALL_LOG, "nx " + process.argv.slice(2).join(" ") + "\\n");\n' > node_modules/nx/dist/bin/nx.js
  echo "bun: 248 packages installed [596.00ms]"
fi
STUB
for tool in cargo rustup uv; do
  cat >"$bin/$tool" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
printf '%s %s\n' "$(basename "$0")" "$*" >> "$CALL_LOG"
STUB
done
chmod +x "$bin"/*
# `bootstrap` shells back out to `just js-install`, so the real `just` has to
# stay reachable through the trimmed PATH the stubs are served from, as do the
# real node and git the wrapper uses.
ln -s "$(command -v just)" "$bin/just"
ln -s "$(command -v node)" "$bin/node"
ln -s "$(command -v git)" "$bin/git"

run_recipe() {
  local log="$1"
  shift
  : >"$log"
  CALL_LOG="$log" PATH="$bin:/usr/bin:/bin" HOME="$tmp/home" \
    just --justfile "$fixture/justfile" --working-directory "$fixture" "$@" \
    >"$tmp/out" 2>"$tmp/err" || {
    cat "$tmp/err" >&2
    fail "'just $*' failed against the stubbed fixture"
  }
}

# The line number of the first call matching $2, or nothing when it never ran.
# "Never ran" is the case this gate exists to report, so it must return empty
# rather than let `pipefail` abort the script before the diagnostic below.
first_call() {
  grep -Fxn "$2" "$1" | head -1 | cut -d: -f1 || true
}
first_nx() {
  grep -n '^nx ' "$1" | head -1 | cut -d: -f1 || true
}

# A fresh checkout: no node_modules at all.
run_recipe "$tmp/fresh.calls" check all
installed_at="$(first_call "$tmp/fresh.calls" "$install_line")"
nx_at="$(first_nx "$tmp/fresh.calls")"
[[ -n "$installed_at" ]] ||
  fail "'just check all' never ran '$install_line' in a fresh checkout, so it assumes an already-bootstrapped one"
[[ -n "$nx_at" ]] || fail "'just check all' never reached Nx; this gate is checking the wrong recipe"
[[ "$installed_at" -lt "$nx_at" ]] ||
  fail "the workspace was installed at call $installed_at, after Nx ran at call $nx_at"
grep -q '^nx run-many --all ' "$tmp/fresh.calls" ||
  fail "'just check all' did not hand the full sweep to 'nx run-many --all'"
if grep -q 'packages installed' "$tmp/out" "$tmp/err"; then
  fail "the install printed on success; every gate run would carry that noise"
fi

# Installed and current: no second install.
run_recipe "$tmp/warm.calls" check all
[[ -z "$(first_call "$tmp/warm.calls" "$install_line")" ]] ||
  fail "a current install was installed again; every gate run would pay for it"

# A lockfile newer than the install: install again before Nx.
sleep 1
touch "$fixture/bun.lock"
run_recipe "$tmp/stale.calls" check all
[[ -n "$(first_call "$tmp/stale.calls" "$install_line")" ]] ||
  fail "a bun.lock newer than the install did not reinstall, so Nx would run on stale dependencies"

run_recipe "$tmp/bootstrap.calls" bootstrap
[[ -n "$(first_call "$tmp/bootstrap.calls" "$install_line")" ]] ||
  fail "bootstrap no longer reaches '$install_line'; a clean clone would be left without it"
[[ -n "$(first_call "$tmp/bootstrap.calls" 'uv sync --project python --frozen --no-install-workspace --quiet')" ]] ||
  fail "bootstrap no longer syncs the uv workspace; a clean clone would have no Python SDK environment"
[[ -n "$(first_call "$tmp/bootstrap.calls" 'shell-tools install')" ]] ||
  fail "bootstrap no longer installs the pinned shell toolchain; a clean clone could not run the shell format, lint or coverage targets"

# The wrapper's own streams: Nx's stdout (`show projects --json`, say) reaches
# the caller untouched while its stderr stays stderr, and a failed run keeps
# Nx's exit status and replays each advertised task log that is the file Nx
# itself would write for that hash. A "full log:" line is subprocess output, so
# one naming anything else — a file elsewhere, a symlink planted in the area, a
# `..` escape, a directory — is named and refused, never read; one this machine
# cannot read is named with how to see that task's output.
#
# Where task logs live is not stubbed: the wrapper asks Nx's own
# `terminalOutputPathForHash`, so the fixture forwards that one module to the
# real installed package, and its cache directory follows nx.json and
# NX_CACHE_DIRECTORY exactly as a real run's does.
real_nx_module="$root/node_modules/nx/dist/src/tasks-runner/terminal-output-path.js"
[[ -f "$real_nx_module" ]] ||
  fail "the installed Nx no longer ships $real_nx_module; scripts/nx locates task logs through it — update both"
summary="$root/node_modules/nx/dist/src/tasks-runner/life-cycles/summary-terminal-output-life-cycle.js"
# shellcheck disable=SC2016 # the JavaScript template literal is matched as text, not expanded
grep -qF 'full log: ${(0, terminal_output_path_1.terminalOutputPathForHash)(task.hash)}' "$summary" ||
  fail "the installed Nx no longer prints each failed task's 'full log:' path from terminalOutputPathForHash ($summary); update scripts/nx's replay to what it prints now"
printf '{}\n' >"$fixture/nx.json"
mkdir -p "$fixture/node_modules/nx/src/tasks-runner"
forward="$fixture/node_modules/nx/src/tasks-runner/terminal-output-path.js"
real_nx_literal="$(node -p 'JSON.stringify(process.argv[1])' "$real_nx_module")"
printf 'module.exports = require(%s);\n' "$real_nx_literal" >"$forward"
cat >"$fixture/node_modules/nx/dist/bin/nx.js" <<'STUB'
const fail = process.env.NX_STUB_FAIL;
process.stdout.write('["oneharness"]\n');
process.stderr.write("nx-stub: diagnostics\n");
if (fail) {
  process.stderr.write(fail.split(":").map((p) => `  full log: ${p}\n`).join(""));
  process.exit(3);
}
STUB
run_nx() {
  local status=0
  CALL_LOG="$tmp/nx.calls" PATH="$bin:/usr/bin:/bin" HOME="$tmp/home" \
    "$fixture/scripts/nx" show projects --json >"$tmp/nx.out" 2>"$tmp/nx.err" || status=$?
  echo "$status"
}
[[ "$(run_nx)" -eq 0 ]] || {
  cat "$tmp/nx.err" >&2
  fail "scripts/nx failed a passing Nx run"
}
[[ "$(cat "$tmp/nx.out")" == '["oneharness"]' ]] ||
  fail "scripts/nx changed Nx's stdout (got: $(cat "$tmp/nx.out")); machine-readable output must pass through untouched"
grep -qxF 'nx-stub: diagnostics' "$tmp/nx.err" || fail "scripts/nx lost Nx's stderr"

# One failed run advertising $1 (colon-separated paths): Nx's status survives,
# nothing reaches stdout, and nothing outside the area is read.
failed_run() {
  local nx_status
  NX_STUB_FAIL="$1"
  export NX_STUB_FAIL
  nx_status="$(run_nx)"
  unset NX_STUB_FAIL
  [[ "$nx_status" -eq 3 ]] || {
    cat "$tmp/nx.err" >&2
    fail "scripts/nx turned Nx's exit status 3 into $nx_status"
  }
  [[ "$(cat "$tmp/nx.out")" == '["oneharness"]' ]] || fail "a failed run's task-log replay leaked onto stdout"
  if grep -qF 'secret:' "$tmp/nx.err" || grep -q '^set ' "$tmp/nx.err"; then
    cat "$tmp/nx.err" >&2
    fail "scripts/nx read a file outside Nx's task-log area because a 'full log:' line named it"
  fi
}
refused() {
  grep -qF "the task log $1 is not a file in this workspace's Nx task-log area ($2)" "$tmp/nx.err" ||
    {
      cat "$tmp/nx.err" >&2
      fail "a 'full log:' path outside the task-log area ($1) was not named as refused"
    }
}

# Nx names paths from its physical working directory (macOS's temp dir sits
# behind the /var -> /private/var link), so the fixture's paths are spelled the
# same way.
fixture_phys="$(cd "$fixture" && pwd -P)"
area="$fixture_phys/.nx/cache/terminalOutputs"
mkdir -p "$area/subdir" "$tmp/task-logs"
echo 'error[E0425]: the failing task output' >"$area/1234567890"
echo 'secret: a file outside the task-log area' >"$tmp/task-logs/outside.log"
ln -s "$tmp/task-logs/outside.log" "$area/4242"
failed_run "$area/1234567890:$area/9999:$tmp/task-logs/outside.log:$area/4242:$area/../../../justfile:$area/subdir"
grep -qxF 'error[E0425]: the failing task output' "$tmp/nx.err" ||
  fail "a failed run did not replay the advertised task log to stderr"
grep -qF "the task log $area/9999 is not readable on this machine; re-run the failed task" "$tmp/nx.err" ||
  fail "an advertised task log this machine cannot read was skipped silently"
for outside in "$tmp/task-logs/outside.log" "$area/4242" "$area/../../../justfile" "$area/subdir"; do
  refused "$outside" "$area"
done

# A cache directory configured through NX_CACHE_DIRECTORY moves the area with
# it: a log there is replayed, and one at the default location is refused.
custom="$tmp/custom-cache/terminalOutputs"
mkdir -p "$custom"
echo 'error: output kept in the configured cache' >"$custom/777"
NX_CACHE_DIRECTORY="$tmp/custom-cache" failed_run "$custom/777:$area/1234567890"
grep -qxF 'error: output kept in the configured cache' "$tmp/nx.err" ||
  fail "a task log in the NX_CACHE_DIRECTORY cache was not replayed"
grep -q '^error\[E0425\]' "$tmp/nx.err" && fail "a log outside the configured cache directory was replayed"
refused "$area/1234567890" "$custom"

# A cache directory configured in nx.json moves the area the same way.
printf '{"cacheDirectory": "configured-cache"}\n' >"$fixture/nx.json"
mkdir -p "$fixture/configured-cache/terminalOutputs"
echo 'error: output kept in the nx.json cache' >"$fixture/configured-cache/terminalOutputs/616"
failed_run "$fixture_phys/configured-cache/terminalOutputs/616:$area/1234567890"
grep -qxF 'error: output kept in the nx.json cache' "$tmp/nx.err" ||
  {
    cat "$tmp/nx.err" >&2
    fail "a task log in nx.json's cacheDirectory was not replayed"
  }
grep -q '^error\[E0425\]' "$tmp/nx.err" && fail "a log outside nx.json's cacheDirectory was replayed"
refused "$area/1234567890" "$fixture_phys/configured-cache/terminalOutputs"
printf '{}\n' >"$fixture/nx.json"

# A task-log area that is itself a link elsewhere authorizes nothing, and an
# authorized log this machine cannot read says why.
mkdir -p "$tmp/linked-cache" "$tmp/elsewhere"
echo 'secret: behind a linked task-log area' >"$tmp/elsewhere/555"
ln -s "$tmp/elsewhere" "$tmp/linked-cache/terminalOutputs"
NX_CACHE_DIRECTORY="$tmp/linked-cache" failed_run "$tmp/linked-cache/terminalOutputs/555"
refused "$tmp/linked-cache/terminalOutputs/555" "$tmp/linked-cache/terminalOutputs"
# A link further up — the workspace's `.nx`, or the cache directory itself —
# moves the area just as surely, so it authorizes nothing either.
mv "$fixture/.nx" "$tmp/real-dot-nx"
ln -s "$tmp/real-dot-nx" "$fixture/.nx"
failed_run "$area/1234567890"
refused "$area/1234567890" "$area"
rm "$fixture/.nx"
mv "$tmp/real-dot-nx" "$fixture/.nx"
mkdir -p "$tmp/elsewhere-cache/terminalOutputs"
echo 'secret: behind a linked cache directory' >"$tmp/elsewhere-cache/terminalOutputs/808"
ln -s "$tmp/elsewhere-cache" "$fixture/linked-cache"
NX_CACHE_DIRECTORY=linked-cache failed_run "$fixture_phys/linked-cache/terminalOutputs/808"
refused "$fixture_phys/linked-cache/terminalOutputs/808" "$fixture_phys/linked-cache/terminalOutputs"
rm "$fixture/linked-cache"
echo 'locked' >"$area/31337"
chmod 000 "$area/31337"
if [[ ! -r "$area/31337" ]]; then
  failed_run "$area/31337"
  grep -qF "the task log $area/31337 is Nx's own but could not be read (EACCES" "$tmp/nx.err" ||
    {
      cat "$tmp/nx.err" >&2
      fail "an unreadable task log was not reported with the read error"
    }
fi
chmod 600 "$area/31337"

# An Nx that cannot say where it keeps logs: Nx's status still stands and the
# log is named with the next action, not a stack trace.
rm "$forward"
failed_run "$area/1234567890"
grep -qF "the task log $area/1234567890 is not read: the installed Nx did not say where it keeps task logs" "$tmp/nx.err" ||
  {
    cat "$tmp/nx.err" >&2
    fail "a missing Nx task-log module was not reported with its next action"
  }
grep -q 'at Module\|node:internal' "$tmp/nx.err" && fail "a missing Nx task-log module printed a stack trace"

# A replay that itself fails — here a node that dies running it — is named,
# and Nx's own status and stdout still stand rather than the replay's.
crash_bin="$tmp/crash-bin"
mkdir -p "$crash_bin"
cp "$bin"/bun "$bin"/cargo "$bin"/rustup "$bin"/uv "$crash_bin/"
for tool in just git; do ln -s "$(command -v "$tool")" "$crash_bin/$tool"; done
cat >"$crash_bin/node" <<STUB
#!/usr/bin/env bash
case "\$*" in *terminalOutputPathForHash*) echo 'replay-node: killed' >&2; exit 9 ;; esac
exec "$(command -v node)" "\$@"
STUB
chmod +x "$crash_bin/node"
status=0
NX_STUB_FAIL="$area/1234567890" CALL_LOG="$tmp/nx.calls" PATH="$crash_bin:/usr/bin:/bin" HOME="$tmp/home" \
  "$fixture/scripts/nx" show projects --json >"$tmp/nx.out" 2>"$tmp/nx.err" || status=$?
[[ "$status" -eq 3 ]] || {
  cat "$tmp/nx.err" >&2
  fail "a failed task-log replay turned Nx's exit status 3 into $status"
}
[[ "$(cat "$tmp/nx.out")" == '["oneharness"]' ]] || fail "a failed task-log replay changed Nx's stdout"
grep -qF "nx: replaying the failed tasks' logs failed (above); the run's own result stands" "$tmp/nx.err" ||
  {
    cat "$tmp/nx.err" >&2
    fail "a failed task-log replay was not named"
  }

# A bun that fails the way a stale lockfile really does: the reason survives,
# Nx never runs, and the message names a next action.
failing_bin="$tmp/failing-bin"
mkdir -p "$failing_bin"
cat >"$failing_bin/bun" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
printf 'bun %s\n' "$*" >> "$CALL_LOG"
echo 'error: lockfile had changes, but lockfile is frozen' >&2
exit 1
STUB
chmod +x "$failing_bin/bun"
for tool in just node git; do ln -s "$(command -v "$tool")" "$failing_bin/$tool"; done
rm -rf "$fixture/node_modules"
: >"$tmp/loud.calls"
status=0
CALL_LOG="$tmp/loud.calls" PATH="$failing_bin:/usr/bin:/bin" HOME="$tmp/home" \
  just --justfile "$fixture/justfile" --working-directory "$fixture" check all \
  >"$tmp/out" 2>"$tmp/err" || status=$?
[[ "$status" -ne 0 ]] ||
  fail "a failing '$install_line' left 'just check all' green; the gate would run on absent dependencies"
grep -qF 'error: lockfile had changes, but lockfile is frozen' "$tmp/err" || {
  cat "$tmp/err" >&2
  fail "the failed install swallowed bun's own output; the reason must survive to the reader"
}
grep -qF "just bootstrap" "$tmp/err" || fail "the failed install named no next action"
if grep -q '^nx ' "$tmp/loud.calls"; then
  fail "Nx ran after the workspace install failed"
fi

echo "check-js-install: ok"
