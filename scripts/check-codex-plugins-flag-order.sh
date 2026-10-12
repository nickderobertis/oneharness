#!/usr/bin/env bash
# llmlint: ignore-file[e2e_not_mocked] This is the hermetic half of a live phase: the real phase from scripts/e2e-lib.sh runs end to end, and only `codex` is a scripted stand-in, because a gate run cannot rely on codex being installed and because what is tested here is the phase's own reading of the matrix and its verdicts, including a codex that answers wrongly or crashes, which real codex cannot be made to do. Real codex is asked every row by the phase itself in the live codex suite (scripts/e2e-codex.sh).
# Hermetic coverage for oh_codex_plugins_flag_order against the rows of
# tests/fixtures/codex-plugins-flag-order.json.
set -euo pipefail

fail() {
  echo "check-codex-plugins-flag-order: $1" >&2
  echo "  rerun with 'bash -x scripts/check-codex-plugins-flag-order.sh' to inspect the failing case" >&2
  exit 1
}

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)" || fail "could not resolve the repository root from ${BASH_SOURCE[0]}"
matrix="$root/tests/fixtures/codex-plugins-flag-order.json"

tmp="$(mktemp -d)" || fail "could not create a scratch dir — check that the host temp dir is writable (df -h \"${TMPDIR:-/tmp}\")"
trap 'rm -rf "$tmp" || echo "check-codex-plugins-flag-order: could not remove $tmp; delete it by hand" >&2' EXIT

# A `codex` whose `features list` reports `plugins` as `$model` resolves it
# from its args, appending each call's CODEX_HOME and args to $tmp/calls:
#   measured  `-c` last-wins, then `--enable`, then `--disable` (real codex)
#   argv      every spelling applied in argv order, the last winning
#   silent    prints no `plugins` row at all
#   crashed   prints the measured row, then exits non-zero
write_fake_codex() {
  local model="$1" bin="$tmp/bin"
  rm -rf "$bin" "$tmp/calls" || fail "could not clear the previous case's fake codex under $tmp"
  mkdir -p "$bin" || fail "could not create $bin for the fake codex"
  cat >"$bin/codex" <<FAKE || fail "could not write the fake codex to $bin/codex"
#!/usr/bin/env bash
set -euo pipefail
[ "\$1 \$2" = "features list" ] || { echo "unexpected codex call: \$*" >&2; exit 2; }
shift 2
printf '%s|%s\n' "\${CODEX_HOME:-}" "\$*" >>"$tmp/calls" || { echo "fake codex: could not log to $tmp/calls" >&2; exit 2; }
value=true enable=false disable=false
while [ "\$#" -gt 1 ]; do
  case "\$1 \$2" in
    "-c features.plugins=true") value=true ;;
    "-c features.plugins=false") value=false ;;
    "--enable plugins") enable=true; [ "$model" = argv ] && value=true ;;
    "--disable plugins") disable=true; [ "$model" = argv ] && value=false ;;
  esac
  shift
done
if [ "$model" != argv ]; then
  [ "\$enable" = true ] && value=true
  [ "\$disable" = true ] && value=false
fi
[ "$model" = silent ] || printf 'plugins                                  stable             %s\n' "\$value"
printf 'shell_tool                               stable             true\n'
[ "$model" != crashed ] || { echo "fake codex: crashed" >&2; exit 3; }
FAKE
  chmod +x "$bin/codex" || fail "could not make $bin/codex executable"
}

# The phase's combined output in $out and its exit status in $status, run in a
# child so its own `fail` cannot end this script.
run_phase() {
  set +e
  out="$(PATH="$tmp/bin:$PATH" bash -c 'set -euo pipefail; source "$1/scripts/e2e-lib.sh"; oh_codex_plugins_flag_order' _ "$root" 2>&1)"
  status=$?
  set -e
}

rows="$(jq -e '.rows | length' "$matrix")" || fail "could not count the rows of $matrix — restore it from git"

# 1. Real codex's precedence passes, asking every row under an empty CODEX_HOME.
write_fake_codex measured
run_phase
[ "$status" -eq 0 ] || fail "the measured precedence must pass, got exit $status: $out"
case "$out" in
  *"PASS: codex resolves plugins as -c last-wins"*) ;;
  *) fail "a pass must say so, got: $out" ;;
esac
calls="$(wc -l <"$tmp/calls")" || fail "the fake codex left no call log at $tmp/calls"
calls="${calls//[[:space:]]/}"
[ "$calls" -eq "$rows" ] || fail "the phase must ask codex all $rows rows, asked $calls: $(cat "$tmp/calls")"
grep -qx '[^|]*|-c features.plugins=false --enable plugins' "$tmp/calls" ||
  fail "a row's args must reach codex as separate words, got: $(cat "$tmp/calls")"
home="$(head -n 1 "$tmp/calls")" || fail "could not read the first call from $tmp/calls"
home="${home%%|*}"
case "$home" in
  "" | "${CODEX_HOME:-<unset>}") fail "the phase must ask under its own scratch CODEX_HOME, got '$home'" ;;
esac
[ ! -e "$home" ] || fail "the scratch CODEX_HOME $home must be removed after the phase"

# 2. A codex applying plain argv order fails on the first row it decides
#    differently, naming it.
write_fake_codex argv
run_phase
[ "$status" -ne 0 ] || fail "an argv-order codex must fail the phase, got: $out"
named="\`codex features list --disable plugins --enable plugins\` left plugins=true, expected false"
case "$out" in
  *"$named"*) ;;
  *) fail "the failure must name the row and both values, got: $out" ;;
esac

# 3. A codex that reports no plugins row is unreadable, never a pass.
write_fake_codex silent
run_phase
[ "$status" -ne 0 ] || fail "a codex without a plugins row must fail the phase, got: $out"
case "$out" in
  *"left plugins=<unreadable>"*) ;;
  *) fail "an unreadable feature must say so, got: $out" ;;
esac

# 4. A codex that printed the right row and then failed answered nothing.
write_fake_codex crashed
run_phase
[ "$status" -ne 0 ] || fail "a codex exiting non-zero must fail the phase, got: $out"
case "$out" in
  *"left plugins=<unreadable>"*"codex exited 3: fake codex: crashed"*) ;;
  *) fail "a failed codex must be reported with its exit status and stderr, got: $out" ;;
esac
