#!/usr/bin/env bash
#
# Behavioral test of the release-target drift gate.
#
# That gate's whole job is to fail, and a gate nobody has watched fail is not
# known to work — which matters more here than usual, because what it protects
# against is an inventory going stale in silence. So it is driven against a
# staged checkout, once per way the declaration, the release configuration and
# the probe can drift apart, and asserted to go red naming what it wants.
#
# Quiet on success, one line. On failure it prints what the gate said.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

fail() {
  echo "check-release-targets-test: $1" >&2
  [ -s "$work/out" ] && cat "$work/out" >&2
  exit 1
}

# Everything the gate reads: the declaration, the release workflow, the probe
# whose registry list it mirrors, the two derivation scripts, and every manifest
# it resolves a name from, plus the packaging scripts it traces each manifest
# to. Staged into a real repository because the gate reads the committed
# manifest set.
staged=(
  release-targets.toml
  .github/workflows/release.yml
  scripts/check-release-targets.sh
  scripts/release-probe.sh
  scripts/publish-crates.sh
  scripts/npm-build.mjs
  scripts/sdk-pack.mjs
  scripts/python-sdk-pack.mjs
  Cargo.toml
  crates/oneharness-core/Cargo.toml
  pyproject.toml
  python/oneharness-sdk/pyproject.toml
  npm/oneharness/package.json
  npm/oneharness-sdk/package.json
)

# $1 = fixture name. Leaves a fresh staged checkout at $work/$1 and prints it.
stage() {
  local root="$work/$1"
  rm -rf "$root"
  for file in "${staged[@]}"; do
    mkdir -p "$root/$(dirname "$file")"
    cp "$file" "$root/$file"
  done
  git -C "$root" init -q
  git -C "$root" add -A
  printf '%s\n' "$root"
}

# Rewrite a staged file through an awk program. $1 = fixture root,
# $2 = repository-relative path, $3 = awk program.
rewrite() {
  local target="$1/$2"
  awk "$3" "$target" >"$work/rewritten"
  if cmp -s "$target" "$work/rewritten"; then
    fail "the mutation for $2 changed nothing; update this case's awk program to match that file's current shape, or drop the case if what it mutated is gone"
  fi
  mv "$work/rewritten" "$target"
}

# $1 = fixture name, $2 = description, $3 = text the finding must name.
assert_red() {
  local root="$work/$1" description=$2 expected=$3
  git -C "$root" add -A
  if bash "$root/scripts/check-release-targets.sh" >"$work/out" 2>&1; then
    fail "$description should have failed the gate; restore the check for it in scripts/check-release-targets.sh, or drop this case if that drift can no longer happen"
  fi
  grep -Fq "$expected" "$work/out" ||
    fail "$description failed the gate without naming '$expected'; restore that detail in the gate's diagnostic, or update this case's expected text to what the gate says now"
}

# The real tree passes. Anchoring here first means every red below is the
# mutation rather than a gate that rejects everything.
root="$(stage baseline)"
if ! bash "$root/scripts/check-release-targets.sh" >"$work/out" 2>&1; then
  fail "the checked-in declaration should pass the gate; reconcile release-targets.toml with what the release configuration publishes (the gate names each drift above) before reading any case below"
fi

root="$(stage no-declaration)"
rm "$root/release-targets.toml"
assert_red no-declaration "no declaration at all" \
  "release-targets.toml is missing"

root="$(stage empty-declaration)"
rewrite "$root" release-targets.toml '/^\[\[target\]\]$/ { exit } { print }'
assert_red empty-declaration "a declaration with no targets" \
  "declares no [[target]] entries"

root="$(stage schema-drift)"
rewrite "$root" release-targets.toml '{ sub(/^schema_version = 1$/, "schema_version = 2"); print }'
assert_red schema-drift "a declaration written to a version this gate cannot read" \
  "declares schema_version '2'"

root="$(stage manifestless)"
rewrite "$root" release-targets.toml '!/^manifest = "Cargo.toml"$/ { print }'
assert_red manifestless "a target with no manifest" \
  "declares 'crate:oneharness' with no manifest"

root="$(stage idless)"
rewrite "$root" release-targets.toml '!/^id = "crate:oneharness"$/ { print }'
assert_red idless "a target with no id" \
  "has a [[target]] with no id"

root="$(stage duplicate-id-in-block)"
rewrite "$root" release-targets.toml '
  { print }
  /^id = "crate:oneharness"$/ { print "id = \"crate:oneharness-again\"" }
'
assert_red duplicate-id-in-block "one [[target]] carrying two ids" \
  "with two"

root="$(stage duplicate-manifest-in-block)"
rewrite "$root" release-targets.toml '
  { print }
  /^manifest = "Cargo.toml"$/ { print "manifest = \"pyproject.toml\"" }
'
assert_red duplicate-manifest-in-block "one [[target]] carrying two manifests" \
  "with two"

# Two rows answering to one id: only one of them is ever consulted, and a
# consumer cannot tell which.
root="$(stage duplicate-id)"
cat >>"$root/release-targets.toml" <<'DUPLICATE'

[[target]]
id = "crate:oneharness"
manifest = "Cargo.toml"
DUPLICATE
assert_red duplicate-id "one id declared by two targets" \
  "declares 'crate:oneharness' more than once"

root="$(stage missing-manifest)"
rm "$root/python/oneharness-sdk/pyproject.toml"
assert_red missing-manifest "a declaration pointing at a manifest that is gone" \
  'declares manifest "python/oneharness-sdk/pyproject.toml" for pypi:oneharness-sdk, which does not exist'

