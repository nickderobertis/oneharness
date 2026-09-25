#!/usr/bin/env bash
# llmlint: ignore-file[new_code_lands_in_a_project] The rule presumes an Nx project graph; this repository has none by a recorded decision (`AGENTS.md`: root `just` delegates to Cargo/Bun without Nx because the two-package graph is static), so no project definition can cover this file. What runs it is `just lint-workflows`, in `check` and CI.
# Exercise the release verifier through stand-in registry clients and installed
# packages, including delayed installs and failures from its smoke commands.
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
# Only `npm install` lags: `npm init` in a fresh directory is local and always
# works, so a case that makes it fail would be proving the wrong thing.
[ "${1:-}" = install ] || exit 0
count="$STUB_STATE/npm.count"
n=$(( $(cat "$count" 2>/dev/null || echo 0) + 1 ))
printf '%s' "$n" >"$count"
if [ "$n" -le "${STUB_NPM_FAILS:-0}" ]; then
  echo "npm error code E404 (attempt $n)" >&2
  exit 1
fi
# A successful install leaves the package behind, because the Node smoke
# program this test exists to exercise imports it and reads its manifest.
case "$*" in
  *@oneharness/sdk@*)
    mkdir -p node_modules/@oneharness/sdk
    printf '{"name":"@oneharness/sdk","version":"%s","type":"module","main":"index.js"}\n' \
      "$STUB_SDK_VERSION" >node_modules/@oneharness/sdk/package.json
    [ -z "${STUB_SDK_MANIFEST_NULL:-}" ] || echo null >node_modules/@oneharness/sdk/package.json
    # The CLI the SDK depends on exactly, as npm would hoist it beside the SDK.
    mkdir -p node_modules/oneharness-cli
    printf '{"name":"oneharness-cli","version":"%s"}\n' "$STUB_CLI_DIST_VERSION" >node_modules/oneharness-cli/package.json
    # An SDK that imports and reports the right version but cannot reach the CLI
    # it packages answers an empty registry: installable, and useless. Composed
    # here rather than inlined, because `${VAR:-[{...}]}` ends at the first `}`.
    registry='[{ id: "stub-harness" }]'
    [ -z "${STUB_SDK_REGISTRY_EMPTY:-}" ] || registry='[]'
    [ -z "${STUB_SDK_REGISTRY_NOT_A_LIST:-}" ] || registry='{ "id": "stub-harness" }'
    cat >node_modules/@oneharness/sdk/index.js <<PKG
export class OneHarness {
  async list() {
    return $registry;
  }
}
PKG
    ;;
esac
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
  if [ -n "${STUB_CLI_SILENT_FAILURE:-}" ]; then
    exit 1
  fi
  printf '%s %s\n' "${STUB_CLI_NAME:-oneharness}" "${STUB_CLI_VERSION:?}"
  if [ -n "${STUB_CLI_EXTRA_LINE:-}" ]; then
    printf '%s\n' "$STUB_CLI_EXTRA_LINE"
  fi
fi
STUB

# The two SDK targets prove themselves through a program the script embeds, and
# those programs are real logic — a version assertion and a registry call. So
# the interpreters are REAL, behind wrappers that only record the call, and what
# stands in is the package each program imports. `python` is a wrapper because
# a host may only ship `python3`.
for interpreter in python node; do
  real="$(command -v "${interpreter}3" || command -v "$interpreter")" || {
    echo "check-verify-published: no $interpreter on PATH to run the SDK smoke programs with; install $interpreter and rerun this test" >&2
    exit 1
  }
  cat >"$tmp/bin/$interpreter" <<STUB
#!/usr/bin/env bash
printf '$interpreter %s\n' "\$*" >>"\$CALL_LOG"
exec "$real" "\$@"
STUB
  chmod +x "$tmp/bin/$interpreter"
done
chmod +x "$tmp/bin/pip" "$tmp/bin/npm" "$tmp/bin/oneharness"

