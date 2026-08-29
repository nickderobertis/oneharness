#!/usr/bin/env bash
#
# Behavioral test of the release-target drift gate.
#
# That gate's whole job is to fail, and a gate nobody has watched fail is not
# known to work — which matters more here than usual, because what it protects
# against is an inventory going stale in silence. So it is driven against a
# staged checkout, once per way the declaration can leave the canonical schema
# and once per way it, the release configuration and the probe can drift apart,
# and asserted to go red naming what it wants.
#
# The schema half needs the passes as much as the refusals: `[[retired]]` is a
# key this repository declares nothing under today, so without a fixture that
# uses it, "the gate accepts a retirement" and "the gate has never seen one"
# would look identical.
#
# Quiet on success, one line. On failure it prints what the gate said.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

fail() {
  echo "check-release-targets-test: $1" >&2
  exit 1
}

# The same, with the gate's own output for the case that just ran printed
# beneath it. Only an assertion that captured one says this: `rewrite` below
# fails before any gate has run, and one capture is reused across cases, so
# printing it unconditionally would name the previous case's diagnostic as this
# failure's cause.
fail_showing() {
  echo "check-release-targets-test: $1" >&2
  echo "  what the gate said:" >&2
  cat "$work/out" >&2
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
    fail_showing "$description should have failed the gate; restore the check for it in scripts/check-release-targets.sh, or drop this case if that drift can no longer happen"
  fi
  grep -Fq "$expected" "$work/out" ||
    fail_showing "$description failed the gate without naming '$expected'; restore that detail in the gate's diagnostic, or update this case's expected text to what the gate says now"
}

# $1 = fixture name, $2 = description. The gate must accept it.
assert_green() {
  local root="$work/$1" description=$2
  git -C "$root" add -A
  if ! bash "$root/scripts/check-release-targets.sh" >"$work/out" 2>&1; then
    fail_showing "$description should have passed the gate; fix whichever side is wrong — the fixture, or the check that now rejects it"
  fi
}

# $1 = fixture name, $2 = description, $3 = text the gate must say while passing.
assert_green_saying() {
  local root="$work/$1" description=$2 expected=$3
  assert_green "$1" "$description"
  grep -Fq "$expected" "$work/out" ||
    fail_showing "$description passed the gate without saying '$expected'; a green that stops naming the deviation reads as conformance the document has not got, so restore that in the gate's success line — or, if the canonical schema now expresses these names, drop the deviation and this case together"
}

# The real tree passes. Anchoring here first means every red below is the
# mutation rather than a gate that rejects everything.
root="$(stage baseline)"
if ! bash "$root/scripts/check-release-targets.sh" >"$work/out" 2>&1; then
  fail_showing "the checked-in declaration should pass the gate; reconcile release-targets.toml with what the release configuration publishes it names each drift below, before any case runs"
fi

# The cases below hold the document to the canonical schema — the shape six
# repositories share, so that a consumer needs no knowledge of this one to read
# it.

# A key nobody declared, which is the likeliest defect in a hand-written
# document: read as an absent `manifest`, it publishes an answer nobody wrote.
root="$(stage misspelled-key)"
rewrite "$root" release-targets.toml '{ sub(/^manifest = "Cargo.toml"$/, "manifset = \"Cargo.toml\""); print }'
assert_red misspelled-key "a key this schema does not declare" \
  'names "manifset" in [[target]] 2, which schema_version 1 does not declare'

root="$(stage unknown-table)"
cat >>"$root/release-targets.toml" <<'TABLE'

[extra]
key = "value"
TABLE
assert_red unknown-table "a table this schema does not declare" \
  "opens [extra], which schema_version 1 does not declare"

root="$(stage unreadable-line)"
printf '\nnonsense\n' >>"$root/release-targets.toml"
assert_red unreadable-line "a line that is not a key = value" \
  "that is not a \`key = value\`: nonsense"

