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
cache_hit="${2:-false}"

if [ "$cache_hit" != "true" ]; then
  # --force because a cache miss must replace whatever else put a `just` in
  # Cargo's bin dir (rust-cache restores one), so the version verified below is
  # the one installed here.
  cargo install just --locked --version "$version" --force
fi

installed="$(just --version)"
if [ "$installed" != "just $version" ]; then
  echo "::error::setup-just has '$installed' on PATH but .tool-versions pins just $version" >&2
  exit 1
fi