# The oneharness_sdk a `pip install` would have put on the path. Three versions
# rather than one, because a real broken release moves them independently: an
# SDK published against the wrong CLI pin leaves `__version__` right and one
# dist-info wrong, and the program under test has to say which disagreed.
#   $1 what oneharness_sdk.__version__ reports
#   $2 the oneharness-sdk dist metadata version
#   $3 the oneharness-cli dist metadata version
make_python_sdk() {
  local version="$1" sdk_dist="$2" cli_dist="$3" root="$tmp/pyfake" dist dist_version registry
  rm -rf "$root"
  mkdir -p "$root/oneharness_sdk"
  # Composed first for the same reason the npm stub composes it: a `${VAR:-...}`
  # default containing braces ends at the first one.
  registry='[{"id": "stub-harness"}]'
  [ -z "${STUB_SDK_REGISTRY_EMPTY:-}" ] || registry='[]'
  [ -z "${STUB_SDK_REGISTRY_NOT_A_LIST:-}" ] || registry='{"id": "stub-harness"}'
  cat >"$root/oneharness_sdk/__init__.py" <<PYPKG
__version__ = "$version"


class OneHarness:
    async def list(self):
        return $registry
PYPKG
  # importlib.metadata reads these, and reads the hyphenated Name it was asked
  # for, so the two distributions the program checks are both declared here.
  for dist in oneharness-sdk oneharness-cli; do
    if [ "$dist" = oneharness-sdk ]; then dist_version="$sdk_dist"; else dist_version="$cli_dist"; fi
    mkdir -p "$root/${dist//-/_}-$dist_version.dist-info"
    cat >"$root/${dist//-/_}-$dist_version.dist-info/METADATA" <<META
Metadata-Version: 2.1
Name: $dist
Version: $dist_version
META
  done
}

