#!/usr/bin/env bash
#
# Format-check, format or lint one project's shell scripts with the pinned
# shfmt and shellcheck (scripts/shell-tools.sh).
#
#   scripts/shell-check.sh format <dir>...        # shfmt -d: fail on any diff
#   scripts/shell-check.sh format-write <dir>...  # shfmt -w: rewrite in place
#   scripts/shell-check.sh lint <dir>...          # shellcheck: fail on any finding
#
# A project's shell scripts are every file under its directories that git
# tracks or would track (new, not ignored) and is shell: a `.sh` name, or a
# first line that is a sh/bash shebang — which is how `scripts/nx` and
# `.githooks/pre-push` are found without a list to keep.
#
# The shell style is recorded here, once, as shfmt's printer flags: 2-space
# indents and indented case arms. Flags rather than an .editorconfig, whose
# sections match by file name and so cannot reach an extensionless script
# found by its shebang.
#
# Quiet on success, one line; a failure keeps the tool's own report (the diff,
# or each finding with its file, line and code) and says how to fix it.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

usage() {
  echo "shell-check: usage: scripts/shell-check.sh format|format-write|lint <dir>..." >&2
  exit 2
}
[ "$#" -ge 2 ] || usage
mode="$1"
shift
case "$mode" in format | format-write | lint) ;; *) usage ;; esac
for dir in "$@"; do
  [ -d "$dir" ] || {
    echo "shell-check: '$dir' is not a directory under the repository root" >&2
    exit 2
  }
done

if ! listed="$(git ls-files --cached --others --exclude-standard -- "$@")"; then
  echo "shell-check: git could not list the files under $* (above); run this from a git checkout" >&2
  exit 1
fi
if ! sorted="$(printf '%s\n' "$listed" | sort -u)"; then
  echo "shell-check: could not sort the file list (above); fix: check that sort is on PATH, then re-run" >&2
  exit 1
fi
files=()
while IFS= read -r path; do
  [ -n "$path" ] && [ -f "$path" ] || continue
  case "$path" in
    *.sh) files+=("$path") ;;
    *)
      first="$(head -n 1 -- "$path")" || {
        echo "shell-check: could not read $path (above); fix: restore its read permission, or 'git checkout -- $path', then re-run" >&2
        exit 1
      }
      if [[ "$first" =~ ^\#!.*[/\ ](ba)?sh([[:space:]]|$) ]]; then files+=("$path"); fi
      ;;
  esac
done <<<"$sorted"
[ "${#files[@]}" -gt 0 ] || {
  echo "shell-check: found no shell scripts under $*; a project owning none declares no shell targets" >&2
  exit 1
}

tools="$root/scripts/shell-tools.sh"
style=(-i 2 -ci)
case "$mode" in
  format)
    if ! bash "$tools" exec shfmt "${style[@]}" -d -- "${files[@]}"; then
      echo "shell-check: the scripts above are not shfmt-formatted (style: shfmt ${style[*]}, in scripts/shell-check.sh); run 'just format', then re-run" >&2
      exit 1
    fi
    echo "shell-check: shfmt ok (${#files[@]} scripts under $*)"
    ;;
  format-write)
    if ! bash "$tools" exec shfmt "${style[@]}" -w -- "${files[@]}"; then
      echo "shell-check: shfmt could not rewrite the scripts above; fix the parse error it names (or make the file writable), then re-run 'just format'" >&2
      exit 1
    fi
    ;;
  lint)
    if ! bash "$tools" exec shellcheck -- "${files[@]}"; then
      echo "shell-check: shellcheck reported the findings above; fix each (or, where the code is right, disable that code at that line with its reason), then re-run" >&2
      exit 1
    fi
    echo "shell-check: shellcheck ok (${#files[@]} scripts under $*)"
    ;;
esac
