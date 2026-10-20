#!/usr/bin/env bash
# Drive `verify-published.sh npm-cli` through the REAL npm client against a
# local registry double (scripts/npm-registry-double.mjs) serving packages the
# real scripts/npm-build.mjs assembled: the launcher, and a platform package per
# release target whose binary is a stand-in script. What it proves is what npm
# itself does with an optional dependency the registry does not serve yet.
set -euo pipefail

case $(uname -s) in
  MINGW* | MSYS* | CYGWIN*)
    echo "check-verify-published-npm: skipped on Windows because the launcher execs the platform package's bin/oneharness.exe, which a shell script cannot stand in for" >&2
    exit 0
    ;;
esac

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

for tool in node npm; do
  command -v "$tool" >/dev/null || {
    echo "check-verify-published-npm: no $tool on PATH; install Node.js (it ships npm) and rerun this test" >&2
    exit 1
  }
done

tmp="$(mktemp -d)"
registry_pid=""
cleanup() {
  if [ -n "$registry_pid" ]; then
    kill "$registry_pid" 2>/dev/null || true
    wait "$registry_pid" 2>/dev/null || true
  fi
  rm -rf "$tmp"
}
trap cleanup EXIT

VERSION_UNDER_TEST=9.9.9
host_package="@oneharness/cli-$(node -p 'process.platform + "-" + process.arch')"
hidden="$tmp/registry/${host_package/\//+}@$VERSION_UNDER_TEST.hidden"

fail() {
  echo "check-verify-published-npm: $1" >&2
  # The scratch directory is removed on exit, so the request log is printed
  # here rather than pointed at.
  if [ -s "$tmp/requests" ]; then
    echo "  Registry requests the case made:" >&2
    sed 's/^/    /' "$tmp/requests" >&2
  fi
  echo "  Next: the case's own output and its registry requests are above; re-run with 'bash -x scripts/check-verify-published-npm.sh'." >&2
  exit 1
}

