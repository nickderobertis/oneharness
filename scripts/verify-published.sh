#!/usr/bin/env bash
# Prove a just-published artifact is installable by retrying, to a bound, what
# its consumer does: the install plus a smoke of it, with the last attempt's
# output surfaced when the bound runs out.
#
# A registry's metadata API answers before the index a consumer installs from
# does, and npm installs the per-platform `@oneharness/cli-<platform>-<arch>`
# as an OPTIONAL dependency — a `npm install -g` that resolved nothing still
# exits 0, and only a look at what it installed finds the platform package
# missing. npm makes each package's new version visible on its own clock:
# v0.25.8's wrapper appeared 152 s after publish-npm returned and its
# @oneharness/cli-linux-x64 411 s after, so an attempt can install the wrapper
# without the binary it execs and still be a release in flight.
#
# An attempt that installed the exact platform package and whose binary then
# fails is a broken release, not a lag — a published tarball never changes — so
# it ends the wait at once instead of spending the rest of the bound.
#
# Usage: scripts/verify-published.sh <pypi-cli|pypi-sdk|npm-cli|npm-sdk> <version>
# Reads: VERIFY_ATTEMPTS (default 90 for npm, 30 for PyPI), VERIFY_DELAY
# seconds (default 10).
set -euo pipefail

target="${1:-}"
version="${2:-}"
# npm: v0.25.8's last package became visible 458 s after publish-npm returned,
# past the old 30 x 10 s bound; 90 x 10 s waits 900 s, about twice that worst
# observed lag. PyPI has never lagged past its first retries, so it keeps 30.
case "$target" in
  npm-*) attempts="${VERIFY_ATTEMPTS:-90}" ;;
  *) attempts="${VERIFY_ATTEMPTS:-30}" ;;
esac
delay="${VERIFY_DELAY:-10}"
# The status an attempt returns when a retry cannot change its answer.
broken=3

usage() {
  printf 'verify-published: %s\n' "$1" >&2
  printf '  Next: scripts/verify-published.sh <pypi-cli|pypi-sdk|npm-cli|npm-sdk> <version>, e.g. scripts/verify-published.sh pypi-cli 0.7.1\n' >&2
  exit 2
}

[ "$#" -le 2 ] || usage "$# arguments given; it takes a target and a version"
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
[[ "$version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?(\+[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$ ]] ||
  usage "'$version' is not an x.y.z version this release could have published"
# A build part may carry hyphens of its own, so the prerelease is read from
# the version with that part already removed.
without_build="${version%%+*}"
if [[ "$without_build" == *-* ]]; then
  prerelease="${without_build#*-}"
  IFS=. read -ra identifiers <<<"$prerelease"
  for identifier in "${identifiers[@]}"; do
    [[ ! "$identifier" =~ ^0[0-9]+$ ]] ||
      usage "'$version' has a numeric prerelease identifier with a leading zero"
  done
fi
case "$attempts" in
  "" | *[!0-9]*) usage "VERIFY_ATTEMPTS='$attempts' is not a whole number of attempts" ;;
esac
[ "${#attempts}" -le 4 ] && [ "$attempts" -ge 1 ] && [ "$attempts" -le 1000 ] ||
  usage "VERIFY_ATTEMPTS='$attempts' must be between 1 and 1000 attempts"
case "$delay" in
  "" | *[!0-9]*) usage "VERIFY_DELAY='$delay' is not a whole number of seconds" ;;