# Each required field, dropped. A target with no short name cannot be named by
# a host document or a plan node; one with no `what` or `published_by` leaves a
# reader the identifier alone where they were promised a sentence.
root="$(stage nameless-target)"
rewrite "$root" release-targets.toml '!/^name = "cli-crate"$/ { print }'
assert_red nameless-target "a target with no short name" \
  'declares no name in [[target]] 2 ("crate:oneharness")'

root="$(stage whatless-target)"
rewrite "$root" release-targets.toml '!/^what = "The .oneharness. binary as/ { print }'
assert_red whatless-target "a target that says nothing about what a dependent gets" \
  'declares no what in [[target]] 2 ("crate:oneharness")'

root="$(stage publisherless-target)"
rewrite "$root" release-targets.toml '!/^published_by = ".github\/workflows\/release.yml — the publish-crates job, second/ { print }'
assert_red publisherless-target "a target that names no publishing job" \
  'declares no published_by in [[target]] 2 ("crate:oneharness")'

# Blank is its own defect: the key is there and says nothing.
root="$(stage blank-prose)"
rewrite "$root" release-targets.toml '{ sub(/^what = "The reusable engine.*$/, "what = \"   \""); print }'
assert_red blank-prose "a target whose sentence is blank" \
  'leaves what blank in [[target]] 1'

# An identifier that names no registry: `oneharness-cli` alone is two artifacts.
root="$(stage unqualified-id)"
rewrite "$root" release-targets.toml '{ sub(/^id = "pypi:oneharness-sdk"$/, "id = \"oneharness-sdk\""); print }'
assert_red unqualified-id "an identifier that names no registry" \
  'as "oneharness-sdk", which is not <registry>:<name>'

# The one identifier shape this gate takes and the canonical reader does not: a
# scoped npm name, whose leading `@` that reader's RegistryId will not let a
# name open with. The artifacts are real, so the document keeps the names npm
# serves — and the gate's own pass has to name them, because a green that said
# nothing would read as a document the canonical reader takes when it refuses
# the whole of it. Both halves are proven: that the pass names them, and that
# the window is exactly a scope rather than any leading `@`.
root="$(stage scoped-name-reported)"
assert_green_saying scoped-name-reported "the checked-in declaration's scoped npm names" \
  'npm:@oneharness/sdk, whose leading @ its RegistryId refuses'

root="$(stage at-sign-without-a-scope)"
rewrite "$root" release-targets.toml '{ sub(/^id = "npm:@oneharness\/sdk"$/, "id = \"npm:@oneharness\""); print }'
assert_red at-sign-without-a-scope "a name opening with @ that is not a scope" \
  'as "npm:@oneharness", which is not <registry>:<name>'

# Two targets answering to one short name: that name is what a host document
# and a plan node select by, so two of them are two answers to one question.
root="$(stage repeated-short-name)"
rewrite "$root" release-targets.toml '{ sub(/^name = "cli-npm"$/, "name = \"core\""); print }'
assert_red repeated-short-name "one short name taken by two targets" \
  "gives the short name 'core' to more than one target"

# Every way a value can be written that this reader will not read. A value it
# skipped past would be a field nobody declared taking effect as an absent one.
root="$(stage unreadable-string)"
rewrite "$root" release-targets.toml '{ sub(/^name = "core"$/, "name = \"co\\re"); print }'
assert_red unreadable-string "a string this reader cannot read" \
  "as a string this reader cannot read"

root="$(stage trailing-after-string)"
rewrite "$root" release-targets.toml '{ sub(/^name = "core"$/, "name = \"core\" \"engine\""); print }'
assert_red trailing-after-string "a second value after a string" \
  "writes name in [[target]] 1 with something after its value"

root="$(stage trailing-after-number)"
rewrite "$root" release-targets.toml '{ sub(/^schema_version = 1$/, "schema_version = 1 2"); print }'
assert_red trailing-after-number "a second value after a number" \
  "writes schema_version in the document with something after its value"

