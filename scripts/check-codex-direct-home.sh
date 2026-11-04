#!/usr/bin/env bash
# llmlint: ignore-file[e2e_not_mocked] The subprocess double observes the environment at the native CLI boundary; the live Codex suite proves the same launch against real Codex on Windows.
# The direct execpolicy control must resolve its isolated home on every OS.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)" || {
  echo "check-codex-direct-home: could not resolve the repository; run this script inside a readable checkout" >&2
  exit 1
}
source "$root/scripts/e2e-lib.sh" || {
  echo "check-codex-direct-home: could not source $root/scripts/e2e-lib.sh; restore its read permission" >&2
  exit 1
}
fail() {
  echo "check-codex-direct-home: $1" >&2
  echo "  inspect _oh_codex_direct in scripts/e2e-lib.sh; rerun with bash -x scripts/check-codex-direct-home.sh" >&2
  exit 1
}
scratch="$(mktemp -d)" || fail "could not create scratch space; check the host temp directory"
trap 'rm -rf "$scratch" || fail "could not remove $scratch; remove it by hand"' EXIT
mkdir -p "$scratch/bin" "$scratch/project" "$scratch/home with spaces" || fail "could not create fixture directories; check $scratch permissions"
cat >"$scratch/bin/codex" <<'CODEX' || fail "could not write the Codex fixture; check $scratch permissions"
#!/usr/bin/env bash
set -euo pipefail
[ "$1" = exec ] || { echo "expected exec, got: $*" >&2; exit 2; }
[ "$CODEX_HOME" = "$EXPECTED_CODEX_HOME" ] || {
  echo "Error finding codex home: got $CODEX_HOME, expected $EXPECTED_CODEX_HOME" >&2
  exit 3
}
[ -d "$POSIX_CODEX_HOME" ] || { echo "missing home: $POSIX_CODEX_HOME" >&2; exit 4; }
printf 'direct turn reached its home\n'
CODEX
chmod +x "$scratch/bin/codex" || fail "could not make the Codex fixture executable; check $scratch permissions"
export PATH="$scratch/bin:$PATH"
export POSIX_CODEX_HOME="$scratch/home with spaces"
export EXPECTED_CODEX_HOME
# The host's real conversion: identity on Unix, a drive path on Git Bash.
EXPECTED_CODEX_HOME="$(oh_native_path "$POSIX_CODEX_HOME")" || fail "could not convert $POSIX_CODEX_HOME; check cygpath on this host"
_oh_codex_direct "$scratch/project" "$POSIX_CODEX_HOME" 'echo marker'
[ "$_OH_CODEX_OUT" = 'direct turn reached its home' ] || fail "direct Codex launch lost its home: $_OH_CODEX_OUT"
# Exercise the Windows boundary on Unix too, including a conversion failure.
cat >"$scratch/bin/cygpath" <<'CYGPATH' || fail "could not write the converter fixture; check $scratch permissions"
#!/usr/bin/env bash
printf 'C:/native home with spaces\n'
CYGPATH
chmod +x "$scratch/bin/cygpath" || fail "could not make the converter executable; check $scratch permissions"
EXPECTED_CODEX_HOME='C:/native home with spaces'
_oh_codex_direct "$scratch/project" "$POSIX_CODEX_HOME" 'echo marker'
[ "$_OH_CODEX_OUT" = 'direct turn reached its home' ] || fail "direct Codex launch passed a POSIX home to a native CLI: $_OH_CODEX_OUT"
cat >"$scratch/bin/cygpath" <<'CYGPATH' || fail "could not write the converter fixture; check $scratch permissions"
#!/usr/bin/env bash
exit 1
CYGPATH
if (_oh_codex_direct "$scratch/project" "$POSIX_CODEX_HOME" 'echo marker') >"$scratch/conversion.log" 2>&1; then
  fail 'direct Codex launch must fail when its home cannot be converted'
fi
# A converter that succeeds without a path must also stop before the launch.
cat >"$scratch/bin/cygpath" <<'CYGPATH' || fail "could not write the converter fixture; check $scratch permissions"
#!/usr/bin/env bash
exit 0
CYGPATH
if (_oh_codex_direct "$scratch/project" "$POSIX_CODEX_HOME" 'echo marker') >"$scratch/empty.log" 2>&1; then
  fail 'direct Codex launch must fail when conversion returns an empty path'
fi