esac
[ "${#delay}" -le 4 ] && [ "$delay" -le 3600 ] || usage "VERIFY_DELAY='$delay' exceeds the 3600-second bound"
# Shell arithmetic reads a leading zero as octal, so `08` would pass the checks
# above and then fail where the exhausted bound is reported.
attempts=$((10#$attempts))
delay=$((10#$delay))

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# Each CLI attempt ends in this: an install nothing was run against proves only
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


# Not `assert`: PYTHONOPTIMIZE strips those, and every check would then pass.
def require(holds, message):
    if not holds:
        raise SystemExit(message)


expected = sys.argv[1]
require(__version__ == expected, f"oneharness_sdk.__version__ is {__version__}, not {expected}")
# Each distribution names itself. An SDK whose own metadata is wrong and an SDK
# installed beside a different CLI are different broken releases, and the
# exhausted bound surfaces only what the last attempt said — so a bare check
# here would report a broken release with nothing to act on.
for dist in ("oneharness-sdk", "oneharness-cli"):
    installed = version(dist)
    require(installed == expected, f"the installed {dist} is {installed}, not {expected}")


async def verify():
    harnesses = await OneHarness().list()
    require(
        isinstance(harnesses, list)
        and harnesses
        and all(isinstance(item, dict) and isinstance(item.get("id"), str) for item in harnesses),
        f"the installed SDK did not return the packaged CLI registry: got {harnesses!r}",
    )


asyncio.run(verify())
PY
}

attempt_npm_cli() {
  local global platform
  npm install -g --prefer-online "oneharness-cli@$version" || return 1
  global="$(npm root -g)" || return 1
  # Resolved as the launcher resolves it, from the installed wrapper, and named
  # on stdout so the smoke's verdict can say which package shipped the binary.
  platform="$(
    node - "$global" "$version" "$broken" <<'NODE'
const { existsSync, readFileSync } = require("node:fs");
const { createRequire } = require("node:module");
const path = require("node:path");

const [globalRoot, expected, broken] = process.argv.slice(2);
const fail = (message, status) => {
  process.stderr.write(`${message}\n`);
  process.exit(status);
};
const wrapperManifest = path.join(globalRoot, "oneharness-cli", "package.json");
if (!existsSync(wrapperManifest)) fail(`npm reported success but installed no oneharness-cli under ${globalRoot}`, 1);
const wrapper = JSON.parse(readFileSync(wrapperManifest, "utf8"));
if (wrapper?.version !== expected) fail(`npm installed oneharness-cli ${JSON.stringify(wrapper?.version)}, not ${expected}`, 1);
const platform = `@oneharness/cli-${process.platform}-${process.arch}`;
const pinned = wrapper.optionalDependencies?.[platform];
if (pinned !== expected) {
  fail(`oneharness-cli@${expected} pins ${platform} at ${JSON.stringify(pinned)}, not ${expected}: the published launcher cannot install this host's binary`, Number(broken));
}
let installed;
try {
  installed = JSON.parse(readFileSync(createRequire(wrapperManifest).resolve(`${platform}/package.json`), "utf8"));
} catch (error) {
  if (error.code === "MODULE_NOT_FOUND") {
    fail(`npm installed oneharness-cli@${expected} without its platform package ${platform}@${expected}: the registry is not serving it yet`, 1);
  }
  fail(`npm installed ${platform}, and its package.json cannot be read the way the launcher resolves it: ${error.code ?? error.name}: ${error.message}`, 1);
}
if (installed?.version !== expected) fail(`npm installed ${platform} ${JSON.stringify(installed?.version)}, not ${expected}`, 1);
process.stdout.write(platform);
NODE
  )" || return "$?"
  smoke_cli && return 0
  printf '%s@%s is installed and the oneharness it ships failed the smoke above: a published package a retry cannot change\n' "$platform" "$version" >&2
  return "$broken"
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
import { createRequire } from "node:module";

const expected = process.argv[2];
const isObject = (value) => typeof value === "object" && value !== null && !Array.isArray(value);
const manifest = JSON.parse(readFileSync("node_modules/@oneharness/sdk/package.json", "utf8"));
if (!isObject(manifest)) {
  throw new Error(`installed SDK package.json is not a JSON object: got ${JSON.stringify(manifest)}`);
}
if (manifest.version !== expected) {
  throw new Error(`installed SDK version ${manifest.version} does not match ${expected}`);
}
// The SDK pins its CLI exactly, so a different oneharness-cli beside it is the
// pin having failed. Resolved from the SDK's own location, as its import is.
const cliManifestPath = createRequire(`${process.cwd()}/node_modules/@oneharness/sdk/package.json`).resolve(
  "oneharness-cli/package.json",
);
const cliManifest = JSON.parse(readFileSync(cliManifestPath, "utf8"));
if (!isObject(cliManifest) || cliManifest.version !== expected) {
  throw new Error(`the installed oneharness-cli is ${JSON.stringify(cliManifest?.version)}, not ${expected}`);
}
// Imported only now: resolving the package reads this same manifest, and would
// refuse a malformed one with a loader error that names nothing to act on.
const { OneHarness } = await import("@oneharness/sdk");
const harnesses = await new OneHarness().list();
if (
  !Array.isArray(harnesses) ||
  harnesses.length === 0 ||
  !harnesses.every((item) => isObject(item) && typeof item.id === "string")
) {
  throw new Error(`installed SDK did not return the packaged CLI registry: got ${JSON.stringify(harnesses)}`);
}
NODE
  ) || return 1
}

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
  run_attempt >"$last" 2>&1 && outcome=0 || outcome=$?
  if [ "$outcome" -eq 0 ]; then
    # One line on success: the package managers' own chatter is captured, and
    # replayed only when the bound runs out and somebody needs the cause.
    printf 'verify-published: installed and smoke-tested %s on attempt %s of %s.\n' "$what" "$i" "$attempts"
    exit 0
  fi
  [ "$outcome" -ne "$broken" ] || break
  if [ "$i" -lt "$attempts" ]; then
    sleep "$delay"
  fi
done

printf -- '--- what the last attempt said ---\n' >&2
cat "$last" >&2
printf -- '--- end of the last attempt ---\n' >&2
if [ "$outcome" -eq "$broken" ]; then
  printf '::error::%s is published and broken: attempt %s installed it, and the failure above is in what it installed, which no retry changes.\n' "$what" "$i" >&2
  printf '  Next: fix the named package and cut a new release; re-running this job cannot pass.\n' >&2
  exit 1
fi
printf '::error::%s was still not installable after %s attempts over ~%s seconds. The last attempt output is above.\n' \
  "$what" "$attempts" "$((attempts * delay))" >&2
printf '  Next: the publish itself already happened, so this is either a registry still propagating (re-run this job) or a genuinely broken artifact — the last attempt above says which.\n' >&2
exit 1