root="$(stage malformed-list)"
rewrite "$root" release-targets.toml '{ sub(/^covers = \[$/, "covers = [npm:@oneharness/cli-linux-x64]"); print }'
assert_red malformed-list "a list of something other than quoted names" \
  "as something other than a list of quoted names"

# A list whose closing bracket is not the end of the line: whatever follows is a
# value or a key nobody would ever read.
root="$(stage trailing-after-list)"
rewrite "$root" release-targets.toml '{ sub(/^\]$/, "] manifest = \"elsewhere\""); print }'
assert_red trailing-after-list "a second value after a list" \
  "writes covers in [[target]] 5 with something after its closing bracket"

# Truncated at the opening bracket, because any later line carrying a `]` — the
# `[[target]]` header below it does — would close the list somewhere nobody meant.
root="$(stage unclosed-list)"
rewrite "$root" release-targets.toml '/^\]$/ { exit } { print }'
assert_red unclosed-list "a list that is never closed" \
  "leaves covers open in [[target]] 5"

# A value written as something other than what its key holds. The brackets and
# the quotes are the only thing that tells them apart: once they are gone,
# `name = ["core"]` and `manifest = 1` read as an ordinary string and would pass
# every check below, so each is refused where the value is read.
root="$(stage list-for-a-scalar)"
rewrite "$root" release-targets.toml '{ sub(/^name = "core"$/, "name = [\"core\"]"); print }'
assert_red list-for-a-scalar "a one-element list where a key holds a string" \
  'writes name in [[target]] 1 as a list; it holds one quoted string'

root="$(stage number-for-a-scalar)"
rewrite "$root" release-targets.toml '{ sub(/^manifest = "Cargo.toml"$/, "manifest = 1"); print }'
assert_red number-for-a-scalar "a number where a key holds a string" \
  'writes manifest in [[target]] 2 as a whole number; it holds one quoted string'

root="$(stage string-for-a-number)"
rewrite "$root" release-targets.toml '{ sub(/^schema_version = 1$/, "schema_version = \"1\""); print }'
assert_red string-for-a-number "a quoted string where a key holds a number" \
  'writes schema_version in the document as a quoted string; it holds a whole number'

root="$(stage string-for-a-list)"
rewrite "$root" release-targets.toml '
  /^covers = \[$/ { print "covers = \"npm:@oneharness/cli-linux-x64\""; skipping = 1; next }
  skipping && /^\]$/ { skipping = 0; next }
  skipping { next }
  { print }
'
assert_red string-for-a-list "a quoted string where a key holds a list" \
  'writes covers in [[target]] 5 as a quoted string; it holds a list'

# The bounds each validated type carries, so a refusal quoting a value is still
# a sentence a reader can act on.
root="$(stage overlong-id)"
rewrite "$root" release-targets.toml '
  /^id = "crate:oneharness-core"$/ {
    name = ""
    while (length(name) < 130) name = name "oneharness-core-"
    print "id = \"crate:" name "\""
    next
  }
  { print }
'
assert_red overlong-id "an identifier past its bound" \
  "as an identifier longer than 128 characters"

# The alphabet a short name is held to, which is `TargetName`'s: it is typed
# into a host document and a plan node's `consumes` map, so a name those cannot
# spell is a target nothing can select.
root="$(stage short-name-outside-its-alphabet)"
rewrite "$root" release-targets.toml '{ sub(/^name = "core"$/, "name = \"-core\""); print }'
assert_red short-name-outside-its-alphabet "a short name that does not start with a letter or a digit" \
  'writes the short name in [[target]] 1 ("crate:oneharness-core") as "-core"'

root="$(stage overlong-short-name)"
rewrite "$root" release-targets.toml '{ sub(/^name = "core"$/, "name = \"core-engine-crate-as-a-rust-dependent-takes-it-with-every-word-spelled-out\""); print }'
assert_red overlong-short-name "a short name past its bound" \
  "as more than 64 characters"

