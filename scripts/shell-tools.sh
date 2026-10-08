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
# The shellcheck and shfmt pins are their projects' release binaries, checked against
# the SHA-256 recorded below for that exact version and platform; a version
# with no recorded checksum is refused rather than trusted. kcov publishes no
# binaries, so its pin is a source build, verified the same way, and it is
# built on Linux only: shell coverage is measured on Linux (AGENTS.md says
# why), so macOS and Windows skip it and run the shell tests uninstrumented.
# The build needs cmake (or uv, which supplies one) and kcov's libraries'
# headers; on Debian/Ubuntu:
#   sudo apt-get install binutils-dev libcurl4-openssl-dev libdw-dev libiberty-dev libssl-dev zlib1g-dev
#
# Quiet on success, one line; a failure names the tool, the step and the fix.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
pins="$root/.shell-tool-versions"

die() {
  printf 'shell-tools: %s\n' "$1" >&2
  shift
  for line in "$@"; do printf '  %s\n' "$line" >&2; done
  exit "${SHELL_TOOLS_STATUS:-1}"
}

usage() {
  SHELL_TOOLS_STATUS=2 die "usage: scripts/shell-tools.sh install | exec <tool> [args...] | path <tool>" \
    "tools: shellcheck, shfmt, kcov (pinned in .shell-tool-versions)"
}

# The pinned version of $1: exactly one x.y.z (or kcov's bare integer) on
# exactly one line, so a line listing two versions can never pin the first.
pin() {
  local lines
  [ -f "$pins" ] || die "no $pins to read the $1 pin from" "fix: restore .shell-tool-versions at the repository root"
  lines="$(awk -v tool="$1" '$1 == tool { $1 = ""; sub(/^[ \t]+/, ""); print }' "$pins")"
  case "$(printf '%s' "$lines" | grep -c . || true)" in
    1) ;;
    0) die ".shell-tool-versions has no $1 line" "fix: pin it as '$1 <version>'" ;;
    *) die ".shell-tool-versions names $1 on more than one line" "fix: keep exactly one '$1 <version>' line" ;;
  esac
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

# Every download and staging directory, removed however the run ends.
scratch=()
cleanup() { [ "${#scratch[@]}" -eq 0 ] || rm -rf "${scratch[@]}"; }
trap cleanup EXIT

bin_of() { printf '%s/%s-%s/bin/%s%s\n' "$tools_dir" "$1" "$2" "$1" "$exe"; }

# The release asset for a tool, version and this platform, and its SHA-256.
# A bump adds the new version's lines here, from the release's own digests.
asset() {
  local tool="$1" version="$2" name
  case "$tool" in
    shellcheck)
      case "$os" in
        windows) name="shellcheck-v$version.zip" ;;
        *) name="shellcheck-v$version.$os.$arch.tar.gz" ;;
      esac
      printf 'https://github.com/koalaman/shellcheck/releases/download/v%s/%s %s\n' "$version" "$name" "$name"
      ;;
    shfmt)
      local goarch=amd64
      [ "$arch" = aarch64 ] && [ "$os" != windows ] && goarch=arm64
      name="shfmt_v${version}_${os}_$goarch$exe"
      printf 'https://github.com/mvdan/sh/releases/download/v%s/%s %s\n' "$version" "$name" "$name"
      ;;
    kcov)
      printf 'https://github.com/SimonKagstrom/kcov/archive/refs/tags/v%s.tar.gz kcov-%s.tar.gz\n' "$version" "$version"
      ;;
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

# What the installed binary says its version is must be the pin.
reports_version() {
  local tool="$1" version="$2" bin="$3" out
  out="$("$bin" --version 2>&1)" || return 1
  case "$tool" in
    shellcheck) grep -qx "version: $version" <<<"${out//$'\r'/}" ;;
    shfmt) [ "${out//$'\r'/}" = "v$version" ] || [ "${out//$'\r'/}" = "$version" ] ;;
    kcov) [ "${out//$'\r'/}" = "kcov $version" ] ;;
  esac
}

