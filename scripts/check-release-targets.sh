#!/usr/bin/env bash
# Drift gate for release-targets.toml against what this repository really
# publishes.
#
# A consumer sequencing work across repositories reads release-targets.toml to
# learn which artifact to wait on. A hand-written inventory is exactly the thing
# that goes stale in silence — a repository that declares no target for an
# artifact grants no hold at all, and nobody learns that the hold stopped
# happening. So the published set is DERIVED here from the release
# configuration itself rather than transcribed:
#
#   crates  — the publish_if_missing calls in scripts/publish-crates.sh, which
#             release.yml's publish-crates job runs.
#   pypi    — every committed pyproject.toml, cross-checked against the number
#             of Trusted-Publishing steps release.yml actually has.
#   npm     — every committed manifest under npm/, plus the per-platform names
#             scripts/npm-build.mjs generates, published by scripts/publish-npm.sh.
#
# A per-platform package is not a target of its own: it exists so the launcher
# can resolve one at the launcher's exact version. It is accounted for by
# appearing in a declared launcher's optionalDependencies, and a generated name
# that appears in none of them is a finding rather than a silent drop.
#
# Fails in both directions: a published name no target covers, and a declared
# target naming something this repository does not publish.
#
# Quiet on success, one line. On failure it names each drift and the fix.
set -euo pipefail

cd "$(dirname "$0")/.."

declarations="release-targets.toml"
release_workflow=".github/workflows/release.yml"
probe="scripts/release-probe.sh"
DECLARATION_SCHEMA_VERSION=1

fails=0
fail() {
	printf 'release-target drift: %s\n' "$1" >&2
	fails=$((fails + 1))
}

# The first `name = "..."` inside a named TOML section. $1 = file, $2 = section.
toml_section_name() {
	awk -v section="[$2]" '
		$0 == section { inside = 1; next }
		inside && /^\[/ { exit }
		inside && match($0, /^[[:space:]]*name[[:space:]]*=[[:space:]]*"[^"]+"/) {
			line = substr($0, RSTART, RLENGTH)
			sub(/^[^"]*"/, "", line)
			sub(/"$/, "", line)
			print line
			exit
		}
	' "$1"
}

json_package_name() {
	sed -n 's/^  "name": "\([^"]*\)".*$/\1/p' "$1" | head -n 1
}

# The optionalDependencies block of a package.json, or nothing. A platform
# package is covered by being PINNED there, so the whole file is the wrong
# haystack: an incidental mention elsewhere must not read as coverage.
json_optional_dependencies() {
	awk '
		/^  "optionalDependencies": \{$/ { inside = 1; next }
		inside && /^  \}/ { exit }
		inside { print }
	' "$1"
}

[ -f "$declarations" ] || {
	echo "check-release-targets: $declarations is missing; restore it — a repository that declares no release target grants a waiting consumer no hold at all." >&2
	exit 1
}

# Read as [[target]] BLOCKS rather than as two independent scans, so a target
# with two ids, or an id whose manifest went missing, is a finding here instead
# of a silently shifted pairing later.
SEP=$'\037'
targets="$(awk -v sep="$SEP" '
	/^\[\[target\]\]$/ {
		if (open) print id sep manifest
		open = 1; id = ""; manifest = ""; next
	}
	open && match($0, /^id = "[^"]*"$/) {
		if (id != "") { print "!duplicate" sep "id"; exit }
		id = $0; sub(/^id = "/, "", id); sub(/"$/, "", id); next
	}
	open && match($0, /^manifest = "[^"]*"$/) {
		if (manifest != "") { print "!duplicate" sep "manifest"; exit }
		manifest = $0; sub(/^manifest = "/, "", manifest); sub(/"$/, "", manifest); next
	}
	END { if (open) print id sep manifest }
' "$declarations")"

declared_version="$(sed -n 's/^schema_version = \([0-9]*\)$/\1/p' "$declarations")"
[ "$declared_version" = "$DECLARATION_SCHEMA_VERSION" ] ||
	fail "$declarations declares schema_version '$declared_version' and this gate reads exactly one, version $DECLARATION_SCHEMA_VERSION; leave a single schema_version line saying which shape the file is written in, then bring whichever side is behind up to it"

if [ -z "$targets" ]; then
	echo "check-release-targets: $declarations declares no [[target]] entries; restore them." >&2
	exit 1
fi
while IFS="$SEP" read -r id manifest; do
	if [ "$id" = "!duplicate" ]; then
		fail "$declarations has a [[target]] with two $manifest entries; each target declares exactly one id and one manifest, so split it into two targets or drop the spare line"
		continue
	fi
	[ -n "$id" ] || fail "$declarations has a [[target]] with no id (manifest '$manifest'); a target with no identifier can never be asked about, so give it one of the form <registry>:<name>"
	[ -n "$manifest" ] || fail "$declarations declares '$id' with no manifest; add a manifest = \"<path>\" line to that target, since the manifest is what pins a declared name to a real package"
