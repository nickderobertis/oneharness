#!/usr/bin/env bash
# llmlint: ignore-file[new_code_lands_in_a_project] The rule presumes an Nx project graph; this repository has none by a recorded decision (`AGENTS.md`: root `just` delegates to Cargo/Bun without Nx because the two-package graph is static), so no project definition can cover this file. What runs it is `just lint-workflows`, in `check` and CI.
# Hermetic behavioral test for scripts/verify-published.sh, against stand-in
# registries.
#
# A real registry cannot rehearse the one case this script exists for — an
# artifact that is published but not yet resolvable — and a real publish cannot
# be rolled back. So `pip`, `npm`, `python`, `node` and the installed
# `oneharness` are stubbed, and the cases are the ones a release meets: an
# install that only works after a retry (npm's per-platform package is an
# OPTIONAL dependency, so the install exits 0 and only the smoke says the binary
# is missing — which is why the smoke has to be inside the retried unit), and an
# install that never works, whose last error must reach the reader.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/bin" "$tmp/state"

VERSION_UNDER_TEST=9.9.9

fail() {
  echo "check-verify-published: $1" >&2
  echo "  Next: re-run with 'bash -x scripts/check-verify-published.sh'. Every stubbed tool records its arguments in \$tmp/calls and the case's own output is above — a broken assertion here means a release either declares a published artifact broken or declares a broken one published." >&2
  exit 1
}

# Registries that answer, or lag for a set number of calls before they do. The
# counts live in files so they survive across the retries of one case.
cat >"$tmp/bin/pip" <<'STUB'
#!/usr/bin/env bash
printf 'pip %s\n' "$*" >>"$CALL_LOG"
count="$STUB_STATE/pip.count"
n=$(( $(cat "$count" 2>/dev/null || echo 0) + 1 ))
printf '%s' "$n" >"$count"
if [ "$n" -le "${STUB_PIP_FAILS:-0}" ]; then
  echo "ERROR: Could not find a version that satisfies the requirement (attempt $n)" >&2
  exit 1
fi
STUB

cat >"$tmp/bin/npm" <<'STUB'
#!/usr/bin/env bash
printf 'npm %s\n' "$*" >>"$CALL_LOG"
count="$STUB_STATE/npm.count"
n=$(( $(cat "$count" 2>/dev/null || echo 0) + 1 ))
printf '%s' "$n" >"$count"
if [ "$n" -le "${STUB_NPM_FAILS:-0}" ]; then
  echo "npm error code E404 (attempt $n)" >&2
  exit 1
fi
STUB

# The installed CLI. It fails while the per-platform package it execs is still
# unresolvable, which is exactly what an optional-dependency lag looks like.
cat >"$tmp/bin/oneharness" <<'STUB'
#!/usr/bin/env bash
printf 'oneharness %s\n' "$*" >>"$CALL_LOG"
if [ -n "${STUB_CLI_BROKEN_SUBCOMMAND:-}" ] && [ "${1:-}" = "$STUB_CLI_BROKEN_SUBCOMMAND" ]; then
  echo "oneharness: $1 failed against a half-installed package" >&2
  exit 1
fi
if [ "${1:-}" = "--version" ]; then
  count="$STUB_STATE/oneharness.count"
  n=$(( $(cat "$count" 2>/dev/null || echo 0) + 1 ))
  printf '%s' "$n" >"$count"
  if [ "$n" -le "${STUB_CLI_FAILS:-0}" ]; then
    echo "oneharness: no binary package found for this platform (attempt $n)" >&2
    exit 1
  fi
  printf 'oneharness %s\n' "${STUB_CLI_VERSION:?}"
fi
STUB

for interpreter in python node; do
  cat >"$tmp/bin/$interpreter" <<'STUB'
#!/usr/bin/env bash
printf '%s %s\n' "$(basename "$0")" "$*" >>"$CALL_LOG"
cat >/dev/null
if [ -n "${STUB_SMOKE_FAILS:-}" ]; then
  echo "$(basename "$0"): AssertionError from the SDK smoke program" >&2
  exit 1
fi
STUB
  chmod +x "$tmp/bin/$interpreter"
done
chmod +x "$tmp/bin/pip" "$tmp/bin/npm" "$tmp/bin/oneharness"

# $1 target, $2 description; the STUB_* variables in the environment pick how
# the stand-in registries behave. Leaves $status, $tmp/out and $tmp/err.
run_case() {
  rm -rf "$tmp/state"
  mkdir -p "$tmp/state"
  : >"$tmp/calls"
  description="$2"
  set +e
  CALL_LOG="$tmp/calls" STUB_STATE="$tmp/state" \
    STUB_CLI_VERSION="${STUB_CLI_VERSION:-$VERSION_UNDER_TEST}" \
    PATH="$tmp/bin:$PATH" VERIFY_ATTEMPTS="${ATTEMPTS_OVERRIDE-3}" VERIFY_DELAY="${DELAY_OVERRIDE-0}" \
    bash "$root/scripts/verify-published.sh" "$1" "${VERSION_OVERRIDE-$VERSION_UNDER_TEST}" \
    >"$tmp/out" 2>"$tmp/err"
  status=$?
  set -e
}

# The arguments a caller can get wrong, driven as a caller gets them wrong.
expect_usage_error() {
  local expected="$1"
  shift
  description="$expected"
  set +e
  PATH="$tmp/bin:$PATH" CALL_LOG="$tmp/calls" STUB_STATE="$tmp/state" \
    bash "$root/scripts/verify-published.sh" "$@" >"$tmp/out" 2>"$tmp/err"
  status=$?
  set -e
  expect_status 2
  expect_said "$tmp/err" "$expected"
}

