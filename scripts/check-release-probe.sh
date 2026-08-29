#!/usr/bin/env bash
#
# Behavioral test of every way the release probe declines to answer.
#
# The one thing this probe must never do is report a question it could not
# answer as the answer "no release yet": a consumer reads the second as a fact
# about the registry and stops waiting, or waits for an artifact that will never
# exist. So every path that cannot produce a version is driven through the real
# script here and asserted to be non-zero with a reason on stderr and NOTHING on
# stdout — never exit 0 with empty output, which is the empty answer and belongs
# to the registry alone.
#
# Offline by construction. The refusals decided from the declaration run against
# a curl that records being called, and that recording must stay empty — which
# is also the proof that an unrecognised identifier costs a caller nothing. The
# paths past that point are driven against a curl that answers as a broken
# registry would, since no real registry answers on demand.
#
# The two answers a real registry owns — a version, and nothing for an artifact
# it has never served — are proven by scripts/release-probe-live.sh
# (`just release-probe-live`), out of the offline gate like every other
# network-touching check here.
#
# Quiet on success, one line. On failure it prints what the probe said.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

fail() {
  echo "check-release-probe: $1" >&2
  [ -s "$work/err" ] && { echo "  the probe's stderr:" >&2; cat "$work/err" >&2; }
  exit 1
}

# The stubbed halves need an extensionless executable, which is a Unix shape —
# the same reason scripts/check-local-gate.sh names. Windows keeps every
# assertion that does not need one.
stubbed=1
case $(uname -s) in
  MINGW* | MSYS* | CYGWIN*) stubbed=0 ;;
esac

reached="$work/network-reached"
if [ "$stubbed" -eq 1 ]; then
  mkdir -p "$work/bin"
  # A curl that records the call and answers however a case asks it to. Its
  # variables are read by this stub, never by the probe, which still reads only
  # PATH and HOME.
  cat >"$work/bin/curl" <<'STUB'
#!/usr/bin/env bash
printf 'curl %s\n' "$*" >>"$NETWORK_REACHED"
out=""
while [ "$#" -gt 0 ]; do
  case $1 in
    --output) out=$2; shift 2 ;;
    *) shift ;;
  esac
done
if [ "${STUB_TRANSPORT_FAILS:-0}" = 1 ]; then
  echo "stub: could not resolve host" >&2
  exit 6
fi
if [ -n "$out" ]; then printf '%s' "${STUB_BODY:-}" >"$out"; fi
printf '%s' "${STUB_STATUS:-200}"
STUB
  chmod +x "$work/bin/curl"

  # A PATH with only what the probe needs before it reads a registry, so a case
  # can take away curl or both JSON readers without taking away the shell — the
  # interpreter included, since `env bash` resolves on this PATH too.
  mkdir -p "$work/minbin"
  for tool in bash dirname sed awk grep tr head mktemp rm cat; do
    # `type -P` resolves the executable FILE. `command -v` would answer with a
    # shell function of the same name where a developer's profile defines one,
    # and the link this makes from that answer points at itself — leaving the
    # tool missing, and every case below refused for a reason it is not testing.
    path="$(type -P "$tool")" ||
      fail "no $tool on this host, so the restricted-PATH cases cannot be built; install $tool (it is in coreutils on Linux and macOS) and rerun"
    ln -s "$path" "$work/minbin/$tool"
  done
fi

# $1 = description, $2 = the reason the refusal must give, $3 = PATH to run
# under, $4 = the probe to run, then the probe's arguments. Asserts a refusal a
# caller cannot mistake for the empty answer, on the branch this case is about —
# a case refused for some other reason is a case that tests nothing.
assert_not_answered() {
  local description=$1 reason=$2 path=$3 script=$4 status=0
  shift 4
  : >"$reached"
  NETWORK_REACHED="$reached" PATH="$path" \
    "$script" "$@" >"$work/out" 2>"$work/err" || status=$?
  [ "$status" -ne 0 ] ||
    fail "$description was answered (exit 0) instead of refused; a caller cannot tell it from 'no release yet' — end that branch in scripts/release-probe.sh through unanswered/usage_error instead of letting it fall through to the version print"
  [ ! -s "$work/out" ] ||
    fail "$description wrote '$(cat "$work/out")' to stdout; a refusal says nothing there — in scripts/release-probe.sh, refuse before anything is printed and route the message to stderr through unanswered/usage_error"
  [ -s "$work/err" ] ||
    fail "$description gave no reason on stderr, so a caller learns only that something went wrong; give that branch in scripts/release-probe.sh an unanswered/usage_error call naming what it could not establish"
  grep -Fq "$reason" "$work/err" ||
    fail "$description was refused for some reason other than '$reason', so it exercised a branch it is not about; fix whichever branch of scripts/release-probe.sh now swallows it, or point this case's expected reason at the branch it really reaches"
}

