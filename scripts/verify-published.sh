#!/usr/bin/env bash
# llmlint: ignore-file[new_code_lands_in_a_project] The rule presumes an Nx project graph; this repository has none by a recorded decision (`AGENTS.md`: root `just` delegates to Cargo/Bun without Nx because the two-package graph is static), so no project definition can cover this file. What runs it is release.yml's verify jobs, and scripts/check-verify-published.sh covers it from `just lint-workflows`.
# Prove a just-published artifact is installable by doing what its consumer
# does, retried until the registry catches up.
#
# A registry answers its metadata API before the thing a consumer actually
# reaches for is resolvable: PyPI's JSON API before the simple index a `pip
# install` reads, and npm's `view` before the per-platform
# `@oneharness/cli-<platform>-<arch>` package the launcher needs — which npm
# installs as an OPTIONAL dependency, so a `npm install -g` that resolved
# nothing still exits 0 and only the binary's absence says so. A metadata probe
# followed by one install therefore reddens a release that published perfectly,
# and a person then has to work out whether it is a false negative or a broken
# release.
#
# So the thing that is waited on here IS the consumer's operation — the install
# and the smoke that proves the install usable — retried to a bound, with the
# last attempt's output surfaced when the bound runs out.
#
# Usage: scripts/verify-published.sh <pypi-cli|pypi-sdk|npm-cli|npm-sdk> <version>
# Reads: VERIFY_ATTEMPTS (default 30), VERIFY_DELAY seconds (default 10).
set -euo pipefail

target="${1:-}"
version="${2:-}"
attempts="${VERIFY_ATTEMPTS:-30}"
delay="${VERIFY_DELAY:-10}"

usage() {
  printf 'verify-published: %s\n' "$1" >&2
  printf '  Next: scripts/verify-published.sh <pypi-cli|pypi-sdk|npm-cli|npm-sdk> <version>, e.g. scripts/verify-published.sh pypi-cli 0.7.1\n' >&2
  exit 2
}

[ -n "$target" ] || usage "no target to verify"
[ -n "$version" ] || usage "no version to verify"
case "$target" in
  pypi-cli | pypi-sdk | npm-cli | npm-sdk) ;;
  *) usage "'$target' is not a target this script knows how to install" ;;
esac
# The version is composed into a pip requirement and an npm package spec, so it
# is matched against the shape release-plz actually publishes — semver's
# major.minor.patch, then at most one prerelease and one build part — rather
# than merely swept for dangerous characters: `1..2`, `-` and `.` all pass an
# allowlist.
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z]+(\.[0-9A-Za-z]+)*)?(\+[0-9A-Za-z]+(\.[0-9A-Za-z]+)*)?$ ]] ||
  usage "'$version' is not an x.y.z version this release could have published"
case "$attempts" in
  "" | *[!0-9]*) usage "VERIFY_ATTEMPTS='$attempts' is not a whole number of attempts" ;;
esac
[ "$attempts" -ge 1 ] || usage "VERIFY_ATTEMPTS='$attempts' verifies nothing; the bound must allow at least one attempt"
case "$delay" in
  "" | *[!0-9]*) usage "VERIFY_DELAY='$delay' is not a whole number of seconds" ;;
esac

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# Every attempt ends in this: an install nothing was run against proves only
# that a registry answered.
smoke_cli() {
  local installed
  installed="$(oneharness --version)" || {
    printf 'the installed oneharness could not run --version\n' >&2
    return 1
  }
  # The whole shape AND the value, from process output that is external like any
  # other input: clap prints exactly `<bin> <version>`, so the name must be this
  # binary, the version field must EQUAL the asked-for one, and nothing may
  # follow. A substring would accept `oneharness 9.9.99` for 9.9.9, which is a
  # release that shipped the wrong artifact — the thing this is here to catch.
  if [ "$installed" != "oneharness $version" ]; then
    printf 'the installed oneharness reports %s, not oneharness %s\n' "$installed" "$version" >&2
    return 1
  fi
  # Each smoke step says which one it was: a CLI that exits nonzero silently
  # would otherwise leave the exhausted bound reporting no cause at all.
  if ! oneharness --help >/dev/null; then
    printf 'the installed oneharness could not run --help\n' >&2
    return 1
  fi
  if ! oneharness list >/dev/null; then
    printf 'the installed oneharness could not run list\n' >&2
    return 1
  fi
}

