#!/usr/bin/env bash
# Verify the crates Cargo will assemble, including the binary's registry-resolved
# oneharness-core dependency. A release-worthy core change is the one permitted
# transition: release-plz must bump it before that dependency can exist upstream.
set -euo pipefail

cd "$(dirname "$0")/.."

core_tag="$(git tag --merged HEAD --list 'oneharness-core-v*' --sort=-version:refname | head -n 1)"
if [ -z "$core_tag" ]; then
  git fetch --quiet --tags origin || {
    echo "package-crates: core release tags are absent and could not be fetched from origin; check network access and retry" >&2
    exit 1
  }
  core_tag="$(git tag --merged HEAD --list 'oneharness-core-v*' --sort=-version:refname | head -n 1)"
  if [ -z "$core_tag" ]; then
    echo "package-crates: no merged oneharness-core release tag found after fetching origin; restore the release tags and retry" >&2
    exit 1
  fi
fi

core_output="$(mktemp)"
output_file="$(mktemp)"
trap 'rm -f "$core_output" "$output_file"' EXIT
if ! cargo package --locked --allow-dirty --manifest-path crates/oneharness-core/Cargo.toml >"$core_output" 2>&1; then
  cat "$core_output" >&2
  echo "package-crates: core package verification failed; fix the Cargo diagnostics above and rerun 'just package-crates'" >&2
  exit 1
fi
if cargo package --locked --allow-dirty --manifest-path Cargo.toml >"$output_file" 2>&1; then
  rm -f "$core_output" "$output_file"
  trap - EXIT
  echo "package-crates: ok"
  exit 0
fi

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

cat "$output_file" >&2
echo "package-crates: binary cannot be packaged against its published oneharness-core dependency, and no release-worthy core change will make release-plz bump it; commit the core API change as fix/feat/perf (or BREAKING CHANGE), then rerun 'just package-crates'" >&2
exit 1