# Refused from the declaration alone, before any registry is read.
stub_path=$PATH
if [ "$stubbed" -eq 1 ]; then stub_path="$work/bin:$PATH"; fi

assert_declined_offline() {
  local description=$1 reason=$2
  shift 2
  assert_not_answered "$description" "$reason" "$stub_path" scripts/release-probe.sh "$@"
  if [ "$stubbed" -eq 1 ] && [ -s "$reached" ]; then
    fail "$description read the network before refusing; decide an unrecognised identifier from the declaration alone"
  fi
}

# $1 = description, $2 = the one line stdout must carry, or "" for the empty
# answer, $3 = PATH to run under, then the probe's arguments. The counterpart
# of assert_not_answered: a probe that refused everything would satisfy every
# case above, so the two answers a caller acts on are asserted here too.
assert_answered() {
  local description=$1 expected=$2 path=$3 status=0
  shift 3
  : >"$reached"
  NETWORK_REACHED="$reached" PATH="$path" \
    scripts/release-probe.sh "$@" >"$work/out" 2>"$work/err" || status=$?
  [ "$status" -eq 0 ] ||
    fail "$description was refused (exit $status) instead of answered; a caller holds forever on a refusal, so fix whichever branch of scripts/release-probe.sh now swallows it — its stderr is below"
  [ "$(cat "$work/out")" = "$expected" ] ||
    fail "$description answered '$(cat "$work/out")' where a caller is promised '$expected'; scripts/release-probe.sh must print the version and nothing else, or nothing at all for an artifact the registry has never served"
}

assert_declined_offline "no identifier at all" "takes exactly one registry-qualified identifier"
assert_declined_offline "two identifiers" "takes exactly one registry-qualified identifier" \
  crate:oneharness crate:oneharness-core
assert_declined_offline "an unqualified name" "is not a release target of this repository" oneharness
assert_declined_offline "an unknown registry" "is not a release target of this repository" cargo:oneharness
# The trap this matters most for: a real PyPI project name that is not the one
# this repository publishes. PyPI would answer 404, and reporting that as "no
# release yet" would leave a consumer waiting on a distribution nobody ships.
assert_declined_offline "a name this repository does not publish" \
  "is not a release target of this repository" pypi:oneharness
assert_declined_offline "a declared name under the wrong registry" \
  "is not a release target of this repository" npm:oneharness-sdk

# A probe with no declarations to read recognises nothing, and says so rather
# than answering for everything or for nothing.
fixture="$work/no-declaration"
mkdir -p "$fixture/scripts"
cp scripts/release-probe.sh "$fixture/scripts/release-probe.sh"
assert_not_answered "a checkout with no release-targets.toml" \
  "cannot read" "$stub_path" "$fixture/scripts/release-probe.sh" crate:oneharness
# The declaration authorizes which registries get read, so it is refused when
# it is not the shape this probe reads — never scanned for anything that looks
# like an id.
: >"$fixture/release-targets.toml"
assert_not_answered "a declaration with no schema version" \
  "declares schema_version ''" "$stub_path" \
  "$fixture/scripts/release-probe.sh" crate:oneharness
printf 'schema_version = 2\n\n[[target]]\nid = "crate:oneharness"\nmanifest = "Cargo.toml"\n' \
  >"$fixture/release-targets.toml"
assert_not_answered "a declaration written to a later version" \
  "declares schema_version '2'" "$stub_path" \
  "$fixture/scripts/release-probe.sh" crate:oneharness
printf 'schema_version = 1\n' >"$fixture/release-targets.toml"
assert_not_answered "a declaration with no targets" \
  "declares no release targets" "$stub_path" \
  "$fixture/scripts/release-probe.sh" crate:oneharness
printf 'schema_version = 1\nid = "crate:oneharness"\n' >"$fixture/release-targets.toml"
assert_not_answered "an id outside any [[target]] block" \
  "declares no release targets" "$stub_path" \
  "$fixture/scripts/release-probe.sh" crate:oneharness
# Only a [[target]] is a release target. A [[retired]] entry names an artifact
# this repository does not publish any more and a `covers` entry names one
# nothing depends on by name, and both carry a registry-qualified id — so a
# probe that answered for either would report a version for something no
# consumer may wait on. Written with the [[target]] carrying no id of its own,
# which is what would let a block boundary that does not close leak the next
# entry's id into the declared set.
cat >"$fixture/release-targets.toml" <<'NEIGHBOURS'
schema_version = 1

[[target]]
name = "cli-crate"
covers = ["npm:@oneharness/cli-linux-x64"]

