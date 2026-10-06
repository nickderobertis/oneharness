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
  npm/oneharness/bin/oneharness.js
  npm/oneharness-sdk/package.json
  npm/oneharness-sdk/test/package-e2e.mjs
  scripts/npm-e2e.sh
  release-platforms.toml
  .github/workflows/package-pr.yml
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
  'names "manifset" in [[target]] 2, which schema_version 2 does not declare'

root="$(stage unknown-table)"
cat >>"$root/release-targets.toml" <<'TABLE'

[extra]
key = "value"
TABLE
assert_red unknown-table "a table this schema does not declare" \
  "opens [extra], which schema_version 2 does not declare"

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

# A leading `@` commits a name to the scoped form and is decided there in full.
# That is the half of the identifier grammar a plain-name reading would hide:
# the plain alphabet holds `@` and `/` anywhere after the first character, so a
# reader that fell back to it would take all four of these as ordinary names.
# The checked-in document's own six scoped ids are accepted — the baseline pass
# above is that, and the gate would not have reached any case here otherwise.
root="$(stage at-sign-without-a-scope)"
rewrite "$root" release-targets.toml '{ sub(/^id = "npm:@oneharness\/sdk"$/, "id = \"npm:@oneharness\""); print }'
assert_red at-sign-without-a-scope "a name opening with @ that is not a scope" \
  'as "npm:@oneharness", which is not <registry>:<name>'

root="$(stage scope-with-no-package)"
rewrite "$root" release-targets.toml '{ sub(/^id = "npm:@oneharness\/sdk"$/, "id = \"npm:@oneharness/\""); print }'
assert_red scope-with-no-package "a scope whose package half is empty" \
  'as "npm:@oneharness/", which is not <registry>:<name>'

root="$(stage no-scope-before-the-slash)"
rewrite "$root" release-targets.toml '{ sub(/^id = "npm:@oneharness\/sdk"$/, "id = \"npm:@/sdk\""); print }'
assert_red no-scope-before-the-slash "a scoped name with nothing in its scope" \
  'as "npm:@/sdk", which is not <registry>:<name>'

root="$(stage scope-inside-a-scope)"
rewrite "$root" release-targets.toml '{ sub(/^id = "npm:@oneharness\/sdk"$/, "id = \"npm:@oneharness/sdk/node\""); print }'
assert_red scope-inside-a-scope "a scoped name carrying a second slash" \
  'as "npm:@oneharness/sdk/node", which is not <registry>:<name>'

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
rewrite "$root" release-targets.toml '{ sub(/^schema_version = 2$/, "schema_version = 2 3"); print }'
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
rewrite "$root" release-targets.toml '{ sub(/^schema_version = 2$/, "schema_version = \"2\""); print }'
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

# And the other half of that rule: a control character is an ASCII control, not
# whatever the runner's locale calls one. This case is red only where that
# distinction is real — on the Windows job, whose locale reads the continuation
# bytes of a UTF-8 character as controls, so every em dash in the checked-in
# document was a finding there. It is a case of its own rather than the
# baseline's coverage, because prose rewritten into plain ASCII would take that
# coverage away without anything saying so.
root="$(stage prose-outside-ascii)"
rewrite "$root" release-targets.toml '
  /^what = "The reusable engine/ { print "what = \"The reusable engine — a naïve dependent takes it whole.\""; next }
  { print }
'
assert_green prose-outside-ascii "a sentence carrying a character outside ASCII"

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
rewrite "$root" release-targets.toml '{ sub(/^schema_version = 2$/, "schema_version = 3"); print }'
assert_red schema-drift "a declaration written to a version this gate cannot read" \
  "declares schema_version '3'"

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
    print "  \"riscv64gc-unknown-linux-gnu\": { platform: \"linux\", arch: \"riscv64\", exe: false },"
  }
'
assert_red uncovered "a per-platform package no launcher pins" \
  "publishes '@oneharness/cli-linux-riscv64' and no declared npm target's optionalDependencies pins it"

# The platform set: release-platforms.toml states it once, and every list that
# restates a share of it is held to it in both directions. Each case drops or
# alters Windows ARM64 — the platform that went missing from all of them at once
# (#1413) — in one list only.

