#!/usr/bin/env bash
# Prove the workflow drift gate rejects a missing release-tag boundary check.
set -euo pipefail

cd "$(dirname "$0")/.."

work="$(mktemp -d)"
workflow=.github/workflows/release.yml
cp "$workflow" "$work/release.yml"
restore() {
  cp "$work/release.yml" "$workflow"
  rm -rf "$work"
}
trap restore EXIT

# The single-quoted program is JavaScript; $tag is fixture text, not shell.
# shellcheck disable=SC2016
node -e '
  const fs = require("node:fs");
  const path = process.argv[1];
  const source = fs.readFileSync(path, "utf8").replaceAll("\r\n", "\n");
  const line = "          if ! [[ \"$tag\" =~ ^v[0-9]+\\.[0-9]+\\.[0-9]+([+-][0-9A-Za-z.-]+)?$ ]]; then\n";
  if (!source.includes(line)) throw new Error("release tag validation fixture is missing");
  fs.writeFileSync(path, source.replace(line, ""));
' "$workflow"

if bash scripts/check-workflows.sh >"$work/stdout" 2>"$work/stderr"; then
  echo 'check-workflows-e2e: missing release-tag validation unexpectedly passed' >&2
  exit 1
fi
grep -Fq 'release.yml must validate release-event tags before using them in paths' "$work/stderr" || {
  echo 'check-workflows-e2e: drift failure lacked the expected diagnostic' >&2
  cat "$work/stderr" >&2
  exit 1
}

echo 'check-workflows-e2e: ok'
