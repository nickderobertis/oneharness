#!/usr/bin/env bash
#
# Run a command and refuse it if it left scratch directories behind.
#
# Nothing enforces the suites' scratch guards at the type level — a new test can
# still hand-roll a `create_dir_all` — and an abandoned directory is invisible
# until a host's root filesystem is full. So the suite's own run is the check.
#
# The prefix is read from `io::scratch::PREFIX` rather than copied, so the sweep
# still matches the day that constant changes.
#
# Directories only. The temp directory is shared with every other process on the
# host, including real `oneharness` runs, which write and clean up temp *files*
# of their own; reading one of those as a leak would fail the gate on someone
# else's work. No part of the product creates a temp *directory*.
#
# Silent on success: the wrapped command's output is captured and replayed only
# when that command fails or when it left scratch behind — where it is the only
# account of what the suite was doing at the time.
#
# Usage: scripts/check-temp-leaks.sh <command> [args...]
#   OH_SCRATCH_ROOTS  colon-separated roots to watch (default: "$TMPDIR:/tmp").
set -euo pipefail

if [ "$#" -eq 0 ]; then
  echo "check-temp-leaks: no command to run." >&2
  echo "  fix: pass the command to watch, as in 'scripts/check-temp-leaks.sh cargo test'." >&2
  exit 2
fi

# `/tmp` as well as `$TMPDIR`: the control tests root their sockets there
# deliberately, because a socket path is an address with a `sun_path` budget.
IFS=':' read -r -a scratch_roots <<< "${OH_SCRATCH_ROOTS:-${TMPDIR:-/tmp}:/tmp}"
# A list of nothing but separators names no root, and sweeping none of them would
# read as a clean run.
named=0
for dir in ${scratch_roots[@]+"${scratch_roots[@]}"}; do
  [ -z "$dir" ] || named=1
done
if [ "$named" -eq 0 ]; then
  echo "check-temp-leaks: OH_SCRATCH_ROOTS='${OH_SCRATCH_ROOTS-}' names no scratch root to watch." >&2
  echo "  fix: set it to colon-separated directories, or unset it to watch \$TMPDIR and /tmp." >&2
  exit 2
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
scratch_source="$repo_root/crates/oneharness-core/src/io/scratch.rs"
prefix=$(sed -n 's/^pub const PREFIX: &str = "\(.*\)";$/\1/p' "$scratch_source")
if [ -z "$prefix" ]; then
  echo "check-temp-leaks: could not read the scratch prefix from $scratch_source." >&2
  echo "  fix: restore the 'pub const PREFIX: &str = \"...\";' declaration, or point this gate at wherever it moved to." >&2
  exit 2
fi

# Each root is resolved before it is swept: `find` does not follow a symlinked
# starting point, so a root spelled through one would match nothing and report
# every leak as a clean run. That is not a hypothetical spelling — macOS hands
# it to every run for free (`/tmp` is a symlink to `/private/tmp`) and
# `just test-symlinked-tmp` reproduces it on Linux, where `$TMPDIR` is the
# symlink and the scratch space lands in the directory behind it.
# A subshell body: resolving a root must never move the gate's own directory.
resolve_root() (
  CDPATH='' cd -- "$1" 2>/dev/null && pwd -P
)