done < <(printf '%s\n' "$targets")

declared_ids="$(printf '%s\n' "$targets" | cut -d"$SEP" -f1)"
declared_manifests="$(printf '%s\n' "$targets" | cut -d"$SEP" -f2)"

# An id is what a consumer names to wait on one artifact, so two targets sharing
# one is not a duplicate entry to tidy up: it means two rows answer to the same
# name and only one of them is ever consulted.
while read -r id; do
	[ -n "$id" ] || continue
	fail "$declarations declares '$id' more than once; every target answers to exactly one id, so keep the row that names the artifact a consumer waits on and drop the other"
done < <(printf '%s\n' "$declared_ids" | sort | uniq -d)

# The registries this gate can read a name for. The probe is the single source
# of which registries are supported at all; this list mirrors it and the two are
# gated against each other, so support added on one side cannot leave the other
# quietly stale.
GATE_REGISTRIES="crate npm pypi"
probe_registries="$(awk '
	/^case "\$registry" in$/ { inside = 1; next }
	inside && /^esac$/ { exit }
	inside && match($0, /^  [a-z]+\)$/) {
		entry = $0; gsub(/[ )]/, "", entry); print entry
	}
' "$probe" | sort | tr '\n' ' ' | sed 's/ $//')"
[ "$probe_registries" = "$GATE_REGISTRIES" ] ||
	fail "$probe answers for [$probe_registries] while this gate can read a name for [$GATE_REGISTRIES]; teach this gate the new registry (its manifest's name extraction) or drop it from the probe — a declared target on a registry only one side knows is unaskable"

# Every script release.yml runs. A manifest is only in the published set if one
# of these packages it, so a manifest committed with nothing to build it is a
# finding rather than a name presumed published.
release_scripts="$(grep -oE 'scripts/[A-Za-z0-9._-]+\.(sh|mjs)' "$release_workflow" | sort -u)"

# $1 = manifest. Prints each packaging entry point that reaches it, or nothing.
packagers_for() {
	local manifest="$1" dir script anchored joined
	dir="$(dirname "$manifest")"
	if [ "$dir" = "." ]; then
		# A repository-root manifest names no directory a packer could mention,
		# so what packages it is the build backend it declares.
		if grep -Fq 'build-backend = "maturin"' "$manifest" &&
			grep -Fq 'uses: PyO3/maturin-action' "$release_workflow"; then
			echo "PyO3/maturin-action"
		fi
		return
	fi
	# The directory as a path, and as the argument list a JS path join spells it
	# with — the two ways this repository's packers name where they read from.
	# Terminated, so `npm/oneharness` does not match `npm/oneharness-sdk`.
	anchored="$(printf '%s' "$dir" | sed 's/[.]/[.]/g')([^A-Za-z0-9_-]|$)"
	joined="\"$(printf '%s' "$dir" | sed 's|/|", "|g')\""
	for script in $release_scripts; do
		[ -f "$script" ] || continue
		if grep -Eq "$anchored" "$script" || grep -Fq "$joined" "$script"; then
			echo "$script"
		fi
	done
}

published=""      # one "<id>\t<manifest>" per artifact traced to a publish path
platform_names="" # per-platform npm packages, covered by a launcher rather than declared

publish_crates="scripts/publish-crates.sh"
grep -Fq 'run: scripts/publish-crates.sh' "$release_workflow" ||
	fail "$release_workflow no longer runs $publish_crates, so the crate names below are derived from a script nothing publishes; point this check at whatever publishes the crates now"
while read -r manifest name; do
	[ -n "$name" ] || continue
	published="${published}crate:${name}	${manifest}
"
done < <(sed -n 's/^publish_if_missing \([^ ]*\) \([^ ]*\) .*$/\1 \2/p' "$publish_crates")
[ -n "$published" ] ||
	fail "$publish_crates declares no publish_if_missing calls, so no crate could be derived; restore them or point this check at the new publisher"

pypi_publish_steps="$(grep -c 'uses: pypa/gh-action-pypi-publish' "$release_workflow" || true)"
pypi_count=0
while read -r manifest; do
	[ -n "$manifest" ] || continue
	name="$(toml_section_name "$manifest" project)"
	if [ -z "$name" ]; then
		fail "$manifest has no [project] name, so the PyPI distribution it builds cannot be derived; restore it"
		continue
	fi
	if [ -z "$(packagers_for "$manifest")" ]; then
		fail "$manifest is committed but nothing $release_workflow runs packages it, so whatever it declares is published by no path this gate can find; build it from one of the packaging scripts the release runs, or drop the manifest"
		continue
	fi
	pypi_count=$((pypi_count + 1))
	published="${published}pypi:${name}	${manifest}
