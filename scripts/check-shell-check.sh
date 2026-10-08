#!/usr/bin/env bash
#
# Behavioral test of scripts/shell-check.sh, the shell `format` and `lint`
# targets, with the real pinned shfmt and shellcheck.
#
# A staged git checkout carries the script, the pin file and one project
# directory: a `.sh` script, an extensionless bash hook, an untracked new
# script, and two files that are not shell. Each
# mode is then driven the way a project target runs it — clean, then with an
# unformatted line and a shellcheck finding — and must find exactly the shell
# files, pass the clean tree, and fail the broken one with the tool's own
# report and a fix.
#
# Quiet on success, one line. On failure it names the case and what it saw.
set -Eeuo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

fail() {
  echo "check-shell-check: $1" >&2
  shift
  for line in "$@"; do echo "  $line" >&2; done
  echo "  fix: restore that behaviour in scripts/shell-check.sh, then rerun 'bash scripts/check-shell-check.sh'" >&2
  exit 1
}

# A setup step that fails (a scratch directory, a copy, a write) says which,
# rather than ending the run on the bare error.
setup_failed() {
  # Once, from the shell the step ran in, not again from each enclosing one.
  [ "$BASH_SUBSHELL" = 0 ] || return 0
  echo "check-shell-check: \`$1\` failed at line $2 (above); fix: check that ${TMPDIR:-/tmp} and target/ are writable and have room, then rerun 'bash scripts/check-shell-check.sh'" >&2
}
trap 'setup_failed "$BASH_COMMAND" "$LINENO"' ERR

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
stage="$work/repo"
mkdir -p "$stage/scripts" "$stage/proj" "$stage/empty"
cp scripts/shell-check.sh scripts/shell-tools.sh "$stage/scripts/"
cp .shell-tool-versions "$stage/"
git -C "$stage" init --quiet
cat >"$stage/proj/tool.sh" <<'SH'
#!/usr/bin/env bash
set -euo pipefail

greet() {
  echo "hello, $1"
}
greet "$@"
SH
cat >"$stage/proj/hook" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
case "${1:-}" in
  a) echo a ;;
esac
SH
printf '#!/usr/bin/env python3\nprint("not shell")\n' >"$stage/proj/helper.py"
printf 'notes, not a script\n' >"$stage/proj/README"
printf 'empty\n' >"$stage/empty/README"
git -C "$stage" add -A
printf '#!/usr/bin/env bash\necho new\n' >"$stage/proj/new.sh"

run() {
  status=0
  (cd "$stage" && bash scripts/shell-check.sh "$@") >"$work/out" 2>&1 || status=$?
}
expect() {
  local want="$1" text="$2" case="$3"
  [ "$status" = "$want" ] || fail "$case: exited $status, not $want" "$(cat "$work/out")"
  grep -Fq -- "$text" "$work/out" || fail "$case: did not say '$text'" "$(cat "$work/out")"
}

# Clean: the two tracked shell files and the untracked one, nothing else.
run format proj
expect 0 "shell-check: shfmt ok (3 scripts under proj)" "a formatted project"
run lint proj
expect 0 "shell-check: shellcheck ok (3 scripts under proj)" "a clean project"

# An unformatted line in the extensionless hook fails `format` with shfmt's
# diff; `format-write` repairs it.
cat >"$stage/proj/hook" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
case "${1:-}" in
a) echo a ;;
esac
SH
run format proj
expect 1 "run 'just format'" "an unformatted hook"
grep -Fq -- "+  a) echo a ;;" "$work/out" || fail "an unformatted hook failed without shfmt's diff" "$(cat "$work/out")"
run format-write proj
[ "$status" = 0 ] || fail "format-write exited $status" "$(cat "$work/out")"
run format proj
expect 0 "shfmt ok" "a hook format-write rewrote"

# A shellcheck finding in the untracked script fails `lint` with the finding.
cat >"$stage/proj/new.sh" <<'SH'
#!/usr/bin/env bash
echo $1
SH
run lint proj
expect 1 "SC2086" "an unquoted expansion"
grep -Fq "proj/new.sh" "$work/out" || fail "the shellcheck finding did not name its file" "$(cat "$work/out")"
grep -Fq "shellcheck reported the findings above" "$work/out" || fail "the lint failure gave no fix" "$(cat "$work/out")"

run lint empty
expect 1 "found no shell scripts under empty" "a directory with no shell"
run lint missing
expect 2 "'missing' is not a directory" "a directory that does not exist"
run check proj
expect 2 "usage: scripts/shell-check.sh" "an unknown mode"

echo "check-shell-check: ok"
