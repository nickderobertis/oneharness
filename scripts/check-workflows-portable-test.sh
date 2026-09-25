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
printf 'sed --expression=%q %q\n' 's/x/y/' "$work/file" >"$work/long.sh"
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
long|'--expression=s/x/y/'; use only
CASES

# Called with no script, the runner says how to call it rather than running nothing.
status=0
bash scripts/with-portable-sed.sh >"$work/out" 2>&1 || status=$?
[ "$status" = 2 ] ||
  fail "the portable-sed runner exited $status when given no script" "$(cat "$work/out")" \
    "fix: restore its argument-count check in scripts/with-portable-sed.sh"
grep -Fq "usage: scripts/with-portable-sed.sh <script>" "$work/out" ||
  fail "the portable-sed runner refused a missing script without its usage line" "$(cat "$work/out")" \
    "fix: restore that usage message in scripts/with-portable-sed.sh"
bash scripts/with-portable-sed.sh "$work/portable.sh" >"$work/out" 2>&1 ||
  fail "the portable-sed runner refused 'sed -i.bak', which both sed families read alike" "$(cat "$work/out")" \
    "fix: accept an attached -i suffix in scripts/with-portable-sed.sh"
[ "$(cat "$work/file")" = y ] ||
  fail "the portable-sed runner did not run the host's sed" "fix: check the exec at the end of its shim"

# A step that runs the runner again (this test does) must reach the real sed,
# not refuse its outer shim's probe.
printf 'x\n' >"$work/file"
printf 'bash %q %q\n' "$repo_root/scripts/with-portable-sed.sh" "$work/portable.sh" >"$work/nested.sh"
bash scripts/with-portable-sed.sh "$work/nested.sh" >"$work/out" 2>&1 ||
  fail "a nested portable-sed run refused a portable call" "$(cat "$work/out")" \
    "fix: pass the real sed to the child through PORTABLE_SED_REAL in scripts/with-portable-sed.sh"
[ "$(cat "$work/file")" = y ] ||
  fail "a nested portable-sed run did not run the host's sed" "fix: check PORTABLE_SED_REAL in scripts/with-portable-sed.sh"

# An inherited real sed that is not one is refused before any step runs.
printf 'touch %q\n' "$work/ran" >"$work/marker.sh"
printf '#!/usr/bin/env bash\nexit 0\n' >"$work/not-sed"
chmod +x "$work/not-sed"
for bad in "$work/not-sed" relative-sed "$work/missing-sed"; do
  status=0
  PORTABLE_SED_REAL="$bad" bash scripts/with-portable-sed.sh "$work/marker.sh" >"$work/out" 2>&1 || status=$?
  [ "$status" = 2 ] && [ ! -e "$work/ran" ] ||
    fail "the portable-sed runner accepted PORTABLE_SED_REAL='$bad' (exit $status)" "$(cat "$work/out")" \
      "fix: restore the working-sed check on PORTABLE_SED_REAL in scripts/with-portable-sed.sh"
  grep -Fq "is not an absolute path to a working sed" "$work/out" ||
    fail "the portable-sed runner refused PORTABLE_SED_REAL='$bad' without saying why" "$(cat "$work/out")" \
      "fix: restore that refusal's message in scripts/with-portable-sed.sh"
done

# A step failing for its own reason keeps its own exit status.
printf 'exit 3\n' >"$work/failing.sh"
status=0
bash scripts/with-portable-sed.sh "$work/failing.sh" >"$work/out" 2>&1 || status=$?
[ "$status" = 3 ] ||
  fail "the portable-sed runner turned a step's exit 3 into $status" "$(cat "$work/out")" \
    "fix: exit with the child's status in scripts/with-portable-sed.sh"
grep -Fq "failed with exit 3, with no sed call refused" "$work/out" ||
  fail "the portable-sed runner passed on a step's failure without saying it was the step's own" "$(cat "$work/out")" \
    "fix: restore the failed-step diagnostic in scripts/with-portable-sed.sh"