fetch() {
  local url="$1" file="$2" name="$3" want got
  want="$(checksum "$name")" ||
    die "no SHA-256 recorded for $name" \
      "fix: add '$name) echo <sha256> ;;' to checksum() in scripts/shell-tools.sh, from the release's own digest"
  curl --fail --silent --show-error --location --retry 3 --output "$file" "$url" ||
    die "could not download $url" "fix: check network access to github.com, then rerun 'just bootstrap'"
  got="$(sha256 "$file")"
  [ "$got" = "$want" ] ||
    die "$name has SHA-256 $got, but $want is recorded for it" \
      "fix: do not trust the download; re-check the release's digest before changing scripts/shell-tools.sh"
}

build_kcov() {
  local version="$1" dest="$2" work="$3" log="$3/build.log"
  local -a cmake=(cmake)
  if ! command -v cmake >/dev/null 2>&1; then
    command -v uv >/dev/null 2>&1 ||
      die "kcov $version is built from source and needs cmake, and neither cmake nor uv is on PATH" \
        "fix: install cmake (or uv, which supplies it), then rerun 'just bootstrap'"
    cmake=(uv tool run --quiet cmake)
  fi
  fetch "https://github.com/SimonKagstrom/kcov/archive/refs/tags/v$version.tar.gz" "$work/kcov.tar.gz" "kcov-$version.tar.gz"
  tar -xzf "$work/kcov.tar.gz" -C "$work"
  if ! {
    "${cmake[@]}" -S "$work/kcov-$version" -B "$work/build" -DCMAKE_BUILD_TYPE=Release "-DCMAKE_INSTALL_PREFIX=$dest" &&
      "${cmake[@]}" --build "$work/build" --parallel 4 &&
      "${cmake[@]}" --install "$work/build"
  } >"$log" 2>&1; then
    tail -n 25 "$log" >&2
    die "kcov $version did not build (the build log's tail is above)" \
      "fix: install kcov's build dependencies, then rerun 'just bootstrap'; on Debian/Ubuntu:" \
      "sudo apt-get install binutils-dev libcurl4-openssl-dev libdw-dev libiberty-dev libssl-dev zlib1g-dev"
  fi
}

install_one() {
  local tool="$1" version bin dest stage work url name
  version="$(pin "$tool")"
  bin="$(bin_of "$tool" "$version")"
  if [ -x "$bin" ] && reports_version "$tool" "$version" "$bin"; then
    return 0
  fi
  dest="$tools_dir/$tool-$version"
  stage="$dest.partial.$$"
  work="$(mktemp -d)"
  scratch+=("$work" "$stage")
  rm -rf "$stage"
  mkdir -p "$stage/bin"
  read -r url name < <(asset "$tool" "$version")
  case "$tool" in
    shellcheck)
      fetch "$url" "$work/$name" "$name"
      if [ "$os" = windows ]; then
        if command -v unzip >/dev/null 2>&1; then
          unzip -q "$work/$name" -d "$work/x"
        else
          powershell.exe -NoProfile -Command "Expand-Archive -LiteralPath '$(cygpath -w "$work/$name")' -DestinationPath '$(cygpath -w "$work/x")'"
        fi
        cp "$(find "$work/x" -name shellcheck.exe | head -n 1)" "$stage/bin/shellcheck.exe"
      else
        tar -xzf "$work/$name" -C "$work"
        cp "$work/shellcheck-v$version/shellcheck" "$stage/bin/shellcheck"
      fi
      ;;
    shfmt)
      fetch "$url" "$work/$name" "$name"
      cp "$work/$name" "$stage/bin/shfmt$exe"
      ;;
    kcov)
      build_kcov "$version" "$stage" "$work"
      ;;
  esac
  chmod +x "$stage/bin/$tool$exe"
  reports_version "$tool" "$version" "$stage/bin/$tool$exe" ||
    die "the $tool just installed does not report version $version" \
      "it says: $("$stage/bin/$tool$exe" --version 2>&1 | head -n 2 | tr '\n' ' ')" \
      "fix: check the asset recorded for $tool in scripts/shell-tools.sh"
  # kcov's build bakes its install prefix into nothing it reads at run time,
  # so the staged tree can move into place whole.
  rm -rf "$dest"
  mv "$stage" "$dest"
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
  printf '%s\n' "$bin"
}

[ "$#" -ge 1 ] || usage
command="$1"
shift
case "$command" in
  install)
    [ "$#" -eq 0 ] || usage
    mkdir -p "$tools_dir"
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
