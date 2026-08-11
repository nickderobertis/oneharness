#!/usr/bin/env bash
# Verify the crates Cargo will assemble, including the binary's registry-resolved
# oneharness-core dependency. Two transitions are permitted, because in both the
# binary's dependency is on its way to the registry and nothing in the working
# tree can hurry it: a release-worthy core change release-plz has yet to bump,
# and a core version release-plz has already tagged but release.yml has yet to
# publish.
#
# This guards a release from the PR that precedes it; it CANNOT guard the
# release itself and must never run there. At a release tag the binary already
# pins the core version that same run is about to publish, so `cargo package`
# for the binary cannot resolve it — the check is structurally red on every
# core-bumping tag. See the `package` job in ci.yml.
set -euo pipefail

cd "$(dirname "$0")/.."

core_output="$(mktemp)"
output_file="$(mktemp)"
fetch_output="$(mktemp)"
trap 'rm -f "$core_output" "$output_file" "$fetch_output"' EXIT

if ! cargo package --locked --allow-dirty --manifest-path crates/oneharness-core/Cargo.toml >"$core_output" 2>&1; then
  cat "$core_output" >&2
  echo "package-crates: core package verification failed; fix the Cargo diagnostics above and rerun 'just package-crates'" >&2
  exit 1
fi
if cargo package --locked --allow-dirty --manifest-path Cargo.toml >"$output_file" 2>&1; then
  echo "package-crates: ok"
  exit 0
fi

# The binary did not package. Everything below only classifies WHY, so the Cargo
# diagnostics are the finding on every path that does not end in a permitted
# transition.
reject() {
  cat "$output_file" >&2
  echo "package-crates: $1" >&2
  exit 1
}

# Three registry shapes, and the first means the opposite of the other two. A
# version Cargo cannot select is a core release that has not reached crates.io
# yet; a core it selected and then failed to compile — or whose published
# FEATURES the binary's forwarding does not find — is a source drift that a bump
# has to carry. Only the first may be waved through on the strength of a tag.
#
# The feature arm is its own: the binary forwards `mock-harness` to
# `oneharness-core/mock-harness`, and a published core without that feature
# fails before any source is unpacked, worded "for `oneharness-core`" rather
# than "for the requirement", so neither other arm sees it.
core_version_unpublished=false
registry_core_mismatch=false
if grep -Eq "failed to select a version for the requirement \`oneharness-core" "$output_file" &&
  grep -Eq 'candidate versions found which did(n.t| not) match:' "$output_file" &&
  grep -Fq 'location searched: crates.io index' "$output_file" &&
  grep -Eq 'required by package `oneharness v[0-9]+\.[0-9]+\.[0-9]+' "$output_file"; then
  core_version_unpublished=true
  registry_core_mismatch=true
elif grep -Eq 'oneharness-core-[0-9]+\.[0-9]+\.[0-9]+[/\\]' "$output_file" &&
  grep -Eq "could not compile \`oneharness\`" "$output_file"; then
  registry_core_mismatch=true
elif grep -Eq "failed to select a version for \`oneharness-core\`" "$output_file" &&
  grep -Eq "depends on \`oneharness-core\`, with features:" "$output_file"; then
  registry_core_mismatch=true
fi

if [ "$registry_core_mismatch" = false ]; then
  reject "binary packaging failed for a reason other than its registry-resolved oneharness-core transition; fix the Cargo diagnostics above and rerun 'just package-crates'"
fi

# Only a registry-resolution failure needs the release history, so read it here
# rather than up front: on every other path this check stays offline, and a tag
# state it cannot determine can only ever fail packaging.
core_tags_visible() {
  [ -n "$(git tag --merged HEAD --list 'oneharness-core-v*' --sort=-version:refname | head -n 1)" ]
}
if ! core_tags_visible; then
  fetch_args=(--tags --force)
  shallow_repository="$(git rev-parse --is-shallow-repository)" || {
    reject "could not determine whether the checkout is shallow; fix the Git repository and retry"
  }
  case "$shallow_repository" in
    true) fetch_args+=(--unshallow) ;;
    false) ;;
    *) reject "git returned an invalid shallow-repository state '$shallow_repository'; fix the Git repository and retry" ;;
  esac
  # --force, and never --quiet. A CI checkout of a tag writes refs/tags/<tag> as
  # a lightweight ref at the commit while origin carries an annotated tag, so
  # this fetch is rejected as "would clobber existing tag" — and --quiet
  # suppresses that line, which is how a fetch failure once reported itself as
  # an absent network.
  if ! git fetch "${fetch_args[@]}" origin >"$fetch_output" 2>&1; then
    cat "$fetch_output" >&2
    reject "core release tags are absent and could not be fetched from origin; check the Git diagnostics above and retry"
  fi
  if ! core_tags_visible; then
    reject "no merged oneharness-core release tag found after fetching origin; restore the release tags and retry"
  fi
fi

# release-plz has already tagged this core version; only the crates.io publish is
# outstanding, and it is release.yml's to make. Nothing here can bring it forward.
core_version="$(sed -n 's/^version = "\([0-9][^"]*\)"$/\1/p' crates/oneharness-core/Cargo.toml | head -n 1)"
if [ -z "$core_version" ]; then
  reject "could not read oneharness-core's version from crates/oneharness-core/Cargo.toml; fix the manifest and rerun 'just package-crates'"
fi
if [ "$core_version_unpublished" = true ] &&
  [ -n "$(git tag --merged HEAD --list "oneharness-core-v$core_version")" ]; then
  echo "package-crates: binary package awaits the crates.io publish of already-tagged oneharness-core $core_version; core package verified" >&2
  exit 0
fi

core_tag="$(git tag --merged HEAD --list 'oneharness-core-v*' --sort=-version:refname | head -n 1)"

# Before release-plz's generated PR, a legitimate core fix still carries the
# last published version. Permit only a release-worthy commit which both follows
# the last core tag and actually touches the core crate; release-plz will bump it.
pending_core_release=false
# A developer must be able to run the gate before committing the core fix. The
# commit subject is enforced once committed; a dirty core tree is transitional.
if ! git diff --quiet -- crates/oneharness-core || ! git diff --cached --quiet -- crates/oneharness-core; then
  pending_core_release=true
fi
release_subject_re='^(feat|fix|perf)(\([^)]+\))?!?:'
while IFS= read -r commit; do
  subject="$(git show -s --format=%s "$commit")"
  body="$(git show -s --format=%b "$commit")"
  if [[ "$subject" =~ $release_subject_re ]] || grep -q '^BREAKING CHANGE:' <<<"$body"; then
    if git diff-tree --no-commit-id --name-only -r "$commit" -- crates/oneharness-core | grep -q .; then
      pending_core_release=true
      break
    fi
  fi
done < <(git rev-list "$core_tag"..HEAD)

if [ "$pending_core_release" = true ]; then
  echo "package-crates: binary package awaits release-plz's core version bump; core package verified" >&2
  exit 0
fi

reject "binary cannot be packaged against its published oneharness-core dependency, and no release-worthy core change will make release-plz bump it; commit the core API change as fix/feat/perf (or BREAKING CHANGE), then rerun 'just package-crates'"
