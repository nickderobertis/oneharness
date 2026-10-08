#!/usr/bin/env bash
#
# Behavioral test of scripts/shell-tools.sh, the pinned shell toolchain.
#
# The real pins are proven first: the shellcheck and shfmt this checkout runs
# (and kcov, on Linux) report exactly the versions .shell-tool-versions names.
# Then the pin reader and the installer's refusals are driven against a staged
# copy with its own pin file and a tools directory of stand-in binaries: a pin
# the installer finds already in place is used as it is, an absent tool is
# refused with the command that installs it, and a malformed pin or a version
# with no recorded checksum stops the install before any byte is fetched.
# Last, the real pins are installed cold, offline, from the release assets
# this checkout's own install kept: extracted, verified and moved into place,
# replacing a stale install; and a mismatched checksum, an unreachable mirror
# and a failing kcov build each install nothing and say how to recover.
#
# Quiet on success, one line. On failure it names the case and what it saw.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

fail() {
  echo "check-shell-tools: $1" >&2
  shift
  for line in "$@"; do echo "  $line" >&2; done
  echo "  fix: restore that behaviour in scripts/shell-tools.sh, then rerun 'bash scripts/check-shell-tools.sh'" >&2
  exit 1
}

pinned() { awk -v t="$1" '$1 == t { print $2 }' .shell-tool-versions; }

# The toolchain this checkout's targets run is the pinned one.
out="$(bash scripts/shell-tools.sh exec shellcheck --version)" ||
  fail "the pinned shellcheck did not run; run 'just bootstrap' first" "$out"
grep -qx "version: $(pinned shellcheck)" <<<"${out//$'\r'/}" ||
  fail "the shellcheck it runs is not the pinned $(pinned shellcheck)" "$out"
out="$(bash scripts/shell-tools.sh exec shfmt --version)" ||
  fail "the pinned shfmt did not run; run 'just bootstrap' first" "$out"
[ "${out//$'\r'/}" = "v$(pinned shfmt)" ] ||
  fail "the shfmt it runs is not the pinned $(pinned shfmt)" "$out"
linux=0
[ "$(uname -s)" = Linux ] && linux=1
if [ "$linux" = 1 ]; then
  out="$("$(bash scripts/shell-tools.sh path kcov)" --version)" ||
    fail "the pinned kcov did not run; run 'just bootstrap' first" "$out"
  [ "$out" = "kcov $(pinned kcov)" ] || fail "the kcov it runs is not the pinned $(pinned kcov)" "$out"
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
stage="$work/repo"
mkdir -p "$stage/scripts"
cp scripts/shell-tools.sh "$stage/scripts/"
tools="$work/tools"
exe=""
case "$(uname -s)" in MINGW* | MSYS* | CYGWIN*) exe=.exe ;; esac

# A stand-in that answers --version as the pinned tool does and otherwise
# echoes its arguments, so `exec` is seen passing them through.
stand_in() {
  local tool="$1" version="$2" says="$3"
  mkdir -p "$tools/$tool-$version/bin"
  # The stand-in's own expansions are its, at its run time.
  # shellcheck disable=SC2016
  printf '#!/usr/bin/env bash\nif [ "${1:-}" = --version ]; then printf "%%s\\n" %q; else echo "%s ran: $*"; fi\n' \
    "$says" "$tool" >"$tools/$tool-$version/bin/$tool$exe"
  chmod +x "$tools/$tool-$version/bin/$tool$exe"
}
pins() { printf '%s\n' "$@" >"$stage/.shell-tool-versions"; }
run() {
  status=0
  ONEHARNESS_TOOLS_DIR="$tools" bash "$stage/scripts/shell-tools.sh" "$@" >"$work/out" 2>&1 || status=$?
}
expect() {
  local want="$1" text="$2" case="$3"
  [ "$status" = "$want" ] || fail "$case: exited $status, not $want" "$(cat "$work/out")"
  grep -Fq -- "$text" "$work/out" || fail "$case: did not say '$text'" "$(cat "$work/out")"
}

pins "shellcheck 1.2.3" "shfmt 4.5.6" "kcov 78"
stand_in shellcheck 1.2.3 "version: 1.2.3"
stand_in shfmt 4.5.6 "v4.5.6"
stand_in kcov 78 "kcov 78"

run install
if [ "$linux" = 1 ]; then
  expect 0 "shell-tools: shellcheck 1.2.3, shfmt 4.5.6, kcov 78" "an install with every pin in place"
else
  expect 0 "kcov skipped" "an install with every pin in place"
fi
run exec shfmt -d some/file.sh
expect 0 "shfmt ran: -d some/file.sh" "exec of the pinned shfmt"
run path shellcheck
expect 0 "$tools/shellcheck-1.2.3/bin/shellcheck$exe" "path of the pinned shellcheck"