# A published artifact nobody declared — the failure this gate exists for: a
# consumer waiting on the Node SDK would get no hold at all.
root="$(stage undeclared)"
# The single-quoted program is awk; its $0 is awk's whole-line variable.
# shellcheck disable=SC2016
rewrite "$root" release-targets.toml '
  /^\[\[target\]\]$/ { header = $0; buffered = ""; drop = 0; open = 1; next }
  open && /^id = "npm:@oneharness\/sdk"$/ { drop = 1 }
  open {
    buffered = buffered $0 "\n"
    if (/^manifest = /) {
      if (!drop) printf "%s\n%s", header, buffered
      open = 0
    }
    next
  }
  { print }
'
assert_red undeclared "a published npm package with no declared target" \
  "publishes 'npm:@oneharness/sdk' (from npm/oneharness-sdk/package.json) and release-targets.toml declares no target for it"

root="$(stage unpublished)"
cat >>"$root/release-targets.toml" <<'EXTRA'

[[target]]
id = "crate:oneharness-retired"
manifest = "Cargo.toml"
EXTRA
assert_red unpublished "a declared target nothing publishes" \
  "declares 'crate:oneharness-retired', which this repository's release configuration does not publish"

root="$(stage renamed)"
rewrite "$root" python/oneharness-sdk/pyproject.toml \
  '{ sub(/^name = "oneharness-sdk"$/, "name = \"oneharness-client\""); print }'
assert_red renamed "a manifest renamed out from under its declaration" \
  'names "oneharness-client"'

root="$(stage foreign-registry)"
rewrite "$root" release-targets.toml '{ sub(/^id = "npm:@oneharness\/sdk"$/, "id = \"gem:@oneharness/sdk\""); print }'
assert_red foreign-registry "a target on a registry neither side answers for" \
  'declares "gem:@oneharness/sdk", whose registry is not one of'

root="$(stage no-crate-publisher)"
rewrite "$root" .github/workflows/release.yml '!/run: scripts\/publish-crates.sh/ { print }'
assert_red no-crate-publisher "a release workflow that no longer publishes the crates" \
  "no longer runs scripts/publish-crates.sh"

root="$(stage no-crate-calls)"
rewrite "$root" scripts/publish-crates.sh '!/^publish_if_missing / { print }'
assert_red no-crate-calls "a crate publisher with no publish calls to derive from" \
  "declares no publish_if_missing calls"

root="$(stage nameless-wheel)"
rewrite "$root" pyproject.toml '!/^name = "oneharness-cli"$/ { print }'
assert_red nameless-wheel "a pyproject whose distribution name is gone" \
  "pyproject.toml has no [project] name"

root="$(stage no-npm-publisher)"
rewrite "$root" .github/workflows/release.yml '!/scripts\/publish-npm.sh/ { print }'
assert_red no-npm-publisher "a release workflow that no longer publishes the npm packages" \
  "no longer runs scripts/publish-npm.sh"

root="$(stage nameless-npm)"
rewrite "$root" npm/oneharness-sdk/package.json '!/^  "name": "@oneharness\/sdk",$/ { print }'
assert_red nameless-npm "an npm manifest whose package name is gone" \
  'npm/oneharness-sdk/package.json has no top-level "name"'

# A manifest committed with nothing to build it: whatever it declares reaches no
# registry, so the gate must not read it as a published artifact.
root="$(stage unpackaged)"
mkdir -p "$root/python/extra"
cat >"$root/python/extra/pyproject.toml" <<'EXTRA'
[project]
name = "oneharness-extra"
version = "0.0.0"
EXTRA
assert_red unpackaged "a manifest nothing in the release packages" \
  "python/extra/pyproject.toml is committed but nothing .github/workflows/release.yml runs packages it"

root="$(stage no-platform-table)"
rewrite "$root" scripts/npm-build.mjs '!/{ platform: "/ { print }'
assert_red no-platform-table "a build script whose platform table moved" \
  "yielded no per-platform package names"

# A distribution that is built and never published: the release workflow's
# publishing steps must account for every committed pyproject.
root="$(stage unpublished-wheel)"
rewrite "$root" .github/workflows/release.yml '
  /uses: pypa\/gh-action-pypi-publish/ && !dropped { dropped = 1; next }
  { print }
'
assert_red unpublished-wheel "a pyproject with no publishing step" \
  "PyPI publishing step(s) for 2 committed pyproject.toml manifest(s)"

# A new per-platform package that no launcher pins: published, and accounted
# for nowhere, since a platform package is covered rather than declared.
root="$(stage uncovered)"
rewrite "$root" scripts/npm-build.mjs '
  { print }
  /^  "x86_64-pc-windows-msvc":/ {
    print "  \"aarch64-pc-windows-msvc\": { platform: \"win32\", arch: \"arm64\", exe: true },"
  }
'
assert_red uncovered "a per-platform package no launcher pins" \
  "publishes '@oneharness/cli-win32-arm64' and no declared npm target's optionalDependencies pins it"

# The probe owns which registries are answerable; this gate mirrors that list,
# and a registry dropped from one side must not sit stale on the other.
root="$(stage registry-drift)"
rewrite "$root" scripts/release-probe.sh '!/^  npm\)$/ { print }'
assert_red registry-drift "a probe that stopped answering for a declared registry" \
  "while this gate can read a name for"

echo "check-release-targets-test: the drift gate goes red for every way the declaration, the release configuration and the probe can drift apart"