[[retired]]
id = "pypi:oneharness-retired"
why = "Nothing here publishes it again."
NEIGHBOURS
assert_not_answered "a retired id, with the [[target]] above it carrying none" \
  "declares no release targets" "$stub_path" \
  "$fixture/scripts/release-probe.sh" pypi:oneharness-retired
assert_not_answered "a covered id, which is not a target of its own" \
  "declares no release targets" "$stub_path" \
  "$fixture/scripts/release-probe.sh" "npm:@oneharness/cli-linux-x64"
# The same neighbours beside a target that does declare an id, so the refusal is
# the probe recognising a real declared set without them rather than an empty
# one. This is the shape the checked-in declaration has.
cat >"$fixture/release-targets.toml" <<'NEIGHBOURS'
schema_version = 1

[[target]]
id = "crate:oneharness"
name = "cli-crate"
covers = ["npm:@oneharness/cli-linux-x64"]

[[retired]]
id = "pypi:oneharness-retired"
why = "Nothing here publishes it again."
NEIGHBOURS
assert_not_answered "a retired id beside a declared target" \
  "is not a release target of this repository" "$stub_path" \
  "$fixture/scripts/release-probe.sh" pypi:oneharness-retired
assert_not_answered "a covered id beside the target that covers it" \
  "is not a release target of this repository" "$stub_path" \
  "$fixture/scripts/release-probe.sh" "npm:@oneharness/cli-linux-x64"
[ ! -s "$reached" ] ||
  fail "an id the declaration names outside a [[target]] read the network before refusing; decide the declared set from [[target]] blocks alone in scripts/release-probe.sh"
# Declaring an id says it was written down, not that its name is a package name.
# The name becomes a path segment of a registry URL, so one carrying a separator
# would ask a different question and publish that answer as this target's
# version — refused, and before the network, like every other unanswerable id.
printf 'schema_version = 1\n\n[[target]]\nid = "crate:oneharness/../serde"\nmanifest = "Cargo.toml"\n' \
  >"$fixture/release-targets.toml"
assert_not_answered "a declared name that is not a package name" \
  "is not a crate package name" "$stub_path" \
  "$fixture/scripts/release-probe.sh" "crate:oneharness/../serde"
[ ! -s "$reached" ] ||
  fail "a declared name that is not a package name read the network before refusing; validate the name's shape in scripts/release-probe.sh before the request is made"

if [ "$stubbed" -eq 0 ]; then
  echo "check-release-probe: an unanswerable identifier is refused offline (registry-answer cases skipped on Windows)"
  exit 0
fi

# Past the declaration: everything a registry read can do other than answer.
# Each case is a declared target, so it reaches the read and is refused there.
assert_not_answered "a host with no curl" \
  "curl is required" "$work/minbin" scripts/release-probe.sh crate:oneharness
STUB_TRANSPORT_FAILS=1 \
  assert_not_answered "a registry read that fails in transport" \
  "could not reach" "$stub_path" scripts/release-probe.sh crate:oneharness
STUB_STATUS=503 \
  assert_not_answered "a registry answering 503" \
  "returned HTTP 503" "$stub_path" scripts/release-probe.sh crate:oneharness
STUB_BODY='not json at all' \
  assert_not_answered "a registry answering with something that is not JSON" \
  "could not parse" "$stub_path" scripts/release-probe.sh crate:oneharness
STUB_BODY='{"crate":{}}' \
  assert_not_answered "a registry answering without the version field" \
  "without a version at" "$stub_path" scripts/release-probe.sh crate:oneharness
STUB_BODY='{"crate":{"max_stable_version":"see the release notes"}}' \
  assert_not_answered "a registry serving something that is not a version" \
  "which is not a version a caller can use" "$stub_path" \
  scripts/release-probe.sh crate:oneharness
# A single word where a version belongs: it carries nothing a caller can order
# or compare, so it is refused rather than handed on as though it were one.
STUB_BODY='{"crate":{"max_stable_version":"latest"}}' \
  assert_not_answered "a registry serving a word instead of a version" \
  "which is not a version a caller can use" "$stub_path" \
  scripts/release-probe.sh crate:oneharness
# And values that only look like versions because they start like one: a caller
# can neither order nor compare either of these.
STUB_BODY='{"crate":{"max_stable_version":"1latest"}}' \
  assert_not_answered "a registry serving a digit followed by a word" \
  "which is not a version a caller can use" "$stub_path" \
  scripts/release-probe.sh crate:oneharness
STUB_BODY='{"crate":{"max_stable_version":"1.."}}' \
  assert_not_answered "a registry serving a version with empty segments" \
  "which is not a version a caller can use" "$stub_path" \
  scripts/release-probe.sh crate:oneharness