expect_status() {
  [ "$status" -eq "$1" ] || {
    cat "$tmp/out" "$tmp/err" >&2
    fail "$description: expected exit $1, got $status"
  }
}

expect_said() {
  grep -Fq "$2" "$1" || {
    cat "$tmp/out" "$tmp/err" >&2
    fail "$description: expected to say '$2'"
  }
}

expect_calls() {
  local expected="$1" pattern="$2" actual
  actual="$(grep -Fc -- "$pattern" "$tmp/calls" || true)"
  [ "$actual" -eq "$expected" ] || {
    cat "$tmp/calls" >&2
    fail "$description: expected $expected call(s) matching '$pattern', got $actual"
  }
}

# Every target's consumer operation, against registries that answer at once.
run_case pypi-cli "a PyPI CLI install that resolves immediately"
expect_status 0
expect_said "$tmp/out" "on attempt 1 of 3"
expect_calls 1 "pip install --no-cache-dir oneharness-cli==$VERSION_UNDER_TEST"
expect_calls 1 "oneharness list"

run_case pypi-sdk "a PyPI SDK install that resolves immediately"
expect_status 0
expect_calls 1 "pip install --no-cache-dir oneharness-sdk==$VERSION_UNDER_TEST"
expect_calls 1 "python - $VERSION_UNDER_TEST"

run_case npm-cli "an npm CLI install that resolves immediately"
expect_status 0
expect_calls 1 "npm install -g --prefer-online oneharness-cli@$VERSION_UNDER_TEST"

run_case npm-sdk "an npm SDK install that resolves immediately"
expect_status 0
expect_calls 1 "npm install --prefer-online @oneharness/sdk@$VERSION_UNDER_TEST"
expect_calls 1 "node --input-type=module - $VERSION_UNDER_TEST"

# The npm lag this exists for: the meta-package installs every time — an
# unresolvable optional dependency is not an install failure — and only the
# smoke notices, for two attempts. The install must be retried WITH it.
STUB_CLI_FAILS=2 run_case npm-cli "an npm install whose platform package resolves on the third try"
expect_status 0
expect_said "$tmp/out" "on attempt 3 of 3"
expect_calls 3 "npm install -g --prefer-online oneharness-cli@$VERSION_UNDER_TEST"

# A pip index that never serves the version: the bound ends the wait and the
# last attempt's own words are what the reader gets.
STUB_PIP_FAILS=99 run_case pypi-cli "a PyPI install that never resolves"
expect_status 1
expect_said "$tmp/err" "Could not find a version that satisfies the requirement (attempt 3)"
expect_said "$tmp/err" "oneharness-cli $VERSION_UNDER_TEST from PyPI was still not installable after 3 attempts"
expect_calls 3 "pip install --no-cache-dir oneharness-cli==$VERSION_UNDER_TEST"

# An install that resolves the wrong version is a broken release, not a lag —
# and it must not pass because the install exited 0.
STUB_CLI_VERSION=8.8.8 run_case pypi-cli "an install that resolves the wrong version"
expect_status 1
expect_said "$tmp/err" "the installed oneharness reports oneharness 8.8.8, not $VERSION_UNDER_TEST"

# A version that merely CONTAINS the one asked for is a different version.
STUB_CLI_VERSION=9.9.99 run_case pypi-cli "an install that resolves a version containing the asked-for one"
expect_status 1
expect_said "$tmp/err" "the installed oneharness reports oneharness 9.9.99, not $VERSION_UNDER_TEST"

# A smoke step AFTER the version check failing is still a failed install: the
# package resolved, and the thing it installed does not work.
STUB_CLI_BROKEN_SUBCOMMAND=list run_case npm-cli "an installed CLI whose list subcommand fails"
unset STUB_CLI_BROKEN_SUBCOMMAND
expect_status 1
expect_said "$tmp/err" "oneharness: list failed against a half-installed package"

# The two SDK targets prove themselves through a program, and that program
# failing is what a wrong version or an unusable packaged CLI looks like.
STUB_SMOKE_FAILS=1 run_case pypi-sdk "a Python SDK smoke program that refuses"
unset STUB_SMOKE_FAILS
expect_status 1
expect_said "$tmp/err" "python: AssertionError from the SDK smoke program"

STUB_SMOKE_FAILS=1 run_case npm-sdk "a Node SDK smoke program that refuses"
unset STUB_SMOKE_FAILS
expect_status 1
expect_said "$tmp/err" "node: AssertionError from the SDK smoke program"

# A bound read from the environment is an external input like any other: a
# wait of no attempts, or one that is not a number, verifies nothing.
ATTEMPTS_OVERRIDE=0 run_case pypi-cli "a bound of zero attempts"
unset ATTEMPTS_OVERRIDE
expect_status 2
expect_said "$tmp/err" "verifies nothing"

ATTEMPTS_OVERRIDE=soon run_case pypi-cli "a bound that is not a number"
unset ATTEMPTS_OVERRIDE
expect_status 2
expect_said "$tmp/err" "is not a whole number of attempts"

DELAY_OVERRIDE=later run_case pypi-cli "a delay that is not a number of seconds"
unset DELAY_OVERRIDE
expect_status 2
expect_said "$tmp/err" "is not a whole number of seconds"

# A target nobody implemented, and arguments a caller can omit or mangle, are
# all wiring bugs rather than lagging registries.
run_case pypi-nothing "an unknown target"
expect_status 2
expect_said "$tmp/err" "is not a target this script knows how to install"

expect_usage_error "no target to verify"
expect_usage_error "no version to verify" pypi-cli
expect_usage_error "is not an x.y.z version" pypi-cli "9.9.9; rm -rf /"
expect_usage_error "is not an x.y.z version" pypi-cli "1..2"
expect_usage_error "is not an x.y.z version" pypi-cli "-"

echo "check-verify-published: ok"
