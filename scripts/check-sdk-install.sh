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
# bun's own reason on failure, and `bootstrap` reaches the same install. Only
# bun (and the Nx it would install) are stubbed; the real justfile, the real
# scripts/nx and the real node are what run.
set -euo pipefail

case $(uname -s) in
  MINGW* | MSYS* | CYGWIN*)
    echo "check-sdk-install: skipped on Windows because this Unix behavioral harness relies on extensionless executable stubs" >&2
    exit 0
    ;;
esac

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

install_line='bun install --frozen-lockfile'

fail() {
    echo "check-sdk-install: $1" >&2
    echo "  Restore the contract: scripts/nx installs the root Bun workspace ('$install_line')" >&2
    echo "  before Nx runs whenever the install is missing or older than bun.lock, quiet on" >&2
    echo "  success and loud on failure, and bootstrap reaches 'just js-install'." >&2
    exit 1
}

# An isolated checkout holding just enough for the gate recipes and `bootstrap`
# to run: the real justfile, the real Nx wrapper and llmlint installer, and the
# workspace manifests the wrapper compares its install against.
fixture="$tmp/checkout"
mkdir -p "$fixture/scripts" "$fixture/.just-tmp"
cp "$root/justfile" "$root/package.json" "$root/bun.lock" "$fixture/"
cp "$root/scripts/nx" "$root/scripts/nx-base.sh" "$root/scripts/setup-llmlint.sh" "$fixture/scripts/"
git -C "$fixture" init -q

bin="$tmp/bin"
mkdir -p "$bin"
# A bun whose `install` lays down an Nx that only records how it was called.
# Chatty on success, like the real one: that is exactly the noise the
# quiet-on-success assertion below would catch leaking into every gate run.
cat >"$bin/bun" <<'STUB'
#!/usr/bin/env bash
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

# A bun that fails the way a stale lockfile really does: the reason survives,
# Nx never runs, and the message names a next action.
failing_bin="$tmp/failing-bin"
mkdir -p "$failing_bin"
cat >"$failing_bin/bun" <<'STUB'
#!/usr/bin/env bash
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

echo "check-sdk-install: ok"
