#!/usr/bin/env bash
# Validate the squash commit subject that release-plz will consume.
set -euo pipefail

title="${PR_TITLE:-}"
pattern='^(feat|fix|perf|refactor|docs|test|chore|ci|build|style|revert)(\([^)]+\))?(!)?: .+'
if ! [[ "$title" =~ $pattern ]]; then
  echo "PR title must be a Conventional Commit, for example 'fix: validate release manifests'" >&2
  exit 1
fi