root="$work/crlf"
mkdir -p "$root"
while IFS= read -r file; do
  mkdir -p "$root/$(dirname "$file")"
  cp "$file" "$root/$file"
done < <(git ls-files -- scripts .github Cargo.toml crates/oneharness-core/Cargo.toml rust-toolchain.toml \
  pyproject.toml python/oneharness-sdk/pyproject.toml npm/oneharness/package.json \
  npm/oneharness-sdk/package.json justfile release-plz.toml)
release="$root/.github/workflows/release.yml"
# A Windows checkout already carries CRLF, so the fixture strips any carriage
# return before writing its own: either checkout must stage exactly CR-LF.
# Bash, tr and wc handle bytes as they are on every platform; Git for Windows'
# awk and grep may translate carriage returns themselves (on windows-latest
# this check once read none in the CR-LF file awk had written).
to_crlf() {
  local line
  while IFS= read -r line || [ -n "$line" ]; do printf '%s\r\n' "${line%$'\r'}"; done <"$1"
}
# Every line ends CR-LF: with each CR-LF pair removed, no CR or LF is left, so
# CR-CR-LF, a bare LF and a stray CR all fail, mixed or not.
is_crlf() {
  local text
  text="$(cat "$1"; printf x)"
  text="${text%x}"
  [[ "$text" == *$'\r\n' ]] || return 1
  text="${text//$'\r\n'/}"
  [[ "$text" != *[$'\r\n']* ]]
}
printf 'a\r\nb\r\n' >"$work/crlf.sample"
printf 'a\r\r\nb\r\r\n' >"$work/crcrlf.sample"
printf 'a\nb\n' >"$work/lf.sample"
printf 'a\r\r\nb\n' >"$work/mixed.sample"
printf 'a\rb\r\n' >"$work/stray.sample"
if ! is_crlf "$work/crlf.sample"; then
  fail "is_crlf above refused a CR-LF sample" "fix: accept a file whose every line ends CR-LF in is_crlf above"
fi
for sample in crcrlf lf mixed stray; do
  if is_crlf "$work/$sample.sample"; then
    fail "is_crlf above accepted the $sample sample, which is not CR-LF throughout" \
      "fix: require every line feed to follow exactly one carriage return in is_crlf above"
  fi
done
to_crlf .github/workflows/release.yml >"$work/from-lf.yml"
to_crlf "$work/from-lf.yml" >"$release"
for staged in "$work/from-lf.yml" "$release"; do
  is_crlf "$staged" ||
    fail "the CRLF release.yml fixture $staged is not exactly CR-LF (CR-CR-LF or bare LF), which no Windows checkout has" \
      "fix: strip a trailing carriage return in to_crlf above before writing CR-LF"
done
cmp -s "$work/from-lf.yml" "$release" ||
  fail "the CRLF release.yml fixture differs between an LF and a CRLF checkout" \
    "fix: make to_crlf above normalise its input to LF before writing CR-LF"

# Both scripts run from the staged tree, so they read the CRLF release.yml; a
# staged copy without the gate job must then be refused, or they read another.
for script in check-workflows.sh check-workflows-e2e.sh; do
  (cd "$root" && bash "scripts/$script") >"$work/out" 2>&1 ||
    fail "scripts/$script failed over a CRLF release.yml, as a Windows checkout has it" \
      "$(cat "$work/out")" \
      "fix: drop the carriage return in the read that anchored on the line's end, then rerun: bash scripts/check-workflows-portable-test.sh"
done

broken="$work/crlf-broken"
cp -R "$root" "$broken"
printf 'name: Release\r\n' >"$broken/.github/workflows/release.yml"
if (cd "$broken" && bash scripts/check-workflows.sh) >"$work/out" 2>&1; then
  fail "scripts/check-workflows.sh passed over a staged release.yml with no gate job, so it did not read the staged CRLF copy" \
    "fix: keep scripts/check-workflows.sh reading release.yml relative to its own checkout"
fi

echo 'check-workflows-portable-test: ok'