root="$(stage overlong-prose)"
rewrite "$root" release-targets.toml '
  /^what = "The reusable engine/ {
    filler = ""
    while (length(filler) < 420) filler = filler "reasoning that belongs in a comment "
    print "what = \"" filler "\""
    next
  }
  { print }
'
assert_red overlong-prose "a sentence past its bound" \
  "as more than 400 characters"

root="$(stage prose-with-a-control-character)"
rewrite "$root" release-targets.toml '
  /^what = "The reusable engine/ { print "what = \"The reusable engine,\011as a Rust dependent takes it.\""; next }
  { print }
'
assert_red prose-with-a-control-character "a sentence carrying a control character" \
  "with a control character"

root="$(stage absolute-manifest)"
rewrite "$root" release-targets.toml '{ sub(/^manifest = "pyproject.toml"$/, "manifest = \"/pyproject.toml\""); print }'
assert_red absolute-manifest "a manifest path that is absolute" \
  "which is absolute"

root="$(stage repeated-key)"
rewrite "$root" release-targets.toml '
  { print }
  /^manifest = "Cargo.toml"$/ { print "manifest = \"pyproject.toml\"" }
'
assert_red repeated-key "one key written twice in a target" \
  "names manifest twice in [[target]] 2"

# A path is refused on how it is spelled, so it means the same thing in every
# checkout on every platform a consumer runs on.
root="$(stage escaping-probe)"
rewrite "$root" release-targets.toml '{ sub(/^probe = "scripts\/release-probe.sh"$/, "probe = \"../elsewhere/probe.sh\""); print }'
assert_red escaping-probe "a probe path that leaves the repository root" \
  'which leaves the repository root'

root="$(stage drive-qualified-manifest)"
rewrite "$root" release-targets.toml '{ sub(/^manifest = "pyproject.toml"$/, "manifest = \"C:/pyproject.toml\""); print }'
assert_red drive-qualified-manifest "a manifest path naming a drive on the reader's machine" \
  "names a drive on the reader's own machine"

# `covers` names what a target's release also ships and that is NOT a target of
# its own; an id that is both is a document saying two things about one artifact.
root="$(stage covers-a-target)"
rewrite "$root" release-targets.toml '
  { print }
  /^  "npm:@oneharness\/cli-win32-x64",$/ { print "  \"npm:oneharness-cli\"," }
'
assert_red covers-a-target "a covers entry that is also a declared target" \
  "covers 'npm:oneharness-cli', which it also declares as a target of its own"

root="$(stage covers-the-unpublished)"
rewrite "$root" release-targets.toml '
  { print }
  /^  "npm:@oneharness\/cli-win32-x64",$/ { print "  \"npm:@oneharness/cli-sunos-x64\"," }
'
assert_red covers-the-unpublished "a covered name this repository does not publish" \
  "covers 'npm:@oneharness/cli-sunos-x64', which this repository's release configuration does not publish"

# A per-platform package this repository publishes that the declaration says
# nothing about: a consumer reading the document alone would never learn of it.
root="$(stage uncovered-platform)"
rewrite "$root" release-targets.toml '!/^  "npm:@oneharness\/cli-win32-x64",$/ { print }'
assert_red uncovered-platform "a published per-platform package no target covers" \
  "publishes '@oneharness/cli-win32-x64' and no declared target covers it"

# `[[retired]]` is the schema's own field for an artifact this repository once
# published and does not any more. Both halves are proven: a well-formed one is
# accepted, and one that contradicts a target is refused.
root="$(stage twice-covered)"
rewrite "$root" release-targets.toml '
  { print }
  /^  "npm:@oneharness\/cli-win32-x64",$/ { print "  \"npm:@oneharness/cli-linux-x64\"," }
'
assert_red twice-covered "one artifact covered twice" \
  "covers 'npm:@oneharness/cli-linux-x64' from more than one target"

root="$(stage retirement-accepted)"
cat >>"$root/release-targets.toml" <<'RETIRED'