"
done < <(git ls-files '*pyproject.toml')
[ "$pypi_publish_steps" -eq "$pypi_count" ] ||
	fail "$release_workflow has $pypi_publish_steps PyPI publishing step(s) for $pypi_count committed pyproject.toml manifest(s); either a distribution is built and never published, or a publish step has lost its manifest — reconcile the two"

grep -Fq 'scripts/publish-npm.sh' "$release_workflow" ||
	fail "$release_workflow no longer runs scripts/publish-npm.sh, so the npm names below are derived from manifests nothing publishes; point this check at whatever publishes them now"
while read -r manifest; do
	[ -n "$manifest" ] || continue
	if grep -Fq '"private": true' "$manifest"; then continue; fi
	name="$(json_package_name "$manifest")"
	if [ -z "$name" ]; then
		fail "$manifest has no top-level \"name\", so the npm package it publishes cannot be derived; restore it"
		continue
	fi
	if [ -z "$(packagers_for "$manifest")" ]; then
		fail "$manifest is committed but nothing $release_workflow runs packages it, so whatever it declares is published by no path this gate can find; pack it from one of the packaging scripts the release runs, or mark it \"private\": true"
		continue
	fi
	published="${published}npm:${name}	${manifest}
"
done < <(git ls-files 'npm/*/package.json')

# The per-platform packages npm-build.mjs mints, one per entry in its target
# table — the only place those names exist before a release builds them.
while read -r platform arch; do
	[ -n "$arch" ] || continue
	platform_names="${platform_names}@oneharness/cli-${platform}-${arch}
"
done < <(sed -n 's/^.*{ *platform: "\([^"]*\)", *arch: "\([^"]*\)".*$/\1 \2/p' scripts/npm-build.mjs)
[ -n "$platform_names" ] ||
	fail "scripts/npm-build.mjs yielded no per-platform package names; its target table moved, so point the platform-name extractor in this script at that table's new shape"

published_ids="$(printf '%s' "$published" | cut -f1)"

while IFS=$'\t' read -r id manifest; do
	[ -n "$id" ] || continue
	printf '%s\n' "$declared_ids" | grep -Fxq -- "$id" ||
		fail "this repository publishes '$id' (from $manifest) and $declarations declares no target for it, so a consumer waiting on that artifact gets no hold at all; add a [[target]] with id = \"$id\" and manifest = \"$manifest\""
done < <(printf '%s' "$published")

while read -r id; do
	[ -n "$id" ] || continue
	printf '%s\n' "$published_ids" | grep -Fxq -- "$id" ||
		fail "$declarations declares '$id', which this repository's release configuration does not publish; remove that [[target]], or restore whatever published it"
done < <(printf '%s\n' "$declared_ids")

# Each declared manifest must exist and must itself carry the declared name, so
# a rename in the manifest cannot leave the declaration pointing at nothing.
while IFS="$SEP" read -r id manifest; do
	if [ -z "$id" ] || [ -z "$manifest" ] || [ "$id" = "!duplicate" ]; then continue; fi
	if [ ! -f "$manifest" ]; then
		fail "$declarations declares manifest \"$manifest\" for $id, which does not exist; point it at the manifest that builds that artifact"
		continue
	fi
	case "$id" in
	crate:*) actual="$(toml_section_name "$manifest" package)" ;;
	pypi:*) actual="$(toml_section_name "$manifest" project)" ;;
	npm:*) actual="$(json_package_name "$manifest")" ;;
	*)
		fail "$declarations declares \"$id\", whose registry is not one of [$GATE_REGISTRIES]; declare it under a registry $probe answers for, or teach both sides that registry"
		continue
		;;
	esac
	[ "$actual" = "${id#*:}" ] ||
		fail "$declarations declares $id but $manifest names \"$actual\"; one of the two was renamed without the other, so change whichever is wrong until both say the same name"
done < <(printf '%s\n' "$targets")

# A per-platform package is covered by the launcher that pins it, never
# declared. A generated name no declared launcher pins is a published name
# nothing accounts for.
pinned=""
while read -r manifest; do
	case "$manifest" in
	*.json)
		if [ -f "$manifest" ]; then pinned="${pinned}$(json_optional_dependencies "$manifest")"; fi
		;;
	esac
done < <(printf '%s\n' "$declared_manifests")
while read -r name; do
	[ -n "$name" ] || continue
	printf '%s' "$pinned" | grep -Fq "\"$name\": \"" ||
		fail "scripts/npm-build.mjs publishes '$name' and no declared npm target's optionalDependencies pins it, so that published name is accounted for nowhere; add it to npm/oneharness/package.json's optionalDependencies (which is what makes it covered rather than a target of its own)"
done < <(printf '%s\n' "$platform_names")

if [ "$fails" -ne 0 ]; then
	printf 'check-release-targets: %d drift(s) between %s and what this repository publishes\n' "$fails" "$declarations" >&2
	exit 1
fi
echo "check-release-targets: every published artifact is declared or covered"
