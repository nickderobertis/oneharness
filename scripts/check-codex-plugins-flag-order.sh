#!/usr/bin/env bash
# llmlint: ignore-file[e2e_not_mocked] Only `codex` is a scripted double, because the gate has no codex and these cases need one that answers wrongly or crashes on demand; real codex runs the phase in scripts/e2e-codex.sh.
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
[ "\${1:-} \${2:-}" = "features list" ] || { echo "unexpected codex call: \$*" >&2; exit 2; }
shift 2
printf '%s|%s\n' "\${CODEX_HOME:-}" "\$*" >>"$tmp/calls" || { echo "fake codex: could not log to $tmp/calls" >&2; exit 2; }
resolve() {
  local feature="\$1" value=true enable=false disable=false
  shift
  while [ "\$#" -gt 1 ]; do
    case "\$1 \$2" in
      "-c features.\$feature=true") value=true ;;
      "-c features.\$feature=false") value=false ;;
      "--enable \$feature") enable=true; [ "$model" = argv ] && value=true ;;
      "--disable \$feature") disable=true; [ "$model" = argv ] && value=false ;;
    esac
    shift
  done
  if [ "$model" != argv ]; then
    [ "\$enable" = true ] && value=true
    [ "\$disable" = true ] && value=false
  fi
  printf '%s' "\$value"
}
[ "$model" = silent ] || printf 'apps                                     stable             %s\n' "\$(resolve apps "\$@")"
[ "$model" = silent ] || printf 'plugins                                  stable             %s\n' "\$(resolve plugins "\$@")"
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
rows="${rows//$'\r'/}" # native Windows jq ends its output with CRLF

# 1. Real codex's precedence passes, asking every row under an empty CODEX_HOME.
write_fake_codex measured
run_phase
[ "$status" -eq 0 ] || fail "the measured precedence must pass, got exit $status: $out"
case "$out" in
  *"PASS: codex resolves plugins and apps as -c last-wins"*) ;;
  *) fail "a pass must say so, got: $out" ;;
esac
calls="$(wc -l <"$tmp/calls")" || fail "the fake codex left no call log at $tmp/calls"
calls="${calls//[[:space:]]/}"
[ "$calls" -eq $((rows * 2)) ] || fail "the phase must ask codex all $rows rows for plugins and for apps, asked $calls: $(cat "$tmp/calls")"
grep -qx '[^|]*|-c features.plugins=false --enable plugins' "$tmp/calls" ||
  fail "a row's args must reach codex as separate words, got: $(cat "$tmp/calls")"
grep -qx '[^|]*|-c features.apps=false --enable apps' "$tmp/calls" ||
  fail "each row must be asked again with plugins spelled apps, got: $(cat "$tmp/calls")"
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
home="$(head -n 1 "$tmp/calls")" || fail "could not read the first call from $tmp/calls"
home="${home%%|*}"
[ -n "$home" ] || fail "the failing phase must still ask under a scratch CODEX_HOME, got none"
[ ! -e "$home" ] || fail "a failing phase must remove its scratch CODEX_HOME $home too"

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

# 5. A helper's own fail() — here the stderr scratch file it cannot create —
#    still removes the scratch CODEX_HOME. The fake `mktemp` makes directories
#    (logging each) and refuses files.
write_fake_codex measured
real_mktemp="$(command -v mktemp)" || fail "no mktemp on PATH to wrap"
cat >"$tmp/bin/mktemp" <<FAKE || fail "could not write the fake mktemp to $tmp/bin/mktemp"
#!/usr/bin/env bash
set -euo pipefail
[ "\${1:-}" = -d ] || { echo "fake mktemp: refusing a file" >&2; exit 1; }
dir="\$("$real_mktemp" -d)" || { echo "fake mktemp: the real mktemp could not make a directory" >&2; exit 1; }
printf '%s\n' "\$dir" >>"$tmp/made" || { echo "fake mktemp: could not log \$dir to $tmp/made" >&2; exit 1; }
printf '%s\n' "\$dir"
FAKE
chmod +x "$tmp/bin/mktemp" || fail "could not make $tmp/bin/mktemp executable"
run_phase
[ "$status" -ne 0 ] || fail "a helper that cannot create its scratch file must fail the phase, got: $out"
case "$out" in
  *"could not create a scratch file for codex's stderr"*) ;;
  *) fail "the helper's failure must reach the output, got: $out" ;;
esac
home="$(head -n 1 "$tmp/made")" || fail "the fake mktemp made no scratch CODEX_HOME"
[ -n "$home" ] || fail "the fake mktemp logged an empty directory"
[ ! -e "$home" ] || fail "a helper's fail() must still remove the scratch CODEX_HOME $home"
