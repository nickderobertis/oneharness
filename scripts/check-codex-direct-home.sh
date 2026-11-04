#!/usr/bin/env bash
# llmlint: ignore-file[e2e_not_mocked] The subprocess double observes the environment at the native CLI boundary; the live Codex suite proves the same launch against real Codex on Windows.
# The direct execpolicy control must resolve its isolated home on every OS.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$root/scripts/e2e-lib.sh"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
mkdir -p "$scratch/bin" "$scratch/project" "$scratch/home with spaces"
cat >"$scratch/bin/codex" <<'CODEX'
#!/usr/bin/env bash
set -euo pipefail
[ "$1" = exec ] || exit 2
[ "$CODEX_HOME" = "$EXPECTED_CODEX_HOME" ] || {
  echo "Error finding codex home: got $CODEX_HOME, expected $EXPECTED_CODEX_HOME" >&2
  exit 3
}
[ -d "$POSIX_CODEX_HOME" ] || exit 4
printf 'direct turn reached its home\n'
CODEX
chmod +x "$scratch/bin/codex"
export PATH="$scratch/bin:$PATH"
export POSIX_CODEX_HOME="$scratch/home with spaces"
export EXPECTED_CODEX_HOME
# The host's real conversion: identity on Unix, a drive path on Git Bash.
EXPECTED_CODEX_HOME="$(oh_native_path "$POSIX_CODEX_HOME")"
_oh_codex_direct "$scratch/project" "$POSIX_CODEX_HOME" 'echo marker'
[ "$_OH_CODEX_OUT" = 'direct turn reached its home' ] || fail "direct Codex launch lost its home: $_OH_CODEX_OUT"
# Exercise the Windows boundary on Unix too, including a conversion failure.
cat >"$scratch/bin/cygpath" <<'CYGPATH'
#!/usr/bin/env bash
printf 'C:/native home with spaces\n'
CYGPATH
chmod +x "$scratch/bin/cygpath"
EXPECTED_CODEX_HOME='C:/native home with spaces'
_oh_codex_direct "$scratch/project" "$POSIX_CODEX_HOME" 'echo marker'
[ "$_OH_CODEX_OUT" = 'direct turn reached its home' ] || fail "direct Codex launch passed a POSIX home to a native CLI: $_OH_CODEX_OUT"
cat >"$scratch/bin/cygpath" <<'CYGPATH'
#!/usr/bin/env bash
exit 1
CYGPATH
if ( _oh_codex_direct "$scratch/project" "$POSIX_CODEX_HOME" 'echo marker' ) >"$scratch/conversion.log" 2>&1; then
  fail 'direct Codex launch must fail when its home cannot be converted'
fi
