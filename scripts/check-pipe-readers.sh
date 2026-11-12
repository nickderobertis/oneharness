#!/usr/bin/env bash
#
# Refuse a pipeline whose reader can exit before its writer finishes: `grep -q`
# (or -m, -l), `head`, or an awk program that calls `exit`. Under `set -euo
# pipefail` the writer then takes SIGPIPE, the pipeline's status is 141, and the
# check reads a match as a failure — or, as a `$(...)` assignment, dies with a
# silent 141 (check-ci-gate-tier.sh did, on a CI runner). Feed the reader a
# here-string or the file itself, let grep read to the end (`>/dev/null`, no
# -q), or take the first line with `sed -n 1p`, which reads all of its input.
#
#   scripts/check-pipe-readers.sh            # every script ci-contracts:test runs
#   scripts/check-pipe-readers.sh FILE...    # those files
#
# The default set is every `{workspaceRoot}/scripts/*.sh` input of
# tools/ci-contracts/project.json plus the shell-test runner, so a script added
# to that project's inputs is held to this without another edit. It reads one
# line at a time and skips comment lines; a pipeline split across lines is not
# seen. Quiet on success, one line. On failure it names each site and the fix.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)" || {
  echo "check-pipe-readers: could not resolve the repository root from ${BASH_SOURCE[0]}; run it from a checkout as 'bash scripts/check-pipe-readers.sh'" >&2
  exit 2
}
cd "$root" || {
  echo "check-pipe-readers: could not enter the repository root $root; check that it exists and is readable, then re-run" >&2
  exit 2
}

project="tools/ci-contracts/project.json"

if [ "$#" -gt 0 ]; then
  files=("$@")
else
  inputs="$(sed -n 's|^ *"{workspaceRoot}/\(scripts/[A-Za-z0-9_.-]*\.sh\)",*$|\1|p' "$project")" || {
    echo "check-pipe-readers: could not read $project; check that it exists and is readable, then re-run" >&2
    exit 2
  }
  files=(scripts/shell-test.sh scripts/shell-tools.sh)
  while IFS= read -r f; do
    [ -z "$f" ] || files+=("$f")
  done <<<"$inputs"
  [ "${#files[@]}" -gt 2 ] || {
    echo "check-pipe-readers: $project lists no {workspaceRoot}/scripts/*.sh input, so there is nothing to hold; restore its inputs or fix the sed in scripts/check-pipe-readers.sh" >&2
    exit 2
  }
fi

# A lone `|` (never `||`), optional space, then a reader that can stop early.
grep_early='grep([[:space:]]+[^|[:space:]]+)*[[:space:]]+(-[A-Za-z]*[qlm][A-Za-z0-9]*|--quiet|--silent|--max-count|--files-with-matches)([[:space:]=]|$)'
reader="(head([[:space:]]|$)|${grep_early}|awk[[:space:]].*[^A-Za-z_]exit([^A-Za-z_]|$))"
pattern="(^|[^|])\|[[:space:]]*${reader}"

found=0
for f in "${files[@]}"; do
  [ -f "$f" ] || {
    echo "check-pipe-readers: $f does not exist; remove it from $project's inputs or restore it" >&2
    exit 2
  }
  # grep exits 1 for no match; anything above that is a scan that did not finish.
  status=0
  hits="$(grep -nE "$pattern" "$f")" || status=$?
  [ "$status" -le 1 ] || {
    echo "check-pipe-readers: grep could not scan $f (exit $status, above); check that it is readable, then re-run" >&2
    exit 2
  }
  while IFS= read -r hit; do
    [ -n "$hit" ] || continue
    [[ "$hit" =~ ^[0-9]+:[[:space:]]*# ]] && continue
    echo "check-pipe-readers: $f:$hit" >&2
    found=1
  done <<<"$hits"
done

if [ "$found" -ne 0 ]; then
  echo "  Next: each line above pipes into a reader that can exit before its writer finishes, which pipefail turns into exit 141; feed it a here-string (<<<\"\$var\") or the file, let grep read to the end (>/dev/null instead of -q), or use 'sed -n 1p' for head, then re-run 'bash scripts/check-pipe-readers.sh'." >&2
  exit 1
fi

echo "check-pipe-readers: ok"
