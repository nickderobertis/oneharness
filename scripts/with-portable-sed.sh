#!/usr/bin/env bash
# llmlint: ignore-file[new_code_lands_in_a_project] The rule presumes an Nx project graph; this repository has none by a recorded decision (`AGENTS.md`: root `just` delegates to Cargo/Bun without Nx because the two-package graph is static), so no project definition can cover this file and `just lint-workflows` is what runs it.
#
# Run one bash script with a `sed` on PATH that holds it to what BSD sed
# (macOS) and GNU sed (Linux, Git Bash) read alike, so a GNU-only call fails on
# every platform rather than only on macOS.
#
#   scripts/with-portable-sed.sh <script> [args...]
#
# The shim refuses a bare `-i` (BSD reads the next argument as the backup
# suffix), a long option, an option after the script, and any other flag; it
# then runs the host's sed, in POSIX mode where that sed is GNU. A refused call
# fails the run even when the script ignored sed's exit status.
set -euo pipefail

[ "$#" -ge 1 ] || {
  echo "with-portable-sed: usage: scripts/with-portable-sed.sh <script> [args...]" >&2
  exit 2
}

# A nested run keeps the outer run's real sed rather than probing its shim.
real_sed="${PORTABLE_SED_REAL:-$(command -v sed)}"
posix=""
if "$real_sed" --posix -n p </dev/null >/dev/null 2>&1; then
  posix="--posix"
fi

shim="$(mktemp -d)"
trap 'rm -rf "$shim"' EXIT
refusals="$shim/refusals"
: >"$refusals"
{
  echo '#!/usr/bin/env bash'
  printf 'real=%q\nrefusals=%q\nposix=%q\n' "$real_sed" "$refusals" "$posix"
  cat <<'SHIM'
refuse() {
  printf 'sed call BSD and GNU sed read differently: %s\n' "$1" | tee -a "$refusals" >&2
  exit 2
}
operand=0
value=0
for arg in "$@"; do
  if [ "$value" = 1 ]; then
    value=0
    continue
  fi
  case "$arg" in
    -*) [ "$operand" = 0 ] || refuse "option '$arg' after the script; BSD sed stops reading options at the first operand" ;;
    *)
      operand=1
      continue
      ;;
  esac
  case "$arg" in
    -i) refuse "bare -i; BSD sed reads the next argument as the backup suffix, so write -i.bak or redirect to a new file" ;;
    -i?*) ;;
    -e | -f) value=1 ;;
    -n | -E | -nE | -En) ;;
    *) refuse "'$arg'; use only -n, -E, -e, -f and -i<suffix>, which both read alike" ;;
  esac
done
if [ -n "$posix" ]; then
  exec "$real" "$posix" "$@"
fi
exec "$real" "$@"
SHIM
} >"$shim/sed"
chmod +x "$shim/sed"

status=0
PATH="$shim:$PATH" PORTABLE_SED_REAL="$real_sed" bash "$@" || status=$?
if [ -s "$refusals" ]; then
  echo "with-portable-sed: $1 made a sed call macOS would refuse or read differently:" >&2
  sed 's/^/  /' "$refusals" >&2
  echo "  fix: rewrite that call portably, then rerun: scripts/with-portable-sed.sh $1" >&2
  [ "$status" -ne 0 ] || status=1
elif [ "$status" -ne 0 ]; then
  echo "with-portable-sed: $1 failed with exit $status, with no sed call refused; its own output says why. Next: rerun it alone with 'bash -x $1'" >&2
fi
exit "$status"
