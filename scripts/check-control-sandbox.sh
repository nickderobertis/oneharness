#!/usr/bin/env bash
# Hermetic check for `oh_control_sandbox`, the scratch directory the live
# controlled turn (`oh_controlled_turn`) hands `--session-dir`/`--cwd`.
#
# oneharness refuses a control socket whose address passes the platform's
# unix-socket limit — 104 bytes on macOS, NUL included. With macOS's per-user
# temp dir as the base, the socket path of a real Mac run was 105 bytes:
#   /var/folders/36/tjdph2t965j8snz9_vkdnw0r0000gn/T/tmp.UBN09xfCSx/sessions/control/ohnet25164.sock
# So on Darwin the directory must sit under a short base whatever TMPDIR is,
# and elsewhere it stays in the host temp dir.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

fail_check() {
  echo "check-control-sandbox: $1" >&2
  echo "  Next: fix oh_control_sandbox in scripts/e2e-lib.sh, then re-run: bash scripts/check-control-sandbox.sh" >&2
  exit 1
}

work="$(mktemp -d)"
made=""
trap 'rm -rf "$work" ${made:+"$made"}' EXIT

# A TMPDIR as long as the real macOS runner's (/var/folders/<2>/<28>/T).
long_tmp="$work/var/folders/36/tjdph2t965j8snz9_vkdnw0r0000gn/T"
mkdir -p "$long_tmp"

# The session id the controlled turn passes, at its longest ($RANDOM ≤ 32767).
socket_in() { printf '%s/sessions/control/ohnet32767.sock' "$1"; }

sandbox_for() {
  # shellcheck disable=SC2016  # $1 is the inner shell's argument
  TMPDIR="$long_tmp" OH_E2E_OS="$1" bash -c 'source "$1"; oh_control_sandbox' _ "$root/scripts/e2e-lib.sh"
}

darwin="$(sandbox_for Darwin)" || fail_check "oh_control_sandbox failed for Darwin"
made="$darwin"
[ -d "$darwin" ] || fail_check "oh_control_sandbox printed '$darwin' for Darwin, which is not a directory"
socket="$(socket_in "$darwin")"
# 103 bytes of path leaves the terminating NUL inside the 104-byte limit.
[ "${#socket}" -le 103 ] ||
  fail_check "on Darwin with TMPDIR=$long_tmp the control socket would be ${#socket} bytes ($socket), past macOS's 104-byte unix-socket address limit"
rm -rf "$darwin"
made=""

linux="$(sandbox_for Linux)" || fail_check "oh_control_sandbox failed for Linux"
made="$linux"
case "$linux" in
  "$long_tmp"/*) ;;
  *) fail_check "on Linux the sandbox left the host temp dir: got '$linux', expected it under TMPDIR=$long_tmp" ;;
esac

echo "check-control-sandbox: ok"
