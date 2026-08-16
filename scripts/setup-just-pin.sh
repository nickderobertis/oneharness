#!/usr/bin/env bash
# Read the pinned `just` out of `.tool-versions` and emit it as the setup-just
# action's `version` output line (`version=x.y.z` on stdout, which the action
# appends to `$GITHUB_OUTPUT`).
#
# `.tool-versions` is the single pin — asdf/mise read the same file for a local
# clone, so CI cannot drift to a different `just` than a developer runs — and it
# is a FILE that becomes a cache key, a workflow output, and a `cargo install`
# argument. asdf's format allows a tool to list SEVERAL versions on its line and
# to appear on several lines, so "the version" is only well defined once exactly
# one of each is proven: a reader taking the first token of the first match would
# turn `just 1.2.3 2.0.0` into a confident, wrong pin, cached under a key that
# claims otherwise.
set -euo pipefail

file="${1:-.tool-versions}"

refuse() {
  echo "::error::$file must pin just as exactly one x.y.z version for the setup-just action to install ($1)" >&2
  exit 1
}

[ -f "$file" ] || refuse "no such file"

# Every token after the tool name, on every line naming it — never just field
# two, which is what hides a second version.
lines="$(awk '$1 == "just" { $1 = ""; sub(/^[ \t]+/, ""); print }' "$file")"
case "$(printf '%s' "$lines" | grep -c . || true)" in
  1) ;;
  0) refuse "no just line" ;;
  *) refuse "$(printf '%s' "$lines" | grep -c .) just lines" ;;
esac

# Word-split deliberately: the line's tokens are what must number exactly one.
# shellcheck disable=SC2086
set -- $lines
[ "$#" -eq 1 ] || refuse "$# versions on the just line: $lines"

[[ "$1" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || refuse "found '$1'"

echo "version=$1"
