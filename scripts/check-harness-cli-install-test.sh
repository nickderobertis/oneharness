#!/usr/bin/env bash
# Hermetic proof of how pinned harness CLIs are installed and verified, with no
# network: the Cursor pinning helpers run against a fixture installer, and
# verify-harness-cli-pins.sh runs with stub npm/curl on PATH that install
# binaries reporting whatever version a case dictates.
#
# The helpers are the only way any workflow installs Cursor, and the verifier is
# what proves a moved pin on every OS; both must refuse to install or report the
# wrong build rather than pass.
set -euo pipefail

fail() {
  echo "check-harness-cli-install-test: $1" >&2
  echo "  ${2:-rerun with 'bash -x scripts/check-harness-cli-install-test.sh' to inspect the failing case}" >&2
  exit 1
}

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)" ||
  fail "cannot resolve the repository root above ${BASH_SOURCE[0]}" "run it from a checkout"
tmp="$(mktemp -d "${TMPDIR:-/tmp}/check-harness-cli-install-test.XXXXXX")" ||
  fail "cannot create scratch space under ${TMPDIR:-/tmp}" "point TMPDIR at a writable directory, then rerun"
cleanup() { rm -rf "$tmp" || echo "check-harness-cli-install-test: could not remove $tmp; remove it by hand" >&2; }
trap cleanup EXIT

write() {
  local path="$1"
  shift
  printf '%s\n' "$@" >"$path" || fail "cannot write fixture $path" "check that $tmp is writable, then rerun"
}

# $1 description, $2 expected exit, $3 substring; the rest is the command.
expect() {
  local description="$1" expected="$2" needle="$3" out status=0
  shift 3
  out="$("$@" 2>&1)" || status=$?
  [ "$status" -eq "$expected" ] || fail "$description: exit $status, want $expected; output: $out"
  case "$out" in
    *"$needle"*) ;;
    *) fail "$description: output lacks '$needle': $out" ;;
  esac
}

pin=2026.11.22-e9042b2
old=2020.01.01-abcdef0

# --- Cursor, Unix: the vendor installer with its embedded build swapped. ------
# The fixture has the shape of cursor.com/install: its build written into the
# download URL and the install paths, and an install that records which build
# it would fetch.
# shellcheck disable=SC2016 # fixture installer text: it expands when the installer runs, not here
write "$tmp/installer.sh" \
  "DOWNLOAD_URL=\"https://downloads.cursor.com/lab/$old/linux/x64/agent-cli-package.tar.gz\"" \
  "FINAL_DIR=\"\$HOME/.local/share/cursor-agent/versions/$old\"" \
  'mkdir -p "$HOME/.local/bin"' \
  'build="${DOWNLOAD_URL#https://downloads.cursor.com/lab/}"' \
  'printf "#!/bin/sh\necho %s\n" "${build%%/*}" >"$HOME/.local/bin/cursor-agent"' \
  'chmod +x "$HOME/.local/bin/cursor-agent"'
# curl on Windows reads a file URL by its drive path, not an MSYS one.
file_url() {
  if command -v cygpath >/dev/null 2>&1; then printf 'file:///%s' "$(cygpath -m "$1")"; else printf 'file://%s' "$1"; fi
}
unix() { env CURSOR_INSTALLER_URL="$(file_url "$tmp/$1")" "${@:2}" bash "$root/scripts/install-cursor-agent.sh" "$pin"; }

mkdir -p "$tmp/home" || fail "cannot create $tmp/home" "check that $tmp is writable, then rerun"
expect "the pinned Unix installer runs" 0 "" unix installer.sh HOME="$tmp/home"
[ "$("$tmp/home/.local/bin/cursor-agent")" = "$pin" ] ||
  fail "the Unix helper did not install the pinned build: $("$tmp/home/.local/bin/cursor-agent")"
out="$(unix installer.sh OH_CURSOR_PRINT_ONLY=1)" || fail "printing the pinned installer failed: $out"
case "$out" in *"$old"*) fail "the pinned installer still names $old: $out" ;; esac
case "$out" in *"versions/$pin"*) ;; *) fail "the pinned installer's install path is not the pin's: $out" ;; esac

# The build-id boundary every Cursor validator shares: 7 to 40 hex digits.
for ok in 2026.11.22-abcdef1 2026.11.22-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa; do
  out="$(env CURSOR_INSTALLER_URL="$(file_url "$tmp/installer.sh")" OH_CURSOR_PRINT_ONLY=1 \
    bash "$root/scripts/install-cursor-agent.sh" "$ok" 2>&1)" || fail "the Unix helper refused the build $ok: $out"
done
for bad in latest 1.2.3 "$pin; rm -rf /" '' 2026.11.22-abcdef 2026.11.22-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa; do
  expect "the Unix helper refusing version '$bad'" 1 "is not a Cursor build id" \
    env CURSOR_INSTALLER_URL="$(file_url "$tmp/installer.sh")" bash "$root/scripts/install-cursor-agent.sh" "$bad"
