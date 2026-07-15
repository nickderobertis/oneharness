#!/usr/bin/env bash
# Publish the workspace crates in dependency order when their exact manifest
# versions are absent from crates.io. Cargo validates both manifests before any
# registry decision; registry outages fail closed instead of being mistaken for
# a missing release.
set -euo pipefail

cd "$(dirname "$0")/.."

fail() {
  printf 'publish-crates: %s\n' "$1" >&2
  exit 1
}

manifest_version() {
  local manifest="$1" package="$2" package_id version
  if ! package_id="$(cargo pkgid --manifest-path "$manifest" --package "$package" 2>/dev/null)"; then
    fail "cannot validate $package's version in $manifest; run 'cargo metadata --no-deps' and fix the manifest"
  fi
  version="${package_id##*#}"
  version="${version##*@}"
  if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([+-][0-9A-Za-z.-]+)?$ ]]; then
    fail "cargo returned an invalid version '$version' for $package; run 'cargo metadata --no-deps' and fix the manifest"
  fi
  printf '%s\n' "$version"
}

publish_if_missing() {
  local manifest="$1" package="$2" version="$3" status
  if ! status="$(curl --silent --show-error --output /dev/null --write-out '%{http_code}' \
    "https://crates.io/api/v1/crates/${package}/${version}")"; then
    fail "could not query crates.io for $package $version; retry when the registry is reachable"
  fi

  case "$status" in
    200)
      ;;
    404)
      cargo publish --locked --manifest-path "$manifest"
      ;;
    *)
      fail "crates.io returned HTTP $status for $package $version; retry after the registry recovers"
      ;;
  esac
}

core_version="$(manifest_version crates/oneharness-core/Cargo.toml oneharness-core)"
cli_version="$(manifest_version Cargo.toml oneharness)"

if [ -n "${GITHUB_REF_NAME:-}" ] && [ "$GITHUB_REF_NAME" != "v$cli_version" ]; then
  fail "release tag '$GITHUB_REF_NAME' does not match the CLI manifest version 'v$cli_version'; publish the matching release tag"
fi

publish_if_missing crates/oneharness-core/Cargo.toml oneharness-core "$core_version"
publish_if_missing Cargo.toml oneharness "$cli_version"
