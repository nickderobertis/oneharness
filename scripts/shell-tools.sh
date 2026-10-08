#!/usr/bin/env bash
#
# The pinned shell toolchain: install it, and run it.
#
#   scripts/shell-tools.sh install          # `just bootstrap` (and so CI)
#   scripts/shell-tools.sh exec <tool> ...  # run the pinned <tool>
#   scripts/shell-tools.sh path <tool>      # print the pinned <tool>'s path
#
# `.shell-tool-versions` at the repository root is the one pin. Each tool is
# installed into its own versioned directory under the tools cache
# (ONEHARNESS_TOOLS_DIR, default `${XDG_CACHE_HOME:-~/.cache}/oneharness/tools`),
# so worktrees share one install, `cargo clean` leaves it alone, and a pin bump
# can never run the previous version: `exec` resolves only the pinned
# directory, never whatever `shellcheck` or `shfmt` happens to be on PATH, so a
# format or lint verdict is the same on every machine.
#
# The shellcheck and shfmt pins are their projects' release binaries, checked
# against the SHA-256 recorded below for that exact version and platform; a
# version with no recorded checksum is refused rather than trusted. kcov
# publishes no binaries, so its pin is a source build, verified the same way,
# and it is built on Linux only: shell coverage is measured on Linux
# (tools/shell-coverage/AGENTS.md says why), so macOS and Windows skip it and
# run the shell tests uninstrumented. The build needs cmake (or uv, which
# supplies one) and kcov's libraries' headers; on Debian/Ubuntu:
#   sudo apt-get install binutils-dev libcurl4-openssl-dev libdw-dev libiberty-dev libssl-dev zlib1g-dev
#
# Every verified download is kept under the tools cache's `downloads/`, so a
# reinstall needs no network. Downloads come from each project's GitHub
# release unless ONEHARNESS_TOOLS_MIRROR names a base URL serving the same
# asset names (a `file://` directory works); a mirror cannot change what is
# installed, since every asset is still held to its recorded checksum.
#
# Quiet on success, one line; a failure names the tool, the step and the fix.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)" || {
  echo "shell-tools: could not resolve the repository root from ${BASH_SOURCE[0]}; run it from a checkout as 'bash scripts/shell-tools.sh'" >&2
  exit 2
}
pins="$root/.shell-tool-versions"

refuse() {
  local status="$1"
  shift
  printf 'shell-tools: %s\n' "$1" >&2
  shift
  for line in "$@"; do printf '  %s\n' "$line" >&2; done
  exit "$status"
}
die() { refuse 1 "$@"; }

usage() {
  refuse 2 "usage: scripts/shell-tools.sh install | exec <tool> [args...] | path <tool>" \
    "tools: shellcheck, shfmt, kcov (pinned in .shell-tool-versions)"
}

# The pinned version of $1: exactly one x.y.z (or kcov's bare integer) on
# exactly one line, so a line listing two versions can never pin the first.
pin() {
  local lines count
  [ -f "$pins" ] || die "no $pins to read the $1 pin from" "fix: restore .shell-tool-versions at the repository root"
  # Lines naming the tool are counted before any is read, so a second one —
  # even a bare name with no version — can never go unseen.
  count="$(awk -v tool="$1" '$1 == tool { n++ } END { print n + 0 }' "$pins")" ||
    die "could not read $pins (above)" "fix: restore its read permission (or 'git checkout -- .shell-tool-versions'), then re-run"
  case "$count" in
    1) ;;
    0) die ".shell-tool-versions has no $1 line" "fix: pin it as '$1 <version>'" ;;
    *) die ".shell-tool-versions names $1 on more than one line" "fix: keep exactly one '$1 <version>' line" ;;
  esac
  lines="$(awk -v tool="$1" '$1 == tool { $1 = ""; sub(/^[ \t]+/, ""); print }' "$pins")" ||
    die "could not read $pins (above)" "fix: restore its read permission (or 'git checkout -- .shell-tool-versions'), then re-run"
  [[ "$lines" =~ ^[0-9]+(\.[0-9]+)*$ ]] ||
    die ".shell-tool-versions pins $1 as '$lines', which is not exactly one version" "fix: write it as '$1 <version>', e.g. '$1 1.2.3'"
  printf '%s\n' "$lines"
}

