#!/usr/bin/env bash
#
# Run one shell test step of a project's `test` target, under kcov on Linux so
# the shell line-coverage floor can read what it executed.
#
#   scripts/shell-test.sh <project> <script> [args...]
#   scripts/shell-test.sh --id <script> [args...]    # print the step's run id
#
# On Linux the step runs under `kcov ... <out> <script> [args...]`, writing its
# report to target/coverage/shell/<project>/<run id>/ — the project's declared
# `test` output, which scripts/shell-coverage.sh merges with every other
# project's. kcov traces every bash process the step starts, so the scripts a
# test drives count, not only the test itself; it records only files under
# scripts/ and .githooks/. The run id is the last `.sh` argument's name plus
# the arguments after it (`with-portable-sed.sh scripts/check-x.sh` is
# `check-x`, `smoke.sh --install` is `smoke-install`), so one target's steps
# never share a directory and shell-coverage.sh can name a step that left none.
#
# Elsewhere the step runs as plain `bash <script> [args...]`: kcov is built
# only on Linux (scripts/shell-tools.sh), where the floor is measured, and the
# tests themselves still run on every platform.
#
# The step's own exit status is this script's; it prints nothing of its own.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

usage() {
  echo "shell-test: usage: scripts/shell-test.sh <project> <script> [args...] | --id <script> [args...]" >&2
  exit 2
}

run_id() {
  local id="" arg seen=0
  for arg in "$@"; do
    case "$arg" in
      *.sh)
        id="$(basename "$arg" .sh)"
        seen=1
        ;;
      *)
        [ "$seen" = 0 ] && continue
        arg="${arg#"${arg%%[!-]*}"}"
        id+="-${arg//[^A-Za-z0-9_-]/_}"
        ;;
    esac
  done
  [ -n "$id" ] || {
    echo "shell-test: no .sh script among '$*' to name the run after" >&2
    exit 2
  }
  printf '%s\n' "$id"
}

[ "$#" -ge 2 ] || usage
# Under kcov (below): replace its BASH_ENV helper with ours, then run the step.
# BASH_XTRACEFD stops being exported (never unset, which would close kcov's
# descriptor): each bash sets it for itself in shell-trace-env.sh, because one
# inheriting it where the descriptor was closed warns on stderr at startup.
if [ "$1" = --traced ]; then
  shift
  export -n BASH_XTRACEFD
  export BASH_ENV="$root/scripts/shell-trace-env.sh"
  exec bash "$@"
fi
if [ "$1" = --id ]; then
  shift
  run_id "$@"
  exit 0
fi
project="$1"
shift
[[ "$project" =~ ^[a-z0-9-]+$ ]] || {
  echo "shell-test: '$project' is not an Nx project name" >&2
  exit 2
}
script="$1"
# The step must physically be one of scripts/'s own files: no `..`, and no
# link out of the directory kcov measures.
if [[ "$script" == scripts/*.sh && "$script" != */../* && -f "$script" ]] &&
  [ "$(cd "$(dirname "$script")" && pwd -P)" = "$(cd scripts && pwd -P)" ] && [ ! -L "$script" ]; then
  :
else
  echo "shell-test: '$script' is not a script in scripts/; name the step as scripts/<name>.sh" >&2
  exit 2
fi

if [ "$(uname -s)" != Linux ]; then
  exec bash "$@"
fi

id="$(run_id "$@")"
out="$root/target/coverage/shell/$project/$id"
if ! { rm -rf "$out" && mkdir -p "$out"; }; then
  echo "shell-test: could not prepare the report directory $out (above); fix: check that target/ is writable and its disk has room, then rerun the step" >&2
  exit 1
fi
kcov="$(bash scripts/shell-tools.sh path kcov)"
# --bash-dont-parse-binary-dir: record the files this step ran, not every
# script beside it at 0% — shell-coverage.sh adds the full set itself, parsed
# from this tree, so a step whose cached report predates an unrelated script
# cannot restate that script's lines. kcov runs this script again as the step's
# first process (`--traced`), which hands every bash below it
# scripts/shell-trace-env.sh rather than kcov's helper (that file says why).
exec "$kcov" --bash-dont-parse-binary-dir \
  "--bash-parser=$(command -v bash)" \
  "--include-path=$root/scripts,$root/.githooks" \
  "$out" "$root/scripts/shell-test.sh" --traced "$@"
