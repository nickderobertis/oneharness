#!/usr/bin/env bash
# Publish npm package directories/tarballs idempotently. `npm pack --dry-run`
# validates each manifest and yields its canonical name@version; only a registry
# 404 permits publication, while auth/network/server errors fail closed.
set -euo pipefail

fail() {
  printf 'publish-npm: %s\n' "$1" >&2
  exit 1
}

[ "$#" -gt 0 ] || fail "pass at least one package directory or tarball"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

for package in "$@"; do
  if ! metadata="$(npm pack --dry-run --json "$package" 2>"$work/pack-error")"; then
    cat "$work/pack-error" >&2
    fail "cannot read package metadata from '$package'; rebuild the npm artifact"
  fi
  # The single-quoted program is JavaScript; its template expression is not shell.
  # shellcheck disable=SC2016
  if ! identity="$(printf '%s' "$metadata" | node -e '
    let input = "";
    process.stdin.on("data", chunk => input += chunk).on("end", () => {
      const items = JSON.parse(input);
      if (!Array.isArray(items) || items.length !== 1 ||
          typeof items[0]?.name !== "string" || typeof items[0]?.version !== "string") {
        throw new Error("npm pack did not return one name/version");
      }
      process.stdout.write(`${items[0].name}@${items[0].version}`);
    });
  ' 2>"$work/metadata-error")"; then
    cat "$work/metadata-error" >&2
    fail "npm returned invalid metadata for '$package'; rebuild the npm artifact"
  fi

  if npm view "$identity" version >/dev/null 2>"$work/view-error"; then
    :
  elif grep -Eq 'E404|404 Not Found' "$work/view-error"; then
    if ! npm publish "$package" --access public >"$work/publish-output" 2>&1; then
      cat "$work/publish-output" >&2
      fail "npm could not publish '$identity'; fix the reported authentication or package error, then retry the release"
    fi
  else
    cat "$work/view-error" >&2
    fail "cannot query '$identity'; retry when the npm registry is reachable"
  fi
done
