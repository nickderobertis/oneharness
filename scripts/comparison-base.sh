#!/usr/bin/env bash
# Resolve the remote-tracking ref used for merge-base-scoped local linting.
set -euo pipefail

remote=${1:-origin}
base=${2:-}

git check-ref-format --allow-onelevel "refs/remotes/$remote" >/dev/null 2>&1 || {
  echo "comparison-base: '$remote' is not a valid remote name" >&2; exit 2;
}
git remote get-url "$remote" >/dev/null 2>&1 || {
  echo "comparison-base: remote '$remote' does not exist; pass a configured remote" >&2; exit 2;
}
if [[ -z $base ]]; then
  symbolic=$(git symbolic-ref --quiet --short "refs/remotes/$remote/HEAD" 2>/dev/null || true)
  if [[ $symbolic == "$remote/"* ]] && git show-ref --verify --quiet "refs/remotes/$symbolic"; then
    base=${symbolic#"$remote/"}
  else
    current=$(git symbolic-ref --quiet --short HEAD 2>/dev/null || true)
    upstream_remote=$(git config --get "branch.$current.remote" 2>/dev/null || true)
    upstream_merge=$(git config --get "branch.$current.merge" 2>/dev/null || true)
    candidate=${upstream_merge#refs/heads/}
    if [[ $upstream_remote == "$remote" && -n $candidate ]] &&
      git show-ref --verify --quiet "refs/remotes/$remote/$candidate"; then
      base=$candidate
    fi
  fi
fi
if [[ -z $base ]]; then
  echo "comparison-base: cannot discover the base for '$remote'; pass it explicitly: just gate $remote <branch>" >&2
  exit 2
fi
git check-ref-format --branch "$base" >/dev/null 2>&1 || {
  echo "comparison-base: '$base' is not a valid branch name" >&2; exit 2;
}
ref="$remote/$base"
git show-ref --verify --quiet "refs/remotes/$ref" || {
  echo "comparison-base: '$ref' is missing; fetch '$remote' or choose an existing base" >&2; exit 2;
}
printf '%s\n' "$ref"
