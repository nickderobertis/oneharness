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
git -C "$fixture" init -q

bin="$tmp/bin"
mkdir -p "$bin"
for tool in bun cargo rustup uv; do
    cat >"$bin/$tool" <<'STUB'
#!/usr/bin/env bash
printf '%s %s\n' "$(basename "$0")" "$*" >> "$CALL_LOG"
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

echo "check-sdk-install: ok"
