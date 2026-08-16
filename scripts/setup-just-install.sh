#!/usr/bin/env bash
# Put the pinned `just` on PATH for the setup-just action, from the cache when it
# restored one and from crates.io when it did not, and prove the `just` that
# results is that exact version. Both paths must end at the same claim, which is
# what makes this a script rather than two workflow lines: a composite action's
# steps cannot be driven by a test, and an unverified install is how a cached
# binary from another pin, or a `just` some other action left behind, becomes the
# one every recipe in the gate runs.
set -euo pipefail

version="${1:?the pinned just version (x.y.z)}"
cache_hit="${2-}"

# Both arguments come from workflow expressions, so neither is taken on trust:
# the version reaches a `cargo install` argument, and treating "anything but
# `true`" as a miss would read a typo'd expression as a cold cache — a silent
# reinstall on every warm run rather than a failure anyone sees. actions/cache
# emits exactly these three (empty when it never reached the cache service).
if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "::error::setup-just needs an exact x.y.z version, got '$version'" >&2
  exit 1
fi
case "$cache_hit" in
  true | false | '') ;;
  *)
    echo "::error::setup-just needs a cache-hit of 'true', 'false' or empty, got '$cache_hit'" >&2
    exit 1
    ;;
esac

if [ "$cache_hit" != "true" ]; then
  # --force because a cache miss must replace whatever else put a `just` in
  # Cargo's bin dir (rust-cache restores one), so the version verified below is
  # the one installed here.
  log="$(mktemp)"
  trap 'rm -f "$log"' EXIT
  if ! cargo install just --locked --version "$version" --force >"$log" 2>&1; then
    cat "$log" >&2
    echo "::error::setup-just could not install just $version from crates.io" >&2
    exit 1
  fi
fi

installed="$(just --version)"
if [ "$installed" != "just $version" ]; then
  echo "::error::setup-just has '$installed' on PATH but .tool-versions pins just $version" >&2
  exit 1
fi