done
expect "the Unix helper with no version" 1 "expected exactly one argument, the version; got 0" \
  bash "$root/scripts/install-cursor-agent.sh"
expect "the Unix helper with two versions" 1 "expected exactly one argument, the version; got 2" \
  bash "$root/scripts/install-cursor-agent.sh" "$pin" "$pin"
# A second build anywhere in the installer would be fetched unpinned.
write "$tmp/two-builds.sh" "A=https://downloads.cursor.com/lab/$old/x.tar.gz" \
  'B=https://downloads.cursor.com/lab/2019.01.01-0000000/y.tar.gz'
expect "an installer that downloads a second build" 1 "also downloads build 2019.01.01-0000000" unix two-builds.sh
# The vendor's own failure is the helper's failure, with the installer's words.
write "$tmp/failing.sh" "URL=https://downloads.cursor.com/lab/$old/x.tar.gz" 'echo "no space left on device" >&2' 'exit 3'
expect "an installer that fails" 1 "Cursor's installer failed installing $pin with exit 3" unix failing.sh HOME="$tmp/home"
expect "an installer that fails, its output kept" 1 "no space left on device" unix failing.sh HOME="$tmp/home"
write "$tmp/reshaped.sh" 'curl -fsSL https://cdn.cursor.example/agent.tar.gz | tar -xz'
expect "a reshaped Unix installer" 1 "no longer downloads from https://downloads.cursor.com/lab/<version>/" \
  unix reshaped.sh
expect "an unreachable Unix installer" 1 "could not fetch Cursor's installer" unix absent.sh

# --- Cursor, Windows: the same pinning in PowerShell (every runner has pwsh). -
command -v pwsh >/dev/null 2>&1 ||
  fail "pwsh is not on PATH; scripts/cursor-installer.ps1 cannot be tested" \
    "install PowerShell (https://aka.ms/powershell) — GitHub's runners carry it"
native() { if command -v cygpath >/dev/null 2>&1; then cygpath -w "$1"; else printf '%s' "$1"; fi; }
# shellcheck disable=SC2016 # fixture installer text: PowerShell expands it, not bash
write "$tmp/installer.ps1" "\$downloadUrl = 'https://downloads.cursor.com/lab/$old/'" "\$version = '$old'" \
  'Write-Host "installing $version"'
# The helper throws; print the message unwrapped, as a caller's log would show
# it. Paths and the version reach PowerShell as environment values, not source.
ps() {
  # shellcheck disable=SC2016 # PowerShell expands it from the environment, so no path is spliced into its source
  env OH_HELPER="$(native "$root/scripts/cursor-installer.ps1")" OH_VERSION="$1" OH_INSTALLER="$(native "$tmp/$2")" \
    pwsh -NoProfile -NonInteractive -Command \
    'try { & $env:OH_HELPER -Version $env:OH_VERSION -InstallerUrl $env:OH_INSTALLER } catch { [Console]::Error.WriteLine($_.Exception.Message); exit 1 }'
}
out="$(ps "$pin" installer.ps1)" || fail "the Windows helper failed on its fixture: $out"
case "$out" in
  *"$old"*) fail "the pinned Windows installer still names $old: $out" ;;
  *"\$downloadUrl = 'https://downloads.cursor.com/lab/$pin/'"*"\$version = '$pin'"*) ;;
  *) fail "the Windows helper did not pin both of its installer's version lines: $out" ;;
esac
for bad in latest 2026.11.22-abcdef 2026.11.22-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa; do
  expect "the Windows helper refusing '$bad'" 1 "is not a Cursor build id" ps "$bad" installer.ps1
done
for ok in 2026.11.22-abcdef1 2026.11.22-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa; do
  out="$(ps "$ok" installer.ps1 2>&1)" || fail "the Windows helper refused the build $ok: $out"
done
write "$tmp/reshaped.ps1" "\$downloadUrl = 'https://cdn.cursor.example/'"
expect "a reshaped Windows installer" 1 "no longer downloads from https://downloads.cursor.com/lab/<version>/" \
  ps "$pin" reshaped.ps1
write "$tmp/two-builds.ps1" "\$downloadUrl = 'https://downloads.cursor.com/lab/$old/'" \
  "\$fallback = 'https://downloads.cursor.com/lab/2019.01.01-0000000/'"
expect "a Windows installer that downloads a second build" 1 "also downloads build 2019.01.01-0000000" \
  ps "$pin" two-builds.ps1
expect "an unreachable Windows installer" 1 "could not fetch Cursor's installer" ps "$pin" absent.ps1