# `--no-cache-dir` / `--prefer-online`: a retry that re-reads a cached index
# answer from the attempt that failed would never see the release land.
attempt_pypi_cli() {
  pip install --no-cache-dir "oneharness-cli==$version" || return 1
  smoke_cli || return 1
}

attempt_pypi_sdk() {
  pip install --no-cache-dir "oneharness-sdk==$version" || return 1
  python - "$version" <<'PY' || return 1
import asyncio
import sys
from importlib.metadata import version
from oneharness_sdk import OneHarness, __version__

expected = sys.argv[1]
assert __version__ == expected, f"oneharness_sdk.__version__ is {__version__}, not {expected}"
# Each distribution names itself. An SDK whose own metadata is wrong and an SDK
# installed beside a different CLI are different broken releases, and the
# exhausted bound surfaces only what the last attempt said — so a bare assert
# here would report a broken release with nothing to act on.
for dist in ("oneharness-sdk", "oneharness-cli"):
    installed = version(dist)
    assert installed == expected, f"the installed {dist} is {installed}, not {expected}"


async def verify():
    harnesses = await OneHarness().list()
    assert any(
        item["id"] == "codex" for item in harnesses
    ), "the installed SDK did not return the packaged CLI registry"


asyncio.run(verify())
PY
}

attempt_npm_cli() {
  npm install -g --prefer-online "oneharness-cli@$version" || return 1
  smoke_cli || return 1
}

attempt_npm_sdk() {
  local project="$work/sdk-project"
  rm -rf "$project"
  mkdir -p "$project"
  (
    cd "$project" || exit 1
    npm init -y >/dev/null || exit 1
    npm install --prefer-online "@oneharness/sdk@$version" || exit 1
    node --input-type=module - "$version" <<'NODE' || exit 1
import { readFileSync } from "node:fs";
import { OneHarness } from "@oneharness/sdk";

const expected = process.argv[2];
const manifest = JSON.parse(readFileSync("node_modules/@oneharness/sdk/package.json", "utf8"));
if (manifest.version !== expected) {
  throw new Error(`installed SDK version ${manifest.version} does not match ${expected}`);
}
const harnesses = await new OneHarness().list();
if (!harnesses.some(({ id }) => id === "codex")) {
  throw new Error("installed SDK did not return the packaged CLI registry");
}
NODE
  ) || return 1
}

# One attempt at being the consumer of $target, called directly so each branch
# is reachable to a reader (and to shellcheck) from here. This and the label
# below enumerate the targets a second and third time; check-verify-published.sh
# is their drift gate — it reads the allowlist above and drives every target in
# it, so one added without an arm here fails there.
run_attempt() {
  case "$target" in
    pypi-cli) attempt_pypi_cli ;;
    pypi-sdk) attempt_pypi_sdk ;;
    npm-cli) attempt_npm_cli ;;
    npm-sdk) attempt_npm_sdk ;;
    *)
      printf 'verify-published: no install is implemented for the accepted target %s\n' "$target" >&2
      return 1
      ;;
  esac
}

case "$target" in
  pypi-cli) what="oneharness-cli $version from PyPI" ;;
  pypi-sdk) what="oneharness-sdk $version from PyPI" ;;
  npm-cli) what="oneharness-cli@$version from npm" ;;
  npm-sdk) what="@oneharness/sdk@$version from npm" ;;
  *) usage "no description is implemented for the accepted target '$target'" ;;
esac

last="$work/attempt.log"
: >"$last"
for i in $(seq 1 "$attempts"); do
  if run_attempt >"$last" 2>&1; then
    # One line on success: the package managers' own chatter is captured, and
    # replayed only when the bound runs out and somebody needs the cause.
    printf 'verify-published: installed and smoke-tested %s on attempt %s of %s.\n' "$what" "$i" "$attempts"
    exit 0
  fi
  if [ "$i" -lt "$attempts" ]; then
    sleep "$delay"
  fi
done

printf -- '--- what the last attempt said ---\n' >&2
cat "$last" >&2
printf -- '--- end of the last attempt ---\n' >&2
printf '::error::%s was still not installable after %s attempts over ~%s seconds. The last attempt output is above.\n' \
  "$what" "$attempts" "$((attempts * delay))" >&2
printf '  Next: the publish itself already happened, so this is either a registry still propagating (re-run this job) or a genuinely broken artifact — the last attempt above says which.\n' >&2
exit 1