case "$(uname -s)" in
  Linux) os=linux ;;
  Darwin) os=darwin ;;
  MINGW* | MSYS* | CYGWIN*) os=windows ;;
  *) die "unsupported platform '$(uname -s)'" "fix: install shellcheck and shfmt by hand; Linux, macOS and Windows are supported" ;;
esac
case "$(uname -m)" in
  x86_64 | amd64) arch=x86_64 ;;
  aarch64 | arm64) arch=aarch64 ;;
  *) die "unsupported architecture '$(uname -m)'" "fix: run on x86_64 or aarch64" ;;
esac
exe=""
[ "$os" = windows ] && exe=.exe

tools_dir="${ONEHARNESS_TOOLS_DIR:-${XDG_CACHE_HOME:-${HOME:?HOME is unset}/.cache}/oneharness/tools}"
case "$tools_dir" in
  /*) ;;
  *) die "ONEHARNESS_TOOLS_DIR '$tools_dir' is not an absolute path" "fix: unset it, or set it to an absolute directory" ;;
esac
mirror="${ONEHARNESS_TOOLS_MIRROR:-}"
case "$mirror" in
  '' | https://* | file:///*) mirror="${mirror%/}" ;;
  *) die "ONEHARNESS_TOOLS_MIRROR '$mirror' is not an https:// or file:/// URL" "fix: unset it to download from GitHub, or set it to a mirror's base URL" ;;
esac

# Every download and staging directory, removed however the run ends.
scratch=()
cleanup() {
  [ "${#scratch[@]}" -eq 0 ] || rm -rf "${scratch[@]}" ||
    printf 'shell-tools: could not remove its scratch (above); remove what is left of %s by hand\n' "${scratch[*]}" >&2
}
trap cleanup EXIT

bin_of() { printf '%s/%s-%s/bin/%s%s\n' "$tools_dir" "$1" "$2" "$1" "$exe"; }

# The release asset's name for a tool and version on this platform, and the
# URL GitHub serves it from. A bump adds the new version's checksums below,
# from the release's own digests.
asset_name() {
  local tool="$1" version="$2" goarch=amd64
  case "$tool" in
    shellcheck)
      if [ "$os" = windows ]; then
        printf 'shellcheck-v%s.zip\n' "$version"
      else
        printf 'shellcheck-v%s.%s.%s.tar.gz\n' "$version" "$os" "$arch"
      fi
      ;;
    shfmt)
      [ "$arch" = aarch64 ] && [ "$os" != windows ] && goarch=arm64
      printf 'shfmt_v%s_%s_%s%s\n' "$version" "$os" "$goarch" "$exe"
      ;;
    kcov) printf 'kcov-%s.tar.gz\n' "$version" ;;
  esac
}
release_url() {
  case "$1" in
    shellcheck) printf 'https://github.com/koalaman/shellcheck/releases/download/v%s/%s\n' "$2" "$3" ;;
    shfmt) printf 'https://github.com/mvdan/sh/releases/download/v%s/%s\n' "$2" "$3" ;;
    kcov) printf 'https://github.com/SimonKagstrom/kcov/archive/refs/tags/v%s.tar.gz\n' "$2" ;;
  esac
}

checksum() {
  case "$1" in
    shellcheck-v0.11.0.linux.x86_64.tar.gz) echo b7af85e41cc99489dcc21d66c6d5f3685138f06d34651e6d34b42ec6d54fe6f6 ;;
    shellcheck-v0.11.0.linux.aarch64.tar.gz) echo 68a8133197a50beb8803f8d42f9908d1af1c5540d4bb05fdfca8c1fa47decefc ;;
    shellcheck-v0.11.0.darwin.x86_64.tar.gz) echo c2c15e08df0e8fbc374c335b230a7ee958c313fa5714817a59aa59f1aa594f51 ;;
    shellcheck-v0.11.0.darwin.aarch64.tar.gz) echo 339b930feb1ea764467013cc1f72d09cd6b869ebf1013296ba9055ab2ffbd26f ;;
    shellcheck-v0.11.0.zip) echo 8a4e35ab0b331c85d73567b12f2a444df187f483e5079ceffa6bda1faa2e740e ;;
    shfmt_v3.14.1_linux_amd64) echo 76e77641faa025814b77f153b29796b8e6fa2fca03e0c76a691608b86c7ea7bf ;;
    shfmt_v3.14.1_linux_arm64) echo 5f2db09dae91fca848f7adbdd014632e921a383863a2ad7e0450ad3aba0c6489 ;;
    shfmt_v3.14.1_darwin_amd64) echo d33eee0da0f92835b3562e9767a05cee7e4eaeef47daa03bfd09da17b4b590a6 ;;
    shfmt_v3.14.1_darwin_arm64) echo b7c872db63553ccffc7253aba3ed7d4885a27d83f1ba567b1138c6315a5847e5 ;;
    shfmt_v3.14.1_windows_amd64.exe) echo 13629ce28442ca80b6b5a819f7574ab39e1c28c6e26734ca816c9714e04851df ;;
    kcov-43.tar.gz) echo 4cbba86af11f72de0c7514e09d59c7927ed25df7cebdad087f6d3623213b95bf ;;
    *) return 1 ;;
  esac
}

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{ print $1 }'
  else
    shasum -a 256 "$1" | awk '{ print $1 }'
  fi
}

# What the installed binary says its version is must be the pin. What it said
# (and the exit status of a probe that failed) stays in $reported, so a refusal
# can quote it.
reported=""
reports_version() {
  local tool="$1" version="$2" bin="$3" out status=0
  out="$("$bin" --version 2>&1)" || status=$?
  out="${out//$'\r'/}"
  reported="$(head -n 2 <<<"$out" | tr '\n' ' ')"
  if [ "$status" -ne 0 ]; then
    reported+="(--version exited $status)"
    return 1
  fi
  case "$tool" in
    shellcheck) grep -Fqx "version: $version" <<<"$out" ;;
    shfmt) [ "$out" = "v$version" ] || [ "$out" = "$version" ] ;;
    kcov) [ "$out" = "kcov $version" ] ;;
  esac
}

# Put the verified asset $3 for tool $1 at version $2 in $4: from the download
# cache when it holds the recorded bytes, else from the mirror or GitHub.
fetch() {
  local tool="$1" version="$2" name="$3" file="$4" want got url cached
  want="$(checksum "$name")" ||
    die "no SHA-256 recorded for $name" \
      "fix: add '$name) echo <sha256> ;;' to checksum() in scripts/shell-tools.sh, from the release's own digest"
  cached="$tools_dir/downloads/$name"
  if [ -f "$cached" ] && [ "$(sha256 "$cached")" = "$want" ]; then
    cp "$cached" "$file" || die "could not copy the cached $cached" "fix: check that $tools_dir is readable and its disk has room, then rerun 'just bootstrap'"
    return 0
  fi
  url="$(release_url "$tool" "$version" "$name")"
  [ -z "$mirror" ] || url="$mirror/$name"
  curl --fail --silent --show-error --location --retry 3 --output "$file" "$url" ||
    die "could not download $url" "fix: check network access to it (or unset ONEHARNESS_TOOLS_MIRROR), then rerun 'just bootstrap'"
  got="$(sha256 "$file")" ||
    die "could not checksum $file (above)" "fix: install sha256sum (coreutils) or shasum, then rerun 'just bootstrap'"
  [ "$got" = "$want" ] ||
    die "$name has SHA-256 $got, but $want is recorded for it; nothing was installed" \
      "fix: do not trust the download; re-check the release's digest (and any ONEHARNESS_TOOLS_MIRROR) before changing scripts/shell-tools.sh"
  { mkdir -p "$tools_dir/downloads" && cp "$file" "$cached.partial.$$" && mv "$cached.partial.$$" "$cached"; } ||
    die "could not keep $name in $tools_dir/downloads" "fix: check that $tools_dir is writable and its disk has room, then rerun 'just bootstrap'"
}

extract() {
  local archive="$1" into="$2"
  mkdir -p "$into" || return 1
  case "$archive" in
    *.zip)
      if command -v unzip >/dev/null 2>&1; then
        unzip -q "$archive" -d "$into"
      else
        local zip_win into_win
        zip_win="$(cygpath -w "$archive")" || return 1
        into_win="$(cygpath -w "$into")" || return 1
        # The paths reach PowerShell as data, never as part of its source, so
        # the `$env:` references are PowerShell's to expand.
        # shellcheck disable=SC2016
        OH_ZIP="$zip_win" OH_INTO="$into_win" \
          powershell.exe -NoProfile -Command 'Expand-Archive -LiteralPath $env:OH_ZIP -DestinationPath $env:OH_INTO'
      fi
      ;;
    *) tar -xzf "$archive" -C "$into" ;;
  esac
}

build_kcov() {
  local version="$1" dest="$2" src="$3" log="$4"
  local -a cmake=(cmake)
  if ! command -v cmake >/dev/null 2>&1; then
    command -v uv >/dev/null 2>&1 ||
      die "kcov $version is built from source and needs cmake, and neither cmake nor uv is on PATH" \
        "fix: install cmake (or uv, which supplies it), then rerun 'just bootstrap'"
    cmake=(uv tool run --quiet cmake)
  fi
  if ! {
    "${cmake[@]}" -S "$src" -B "$src/build" -DCMAKE_BUILD_TYPE=Release "-DCMAKE_INSTALL_PREFIX=$dest" &&
      "${cmake[@]}" --build "$src/build" --parallel 4 &&
      "${cmake[@]}" --install "$src/build"
  } >"$log" 2>&1; then
    tail -n 25 "$log" >&2
    die "kcov $version did not build (the build log's tail is above); nothing was installed" \
      "fix: install kcov's build dependencies, then rerun 'just bootstrap'; on Debian/Ubuntu:" \
      "sudo apt-get install binutils-dev libcurl4-openssl-dev libdw-dev libiberty-dev libssl-dev zlib1g-dev"
  fi
}

install_one() {
  local tool="$1" version bin dest stage work name found
  version="$(pin "$tool")"
  bin="$(bin_of "$tool" "$version")"
  if [ -x "$bin" ] && reports_version "$tool" "$version" "$bin"; then
    return 0
  fi
  dest="$tools_dir/$tool-$version"
  stage="$dest.partial.$$"
  work="$(mktemp -d)" || die "could not create a scratch directory (above)" "fix: check that ${TMPDIR:-/tmp} is writable and has room, then rerun 'just bootstrap'"
  scratch+=("$work" "$stage")
  name="$(asset_name "$tool" "$version")"
  mkdir -p "$stage/bin" || die "could not create $stage" "fix: check that $tools_dir is writable, then rerun 'just bootstrap'"
  fetch "$tool" "$version" "$name" "$work/$name"
  case "$tool" in
    shellcheck)
      extract "$work/$name" "$work/x" ||
        die "could not unpack $name (above)" "fix: delete $tools_dir/downloads/$name, then rerun 'just bootstrap'"
      found="$(find "$work/x" -type f -name "shellcheck$exe" | head -n 1)" ||
        die "could not search the unpacked $name (above)" "fix: check that ${TMPDIR:-/tmp} is readable, then rerun 'just bootstrap'"
      [ -n "$found" ] || die "$name holds no shellcheck$exe to install" "fix: check the asset recorded for shellcheck in scripts/shell-tools.sh"
      cp "$found" "$stage/bin/shellcheck$exe" ||
        die "could not stage shellcheck from $name" "fix: check that $tools_dir is writable and its disk has room, then rerun 'just bootstrap'"
      ;;
    shfmt)
      cp "$work/$name" "$stage/bin/shfmt$exe" ||
        die "could not stage shfmt from $name" "fix: check that $tools_dir is writable and its disk has room, then rerun 'just bootstrap'"
      ;;
    kcov)
      extract "$work/$name" "$work/x" ||
        die "could not unpack $name (above)" "fix: delete $tools_dir/downloads/$name, then rerun 'just bootstrap'"
      build_kcov "$version" "$stage" "$work/x/kcov-$version" "$work/build.log"
      ;;
  esac
  chmod +x "$stage/bin/$tool$exe" ||
    die "could not make the staged $tool executable (above)" "fix: keep ONEHARNESS_TOOLS_DIR on a filesystem that allows executables (not noexec), then rerun 'just bootstrap'"
  reports_version "$tool" "$version" "$stage/bin/$tool$exe" ||
    die "the $tool just staged does not report version $version; nothing was installed" \
      "it says: $reported" \
      "fix: check the asset recorded for $tool in scripts/shell-tools.sh"
  # The staged tree moves into place whole (kcov's build bakes its prefix
  # into nothing it reads at run time). Whatever was at $dest failed the
  # version check above, so it is no install of this pin worth keeping.
  rm -rf "$dest" || die "could not remove the stale $dest (above)" "fix: check that $tools_dir is writable, then rerun 'just bootstrap'"
  mv "$stage" "$dest" ||
    die "could not move the staged $tool $version into $dest" "fix: check that $tools_dir is writable, then rerun 'just bootstrap'"
}

resolve() {
  local tool="$1" version bin
  case "$tool" in shellcheck | shfmt | kcov) ;; *) usage ;; esac
  version="$(pin "$tool")"
  bin="$(bin_of "$tool" "$version")"
  if [ "$tool" = kcov ] && [ "$os" != linux ]; then
    die "kcov is not installed on $os: shell coverage is measured on Linux only" \
      "fix: run coverage on Linux; scripts/shell-test.sh runs the tests uninstrumented here"
  fi
  [ -x "$bin" ] ||
    die "$tool $version (the .shell-tool-versions pin) is not installed at $bin" \
      "fix: run 'just bootstrap' (or 'bash scripts/shell-tools.sh install')"
  reports_version "$tool" "$version" "$bin" ||
    die "$bin does not report $tool $version, the .shell-tool-versions pin" \
      "it says: $reported" \
      "fix: run 'just bootstrap', which replaces it"
  printf '%s\n' "$bin"
}

[ "$#" -ge 1 ] || usage
command="$1"
shift
case "$command" in
  install)
    [ "$#" -eq 0 ] || usage
    mkdir -p "$tools_dir" || die "could not create $tools_dir" "fix: set ONEHARNESS_TOOLS_DIR to a writable absolute directory, then rerun 'just bootstrap'"
    installed=()
    for tool in shellcheck shfmt kcov; do
      if [ "$tool" = kcov ] && [ "$os" != linux ]; then
        continue
      fi
      install_one "$tool"
      installed+=("$tool $(pin "$tool")")
    done
    summary="$(printf '%s, ' "${installed[@]}")"
    summary="${summary%, }"
    [ "$os" = linux ] || summary+=" (kcov skipped: shell coverage is measured on Linux)"
    echo "shell-tools: $summary"
    ;;
  exec)
    [ "$#" -ge 1 ] || usage
    bin="$(resolve "$1")"
    shift
    exec "$bin" "$@"
    ;;
  path)
    [ "$#" -eq 1 ] || usage
    resolve "$1"
    ;;
  *) usage ;;
esac
