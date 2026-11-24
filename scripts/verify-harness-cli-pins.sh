#!/usr/bin/env bash
# Prove every harness CLI .harness-cli-versions pins installs on this platform
# at its pin and reports that version itself. No credentials and no model call:
# each is installed into scratch space the way the workflows install it (npm at
# the exact version, Goose's installer with GOOSE_VERSION, Cursor through the
# pinning helpers) and asked for `--version`.
#
# ci.yml's check job runs it on Linux, macOS ARM64 and Windows whenever a pin
# or an install helper changes, so a moved pin is proven where the workflows
# install it before it merges. On success it prints one line, the evidence:
#   verify-harness-cli-pins: ok <os>/<arch>: <name> <pin> (<its --version>); ...
#
# Usage: verify-harness-cli-pins.sh [PINS_FILE]
set -euo pipefail

fail() {
  echo "verify-harness-cli-pins: $1" >&2
  echo "verify-harness-cli-pins: ${2:-fix the pin or its install path, then rerun 'bash scripts/verify-harness-cli-pins.sh'}" >&2
  exit 1
}

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)" ||
  fail "cannot resolve the repository root above ${BASH_SOURCE[0]}" "run it from a checkout"
pins="${1:-$root/.harness-cli-versions}"
[ -f "$pins" ] || fail "no pin file at $pins"
# The pin file's shape is check-harness-cli-pins.sh's to hold; refuse to run
# on one it would refuse.
bash "$root/scripts/check-harness-cli-pins.sh" "$pins" "$root"/.github/workflows/*.yml >/dev/null ||
  fail "$pins does not pass scripts/check-harness-cli-pins.sh" "run that check and fix what it names first"

# OH_VERIFY_HARNESS_OS lets the hermetic test drive the Unix install path with
# stubbed installers on every runner, Windows included; CI never sets it.
case "${OH_VERIFY_HARNESS_OS:-$(uname -s)}" in
  Linux*) os=linux ;;
  Darwin*) os=darwin ;;
  MINGW* | MSYS* | CYGWIN*) os=windows ;;
  *) fail "unsupported platform $(uname -s)" ;;
esac
arch="$(uname -m)"

scratch="$(mktemp -d "${TMPDIR:-/tmp}/verify-harness-cli-pins.XXXXXX")" ||
  fail "cannot create scratch space under ${TMPDIR:-/tmp}" "point TMPDIR at a writable directory"
cleanup() { rm -rf "$scratch" || echo "verify-harness-cli-pins: could not remove $scratch; remove it by hand" >&2; }
trap cleanup EXIT

native() { if [ "$os" = windows ]; then cygpath -w "$1"; else printf '%s' "$1"; fi; }

quietly() {
  local what="$1" log="$scratch/install.log"
  shift
  "$@" >"$log" 2>&1 || {
    cat "$log" >&2
    fail "installing $what failed (its output is above)" "check that the pinned version is published for $os/$arch"
  }
}

npm_bin() {
  case "$1" in
    @anthropic-ai/claude-code) echo claude ;;
    @openai/codex) echo codex ;;
    opencode-ai) echo opencode ;;
    @qwen-code/qwen-code) echo qwen ;;
    @charmland/crush) echo crush ;;
    @github/copilot) echo copilot ;;
    *) return 1 ;;
  esac
}

install_and_report() {
  local name="$1" version="$2" bin
  case "$name" in
    goose)
      mkdir -p "$scratch/goose" || fail "cannot create $scratch/goose" "point TMPDIR at a writable directory"
      if [ "$os" = windows ]; then
        quietly goose env GOOSE_BIN_DIR="$(native "$scratch/goose")" GOOSE_VERSION="$version" CONFIGURE=false \
          pwsh -NoProfile -NonInteractive -Command \
          "irm 'https://raw.githubusercontent.com/block/goose/v$version/download_cli.ps1' | iex"
        bin="$scratch/goose/goose.exe"
      else
        quietly goose bash -o pipefail -c "curl -fsSL 'https://github.com/block/goose/releases/download/v$version/download_cli.sh' |
          GOOSE_BIN_DIR='$scratch/goose' GOOSE_VERSION='$version' CONFIGURE=false bash"
        bin="$scratch/goose/goose"
      fi
      ;;
    cursor-agent)
      mkdir -p "$scratch/home" "$scratch/localappdata" ||
        fail "cannot create Cursor's scratch home under $scratch" "point TMPDIR at a writable directory"
      if [ "$os" = windows ]; then
        quietly cursor-agent env LOCALAPPDATA="$(native "$scratch/localappdata")" USERPROFILE="$(native "$scratch/home")" \
          pwsh -NoProfile -NonInteractive -Command \
          "\$s = & '$(native "$root/scripts/cursor-installer.ps1")' -Version '$version'; Invoke-Expression \$s"
        bin="$(find "$scratch/localappdata" "$scratch/home" -iname 'cursor-agent.cmd' -type f -print -quit)" ||
          fail "cannot search $scratch for the cursor-agent.cmd launcher Cursor's installer lays down"
      else
        quietly cursor-agent env HOME="$scratch/home" bash "$root/scripts/install-cursor-agent.sh" "$version"
        bin="$scratch/home/.local/bin/cursor-agent"
      fi
      ;;
    *)
      local command
      command="$(npm_bin "$name")" ||
        fail "$pins pins $name, which this script does not know how to run" "add its binary name to npm_bin in scripts/verify-harness-cli-pins.sh"
      quietly "$name" npm install -g --prefix "$scratch/npm" --no-fund --no-audit "$name@$version"
      if [ "$os" = windows ]; then bin="$scratch/npm/$command.cmd"; else bin="$scratch/npm/bin/$command"; fi
      ;;
  esac
  [ -n "$bin" ] && [ -e "$bin" ] || fail "$name $version installed no runnable binary (looked for ${bin:-a cursor-agent.cmd launcher})"
  local reported
  reported="$("$bin" --version 2>&1 | tr -d '\r')" || fail "$name $version did not run: $reported"
  # The pin must be a whole word of the output (`v` prefix and a closing `.`
  # allowed), so 1.0.1140 is not 1.0.114.
  local word found=0 words
  read -ra words <<<"${reported//$'\n'/ }"
  for word in "${words[@]}"; do
    word="${word#v}"
    if [ "${word%.}" = "$version" ]; then found=1; fi
  done
  [ "$found" = 1 ] ||
    fail "$name reported '$reported', not the pinned $version" "the installer fetched another version; pin it so it cannot"
  evidence="${evidence:+$evidence; }$name $version ($(printf '%s' "$reported" | tr '\n' ' ' | sed 's/ *$//'))"
}

evidence=""

while read -r name version rest; do
  case "$name" in '' | '#'*) continue ;; esac
  [ -z "$rest" ] || fail "malformed pin line: $name $version $rest"
  install_and_report "$name" "$version"
done <"$pins"
[ -n "$evidence" ] || fail "$pins pins nothing"
echo "verify-harness-cli-pins: ok $os/$arch: $evidence"