# --- verify-harness-cli-pins.sh with stub installers. ------------------------
# npm installs a binary that reports the version it was asked for (or
# STUB_REPORTS, or with STUB_SUFFIX appended); curl serves Goose's installer
# and Cursor's fixture.
mkdir -p "$tmp/bin" || fail "cannot create $tmp/bin" "check that $tmp is writable, then rerun"
cat >"$tmp/bin/npm" <<'STUB' || fail "cannot write the npm stub" "check that $tmp is writable, then rerun"
#!/usr/bin/env bash
set -euo pipefail
[ "${STUB_NPM_FAILS:-}" = 1 ] && { echo "npm error 404 Not Found" >&2; exit 1; }
[ "${STUB_NPM_SILENT_FAIL:-}" = 1 ] && exit 7
prefix=""
spec=""
while [ "$#" -gt 0 ]; do
  case "$1" in --prefix) prefix="$2"; shift ;; -*|install) ;; *) spec="$1" ;; esac
  shift
done
pkg="${spec%@*}"; ver="${spec##*@}"
case "$pkg" in
  @anthropic-ai/claude-code) b=claude ;; @openai/codex) b=codex ;; opencode-ai) b=opencode ;;
  @qwen-code/qwen-code) b=qwen ;; @charmland/crush) b=crush ;; @github/copilot) b=copilot ;;
esac
[ "${STUB_NO_BIN:-}" = 1 ] && exit 0
mkdir -p "$prefix/bin"
printf '#!/bin/sh\necho "%s %s"\nexit %s\n' "$b" "${STUB_PREFIX:-}${STUB_REPORTS:-$ver}${STUB_SUFFIX:-}" "${STUB_VERSION_EXIT:-0}" >"$prefix/bin/$b"
chmod +x "$prefix/bin/$b"
STUB
cat >"$tmp/bin/curl" <<STUB || fail "cannot write the curl stub" "check that $tmp is writable, then rerun"
#!/usr/bin/env bash
set -euo pipefail
url=""
for a in "\$@"; do url="\$a"; done
case "\$url" in
  *download_cli.sh) printf '%s\n' 'mkdir -p "\$GOOSE_BIN_DIR"' \
    'printf "#!/bin/sh\necho \" %s\"\n" "\$GOOSE_VERSION" >"\$GOOSE_BIN_DIR/goose"' 'chmod +x "\$GOOSE_BIN_DIR/goose"' ;;
  https://cursor.com/install) cat "$tmp/installer.sh" ;;
  *) echo "curl: unexpected \$url" >&2; exit 22 ;;
esac
STUB
chmod +x "$tmp/bin/npm" "$tmp/bin/curl" ||
  fail "cannot make the stub installers executable" "check that $tmp allows executables, then rerun"
# shellcheck disable=SC2120 # arguments are env assignments, passed through expect
verify() { env PATH="$tmp/bin:$PATH" OH_VERIFY_HARNESS_OS=Linux "$@" bash "$root/scripts/verify-harness-cli-pins.sh"; }

out="$(verify)" || fail "the verifier failed with every stub reporting its pin: $out"
[ "$(printf '%s\n' "$out" | wc -l | tr -d ' ')" = 1 ] || fail "the verifier's success was not one line: $out"
listing="$(cat -- "$root/.harness-cli-versions")" ||
  fail "cannot read .harness-cli-versions" "restore it ('git checkout -- .harness-cli-versions'), then rerun"
while read -r name version; do
  case "$name" in '' | '#'*) continue ;; esac
  case "$out" in
    "verify-harness-cli-pins: ok linux/"*" $name $version ("*"$version"*) ;;
    *) fail "the verifier's evidence names no $name $version: $out" ;;
  esac
done <<<"$listing"

expect "an npm CLI reporting another version" 1 "reported '" verify STUB_REPORTS=0.0.1
# A version that only contains the pin is not the pin.
expect "an npm CLI reporting the pin with a digit more" 1 "not the pinned" verify STUB_SUFFIX=0
# A `v` prefix and a closing `.` are how CLIs print a version (crush, copilot).
expect "versions printed as v<pin>." 0 "verify-harness-cli-pins: ok" verify STUB_PREFIX=v STUB_SUFFIX=.
expect "an install that lays down no binary" 1 "installed no runnable binary" verify STUB_NO_BIN=1
expect "a --version that fails" 1 "--version exited 4" verify STUB_VERSION_EXIT=4
expect "an install failing silently keeps its status" 1 "failed with exit 7" verify STUB_NPM_SILENT_FAIL=1
expect "an npm install that fails" 1 "installing @anthropic-ai/claude-code failed" verify STUB_NPM_FAILS=1
sed 's#^@github/copilot .*#@github/copilot latest#' "$root/.harness-cli-versions" >"$tmp/pins" ||
  fail "cannot write the malformed pin file fixture" "check that $tmp is writable, then rerun"
expect "a pin file the drift check refuses" 1 "does not pass scripts/check-harness-cli-pins.sh" \
  bash "$root/scripts/verify-harness-cli-pins.sh" "$tmp/pins"
expect "a missing pin file" 1 "no pin file at $tmp/absent" bash "$root/scripts/verify-harness-cli-pins.sh" "$tmp/absent"

echo "check-harness-cli-install-test: ok"