[[retired]]
id = "npm:@oneharness/cli-sunos-x64"
why = "A per-platform package the npm build no longer mints. Nothing here publishes it again."
RETIRED
assert_green retirement-accepted "a well-formed retirement"

root="$(stage retirement-of-a-target)"
cat >>"$root/release-targets.toml" <<'RETIRED'

[[retired]]
id = "crate:oneharness"
why = "Not actually retired, which is the point of this case."
RETIRED
assert_red retirement-of-a-target "a retirement of something a target publishes" \
  "retires 'crate:oneharness', which it also declares as a target"

# An entry that wrote nothing still owes every field it declares — and it is the
# entry the reader emits no record for, so it is the one a count taken from
# records would never ask about.
root="$(stage empty-retirement)"
printf '\n[[retired]]\n' >>"$root/release-targets.toml"
assert_red empty-retirement "a retirement that declares nothing at all" \
  'declares no id in [[retired]] 1'

root="$(stage empty-target)"
printf '\n[[target]]\n' >>"$root/release-targets.toml"
assert_red empty-target "a target that declares nothing at all" \
  'declares no name in [[target]] 7'

root="$(stage idless-retirement)"
cat >>"$root/release-targets.toml" <<'RETIRED'

[[retired]]
why = "An artifact this repository stopped publishing, without saying which."
RETIRED
assert_red idless-retirement "a retirement that names no identifier" \
  'declares no id in [[retired]] 1'

root="$(stage retirement-of-a-covered-artifact)"
cat >>"$root/release-targets.toml" <<'RETIRED'

[[retired]]
id = "npm:@oneharness/cli-linux-x64"
why = "Not actually retired, which is the point of this case."
RETIRED
assert_red retirement-of-a-covered-artifact "a retirement of something a target covers" \
  "retires 'npm:@oneharness/cli-linux-x64', which a target also covers"

root="$(stage repeated-retirement)"
cat >>"$root/release-targets.toml" <<'RETIRED'

[[retired]]
id = "pypi:oneharness-retired"
why = "Nothing here publishes it again."

[[retired]]
id = "pypi:oneharness-retired"
why = "Recorded a second time, which is the point of this case."
RETIRED
assert_red repeated-retirement "one artifact retired twice" \
  "retires 'pypi:oneharness-retired' more than once"

root="$(stage reasonless-retirement)"
cat >>"$root/release-targets.toml" <<'RETIRED'

[[retired]]
id = "npm:@oneharness/cli-sunos-x64"
RETIRED
assert_red reasonless-retirement "a retirement that says nothing about why" \
  'declares no why in [[retired]] 1'

# The cases below reconcile the declaration with what the release configuration
# really publishes, in both directions.

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
  "declares no id in [[target]] 2"

root="$(stage duplicate-id-in-block)"
rewrite "$root" release-targets.toml '
  { print }
  /^id = "crate:oneharness"$/ { print "id = \"crate:oneharness-again\"" }
'
assert_red duplicate-id-in-block "one [[target]] carrying two ids" \
  "names id twice in [[target]] 2"

# Two rows answering to one id: only one of them is ever consulted, and a
# consumer cannot tell which.
root="$(stage duplicate-id)"
cat >>"$root/release-targets.toml" <<'DUPLICATE'

[[target]]
id = "crate:oneharness"
name = "cli-crate-again"
what = "The same crate a target above already declares."
published_by = ".github/workflows/release.yml — the publish-crates job, under Cargo.toml's [package] name."
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
name = "retired-crate"
what = "A crate nothing in this repository's release configuration publishes."
published_by = ".github/workflows/release.yml — the publish-crates job, under Cargo.toml's [package] name."
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

# A new per-platform package that no launcher pins: published, and unresolvable,
# since npm finds a platform binary only through the launcher's own pin. (Its
# `covers` half is the uncovered-platform case above.)
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

echo "check-release-targets-test: the drift gate holds the declaration to the canonical schema, and goes red for every way it, the release configuration and the probe can drift apart"