# $1 what the stand-in binary does on --version: `works` or `broken`.
publish_packages() {
  local binary="$tmp/oneharness" declared targets target built
  rm -rf "$tmp/build" "$tmp/registry"
  mkdir -p "$tmp/build" "$tmp/registry"
  if [ "$1" = works ]; then
    cat >"$binary" <<SH
#!/bin/sh
[ "\$1" = --version ] && echo "oneharness $VERSION_UNDER_TEST"
exit 0
SH
  else
    printf '#!/bin/sh\necho "oneharness: cannot execute binary file: Exec format error" >&2\nexit 126\n' >"$binary"
  fi
  chmod +x "$binary"
  cp "$binary" "$binary.exe"
  # Every platform the launcher pins, read from the one list that states them,
  # so npm meets each optional dependency the way it does on the registry.
  # A read loop, not mapfile: macOS's system bash is 3.2.
  declared="$(sed -n 's/^target = "\(.*\)"$/\1/p' release-platforms.toml)" ||
    fail "could not read release-platforms.toml; restore it from git"
  targets=()
  while IFS= read -r target; do
    [ -z "$target" ] || targets+=("$target")
  done <<<"$declared"
  [ "${#targets[@]}" -gt 0 ] || fail "release-platforms.toml declares no target to publish; restore its [[platform]] entries (bash scripts/check-release-targets.sh names what is wrong with it)"
  for target in "${targets[@]}"; do
    case "$target" in
      *-windows-*) built="$binary.exe" ;;
      *) built="$binary" ;;
    esac
    node scripts/npm-build.mjs platform --target "$target" --binary "$built" --version "$VERSION_UNDER_TEST" --out "$tmp/build" >/dev/null
  done
  node scripts/npm-build.mjs launcher --version "$VERSION_UNDER_TEST" --out "$tmp/build" >/dev/null
  for package in "$tmp"/build/*/; do
    npm pack "$package" --pack-destination "$tmp/registry" >/dev/null 2>"$tmp/pack-error" || {
      cat "$tmp/pack-error" >&2
      fail "npm could not pack $package"
    }
  done
}

start_registry() {
  local waited=0
  rm -f "$tmp/port"
  : >"$tmp/requests"
  node scripts/npm-registry-double.mjs "$tmp/registry" "$tmp/port" "$tmp/requests" &
  registry_pid=$!
  until [ -s "$tmp/port" ]; do
    waited=$((waited + 1))
    [ "$waited" -le 100 ] || fail "the registry double did not report a port within 10 seconds"
    sleep 0.1
  done
}

stop_registry() {
  kill "$registry_pid" 2>/dev/null ||
    fail "$description: the registry double had already exited before the case ended; its own error is above"
  wait "$registry_pid" 2>/dev/null || true
  registry_pid=""
}

# $1 attempts, $2 description. A fresh registry (its request clocks start at
# zero), npm prefix and cache each case, and npmrc files of its own, so neither
# this machine's npm config nor an earlier case's install can stand in for what
# the registry served. Leaves $status, $tmp/out, $tmp/err and $tmp/requests.
run_case() {
  description="$2"
  rm -rf "$tmp/prefix" "$tmp/cache"
  start_registry
  : >"$tmp/npmrc-user"
  : >"$tmp/npmrc-global"
  npm_config_registry="http://127.0.0.1:$(cat "$tmp/port")/" npm_config_cache="$tmp/cache" \
  npm_config_prefix="$tmp/prefix" npm_config_userconfig="$tmp/npmrc-user" \
  npm_config_globalconfig="$tmp/npmrc-global" npm_config_audit=false npm_config_fund=false \
  npm_config_update_notifier=false PATH="$tmp/prefix/bin:$PATH" VERIFY_ATTEMPTS="$1" VERIFY_DELAY=0 \
    bash scripts/verify-published.sh npm-cli "$VERSION_UNDER_TEST" >"$tmp/out" 2>"$tmp/err" && status=0 || status=$?
  stop_registry
}

expect_status() {
  [ "$status" -eq "$1" ] || {
    cat "$tmp/out" "$tmp/err" >&2
    fail "$description: expected exit $1, got $status"
  }
}

expect_said() {
  grep -Fq -- "$2" "$1" || {
    cat "$tmp/out" "$tmp/err" >&2
    fail "$description: expected to say '$2'"
  }
}

expect_wrapper_requests() {
  local actual
  actual="$(grep -c '^GET oneharness-cli 9' "$tmp/requests" || true)"
  [ "$actual" -eq "$1" ] ||
    fail "$description: expected $1 install attempt(s) to ask for the wrapper, got $actual"
}

publish_packages works

run_case 3 "a release whose wrapper and platform packages are all served"
expect_status 0
expect_said "$tmp/out" "installed and smoke-tested oneharness-cli@$VERSION_UNDER_TEST from npm on attempt 1 of 3"

# v0.25.8: the wrapper resolvable, its platform package not yet. npm installs
# the wrapper, skips the optional dependency, and exits 0 — twice — then the
# registry serves it and the third attempt installs it.
echo "2 oneharness-cli" >"$hidden"
run_case 5 "a platform package served only after the wrapper's second install"
expect_status 0
expect_said "$tmp/out" "on attempt 3 of 5"
expect_wrapper_requests 3
grep -Fq "GET $host_package (none)" "$tmp/requests" ||
  fail "$description: the registry never withheld $host_package, so the case proved nothing about a lag"

# Withheld past the bound: every attempt installs the wrapper alone, and the
# report names the package the registry never served.
echo "1000 oneharness-cli" >"$hidden"
run_case 3 "a platform package the registry never serves"
expect_status 1
expect_said "$tmp/err" "without its platform package $host_package@$VERSION_UNDER_TEST: the registry is not serving it yet"
expect_said "$tmp/err" "oneharness-cli@$VERSION_UNDER_TEST from npm was still not installable after 3 attempts"
expect_wrapper_requests 3
rm -f "$hidden"

# Served and installed, and its binary cannot run: a published tarball never
# changes, so one attempt is the verdict and the bound is not spent.
publish_packages broken
run_case 3 "a platform package whose binary cannot run --version"
expect_status 1
expect_said "$tmp/err" "the installed oneharness could not run --version"
expect_said "$tmp/err" "$host_package@$VERSION_UNDER_TEST is installed and the oneharness it ships failed the smoke above"
expect_said "$tmp/err" "oneharness-cli@$VERSION_UNDER_TEST from npm is published and broken: attempt 1 installed it"
expect_wrapper_requests 1

echo "check-verify-published-npm: ok"