# A newline the registry really served, which command substitution would
# otherwise tidy away before anything could refuse it.
STUB_BODY='{"crate":{"max_stable_version":"1.2.3\n"}}' \
  assert_not_answered "a registry serving a version with a newline after it" \
  "which is not a version a caller can use" "$stub_path" \
  scripts/release-probe.sh crate:oneharness
# A value whose FIRST line is a version and whose second is anything at all: a
# line-oriented check would accept it, and the caller promised one line would
# then read two.
STUB_BODY='{"crate":{"max_stable_version":"1.2.3\nand something else"}}' \
  assert_not_answered "a registry serving a version with a second line after it" \
  "which is not a version a caller can use" "$stub_path" \
  scripts/release-probe.sh crate:oneharness

STUB_BODY='{"crate":{"max_stable_version":"1.2.3"}}' \
  assert_not_answered "a host with neither JSON reader" \
  "neither jq nor python3" "$work/minbin:$work/bin" \
  scripts/release-probe.sh crate:oneharness

# The two answers a caller acts on. Everything above proves the probe declines
# where it must, which a probe that declined ALWAYS would also satisfy — so the
# distinction those cases exist to protect is only really pinned once both
# answers are driven here too. They need a registry to answer, not a network:
# the stub supplies each registry's own reply and the real script reads it.
#
# release-probe-live.sh drives the same two against the real registries; this is
# what keeps them inside the offline gate, where a regression is caught on every
# run rather than on an opt-in one.
STUB_BODY='{"crate":{"max_stable_version":"1.2.3"}}' \
  assert_answered "a registry serving a version" "1.2.3" "$stub_path" crate:oneharness
# The empty answer, and the only thing that produces it: the registry itself
# saying it has never served this artifact.
STUB_STATUS=404 STUB_BODY='{"detail":"Not Found"}' \
  assert_answered "a registry that has never served the artifact" "" "$stub_path" \
  crate:oneharness

# Each registry's own reply shape and version path, so a registry whose payload
# this probe reads from the wrong key is caught here rather than live.
STUB_BODY='{"info":{"version":"0.11.0"}}' \
  assert_answered "PyPI's own payload" "0.11.0" "$stub_path" pypi:oneharness-cli
STUB_BODY='{"dist-tags":{"latest":"0.11.0"}}' \
  assert_answered "npm's own payload" "0.11.0" "$stub_path" npm:oneharness-cli
# A scoped name is one path segment on npm, so its separator is percent-encoded.
# Unencoded, the registry reads it as a package inside an org and answers 404 —
# which this probe would then report as "never released" for an artifact that is
# published, the one confusion it exists to prevent.
STUB_BODY='{"dist-tags":{"latest":"0.11.0"}}' \
  assert_answered "a scoped npm name" "0.11.0" "$stub_path" npm:@oneharness/sdk
grep -Fq '@oneharness%2fsdk' "$reached" ||
  fail "the scoped npm name was requested as '$(sed -n 's|.*registry.npmjs.org/||p' "$reached" | head -n 1)'; npm serves a scoped package under one percent-encoded path segment, so restore the '/'-to-%2f substitution in the npm arm of scripts/release-probe.sh"

# crates.io serves a prerelease as max_version while max_stable_version holds
# what a consumer may depend on, so the order of the paths is load-bearing.
STUB_BODY='{"crate":{"max_stable_version":"1.2.3","max_version":"2.0.0-rc.1"}}' \
  assert_answered "a crate with a newer prerelease" "1.2.3" "$stub_path" crate:oneharness
# With no stable release at all, the prerelease is what the registry serves.
STUB_BODY='{"crate":{"max_stable_version":"","max_version":"0.1.0-alpha.1"}}' \
  assert_answered "a crate with only a prerelease" "0.1.0-alpha.1" "$stub_path" crate:oneharness

# Both readers, since either can be the one a host has. The refusals above
# exercise them failing; neither was ever proven to produce a right answer.
readers=0
for reader in jq python3; do
  path="$(type -P "$reader")" || continue
  mkdir -p "$work/reader-$reader"
  ln -sf "$path" "$work/reader-$reader/$reader"
  # minbin carries no reader, so each case runs with exactly the one it names.
  STUB_BODY='{"crate":{"max_stable_version":"1.2.3"}}' \
    assert_answered "the $reader reader" "1.2.3" \
    "$work/reader-$reader:$work/minbin:$work/bin" crate:oneharness
  readers=$((readers + 1))
done
[ "$readers" -gt 0 ] ||
  fail "this host has neither jq nor python3, so no reader could be exercised; install one (the probe needs it too) and rerun"

echo "check-release-probe: every path that cannot produce a version is refused rather than reported as 'no release yet', and both answers a caller acts on are distinguishable ($readers reader(s))"
