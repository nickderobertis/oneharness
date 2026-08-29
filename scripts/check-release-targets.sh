#!/usr/bin/env bash
# Drift gate for release-targets.toml: the canonical schema it is written
# against, and what this repository really publishes.
#
# A consumer sequencing work across repositories reads release-targets.toml to
# learn which artifact to wait on. Two things can go wrong with that, and this
# gate holds both.
#
# **The shape.** The document is written against the canonical release-target
# schema — `schema_version = 2` — which nickderobertis/onevcs defines in its
# docs/contract.md. Six repositories write one of these and a reader needs no
# per-repository knowledge to use one, so a document that leaves that shape is a
# finding here rather than a surprise in whatever parses it next.
#
# **The contents.** A hand-written inventory is exactly the thing that goes
# stale in silence — a repository that declares no target for an artifact grants
# no hold at all, and nobody learns that the hold stopped happening. So the
# published set is DERIVED here from the release configuration itself rather
# than transcribed:
#
#   crates  — the publish_if_missing calls in scripts/publish-crates.sh, which
#             release.yml's publish-crates job runs.
#   pypi    — every committed pyproject.toml, cross-checked against the number
#             of Trusted-Publishing steps release.yml actually has.
#   npm     — every committed manifest under npm/, plus the per-platform names
#             scripts/npm-build.mjs generates, published by scripts/publish-npm.sh.
#
# A per-platform package is not a target of its own: it exists so the launcher
# can resolve one at the launcher's exact version. It is accounted for twice,
# and both are needed — a declared target's `covers` list, which is all a
# consumer reading the declaration alone can see, and that launcher's
# optionalDependencies, which is what makes an install resolve.
#
# Fails in both directions: a published name no target declares or covers, and a
# declared or covered name this repository does not publish.
#
# Quiet on success, one line. On failure it names each drift and the fix.
set -euo pipefail

cd "$(dirname "$0")/.."

declarations="release-targets.toml"
release_workflow=".github/workflows/release.yml"
probe="scripts/release-probe.sh"
DECLARATION_SCHEMA_VERSION=2

# llmlint: ignore-block[contracts_have_one_source_or_a_drift_gate] The canonical
# schema is defined by the `onevcs` crate, which is this repository's CONSUMER
# rather than its dependency — depending on it to read the definition would
# invert that — and `just check` is offline by contract, so it cannot be fetched
# at check time either. What keeps the restatement honest is `schema_version`:
# this gate reads exactly the one version it was written for and refuses any
# other by number, so a document brought up to a later schema goes red here
# until this restatement is brought up with it. There is no second source of
# these constants in this repository.
#
# The keys schema_version 2 declares, per table. Spelled out because a key
# nobody declared is the finding: a misspelled `manifset` read as an absent
# `manifest` publishes an answer nobody wrote.
TOP_KEYS="schema_version probe"
TARGET_KEYS="id name what published_by manifest covers"
RETIRED_KEYS="id why"
# And what kind of value each holds; every key not named here holds a quoted
# string. It is enforced where the value is read, because `name = ["core"]`
# would otherwise arrive as the string `core` and `manifest = 1` as the path
# `1`, and each would pass every check that follows.
NUMBER_KEYS="schema_version"
LIST_KEYS="covers"

# What the canonical schema's own validated types accept.
#
# `RegistryId`, which `id` and `covers` are: a lowercase registry word, then
# either a plain name or npm's scoped `@scope/name`. A leading `@` commits the
# name to the scoped form and is decided there in full, which is what refuses
# `@`, `@/cli`, `@scope/` and a second slash rather than reading them as plain
# names that happen to open with an `@`. (The same npm syntax
# scripts/release-probe.sh validates before it builds a URL.)
ID_SYNTAX='^[a-z0-9-]+:(@[A-Za-z0-9][A-Za-z0-9._-]*/[A-Za-z0-9][A-Za-z0-9._-]*|[A-Za-z0-9][A-Za-z0-9._@/-]*)$'
MAX_ID=128
# `TargetName`: the short name a host document and a consumer's plan name a
# target by.
NAME_SYNTAX='^[A-Za-z0-9][A-Za-z0-9._-]*$'
MAX_NAME=64
# `Prose`: one non-blank line, short enough to render beside what it describes.
MAX_PROSE=400
# llmlint: ignore-end[contracts_have_one_source_or_a_drift_gate]

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
# package is pinned there, so the whole file is the wrong haystack: an
# incidental mention elsewhere must not read as a pin.
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