# Whether a `find` diagnostic says only that one entry directly inside `root`
# was gone by the time it was read — `find: '<root>/<name>': No such file or
# directory`, quoted as GNU spells it in the C locale or bare as BSD does.
vanished_entry() {
  local root=$1 line=$2 path name
  path=${line#find: }
  [ "$path" != "$line" ] || return 1
  path=${path%: No such file or directory}
  [ "$path" != "${line#find: }" ] || return 1
  case "$path" in "'"*"'") path=${path#"'"}; path=${path%"'"} ;; esac
  name=${path#"$root/"}
  [ "$name" != "$path" ] && [ -n "$name" ] && [ "${name#*/}" = "$name" ]
}

# A root that does not exist holds nothing to leak (and is swept afterwards if
# the command creates it), unless it existed before the command ran: one the
# command removed is a sweep that sees nothing, not a clean one. One that exists
# but cannot be entered or read — or a symlink leading nowhere — would match
# nothing and read as a clean run, so a sweep that meets one fails instead.
# Every root is settled before the sweep starts, so that refusal is this
# function's own status, never a pipeline's.
snapshot() {
  local dir real
  local -a reals=()
  for dir in "${scratch_roots[@]}"; do
    [ -n "$dir" ] || continue
    if [ ! -e "$dir" ] && [ ! -L "$dir" ]; then
      if grep -qxF -- "$dir" <<< "$present_before"; then
        echo "check-temp-leaks: cannot watch scratch root '$dir': it was removed while the command ran." >&2
        echo "  fix: leave the scratch roots in place while the command runs, then re-run." >&2
        return 2
      fi
      continue
    fi
    if ! real=$(resolve_root "$dir") || [ ! -r "$real" ]; then
      echo "check-temp-leaks: cannot watch scratch root '$dir': it exists but is not a directory this gate can enter and read." >&2
      echo "  fix: point OH_SCRATCH_ROOTS (or TMPDIR) at readable directories, then re-run." >&2
      return 2
    fi
    reals+=("$real")
  done
  # Readability is checked above, so the one failure a sweep may still meet is
  # an entry another process on this shared temp dir deleted mid-sweep, which
  # `find` reports as gone. Anything else it says — the root itself gone
  # included — means the listing is not whole, and a partial listing would read
  # as a clean run.
  local listing="" errors line find_status
  for real in ${reals[@]+"${reals[@]}"}; do
    find_status=0
    listing+=$(LC_ALL=C find "$real" -maxdepth 1 -type d -name "$prefix*" 2>"$sweep_errors")$'\n' || find_status=$?
    errors=""
    while IFS= read -r line; do
      vanished_entry "$real" "$line" || errors+="$line"$'\n'
    done <"$sweep_errors"
    # A failure that said nothing names no vanished entry to excuse it.
    if [ "$find_status" -ne 0 ] && [ ! -s "$sweep_errors" ]; then
      errors="find exited $find_status without saying why"
    fi
    if [ -n "$errors" ]; then
      echo "check-temp-leaks: sweeping scratch root '$real' failed:" >&2
      printf '%s\n' "$errors" | sed 's/^/  /' >&2
      echo "  fix: resolve the error above, then re-run." >&2
      return 2
    fi
  done
  printf '%s' "$listing" | sed '/^$/d' | sort -u
}

# The gate's own files, in one directory made up front so nothing is left to
# fail once the command has run. Its name is not a scratch name, so the sweep
# never counts it. The template names where it goes: a bare `mktemp -d` still
# succeeded on macOS with `$TMPDIR` pointing nowhere, so the command ran over a
# temp dir nothing could write to instead of being refused.
own_parent=${TMPDIR:-/tmp}
if ! own=$(mktemp -d "${own_parent%/}/check-temp-leaks.XXXXXX"); then
  echo "check-temp-leaks: could not create its own working directory under $own_parent." >&2
  echo "  fix: point TMPDIR at a writable directory with free space, then re-run." >&2
  exit 2
fi
# A working directory left behind is named with how to remove it; a command
# that failed keeps its own status.
# shellcheck disable=SC2329  # the EXIT trap below invokes it; shellcheck loses that past the script's final top-level `exit`.
remove_own() {
  local status=$?
  rm -rf "$own" && return
  echo "check-temp-leaks: could not remove its working directory $own." >&2
  echo "  fix: remove it by hand with 'rm -rf $own'." >&2
  [ "$status" -ne 0 ] || exit 1
}
trap remove_own EXIT
sweep_errors="$own/sweep-errors"
# Both streams into one file, so a replay preserves the order the command wrote
# them in rather than the order two buffers happened to flush.
transcript="$own/transcript"

present_before=""
before=$(snapshot) || exit 2
for dir in "${scratch_roots[@]}"; do
  if [ -n "$dir" ] && { [ -e "$dir" ] || [ -L "$dir" ]; }; then present_before+="$dir"$'\n'; fi
done

# Every process of this run carries the token, which is how a scratch directory's
# maker is told apart from another checkout's below.
marker_name=OH_TEMP_LEAKS_RUN
run_marker="$marker_name=$$.$RANDOM$RANDOM"
status=0
env "$run_marker" "$@" >"$transcript" 2>&1 || status=$?

unwatched=0
after=$(snapshot) || unwatched=1
leaked=$(comm -13 <(printf '%s\n' "$before") <(printf '%s\n' "$after"))

# `/tmp` is shared with every other checkout on the host, and their suites mint
# the same names, so a directory made during this run need not be this run's. A
# scratch directory ends in the id of the process that made it
# (`io::scratch::ScratchDir::name`, whose unit test pins that suffix for this
# gate; `check-scratch-prefixes.sh` holds the SDK suites' helpers to it). A
# maker still alive whose environment lacks this run's token is someone else's
# run in progress — a gate that counted it failed a publication on another
# checkout's live coverage suite. A maker that carries the token, has exited, or
# whose environment cannot be read is this run's, and its directory is a leak.
# The suffix is taken at its word, not proven: this gate catches the
# repository's own suites leaking by accident, so a name that spells another
# live process's pid on purpose would pass it.
named_for_a_live_process_lacking_the_run_marker() {
  local pid=${1##*-} environment command
  case "$pid" in '' | *[!0-9]*) return 1 ;; esac
  if [ -r "/proc/$pid/environ" ]; then
    environment=$(tr '\0' '\n' <"/proc/$pid/environ" 2>/dev/null) || return 1
  else
    # `ps e` appends the environment only where it may read it, and prints the
    # bare command otherwise, so output no longer than the command read nothing.
    environment=$(ps eww -o command= -p "$pid" 2>/dev/null) || return 1
    command=$(ps ww -o command= -p "$pid" 2>/dev/null) || return 1
    [ "${#environment}" -gt "${#command}" ] || return 1
    environment=$(tr ' ' '\n' <<< "$environment")
  fi
  [ -n "$environment" ] || return 1
  ! grep -qxF "$run_marker" <<< "$environment"
}
if [ -n "$leaked" ]; then
  kept="" left_out=""
  while IFS= read -r dir; do
    if named_for_a_live_process_lacking_the_run_marker "$dir"; then
      left_out+="${left_out:+, }$dir (pid ${dir##*-})"
    else
      kept+="$dir"$'\n'
    fi
  done <<< "$leaked"
  leaked=${kept%$'\n'}
  # Said rather than dropped, on one line: a descendant of this run that cleared
  # its own environment reads exactly like another checkout's run.
  [ -z "$left_out" ] ||
    echo "check-temp-leaks: left out as other checkouts' runs, their makers alive without this run's $marker_name: $left_out; if one is this run's own, end it and rerun" >&2
fi

if [ "$status" -ne 0 ] || [ -n "$leaked" ] || [ "$unwatched" -eq 1 ]; then
  cat "$transcript" >&2
fi

if [ -n "$leaked" ]; then
  echo "check-temp-leaks: '$1' left scratch directories behind:" >&2
  printf '%s\n' "$leaked" | sed 's/^/  /' >&2
  # Each suite owns its scratch through its own helper, told apart by the prefix
  # it declares, so the fix names the one that made it. These two assignments
  # are what `check-scratch-prefixes.sh` reads each suite's declaration against.
  node_suite="${prefix}sdk-"
  python_suite="${prefix}python-"
  names=$(printf '%s\n' "$leaked" | sed 's#.*/##')
  if grep -q "^$node_suite" <<< "$names"; then
    echo "  fix ($node_suite*): make each with scratch(), scratchSync() or controlScratch() from npm/oneharness-sdk/test/scratch.mjs, then remove what they hold: registerScratchCleanup() from scratch-hook.mjs in a test file, or process.on('exit', removeScratch) in a script such as test/package-e2e.mjs." >&2
  fi
  if grep -q "^$python_suite" <<< "$names"; then
    echo "  fix ($python_suite*): make each with scratch() or control_scratch() from python/oneharness-sdk/test/scratch.py, which registers its removal with the test case — or, outside a test case, scratch_dir() in package_e2e.py, which its ExitStack removes." >&2
  fi
  if grep -v "^$node_suite" <<< "$names" | grep -q -v "^$python_suite"; then
    echo "  fix: own each one with oneharness_core::io::scratch::ScratchDir, which removes it when the test ends — including when the test panics." >&2
  fi
  # Named either way, but a command that failed keeps its status: the leak is
  # usually a consequence of the failure, and reporting it as the outcome would
  # hide the thing to fix first.
  [ "$status" -eq 0 ] && exit 1
fi

# A root the command left unsweepable is a verdict this gate cannot give; the
# command's own failure still wins.
[ "$unwatched" -eq 1 ] && [ "$status" -eq 0 ] && exit 2

# A failed command's status is the verdict, so the gate's output ends on what to
# do about it — after any leak or unswept root, which are usually its
# consequences. One that failed silently leaves a bare exit code from a step the
# caller did not run itself, so the gate says what it ran and what came back.
if [ "$status" -ne 0 ]; then
  if [ -s "$transcript" ]; then
    echo "check-temp-leaks: '$*' exited $status; its output is replayed above." >&2
    echo "  fix: resolve the first error in that output, then rerun '$*'." >&2
  else
    echo "check-temp-leaks: '$*' exited $status without printing anything." >&2
    echo "  fix: run that command directly — this gate captured its output and there was none to replay." >&2
  fi
fi

exit "$status"
