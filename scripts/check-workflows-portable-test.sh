#!/usr/bin/env bash
# llmlint: ignore-file[new_code_lands_in_a_project] The rule presumes an Nx project graph; this repository has none by a recorded decision (`AGENTS.md`: root `just` delegates to Cargo/Bun without Nx because the two-package graph is static), so no project definition can cover this file and `just lint-workflows` is what runs it.
#
# Hold `just lint-workflows` to one behaviour on Linux, macOS and Windows.
#
# Two ways its scripts have answered differently per platform, each driven here
# on this host:
#   - sed: every recipe step runs through scripts/with-portable-sed.sh; this
#     holds the recipe to that and proves the runner refuses the bare `-i` macOS
#     rejected.
#   - CRLF: a Windows checkout gives release.yml CRLF endings, and native
#     Windows tools (node) keep them. The workflow gate and its mutation e2e run
#     over a staged checkout whose release.yml is CRLF.
#
# Quiet on success, one line. On failure it names the script and what it did.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

fail() {
  echo "check-workflows-portable-test: $1" >&2
  shift
  for line in "$@"; do echo "  $line" >&2; done
  exit 1
}

# Every step of the recipe runs through the portable-sed runner, or the calls it
# makes are held to nothing.
steps="$(tr -d '\r' <justfile | sed -n '/^lint-workflows:/,/^$/ { /^    @/p; }')"
[ -n "$steps" ] || fail "read no steps from the lint-workflows recipe in justfile" \
  "fix: keep the recipe's steps as '    @bash scripts/with-portable-sed.sh scripts/<name>.sh', or update the sed reading them here"
unwrapped="$(printf '%s\n' "$steps" | grep -vE '^    @(bash scripts/with-portable-sed\.sh scripts/[^ ]+\.sh >/dev/null|echo .*)$' || true)"
[ -z "$unwrapped" ] || fail "a lint-workflows step bypasses scripts/with-portable-sed.sh, so its sed calls go unchecked" \
  "$unwrapped" \
  "fix: write it as '    @bash scripts/with-portable-sed.sh scripts/<name>.sh >/dev/null'"

# The runner must refuse the call macOS rejected — also where the script
# ignores sed's exit status — and pass the portable spelling of it.
printf 'x\n' >"$work/file"
printf 'sed -i %q %q\n' 's/x/y/' "$work/file" >"$work/bare.sh"
printf 'sed -i %q %q || true\n' 's/x/y/' "$work/file" >"$work/ignored.sh"
printf 'sed -i.bak %q %q\n' 's/x/y/' "$work/file" >"$work/portable.sh"
printf 'sed %q -n %q\n' 's/x/y/' "$work/file" >"$work/late.sh"
printf 'sed -z %q %q\n' 's/x/y/' "$work/file" >"$work/unknown.sh"
while IFS='|' read -r case named; do
  if bash scripts/with-portable-sed.sh "$work/$case.sh" >"$work/out" 2>&1; then
    fail "the portable-sed runner passed $case.sh, a call BSD sed reads differently" \
      "fix: restore its refusal, and the refusal-log check, in scripts/with-portable-sed.sh"
  fi
  grep -Fq "$named" "$work/out" ||
    fail "the portable-sed runner refused $case.sh without saying '$named'" "$(cat "$work/out")" \
      "fix: restore that refusal's message in scripts/with-portable-sed.sh"
done <<'CASES'
bare|bare -i
ignored|bare -i
late|option '-n' after the script
unknown|'-z'; use only
CASES
bash scripts/with-portable-sed.sh "$work/portable.sh" >"$work/out" 2>&1 ||
  fail "the portable-sed runner refused 'sed -i.bak', which both sed families read alike" "$(cat "$work/out")" \
    "fix: accept an attached -i suffix in scripts/with-portable-sed.sh"
[ "$(cat "$work/file")" = y ] ||
  fail "the portable-sed runner did not run the host's sed" "fix: check the exec at the end of its shim"

root="$work/crlf"
mkdir -p "$root"
while IFS= read -r file; do
  mkdir -p "$root/$(dirname "$file")"
  cp "$file" "$root/$file"
done < <(git ls-files -- scripts .github Cargo.toml crates/oneharness-core/Cargo.toml rust-toolchain.toml \
  pyproject.toml python/oneharness-sdk/pyproject.toml npm/oneharness/package.json \
  npm/oneharness-sdk/package.json justfile release-plz.toml)
release="$root/.github/workflows/release.yml"
awk '{ printf "%s\r\n", $0 }' .github/workflows/release.yml >"$release"
grep -q $'\r$' "$release" || fail "could not stage a CRLF release.yml" "fix: check awk writes the carriage returns above"

for script in check-workflows.sh check-workflows-e2e.sh; do
  bash "$root/scripts/$script" >"$work/out" 2>&1 ||
    fail "scripts/$script failed over a CRLF release.yml, as a Windows checkout has it" \
      "$(cat "$work/out")" \
      "fix: drop the carriage return in the read that anchored on the line's end, then rerun: bash scripts/check-workflows-portable-test.sh"
done

echo 'check-workflows-portable-test: ok'
