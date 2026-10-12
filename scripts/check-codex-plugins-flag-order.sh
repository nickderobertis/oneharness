#!/usr/bin/env bash
# Hermetic coverage for oh_codex_plugins_flag_order (scripts/e2e-lib.sh), the
# live phase that asks real codex every row of
# tests/fixtures/codex-plugins-flag-order.json. No real codex runs: each case
# puts a scripted `codex` on PATH and drives the real phase end to end, so the
# matrix it reads, the argv it builds from each row and the verdict it reaches
# are held in the gate rather than first exercised on a paid live runner.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
matrix="$root/tests/fixtures/codex-plugins-flag-order.json"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

fail() {
  echo "check-codex-plugins-flag-order: $1" >&2
  echo "  rerun with 'bash -x scripts/check-codex-plugins-flag-order.sh' to inspect the failing case" >&2
  exit 1
}

# A `codex` whose `features list` reports `plugins` as `$model` resolves it
# from its args, appending each call's CODEX_HOME and args to $tmp/calls:
#   measured  `-c` last-wins, then `--enable`, then `--disable` (real codex)
#   argv      every spelling applied in argv order, the last winning
#   silent    prints no `plugins` row at all
write_fake_codex() {
  local model="$1" bin="$tmp/bin"
  rm -rf "$bin" "$tmp/calls"
  mkdir -p "$bin"
  cat >"$bin/codex" <<FAKE
#!/usr/bin/env bash
set -euo pipefail
[ "\$1 \$2" = "features list" ] || { echo "unexpected codex call: \$*" >&2; exit 2; }
shift 2
printf '%s|%s\n' "\${CODEX_HOME:-}" "\$*" >>"$tmp/calls"
value=true enable=false disable=false
while [ \$# -gt 1 ]; do
  case "\$1 \$2" in
    "-c features.plugins=true") value=true ;;
    "-c features.plugins=false") value=false ;;
    "--enable plugins") enable=true; [ "$model" = argv ] && value=true ;;
    "--disable plugins") disable=true; [ "$model" = argv ] && value=false ;;
  esac
  shift
done
if [ "$model" = measured ]; then
  [ "\$enable" = true ] && value=true
  [ "\$disable" = true ] && value=false
fi
[ "$model" = silent ] || printf 'plugins                                  stable             %s\n' "\$value"
printf 'shell_tool                               stable             true\n'
FAKE
  chmod +x "$bin/codex"
}

# The phase's combined output in $out and its exit status in $status, run in a
# child so its own `fail` cannot end this script.
run_phase() {
  set +e
  out="$(PATH="$tmp/bin:$PATH" bash -c 'set -euo pipefail; source "$1/scripts/e2e-lib.sh"; oh_codex_plugins_flag_order' _ "$root" 2>&1)"
  status=$?
  set -e
}

rows="$(jq -e '.rows | length' "$matrix")" || fail "could not count the rows of $matrix"

# 1. Real codex's precedence passes, asking every row under an empty CODEX_HOME.
write_fake_codex measured
run_phase
[ "$status" -eq 0 ] || fail "the measured precedence must pass, got exit $status: $out"
case "$out" in
  *"PASS: codex resolves plugins as -c last-wins"*) ;;
  *) fail "a pass must say so, got: $out" ;;
esac
calls="$(wc -l <"$tmp/calls" | tr -d ' ')"
[ "$calls" -eq "$rows" ] || fail "the phase must ask codex all $rows rows, asked $calls: $(cat "$tmp/calls")"
grep -qx '[^|]*|-c features.plugins=false --enable plugins' "$tmp/calls" ||
  fail "a row's args must reach codex as separate words, got: $(cat "$tmp/calls")"
home="$(head -n 1 "$tmp/calls" | cut -d'|' -f1)"
case "$home" in
  "" | "${CODEX_HOME:-<unset>}") fail "the phase must ask under its own scratch CODEX_HOME, got '$home'" ;;
esac
[ ! -e "$home" ] || fail "the scratch CODEX_HOME $home must be removed after the phase"

# 2. A codex applying plain argv order — the precedence oneharness first
#    assumed — fails on the first row it decides differently, naming it.
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