# A pin bump never runs the previous version: the new one is not installed.
pins "shellcheck 1.2.4" "shfmt 4.5.6" "kcov 78"
run exec shellcheck --version
expect 1 "shellcheck 1.2.4 (the .shell-tool-versions pin) is not installed" "a bumped pin with the old version installed"
grep -Fq "run 'just bootstrap'" "$work/out" || fail "a missing tool did not name 'just bootstrap'" "$(cat "$work/out")"

# A version with no recorded checksum is refused before anything is fetched.
pins "shellcheck 1.2.3" "shfmt 9.9.9" "kcov 78"
run install
expect 1 "no SHA-256 recorded for shfmt_v9.9.9_" "an install of a version with no recorded checksum"

# Malformed pins are refused, never read as their first version.
for case in "two versions|shfmt 4.5.6 4.5.7|not exactly one version" \
  "two lines|shfmt 4.5.6;shfmt 4.5.6|more than one line" \
  "no line|# shfmt removed|has no shfmt line" \
  "not a version|shfmt latest|not exactly one version"; do
  IFS='|' read -r name line want <<<"$case"
  printf 'shellcheck 1.2.3\nkcov 78\n%s\n' "${line//;/$'\n'}" >"$stage/.shell-tool-versions"
  run exec shfmt --version
  expect 1 "$want" "a pin file with $name for shfmt"
done

pins "shellcheck 1.2.3" "shfmt 4.5.6" "kcov 78"
status=0
ONEHARNESS_TOOLS_DIR=relative/tools bash "$stage/scripts/shell-tools.sh" path shfmt >"$work/out" 2>&1 || status=$?
expect 1 "is not an absolute path" "a relative ONEHARNESS_TOOLS_DIR"
status=0
ONEHARNESS_TOOLS_MIRROR=http://mirror.example ONEHARNESS_TOOLS_DIR="$tools" bash "$stage/scripts/shell-tools.sh" install >"$work/out" 2>&1 || status=$?
expect 1 "is not an https:// or file:/// URL" "a plain-http ONEHARNESS_TOOLS_MIRROR"
for args in "" "exec" "path nope" "frobnicate"; do
  read -r -a argv <<<"$args"
  run "${argv[@]}"
  expect 2 "usage: scripts/shell-tools.sh" "the arguments '$args'"
done

# Cold installs of the real pins, offline: the download cache is seeded with
# the release assets this checkout's own install verified and kept, and the
# mirror names a directory that does not exist, so any fetch fails loudly.
# kcov's build runs through a cmake double that installs a stand-in, since
# its real build is what `just bootstrap` proves.
cp .shell-tool-versions "$stage/.shell-tool-versions"
real_tools="$(dirname "$(dirname "$(dirname "$(bash scripts/shell-tools.sh path shfmt)")")")"
assets=()
for tool in shellcheck shfmt kcov; do
  [ "$tool" = kcov ] && [ "$linux" = 0 ] && continue
  name="$(find "$real_tools/downloads" -maxdepth 1 -name "$tool*$(pinned "$tool")*" ! -name '*.partial.*' 2>/dev/null | head -n 1)"
  [ -n "$name" ] || fail "$real_tools/downloads keeps no $tool $(pinned "$tool") asset to install from; run 'just bootstrap' first"
  assets+=("$name")
done
seed() {
  rm -rf "$tools"
  mkdir -p "$tools/downloads"
  cp "$@" "$tools/downloads/"
}
doubles="$work/doubles"
mkdir -p "$doubles"
cat >"$doubles/cmake" <<'SH'
#!/usr/bin/env bash
# A cmake double: configure records the install prefix, install writes a kcov
# that reports the pinned version there; CMAKE_DOUBLE_FAIL fails every step.
set -euo pipefail
if [ -n "${CMAKE_DOUBLE_FAIL:-}" ]; then
  echo "cmake: $CMAKE_DOUBLE_FAIL" >&2
  exit 1
fi
case "$1" in
  -S)
    mkdir -p "$4"
    for arg in "$@"; do
      case "$arg" in -DCMAKE_INSTALL_PREFIX=*) printf '%s\n' "${arg#*=}" >"$4/prefix" ;; esac
    done
    ;;
  --build) ;;
  --install)
    prefix="$(cat "$2/prefix")"
    mkdir -p "$prefix/bin"
    printf '#!/usr/bin/env bash\necho "kcov %s"\n' "$CMAKE_DOUBLE_VERSION" >"$prefix/bin/kcov"
    ;;