# $1 = fixture root, $2 = workflow, $3 = job. Drops that job's
# aarch64-pc-windows-msvc matrix entry: its `- target:` line and the keys under it.
drop_matrix_entry() {
  rewrite "$1" "$2" "
    \$0 == \"  $3:\" { inside = 1 }
    inside && /^  [^ #]/ && \$0 != \"  $3:\" { inside = 0 }
    inside && /^ +- target: aarch64-pc-windows-msvc\$/ { dropping = 1; next }
    dropping && /^ +- / { dropping = 0 }
    dropping && /^            [a-z-]+: / { next }
    { dropping = 0; print }
  "
}

for job in upload build-wheels build-npm; do
  root="$(stage "matrix-missing-$job")"
  drop_matrix_entry "$root" .github/workflows/release.yml "$job"
  assert_red "matrix-missing-$job" "a release $job matrix missing aarch64-pc-windows-msvc" \
    "release.yml's $job matrix lacks 'aarch64-pc-windows-msvc windows-11-arm"
done

root="$(stage matrix-wrong-archive)"
rewrite "$root" .github/workflows/release.yml '
  /^ +- target: aarch64-pc-windows-msvc$/ { seen = 1 }
  seen && !done && /^            ext: zip$/ { sub(/zip/, "tar.gz"); done = 1 }
  { print }
'
assert_red matrix-wrong-archive "an upload entry whose archive departs from the declared one" \
  "upload matrix has 'aarch64-pc-windows-msvc windows-11-arm tar.gz', which release-platforms.toml does not declare"

root="$(stage matrix-wrong-runner)"
rewrite "$root" .github/workflows/release.yml '
  /^ +- target: aarch64-pc-windows-msvc$/ { seen++ }
  seen == 2 && !done && /^            os: windows-11-arm$/ { sub(/windows-11-arm/, "windows-latest"); done = 1 }
  { print }
'
assert_red matrix-wrong-runner "a build-wheels entry on a runner the declaration does not name" \
  "build-wheels matrix has 'aarch64-pc-windows-msvc windows-latest'"

root="$(stage pr-lane-departs)"
drop_matrix_entry "$root" .github/workflows/package-pr.yml package
assert_red pr-lane-departs "a pull-request lane that stopped building a pull_request platform" \
  "package-pr.yml's package matrix lacks 'aarch64-pc-windows-msvc windows-11-arm'"

root="$(stage pr-lane-extra)"
rewrite "$root" release-platforms.toml '!/^pull_request = true$/ { print }'
assert_red pr-lane-extra "a pull-request lane building a platform not marked pull_request" \
  "package-pr.yml's package matrix has 'aarch64-pc-windows-msvc windows-11-arm', which release-platforms.toml does not declare"

root="$(stage npm-targets-depart)"
rewrite "$root" scripts/npm-build.mjs '{ sub(/platform: "win32", arch: "arm64"/, "platform: \"win32\", arch: \"aarch64\""); print }'
assert_red npm-targets-depart "an npm-side TARGETS entry whose platform departs from the set" \
  "scripts/npm-build.mjs's TARGETS lacks 'aarch64-pc-windows-msvc win32-arm64'"

root="$(stage launcher-map-missing)"
rewrite "$root" npm/oneharness/bin/oneharness.js '!/^  "win32-arm64": / { print }'
assert_red launcher-map-missing "a launcher platform map missing a declared platform" \
  "npm/oneharness/bin/oneharness.js's PACKAGES lacks 'win32-arm64 @oneharness/cli-win32-arm64'"

root="$(stage optional-dependency-missing)"
rewrite "$root" npm/oneharness/package.json '
  /^    "@oneharness\/cli-win32-arm64": / { next }
  /^    "@oneharness\/cli-win32-x64": / { sub(/,$/, "") }
  { print }
'
assert_red optional-dependency-missing "a launcher manifest missing a declared platform's pin" \
  "npm/oneharness/package.json's optionalDependencies lacks '@oneharness/cli-win32-arm64'"

root="$(stage covers-missing-platform)"
rewrite "$root" release-targets.toml '!/^  "npm:@oneharness\/cli-win32-arm64",$/ { print }'
assert_red covers-missing-platform "a covers list missing a declared platform's package" \
  "release-targets.toml's npm:oneharness-cli covers lacks 'npm:@oneharness/cli-win32-arm64'"

root="$(stage sdk-e2e-map-missing)"
rewrite "$root" npm/oneharness-sdk/test/package-e2e.mjs '!/^\t"win32-arm64": / { print }'
assert_red sdk-e2e-map-missing "an SDK package e2e host map missing a declared platform" \
  "package-e2e.mjs's host map lacks 'aarch64-pc-windows-msvc win32-arm64'"

root="$(stage npm-e2e-map-missing)"
rewrite "$root" scripts/npm-e2e.sh '!/^ +win32-arm64\) TARGET=/ { print }'
assert_red npm-e2e-map-missing "a launcher e2e host map missing a declared platform" \
  "scripts/npm-e2e.sh's detect_target lacks 'aarch64-pc-windows-msvc win32-arm64'"

# And the other direction: a platform declared and built nowhere.
root="$(stage platform-built-nowhere)"
cat >>"$root/release-platforms.toml" <<'PLATFORM'

[[platform]]
target = "riscv64gc-unknown-linux-gnu"
runner = "ubuntu-latest"
archive = "tar.gz"
npm = "linux-riscv64"
PLATFORM
assert_red platform-built-nowhere "a declared platform no release matrix builds" \
  "release.yml's upload matrix lacks 'riscv64gc-unknown-linux-gnu ubuntu-latest tar.gz'"

# A misspelled or missing key read as an absent one would build a platform
# nowhere without a word, so the declaration must refuse each.
root="$(stage platform-unknown-key)"
rewrite "$root" release-platforms.toml '{ sub(/^runner = "windows-11-arm"$/, "runnr = \"windows-11-arm\""); print }'
assert_red platform-unknown-key "a platform key the declaration does not define" \
  'names "runnr" in [[platform]] 6, which is not one of'

root="$(stage platform-missing-key)"
rewrite "$root" release-platforms.toml '!/^archive = "zip"$/ { print }'
assert_red platform-missing-key "a platform missing a required key" \
  "[[platform]] 5 declares no archive"

root="$(stage platform-twice)"
cat >>"$root/release-platforms.toml" <<'PLATFORM'

[[platform]]
target = "aarch64-pc-windows-msvc"
runner = "windows-11-arm"
archive = "zip"
npm = "win32-arm64"
PLATFORM
assert_red platform-twice "one target declared twice" \
  "declares target aarch64-pc-windows-msvc more than once"

root="$(stage platform-key-before-first)"
rewrite "$root" release-platforms.toml '!seen && /^\[\[platform\]\]$/ { print "runner = \"ubuntu-latest\""; seen = 1 } { print }'
assert_red platform-key-before-first "a key outside any platform" \
  'has "runner = "ubuntu-latest"" before its first [[platform]]'

root="$(stage platform-key-repeated)"
rewrite "$root" release-platforms.toml '{ print } /^runner = "windows-11-arm"$/ { print "runner = \"windows-latest\"" }'
assert_red platform-key-repeated "one key written twice in a platform" \
  "names runner twice in [[platform]] 6"

root="$(stage platform-pull-request-string)"
rewrite "$root" release-platforms.toml '{ sub(/^pull_request = true$/, "pull_request = \"true\""); print }'
assert_red platform-pull-request-string "pull_request written as a string" \
  "writes pull_request in [[platform]] 6 as a string"

root="$(stage platform-empty-value)"
rewrite "$root" release-platforms.toml '{ sub(/^npm = "win32-arm64"$/, "npm = \"\""); print }'
assert_red platform-empty-value "a required key left empty" \
  "leaves npm empty in [[platform]] 6"

root="$(stage platform-malformed-line)"
rewrite "$root" release-platforms.toml '{ sub(/^archive = "zip"$/, "archive = zip"); print }'
assert_red platform-malformed-line "a platform line that is not key = \"value\"" \
  "has a line in [[platform]] 5 that is not key = \"value\" (or pull_request = true): archive = zip"

root="$(stage platforms-none-declared)"
rewrite "$root" release-platforms.toml '/^#/ || /^$/ { print }'
assert_red platforms-none-declared "a declaration that states no platform" \
  "release-platforms.toml declares no [[platform]]"

root="$(stage platforms-missing)"
rm "$root/release-platforms.toml"
assert_red platforms-missing "a missing platform declaration" \
  "release-platforms.toml is missing"

root="$(stage pr-lane-missing)"
rm "$root/.github/workflows/package-pr.yml"
assert_red pr-lane-missing "a missing pull-request lane" \
  "package-pr.yml is missing, so the platforms release-platforms.toml marks pull_request = true are built on no pull request"

# The probe owns which registries are answerable; this gate mirrors that list,
# and a registry dropped from one side must not sit stale on the other.
root="$(stage registry-drift)"
rewrite "$root" scripts/release-probe.sh '!/^  npm\)$/ { print }'
assert_red registry-drift "a probe that stopped answering for a declared registry" \
  "while this gate can read a name for"

echo "check-release-targets-test: the drift gate holds the declaration to the canonical schema, and goes red for every way it, the release configuration and the probe can drift apart"