# $1 target, $2 description; the STUB_* variables in the environment pick how
# the stand-in registries behave. Leaves $status, $tmp/out and $tmp/err.
run_case() {
  local sdk_version
  rm -rf "$tmp/state"
  mkdir -p "$tmp/state"
  sdk_version="${STUB_SDK_VERSION:-$VERSION_UNDER_TEST}"
  make_python_sdk "$sdk_version" \
    "${STUB_SDK_DIST_VERSION:-$sdk_version}" "${STUB_CLI_DIST_VERSION:-$sdk_version}"
  : >"$tmp/calls"
  description="$2"
  set +e
  CALL_LOG="$tmp/calls" STUB_STATE="$tmp/state" \
    STUB_CLI_VERSION="${STUB_CLI_VERSION:-$VERSION_UNDER_TEST}" STUB_CLI_NAME="${STUB_CLI_NAME:-oneharness}" \
    STUB_SDK_VERSION="$sdk_version" STUB_CLI_DIST_VERSION="${STUB_CLI_DIST_VERSION:-$sdk_version}" STUB_SDK_REGISTRY_EMPTY="${STUB_SDK_REGISTRY_EMPTY:-}" \
    STUB_SDK_REGISTRY_NOT_A_LIST="${STUB_SDK_REGISTRY_NOT_A_LIST:-}" STUB_SDK_MANIFEST_NULL="${STUB_SDK_MANIFEST_NULL:-}" \
    PYTHONPATH="$tmp/pyfake" \
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

# Every target the script accepts must have both an attempt and a label: the
# allowlist is read out of the script itself, so a target added there without
# either arm fails here rather than quietly verifying nothing.
accepted_targets="$(sed -n 's/^  \([a-z| -]*\)) ;;$/\1/p' "$root/scripts/verify-published.sh" | head -1 | tr -d ' ' | tr '|' ' ')"
[ -n "$accepted_targets" ] || fail "verify-published.sh no longer declares its accepted targets in one 'case' allowlist"
for accepted in $accepted_targets; do
  run_case "$accepted" "the accepted target $accepted"
  expect_status 0
  expect_said "$tmp/out" "installed and smoke-tested"
  expect_said "$tmp/out" "$VERSION_UNDER_TEST"
done

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

# Versions with release-plz's prerelease and build parts must reach the exact
# npm package spec and the installed CLI smoke, not fail input validation.
VERSION_OVERRIDE=9.9.9-rc.1 STUB_CLI_VERSION=9.9.9-rc.1 run_case npm-cli "a valid prerelease version"
expect_status 0
expect_calls 1 "npm install -g --prefer-online oneharness-cli@9.9.9-rc.1"

VERSION_OVERRIDE=9.9.9+build.7 STUB_CLI_VERSION=9.9.9+build.7 run_case npm-cli "a valid build version"
expect_status 0
expect_calls 1 "npm install -g --prefer-online oneharness-cli@9.9.9+build.7"

# SemVer identifiers may carry hyphens, in the prerelease and the build part.
VERSION_OVERRIDE=9.9.9-rc-1+build-7 STUB_CLI_VERSION=9.9.9-rc-1+build-7 run_case npm-cli "a valid version with hyphenated identifiers"
expect_status 0
expect_calls 1 "npm install -g --prefer-online oneharness-cli@9.9.9-rc-1+build-7"

# The version release-plz publishes is the root crate's; whatever it is now
# must be one this verifier accepts.
cargo_version="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -n 1)"
[ -n "$cargo_version" ] || fail "could not read the root package version from Cargo.toml; the next step is to read its [package] version line and update this sed to match it"
VERSION_OVERRIDE="$cargo_version" STUB_CLI_VERSION="$cargo_version" run_case npm-cli "the version Cargo.toml would release"
expect_status 0
expect_calls 1 "npm install -g --prefer-online oneharness-cli@$cargo_version"

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
expect_said "$tmp/err" "the installed oneharness reports oneharness 8.8.8, not oneharness $VERSION_UNDER_TEST"

# A version that merely CONTAINS the one asked for is a different version.
STUB_CLI_VERSION=9.9.99 run_case pypi-cli "an install that resolves a version containing the asked-for one"
expect_status 1
expect_said "$tmp/err" "the installed oneharness reports oneharness 9.9.99, not oneharness $VERSION_UNDER_TEST"

# ...and a right-looking version printed by something else is not this binary.
STUB_CLI_NAME=oneharness-mock run_case pypi-cli "a version line printed by another program"
unset STUB_CLI_NAME
expect_status 1
expect_said "$tmp/err" "the installed oneharness reports oneharness-mock $VERSION_UNDER_TEST"

STUB_CLI_EXTRA_LINE='unexpected output' run_case pypi-cli "a version response with an extra line"
expect_status 1
expect_said "$tmp/err" 'unexpected output'

STUB_CLI_SILENT_FAILURE=1 run_case pypi-cli "a version command that fails silently"
expect_status 1
expect_said "$tmp/err" 'the installed oneharness could not run --version'

# A smoke step AFTER the version check failing is still a failed install: the
# package resolved, and the thing it installed does not work.
STUB_CLI_BROKEN_SUBCOMMAND=list run_case npm-cli "an installed CLI whose list subcommand fails"
unset STUB_CLI_BROKEN_SUBCOMMAND
expect_status 1
expect_said "$tmp/err" "oneharness: list failed against a half-installed package"
expect_said "$tmp/err" "the installed oneharness could not run list"

STUB_CLI_BROKEN_SUBCOMMAND=--help run_case npm-cli "an installed CLI whose help fails"
unset STUB_CLI_BROKEN_SUBCOMMAND
expect_status 1
expect_said "$tmp/err" "the installed oneharness could not run --help"

# The two SDK targets prove themselves through a program, and that program
# failing is what a wrong version or an unusable packaged CLI looks like.
STUB_SDK_VERSION=1.1.1 run_case pypi-sdk "a Python SDK that installed a different version"
unset STUB_SDK_VERSION
expect_status 1
expect_said "$tmp/err" "oneharness_sdk.__version__ is 1.1.1, not $VERSION_UNDER_TEST"

STUB_SDK_VERSION=1.1.1 run_case npm-sdk "a Node SDK that installed a different version"
unset STUB_SDK_VERSION
expect_status 1
expect_said "$tmp/err" "installed SDK version 1.1.1 does not match $VERSION_UNDER_TEST"

# The module reports the right version and the distribution beside it does not:
# a release that built `oneharness_sdk` correctly and registered it wrong. The
# first assertion passes here, so only a per-distribution message reaches the
# reader — and it has to name which of the two disagreed.
STUB_SDK_DIST_VERSION=1.1.1 run_case pypi-sdk "a Python SDK whose own distribution metadata disagrees"
unset STUB_SDK_DIST_VERSION
expect_status 1
expect_said "$tmp/err" "the installed oneharness-sdk is 1.1.1, not $VERSION_UNDER_TEST"

# The other half, and the one that matters most: the SDK pins its CLI with an
# exact `==`, so an SDK installed beside a different oneharness-cli is the pin
# having failed. Nothing else in this suite can see that.
STUB_CLI_DIST_VERSION=1.1.1 run_case pypi-sdk "a Python SDK installed beside a different CLI"
unset STUB_CLI_DIST_VERSION
expect_status 1
expect_said "$tmp/err" "the installed oneharness-cli is 1.1.1, not $VERSION_UNDER_TEST"

STUB_CLI_DIST_VERSION=1.1.1 run_case npm-sdk "a Node SDK installed beside a different CLI"
unset STUB_CLI_DIST_VERSION
expect_status 1
expect_said "$tmp/err" "the installed oneharness-cli is \"1.1.1\", not $VERSION_UNDER_TEST"

# An SDK can import, report every version correctly, and still not reach the CLI
# it packages — which is an install a consumer cannot use, on both runtimes.
STUB_SDK_REGISTRY_EMPTY=1 run_case pypi-sdk "a Python SDK that reaches no packaged CLI registry"
unset STUB_SDK_REGISTRY_EMPTY
expect_status 1
expect_said "$tmp/err" "did not return the packaged CLI registry"

STUB_SDK_REGISTRY_EMPTY=1 run_case npm-sdk "a Node SDK that reaches no packaged CLI registry"
unset STUB_SDK_REGISTRY_EMPTY
expect_status 1
expect_said "$tmp/err" "did not return the packaged CLI registry"

# What an SDK hands back is read as data from outside: a registry that is not a
# list of objects, or a manifest that is not an object, is named rather than
# crashing the program on an attribute it assumed.
STUB_SDK_REGISTRY_NOT_A_LIST=1 run_case pypi-sdk "a Python SDK whose registry is not a list"
unset STUB_SDK_REGISTRY_NOT_A_LIST
expect_status 1
expect_said "$tmp/err" "did not return the packaged CLI registry: got {'id': 'stub-harness'}"

STUB_SDK_REGISTRY_NOT_A_LIST=1 run_case npm-sdk "a Node SDK whose registry is not an array"
unset STUB_SDK_REGISTRY_NOT_A_LIST
expect_status 1
expect_said "$tmp/err" 'did not return the packaged CLI registry: got {"id":"stub-harness"}'

STUB_SDK_MANIFEST_NULL=1 run_case npm-sdk "a Node SDK whose package.json is not an object"
unset STUB_SDK_MANIFEST_NULL
expect_status 1
expect_said "$tmp/err" "installed SDK package.json is not a JSON object: got null"

# Python drops `assert` under PYTHONOPTIMIZE, which a runner may set; the
# program's checks must refuse there too.
PYTHONOPTIMIZE=1 STUB_SDK_VERSION=1.1.1 run_case pypi-sdk "a wrong Python SDK version under PYTHONOPTIMIZE"
unset PYTHONOPTIMIZE STUB_SDK_VERSION
expect_status 1
expect_said "$tmp/err" "oneharness_sdk.__version__ is 1.1.1, not $VERSION_UNDER_TEST"

PYTHONOPTIMIZE=1 STUB_CLI_DIST_VERSION=1.1.1 run_case pypi-sdk "a Python SDK beside a different CLI under PYTHONOPTIMIZE"
unset PYTHONOPTIMIZE STUB_CLI_DIST_VERSION
expect_status 1
expect_said "$tmp/err" "the installed oneharness-cli is 1.1.1, not $VERSION_UNDER_TEST"

PYTHONOPTIMIZE=1 STUB_SDK_REGISTRY_EMPTY=1 run_case pypi-sdk "a Python SDK reaching no registry under PYTHONOPTIMIZE"
unset PYTHONOPTIMIZE STUB_SDK_REGISTRY_EMPTY
expect_status 1
expect_said "$tmp/err" "did not return the packaged CLI registry"

# Both SDK targets lag exactly as the CLI ones do: one recovers inside the
# bound, one never does and must surface its last error.
STUB_PIP_FAILS=2 run_case pypi-sdk "a PyPI SDK install that resolves on the third try"
expect_status 0
expect_said "$tmp/out" "on attempt 3 of 3"
expect_calls 3 "pip install --no-cache-dir oneharness-sdk==$VERSION_UNDER_TEST"

STUB_NPM_FAILS=99 run_case npm-sdk "an npm SDK install that never resolves"
expect_status 1
expect_said "$tmp/err" "npm error code E404 (attempt 3)"
expect_said "$tmp/err" "@oneharness/sdk@$VERSION_UNDER_TEST from npm was still not installable after 3 attempts"

# A bound read from the environment is an external input like any other: a
# wait of no attempts, or one that is not a number, verifies nothing.
ATTEMPTS_OVERRIDE=0 run_case pypi-cli "a bound of zero attempts"
unset ATTEMPTS_OVERRIDE
expect_status 2
expect_said "$tmp/err" "between 1 and 1000 attempts"

ATTEMPTS_OVERRIDE=soon run_case pypi-cli "a bound that is not a number"
unset ATTEMPTS_OVERRIDE
expect_status 2
expect_said "$tmp/err" "is not a whole number of attempts"

DELAY_OVERRIDE=later run_case pypi-cli "a delay that is not a number of seconds"
unset DELAY_OVERRIDE
expect_status 2
expect_said "$tmp/err" "is not a whole number of seconds"

DELAY_OVERRIDE=99999 run_case pypi-cli "a delay past the bound"
unset DELAY_OVERRIDE
expect_status 2
expect_said "$tmp/err" "exceeds the 3600-second bound"

# Leading zeros are decimal, not octal: the exhausted bound still reports its
# last error and its arithmetic.
ATTEMPTS_OVERRIDE=09 DELAY_OVERRIDE=00 STUB_NPM_FAILS=99 run_case npm-sdk "a zero-padded bound that exhausts"
unset ATTEMPTS_OVERRIDE DELAY_OVERRIDE STUB_NPM_FAILS
expect_status 1
expect_said "$tmp/err" "npm error code E404 (attempt 9)"
expect_said "$tmp/err" "still not installable after 9 attempts over ~0 seconds"

# A target nobody implemented, and arguments a caller can omit or mangle, are
# all wiring bugs rather than lagging registries.
run_case pypi-nothing "an unknown target"
expect_status 2
expect_said "$tmp/err" "is not a target this script knows how to install"

expect_usage_error "no target to verify"
expect_usage_error "no version to verify" pypi-cli
expect_usage_error "3 arguments given" pypi-cli 9.9.9 npm-cli
expect_usage_error "is not an x.y.z version" pypi-cli "9.9.9; rm -rf /"
expect_usage_error "is not an x.y.z version" pypi-cli "1..2"
expect_usage_error "is not an x.y.z version" pypi-cli "-"
expect_usage_error "is not an x.y.z version" pypi-cli "9.9.9+a+b"
expect_usage_error "is not an x.y.z version" pypi-cli "9.9.9-a..b"
expect_usage_error "is not an x.y.z version" pypi-cli "01.2.3"
expect_usage_error "is not an x.y.z version" npm-cli "1.02.3"
expect_usage_error "is not an x.y.z version" pypi-sdk "1.2.03"
expect_usage_error "numeric prerelease identifier with a leading zero" pypi-cli "9.9.9-01"
expect_usage_error "numeric prerelease identifier with a leading zero" pypi-cli "9.9.9-rc.02+build"

echo "check-verify-published: ok"
