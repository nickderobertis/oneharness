#!/usr/bin/env bash
# Regression gate: `just check` must be self-sufficient for the checkout it is
# verifying, not just for a checkout somebody remembered to bootstrap.
#
# `npm/oneharness-sdk/node_modules` is the ONLY per-checkout artifact `bootstrap`
# creates — every other step writes machine-global state a new checkout inherits
# for free (rustup components, the cargo registry cache, the uv-installed
# llmlint, and `core.hooksPath` in the shared common git dir). It is also
# gitignored, so a fresh clone or `git worktree add` starts without it. When
# `sdk-check` merely *used* those dependencies, `just gate` from the pre-push
# hook — which runs the gate directly, never `bootstrap` — died in every fresh
# worktree on `bun run --cwd npm/oneharness-sdk generate:check` with
# ERR_MODULE_NOT_FOUND. CI never caught it because CI runs `just bootstrap`
# first.
#
# So this asserts the recipe wiring, hermetically: `sdk-check` installs the Node
# SDK dependencies into the checkout before consuming them, and `bootstrap`
# reaches that same install, so the two can never drift apart again. Only the
# external package managers are stubbed; the real justfile is what runs.
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

install_line='bun install --cwd npm/oneharness-sdk --frozen-lockfile'
consume_line='bun run --cwd npm/oneharness-sdk generate:check'

fail() {
    echo "check-sdk-install: $1" >&2
    echo "  Restore sdk-install's contract: sdk-check and bootstrap both reach 'just sdk-install'" >&2
    echo "  before anything reads node_modules, and it stays quiet on success, loud on failure." >&2
    exit 1
}

# An isolated checkout holding just enough for `sdk-check` and `bootstrap` to
# run: the real justfile, the real llmlint installer, and an empty SDK directory.
fixture="$tmp/checkout"
mkdir -p "$fixture/scripts" "$fixture/npm/oneharness-sdk"
cp "$root/justfile" "$fixture/justfile"
cp "$root/scripts/setup-llmlint.sh" "$fixture/scripts/setup-llmlint.sh"
# `sdk-check` runs its test steps under the scratch-leak gate, which reads the
# prefix out of the Rust guard — so the fixture carries both, real, rather than
# stubbing a step of the recipe this gate exists to run for real.
mkdir -p "$fixture/crates/oneharness-core/src/io"
cp "$root/scripts/check-temp-leaks.sh" "$fixture/scripts/check-temp-leaks.sh"
cp "$root/crates/oneharness-core/src/io/scratch.rs" \
    "$fixture/crates/oneharness-core/src/io/scratch.rs"
git -C "$fixture" init -q

# The leak gate watches this instead of the host's temp directory, so a real
# `oneharness` run happening elsewhere on the machine cannot fail this harness.
scratch_root="$tmp/scratch"
mkdir -p "$scratch_root"

bin="$tmp/bin"
mkdir -p "$bin"
# Chatty on success, like the real tools: `bun install` prints a package list
# every time. That is exactly the noise the quiet-on-success assertion below
# would catch leaking into every `just check`.
for tool in bun cargo rustup uv; do
    cat >"$bin/$tool" <<'STUB'
#!/usr/bin/env bash
printf '%s %s\n' "$(basename "$0")" "$*" >> "$CALL_LOG"
echo "$(basename "$0"): 26 packages installed [127.00ms]"
STUB
    chmod +x "$bin/$tool"
done
# `bootstrap` shells back out to `just sdk-install`, so the real `just` has to
# stay reachable through the trimmed PATH the stubs are served from.
ln -s "$(command -v just)" "$bin/just"

run_recipe() {
    local recipe="$1"
    local log="$tmp/$recipe.calls"
    : >"$log"
    CALL_LOG="$log" PATH="$bin:/usr/bin:/bin" HOME="$tmp/home" \
        OH_SCRATCH_ROOTS="$scratch_root" \
        just --justfile "$fixture/justfile" --working-directory "$fixture" "$recipe" \
        >"$tmp/$recipe.out" 2>"$tmp/$recipe.err" || {
        cat "$tmp/$recipe.err" >&2
        fail "'just $recipe' failed against the stubbed fixture"
    }
    printf '%s' "$log"
}

# The line number of the first call matching $2, or nothing when it never ran.
# "Never ran" is the case this gate exists to report, so it must return empty
# rather than let `pipefail` abort the script before the diagnostic below.
first_call() {
    grep -Fxn "$2" "$1" | head -1 | cut -d: -f1 || true
}

sdk_log="$(run_recipe sdk-check)"
installed_at="$(first_call "$sdk_log" "$install_line")"
consumed_at="$(first_call "$sdk_log" "$consume_line")"

[[ -n $installed_at ]] ||
    fail "sdk-check never ran '$install_line', so it assumes an already-bootstrapped checkout"
[[ -n $consumed_at ]] ||
    fail "sdk-check never ran '$consume_line'; this gate is checking the wrong recipe"
[[ $installed_at -lt $consumed_at ]] ||
    fail "sdk-check installed the Node SDK dependencies at call $installed_at, after using them at call $consumed_at"

bootstrap_log="$(run_recipe bootstrap)"
[[ -n $(first_call "$bootstrap_log" "$install_line") ]] ||
    fail "bootstrap no longer reaches '$install_line'; a clean clone would be left without it"

# Now that it runs on every `just check`, sdk-install owes the gate both halves
# of the recipe contract: a silent success, and a failure that keeps bun's own
# reason instead of replacing it with a generic message.
run_install() {
    local extra_path="$1" name="$2"
    set +e
    CALL_LOG="$tmp/$name.calls" PATH="$extra_path:/usr/bin:/bin" HOME="$tmp/home" \
        just --justfile "$fixture/justfile" --working-directory "$fixture" sdk-install \
        >"$tmp/$name.out" 2>"$tmp/$name.err"
    local status=$?
    set -e
    printf '%s' "$status"
}

status="$(run_install "$bin" quiet)"
[[ $status -eq 0 ]] || {
    cat "$tmp/quiet.err" >&2
    fail "sdk-install failed against a succeeding stub (exit $status)"
}
if [[ -s $tmp/quiet.out || -s $tmp/quiet.err ]]; then
    echo "--- what it printed ---" >&2
    cat "$tmp/quiet.out" "$tmp/quiet.err" >&2
    fail "sdk-install printed on success; every gate run would carry that noise"
fi

# A bun that fails the way a stale lockfile really does.
failing_bin="$tmp/failing-bin"
mkdir -p "$failing_bin"
cat >"$failing_bin/bun" <<'STUB'
#!/usr/bin/env bash
echo 'error: lockfile had changes, but lockfile is frozen' >&2
exit 1
STUB
chmod +x "$failing_bin/bun"
ln -s "$(command -v just)" "$failing_bin/just"

status="$(run_install "$failing_bin" loud)"
[[ $status -ne 0 ]] ||
    fail "a failing 'bun install' left sdk-install green; the gate would run on absent dependencies"
grep -qF 'error: lockfile had changes, but lockfile is frozen' "$tmp/loud.err" || {
    cat "$tmp/loud.err" >&2
    fail "sdk-install swallowed bun's own failure output; the reason must survive to the reader"
}
grep -qF "just sdk-install" "$tmp/loud.err" ||
    fail "sdk-install's failure named no next action"

echo "check-sdk-install: ok"