esac
SH
chmod +x "$doubles/cmake"
cold() {
  status=0
  PATH="$doubles:$PATH" CMAKE_DOUBLE_VERSION="$(pinned kcov)" ONEHARNESS_TOOLS_MIRROR="file://$work/no-mirror" \
  ONEHARNESS_TOOLS_DIR="$tools" bash "$stage/scripts/shell-tools.sh" "$@" >"$work/out" 2>&1 || status=$?
}

seed "${assets[@]}"
# A previous install that no longer reports the pin is replaced, not kept.
stand_in shfmt "$(pinned shfmt)" "v0.0.0"
cold install
expect 0 "shell-tools: shellcheck $(pinned shellcheck), shfmt $(pinned shfmt)" "a cold install from the download cache"
for tool in shellcheck shfmt; do
  out="$("$tools/$tool-$(pinned "$tool")/bin/$tool$exe" --version)"
  grep -Fq "$(pinned "$tool")" <<<"$out" || fail "the cold-installed $tool does not report $(pinned "$tool")" "$out"
done
[ "$linux" = 0 ] || [ "$("$tools/kcov-$(pinned kcov)/bin/kcov" --version)" = "kcov $(pinned kcov)" ] ||
  fail "the cold install did not put the built kcov in place" "$(ls -R "$tools")"
leftovers="$(find "$tools" -maxdepth 1 \( -name '*.partial.*' -o -name '*.old.*' \))"
[ -z "$leftovers" ] || fail "a cold install left staging behind" "$leftovers"

# Bytes that do not match the recorded checksum are refused and kept nowhere.
rm -rf "$tools"
stand_in shellcheck "$(pinned shellcheck)" "version: $(pinned shellcheck)"
mkdir -p "$work/mirror"
shfmt_asset="$(basename "$(printf '%s\n' "${assets[@]}" | grep '/shfmt')")"
printf 'not shfmt\n' >"$work/mirror/$shfmt_asset"
status=0
ONEHARNESS_TOOLS_MIRROR="file://$work/mirror" ONEHARNESS_TOOLS_DIR="$tools" \
  bash "$stage/scripts/shell-tools.sh" install >"$work/out" 2>&1 || status=$?
expect 1 "$shfmt_asset has SHA-256" "a mirrored asset with the wrong bytes"
[ ! -e "$tools/shfmt-$(pinned shfmt)" ] && [ ! -e "$tools/downloads/$shfmt_asset" ] ||
  fail "an asset failing its checksum was installed or kept" "$(ls -R "$tools")"

# A download that fails names the URL and the fix.
cold install
expect 1 "could not download file://$work/no-mirror/$shfmt_asset" "an unreachable mirror"

# A mirror serving the recorded bytes installs, and the download is kept, so
# reinstalling needs no network at all.
cp "$(printf '%s\n' "${assets[@]}" | grep '/shfmt')" "$work/mirror/$shfmt_asset"
[ "$linux" = 0 ] || stand_in kcov "$(pinned kcov)" "kcov $(pinned kcov)"
status=0
ONEHARNESS_TOOLS_MIRROR="file://$work/mirror/" ONEHARNESS_TOOLS_DIR="$tools" \
  bash "$stage/scripts/shell-tools.sh" install >"$work/out" 2>&1 || status=$?
expect 0 "shfmt $(pinned shfmt)" "an install from a mirror serving the recorded bytes"
[ -f "$tools/downloads/$shfmt_asset" ] || fail "an install from the mirror did not keep its download" "$(ls -R "$tools")"
rm -rf "$tools/shfmt-$(pinned shfmt)"
cold install
expect 0 "shfmt $(pinned shfmt)" "a reinstall with only the kept download and no mirror"

if [ "$linux" = 1 ]; then
  # A kcov build that fails shows the build's own tail and the dependencies to
  # install, and installs nothing.
  seed "${assets[@]}"
  stand_in shellcheck "$(pinned shellcheck)" "version: $(pinned shellcheck)"
  stand_in shfmt "$(pinned shfmt)" "v$(pinned shfmt)"
  status=0
  PATH="$doubles:$PATH" CMAKE_DOUBLE_FAIL="no libdw found" ONEHARNESS_TOOLS_MIRROR="file://$work/no-mirror" \
    ONEHARNESS_TOOLS_DIR="$tools" bash "$stage/scripts/shell-tools.sh" install >"$work/out" 2>&1 || status=$?
  expect 1 "kcov $(pinned kcov) did not build" "a failing kcov build"
  grep -Fq "cmake: no libdw found" "$work/out" || fail "a failing kcov build hid the build's own error" "$(cat "$work/out")"
  grep -Fq "sudo apt-get install binutils-dev" "$work/out" || fail "a failing kcov build named no dependencies to install" "$(cat "$work/out")"
  [ ! -e "$tools/kcov-$(pinned kcov)" ] || fail "a failing kcov build installed a kcov" "$(ls -R "$tools")"
fi

echo "check-shell-tools: ok"