# Read as a stream of typed fields rather than scanned for the lines this gate
# happens to care about, so a key this schema does not declare, a line that is
# not a key at all, or one key written twice in an entry is a finding instead of
# something silently skipped past.
#
# One record per field: <kind>US<index>US<key>US<value>, where kind is `top`,
# `target` or `retired`, index counts entries of that kind from 1 (0 for `top`),
# and an array yields one record per element. A record whose kind is `!` is a
# refusal, carrying the line it is about.
US=$'\037'
fields="$(awk -v us="$US" -v schema="$DECLARATION_SCHEMA_VERSION" \
	-v topkeys="$TOP_KEYS" -v targetkeys="$TARGET_KEYS" -v retiredkeys="$RETIRED_KEYS" \
	-v numberkeys="$NUMBER_KEYS" -v listkeys="$LIST_KEYS" '
	function refuse(message) { printf "!%s%d%s%s%s%s\n", us, NR, us, "", us, message }
	function declares(key) { return index(" " keys " ", " " key " ") > 0 }
	# What kind of value a key holds: one of "number", "list" or "string".
	function kind_of(key) {
		if (index(" " numberkeys " ", " " key " ") > 0) return "number"
		if (index(" " listkeys " ", " " key " ") > 0) return "list"
		return "string"
	}
	# A value written as something other than what its key holds. Refused rather
	# than coerced: a one-element list and a bare number both read as a perfectly
	# ordinary string once the brackets or the quotes are gone, so nothing after
	# this point could tell them from a value somebody wrote.
	function wrong_kind(key, written,   want) {
		want = kind_of(key)
		if (want == written) return 0
		if (want == "number")
			refuse("writes " key " in " where " as " article(written) "; it holds a whole number, so write it as one")
		else if (want == "list")
			refuse("writes " key " in " where " as " article(written) "; it holds a list, so spell it as [\"<registry>:<name>\", ...]")
		else
			refuse("writes " key " in " where " as " article(written) "; it holds one quoted string, so write it as \"...\"")
		return 1
	}
	function article(written) {
		if (written == "list") return "a list"
		if (written == "number") return "a whole number"
		return "a quoted string"
	}
	# A key written twice is refused rather than resolved: which of the two a
	# reader takes is exactly the thing nobody wrote down.
	function repeated(key) {
		if (!seen[kind, idx, key]++) return 0
		refuse("names " key " twice in " where "; every key is written once, so keep the line that says what this artifact is and drop the other")
		return 1
	}
	function emit(key, value) {
		if (repeated(key)) return
		printf "%s%s%d%s%s%s%s\n", kind, us, idx, us, key, us, value
	}
	# Close a list whose bracket has been found. `tail` is whatever followed that
	# bracket, held to the rule every scalar value is held to: a key holds one
	# value, then nothing but a comment. Dropping it would let a second value, or
	# a whole second key, ride into the document unread.
	function close_list(body, tail) {
		if (tail !~ /^[ \t]*(#.*)?$/) {
			refuse("writes " array_key " in " array_where " with something after its closing bracket; a key holds one value, then nothing but a # comment")
			return
		}
		elements(body)
	}
	# A list of quoted names. Whitespace is stripped first: no name a registry
	# serves carries any, so what is left is the list itself.
	function elements(body,   work) {
		work = body
		gsub(/[ \t\r]/, "", work)
		if (work != "" && work !~ /^("[^"]*",)*"[^"]*",?$/) {
			refuse("writes " array_key " in " array_where " as something other than a list of quoted names; spell it as [\"<registry>:<name>\", ...]")
			return
		}
		while (match(work, /"[^"]*"/)) {
			printf "%s%s%d%s%s%s%s\n", array_kind, us, array_idx, us, array_key, us, substr(work, RSTART + 1, RLENGTH - 2)
			work = substr(work, RSTART + RLENGTH)
		}
	}
	function open_table(name, count) {
		kind = name
		idx = count
		where = "[[" name "]] " count
		keys = (name == "target") ? targetkeys : retiredkeys
	}
	BEGIN { kind = "top"; idx = 0; where = "the document"; keys = topkeys }
	{ line = $0; sub(/[ \t\r]+$/, "", line) }
	# Continuation lines of a list opened above, up to its closing bracket.
	in_list {
		if (!match(line, /\]/)) { buffer = buffer line; next }
		in_list = 0
		close_list(buffer substr(line, 1, RSTART - 1), substr(line, RSTART + 1))
		buffer = ""
		next
	}
	line ~ /^[ \t]*(#.*)?$/ { next }
	line == "[[target]]" { open_table("target", ++targets); next }
	line == "[[retired]]" { open_table("retired", ++retireds); next }
	line ~ /^[ \t]*\[/ {
		refuse("opens " line ", which schema_version " schema " does not declare; the only tables are [[target]] and [[retired]]")
		next
	}
	{
		if (!match(line, /^[A-Za-z_][A-Za-z0-9_]*[ \t]*=[ \t]*/)) {
			refuse("has a line in " where " that is not a `key = value`: " line)
			next
		}
		key = substr(line, 1, RLENGTH)
		sub(/[ \t]*=[ \t]*$/, "", key)
		rest = substr(line, RLENGTH + 1)
		if (!declares(key)) {
			refuse("names \"" key "\" in " where ", which schema_version " schema " does not declare; a misspelled key would otherwise be read as an absent one")
			next
		}
		if (rest ~ /^"/) {
			if (!match(rest, /^"[^"\\]*"/)) {
				refuse("writes " key " in " where " as a string this reader cannot read; spell it on one line as \"...\", with no backslash escape")
				next
			}
			value = substr(rest, 2, RLENGTH - 2)
			if (substr(rest, RLENGTH + 1) !~ /^[ \t]*(#.*)?$/) {
				refuse("writes " key " in " where " with something after its value; a key holds one value, then nothing but a # comment")
				next
			}
			if (wrong_kind(key, "string")) next
			emit(key, value)
			next
		}
		if (rest ~ /^\[/) {
			if (wrong_kind(key, "list")) next
			if (repeated(key)) next
			array_kind = kind; array_idx = idx; array_key = key; array_where = where
			rest = substr(rest, 2)
			if (match(rest, /\]/)) {
				close_list(substr(rest, 1, RSTART - 1), substr(rest, RSTART + 1))
				next
			}
			in_list = 1
			buffer = rest
			next
		}
		if (match(rest, /^[0-9]+/)) {
			value = substr(rest, 1, RLENGTH)
			if (substr(rest, RLENGTH + 1) !~ /^[ \t]*(#.*)?$/) {
				refuse("writes " key " in " where " with something after its value; a key holds one value, then nothing but a # comment")
				next
			}
			if (wrong_kind(key, "number")) next
			emit(key, value)
			next
		}
		refuse("writes " key " in " where " as a value this reader cannot read; every value is a quoted string, a list of them, or a whole number")
	}
	# How many entries of each kind the document opened. Counted from the table
	# headers rather than from the records they went on to emit, because an entry
	# that wrote no field at all still has to answer for the fields it owes: one
	# counted from its records would be an entry nothing was ever asked about.
	END {
		if (in_list) refuse("leaves " array_key " open in " array_where "; close its list with ]")
		printf "count%s%d%s%s%s%d\n", us, 0, us, "target", us, targets + 0
		printf "count%s%d%s%s%s%d\n", us, 0, us, "retired", us, retireds + 0
	}
' "$declarations")"

# $1 = kind, $2 = index, $3 = key. Every value that key holds, one per line.
values_of() {
	printf '%s\n' "$fields" | awk -F"$US" -v k="$1" -v i="$2" -v key="$3" '
		$1 == k && $2 == i && $3 == key { print $4 }
	'
}

# Every refusal the reader made, before anything is asked of what it did read.
while IFS="$US" read -r kind _ _ message; do
	[ "$kind" = "!" ] || continue
	fail "$declarations $message"
done < <(printf '%s\n' "$fields")

# The version is read before the shape is enforced, and refused before it too:
# which keys a document may carry is a fact about the schema it declares.
declared_version="$(values_of top 0 schema_version)"
[ "$declared_version" = "$DECLARATION_SCHEMA_VERSION" ] ||
	fail "$declarations declares schema_version '$declared_version' and this gate reads exactly one, version $DECLARATION_SCHEMA_VERSION; leave a single schema_version line saying which shape the file is written in, then bring whichever side is behind up to it"

# $1 = what the value is, $2 = where it is, $3 = the value.
check_id() {
	[ "${#3}" -le "$MAX_ID" ] ||
		fail "$declarations writes $1 in $2 as an identifier longer than $MAX_ID characters; a registry serves no such name, so correct it to the one it really serves"
	[[ $3 =~ $ID_SYNTAX ]] ||
		fail "$declarations writes $1 in $2 as \"$3\", which is not <registry>:<name> with the name spelled as its registry serves it, an npm scoped package as @scope/name; e.g. crate:oneharness, because one name published to two registries is two artifacts"
}

check_name() {
	[ "${#3}" -le "$MAX_NAME" ] ||
		fail "$declarations writes the short name in $2 as more than $MAX_NAME characters; shorten it to the word a host document and a consumer's plan would type"
	[[ $3 =~ $NAME_SYNTAX ]] ||
		fail "$declarations writes the short name in $2 as \"$3\", which may hold only letters, digits, '-', '_' and '.' and must start with a letter or a digit; it is what a host document and a consumer's plan name this target by"
}

check_prose() {
	if [ -z "${3// /}" ]; then
		fail "$declarations leaves $1 blank in $2; it is what a reader learns from the entry it describes"
		return
	fi
	[ "${#3}" -le "$MAX_PROSE" ] ||
		fail "$declarations writes $1 in $2 as more than $MAX_PROSE characters; cut it to the one sentence a reader acts on and move the reasoning behind it into a comment above that target"
	# A control character is found by its own byte value, and never by
	# `[[:cntrl:]]`: which bytes that class holds is the runner's locale's
	# answer, and under the Windows job's it holds the continuation bytes of a
	# UTF-8 character — so every em dash in this document was a finding there and
	# nowhere else. `tr` deletes the ASCII controls by number, which is one
	# answer on every platform.
	[ "$(printf '%s' "$3" | tr -d '\001-\037\177')" = "$3" ] ||
		fail "$declarations writes $1 in $2 with a control character; it is rendered on one line, so replace it with a space or drop it"
	return 0
}

check_path() {
	case $3 in
	/* | \\*)
		fail "$declarations writes $1 in $2 as \"$3\", which is absolute; it is a path relative to the repository root, because it names something a checkout of this repository carries"
		;;
	[A-Za-z]:*)
		fail "$declarations writes $1 in $2 as \"$3\", which names a drive on the reader's own machine; it is a path relative to the repository root"
		;;
	.. | ../* | ..\\* | */../* | *\\..\\* | */.. | *\\..)
		fail "$declarations writes $1 in $2 as \"$3\", which leaves the repository root; it names something a checkout of this repository carries"
		;;
	esac
}

# $1 = kind, $2 = index, $3 = key, $4 = where. The one value that key holds, or
# "". It cannot hold two: the reader refuses a list written for a scalar key by
# kind, and refuses a key written twice rather than emitting both.
single() {
	values_of "$1" "$2" "$3" | head -n 1 | tr -d '\n'
}

top_probe="$(single top 0 probe "the document")"
[ -z "$top_probe" ] || check_path "probe" "the document" "$top_probe"

target_count="$(values_of count 0 target)"
retired_count="$(values_of count 0 retired)"

if [ "$target_count" -eq 0 ]; then
	echo "check-release-targets: $declarations declares no [[target]] entries; restore one [[target]] per artifact this repository publishes — a declaration that names nothing says less than no declaration at all, because a consumer cannot tell whether this repository publishes nothing or nobody has said what it publishes." >&2
	exit 1
fi

declared_ids=""     # one target id per line, in declaration order
declared_names=""   # one short name per line
declared_manifests="" # one manifest path per line, blank where a target declares none
covered_ids=""      # every id a target covers

entry=1
while [ "$entry" -le "$target_count" ]; do
	where="[[target]] $entry"
	id="$(single target "$entry" id "$where")"
	if [ -n "$id" ]; then
		where="[[target]] $entry (\"$id\")"
		check_id "the identifier" "$where" "$id"
	else
		fail "$declarations declares no id in $where; a target with no identifier can never be asked about, so give it one of the form <registry>:<name>"
	fi
	name="$(single target "$entry" name "$where")"
	if [ -n "$name" ]; then check_name "the short name" "$where" "$name"; else
		fail "$declarations declares no name in $where; the short name is what a host document and a consumer's plan name this target by, so a target without one cannot be waited on"
	fi
	for prose in what published_by; do
		value="$(single target "$entry" "$prose" "$where")"
		if [ -n "$value" ]; then check_prose "$prose" "$where" "$value"; else
			fail "$declarations declares no $prose in $where; every target says what a dependent gets and which workflow and job publish it"
		fi
	done
	# The canonical schema makes `manifest` optional; this repository is stricter
	# on purpose. Every artifact it publishes is built from a committed manifest,
	# and that manifest is what pins a declared name to a real package — without
	# one, a rename in the manifest leaves the declaration pointing at nothing and
	# nothing here notices.
	manifest="$(single target "$entry" manifest "$where")"
	if [ -n "$manifest" ]; then check_path "manifest" "$where" "$manifest"; else
		fail "$declarations declares '$id' with no manifest; add a manifest = \"<path>\" line to that target, since the manifest is what pins a declared name to a real package"
	fi
	while read -r covered; do
		[ -n "$covered" ] || continue
		check_id "a covers entry" "$where" "$covered"
		covered_ids="${covered_ids}${covered}
"
	done < <(values_of target "$entry" covers)
	declared_ids="${declared_ids}${id}
"
	declared_names="${declared_names}${name}
"
	declared_manifests="${declared_manifests}${manifest}
"
	entry=$((entry + 1))
done

retired_ids=""
entry=1
while [ "$entry" -le "$retired_count" ]; do
	where="[[retired]] $entry"
	id="$(single retired "$entry" id "$where")"
	if [ -n "$id" ]; then
		where="[[retired]] $entry (\"$id\")"
		check_id "the identifier" "$where" "$id"
	else
		fail "$declarations declares no id in $where; a retirement records which identifier is no longer published, so it needs one"
	fi
	why="$(single retired "$entry" why "$where")"
	if [ -n "$why" ]; then check_prose "why" "$where" "$why"; else
		fail "$declarations declares no why in $where; a retirement exists to tell a consumer still naming that artifact why it is gone"
	fi
	retired_ids="${retired_ids}${id}
"
	entry=$((entry + 1))
done

# An id is what a consumer names to wait on one artifact, and a short name is
# what a host document calls it. Two entries sharing either is not a duplicate
# to tidy up: it means two rows answer to one name and only one is consulted.
while read -r id; do
	[ -n "$id" ] || continue
	fail "$declarations declares '$id' more than once; every target answers to exactly one id, so keep the row that names the artifact a consumer waits on and drop the other"
done < <(printf '%s' "$declared_ids" | sort | uniq -d)

while read -r name; do
	[ -n "$name" ] || continue
	fail "$declarations gives the short name '$name' to more than one target; that name is what a host document and a consumer's plan select a target by, so two of them are two answers to one question"
done < <(printf '%s' "$declared_names" | sort | uniq -d)

# `covers` names what a target's release also ships and that is NOT a target of
# its own — the whole distinction the key draws — so an id that is both, or one
# two targets cover, is a document saying two things about one artifact.
while read -r covered; do
	[ -n "$covered" ] || continue
	printf '%s' "$declared_ids" | grep -Fxq -- "$covered" &&
		fail "$declarations covers '$covered', which it also declares as a target of its own; an artifact is one or the other, because a consumer waits on a target by name and never waits on something covered"
done < <(printf '%s' "$covered_ids")

while read -r covered; do
	[ -n "$covered" ] || continue
	fail "$declarations covers '$covered' from more than one target; one artifact is shipped by one release, so drop it from every covers list but the target whose release really ships it"
done < <(printf '%s' "$covered_ids" | sort | uniq -d)

# A retired artifact is one this repository does not publish any more, so a
# document that also declares or covers it is two answers about one artifact.
while read -r id; do
	[ -n "$id" ] || continue
	printf '%s' "$declared_ids" | grep -Fxq -- "$id" &&
		fail "$declarations retires '$id', which it also declares as a target; drop whichever is wrong — the [[retired]] entry if this repository still publishes it, the [[target]] if it does not"
	printf '%s' "$covered_ids" | grep -Fxq -- "$id" &&
		fail "$declarations retires '$id', which a target also covers; drop whichever is wrong — the [[retired]] entry if that target's release still ships it, the covers entry if it does not"
done < <(printf '%s' "$retired_ids")

while read -r id; do
	[ -n "$id" ] || continue
	fail "$declarations retires '$id' more than once; keep the [[retired]] entry whose why says what replaced it and drop the other"
done < <(printf '%s' "$retired_ids" | sort | uniq -d)

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
platform_names="" # per-platform npm packages, covered by a target rather than declared

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

published_ids="$(printf '%s' "$published" | cut -f1)
$(printf '%s' "$platform_names" | sed 's/^/npm:/')"

while IFS=$'\t' read -r id manifest; do
	[ -n "$id" ] || continue
	printf '%s\n' "$declared_ids" | grep -Fxq -- "$id" ||
		fail "this repository publishes '$id' (from $manifest) and $declarations declares no target for it, so a consumer waiting on that artifact gets no hold at all; add a [[target]] with id = \"$id\" and manifest = \"$manifest\""
done < <(printf '%s' "$published")

while read -r id; do
	[ -n "$id" ] || continue
	printf '%s\n' "$published_ids" | grep -Fxq -- "$id" ||
		fail "$declarations declares '$id', which this repository's release configuration does not publish; remove that [[target]], or restore whatever published it"
done < <(printf '%s' "$declared_ids")

while read -r id; do
	[ -n "$id" ] || continue
	printf '%s\n' "$published_ids" | grep -Fxq -- "$id" ||
		fail "$declarations covers '$id', which this repository's release configuration does not publish; drop it from that target's covers list, or restore whatever published it"
done < <(printf '%s' "$covered_ids")

# Each declared manifest must exist and must itself carry the declared name, so
# a rename in the manifest cannot leave the declaration pointing at nothing.
entry=1
while [ "$entry" -le "$target_count" ]; do
	id="$(single target "$entry" id "[[target]] $entry")"
	manifest="$(single target "$entry" manifest "[[target]] $entry")"
	entry=$((entry + 1))
	if [ -z "$id" ] || [ -z "$manifest" ]; then continue; fi
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
done

# A per-platform package is covered by a target, never declared as one — and it
# is also PINNED by the launcher whose optionalDependencies resolve it. Both
# are required: the covers list is what a consumer reading the declaration alone
# can see, the pin is what makes `npm install` resolve a binary at all.
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
	printf '%s' "$covered_ids" | grep -Fxq -- "npm:$name" ||
		fail "scripts/npm-build.mjs publishes '$name' and no declared target covers it, so a consumer reading $declarations is told nothing about it; add \"npm:$name\" to the covers list of the npm:oneharness-cli target (covered rather than declared, because nothing depends on a per-platform package by name)"
	printf '%s' "$pinned" | grep -Fq "\"$name\": \"" ||
		fail "scripts/npm-build.mjs publishes '$name' and no declared npm target's optionalDependencies pins it, so npm can never resolve it; add it to npm/oneharness/package.json's optionalDependencies"
done < <(printf '%s\n' "$platform_names")

if [ "$fails" -ne 0 ]; then
	printf 'check-release-targets: %d drift(s) between %s and what this repository publishes\n' "$fails" "$declarations" >&2
	exit 1
fi
echo "check-release-targets: every published artifact is declared or covered, in the shape the canonical schema declares"
