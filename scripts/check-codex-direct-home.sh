#!/usr/bin/env bash
# llmlint: ignore-file[e2e_not_mocked] The subprocess double observes the environment at the native CLI boundary; the live Codex suite proves the same launch against real Codex on Windows.
# The direct execpolicy control must resolve its isolated home, and trust its
# scratch project under the key Codex looks it up by, on every OS.
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
  echo "  inspect _oh_codex_direct and _oh_codex_trust_config in scripts/e2e-lib.sh; rerun with bash -x scripts/check-codex-direct-home.sh" >&2
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
set -euo pipefail
printf 'C:/native home with spaces\n'
CYGPATH
chmod +x "$scratch/bin/cygpath" || fail "could not make the converter executable; check $scratch permissions"
EXPECTED_CODEX_HOME='C:/native home with spaces'
_oh_codex_direct "$scratch/project" "$POSIX_CODEX_HOME" 'echo marker'
[ "$_OH_CODEX_OUT" = 'direct turn reached its home' ] || fail "direct Codex launch passed a POSIX home to a native CLI: $_OH_CODEX_OUT"
cat >"$scratch/bin/cygpath" <<'CYGPATH' || fail "could not write the converter fixture; check $scratch permissions"
#!/usr/bin/env bash
set -euo pipefail
exit 1
CYGPATH
if (_oh_codex_direct "$scratch/project" "$POSIX_CODEX_HOME" 'echo marker') >"$scratch/conversion.log" 2>&1; then
  fail 'direct Codex launch must fail when its home cannot be converted'
fi
# A converter that succeeds without a path must also stop before the launch.
cat >"$scratch/bin/cygpath" <<'CYGPATH' || fail "could not write the converter fixture; check $scratch permissions"
#!/usr/bin/env bash
set -euo pipefail
exit 0
CYGPATH
if (_oh_codex_direct "$scratch/project" "$POSIX_CODEX_HOME" 'echo marker') >"$scratch/empty.log" 2>&1; then
  fail 'direct Codex launch must fail when conversion returns an empty path'
fi
# Codex keys a Windows project by its backslashed long path, folding case but
# not separators, so the trust config must name that spelling beside the
# forward-slash one the rest of the suite passes.
cat >"$scratch/bin/cygpath" <<'CYGPATH' || fail "could not write the converter fixture; check $scratch permissions"
#!/usr/bin/env bash
set -euo pipefail
case "$1" in
  -ml | -m) printf 'C:/Users/runneradmin/project\n' ;;
  -wl) printf 'C:\\Users\\runneradmin\\project\n' ;;
  *) echo "unexpected cygpath $*" >&2; exit 2 ;;
esac
CYGPATH
_oh_codex_trust_config "$scratch/project" "$scratch/config.toml" || fail "the trust config could not be written"
trust="$(cat "$scratch/config.toml")" || fail "could not read back $scratch/config.toml"
for key in "$scratch/project" 'C:/Users/runneradmin/project' 'C:\\Users\\runneradmin\\project'; do
  [ "$(printf '%s\n' "$trust" | grep -cxF "[projects.\"$key\"]")" -eq 1 ] ||
    fail "the trust config must name one [projects.\"$key\"] table, else Codex finds the project untrusted there:
$trust"
done
# A Windows spelling cygpath cannot give, failing or empty, stops the journey
# naming it rather than leaving a config that trusts the project nowhere.
for body in 'exit 1' 'exit 0'; do
  cat >"$scratch/bin/cygpath" <<CYGPATH || fail "could not write the converter fixture; check $scratch permissions"
#!/usr/bin/env bash
set -euo pipefail
case "\$1" in
  -wl) $body ;;
  *) printf 'C:/Users/runneradmin/project\n' ;;
esac
CYGPATH
  if (_oh_codex_trust_config "$scratch/project" "$scratch/config.toml") >"$scratch/trust.log" 2>&1; then
    fail "the trust config must fail when cygpath -wl does '$body'"
  fi
  grep -qF 'cygpath -wl could not convert' "$scratch/trust.log" ||
    fail "the trust config's failure must name the cygpath -wl conversion: $(cat "$scratch/trust.log")"
done
# Prints why `_oh_codex_trust_config $1 $2` failed, failing if it did not.
trust_refused() {
  if (_oh_codex_trust_config "$1" "$2") >"$scratch/trust.log" 2>&1; then
    fail "the trust config of $1 into $2 must fail"
  fi
  cat "$scratch/trust.log" || fail "could not read back $scratch/trust.log; check $scratch permissions"
}
expect_refusal() {
  printf '%s\n' "$1" | grep -qF "$2" || fail "the trust config's failure must say '$2': $1"
}
expect_refusal "$(trust_refused "$scratch/no-such-project" "$scratch/config.toml")" "cannot resolve the scratch project $scratch/no-such-project"
# A path oh_native_path cannot convert, failing or empty.
for body in 'exit 1' 'exit 0'; do
  printf '#!/usr/bin/env bash\n%s\n' "$body" >"$scratch/bin/cygpath" || fail "could not write the converter fixture; check $scratch permissions"
  expect_refusal "$(trust_refused "$scratch/project" "$scratch/config.toml")" "could not convert $scratch/project to a native path to trust"
done
rm -f "$scratch/bin/cygpath" || fail "could not remove the converter fixture; check $scratch permissions"
cat >"$scratch/bin/jq" <<'JQ' || fail "could not write the jq fixture; check $scratch permissions"
#!/usr/bin/env bash
echo 'jq: error: simulated' >&2
exit 2
JQ
chmod +x "$scratch/bin/jq" || fail "could not make the jq fixture executable; check $scratch permissions"
expect_refusal "$(trust_refused "$scratch/project" "$scratch/config.toml")" "could not render the trust config for $scratch/project"
rm -f "$scratch/bin/jq" || fail "could not remove the jq fixture; check $scratch permissions"
expect_refusal "$(trust_refused "$scratch/project" "$scratch/no-such-home/config.toml")" "cannot write the trust config $scratch/no-such-home/config.toml"
