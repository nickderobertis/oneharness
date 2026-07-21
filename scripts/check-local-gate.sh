#!/usr/bin/env bash
# Hermetic behavioral check for the local llmlint gate and comparison-base helper.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
source "$root/scripts/local-llmlint-gate-lib.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/bin"
log="$tmp/calls"
primary_harness=$(llmlint_primary_harness "$root/oneharness.toml")

assert_file_contains() {
  local expected=$1 file=$2 description=$3
  grep -Fxq "$expected" "$file" || {
    echo "check-local-gate: $description; expected '$expected' in $file" >&2
    return 1
  }
}

assert_line_count() {
  local expected=$1 file=$2 description=$3 actual
  actual=$(wc -l < "$file")
  [[ $actual -eq $expected ]] || {
    echo "check-local-gate: $description; expected $expected lines in $file, found $actual" >&2
    return 1
  }
}

cat > "$tmp/bin/llmlint" <<'STUB'
#!/usr/bin/env bash
printf 'llmlint %s\n' "$*" >> "$CALL_LOG"
STUB
cat > "$tmp/bin/$primary_harness" <<'STUB'
#!/usr/bin/env bash
read -r key
[[ $key == test-key ]]
printf '%s %s\n' "$(basename "$0")" "$*" >> "$CALL_LOG"
STUB
chmod +x "$tmp/bin/llmlint" "$tmp/bin/$primary_harness"

if "$root/scripts/local-llmlint-gate.sh" invalid-comparison-ref 2>"$tmp/invalid-ref"; then
  echo "check-local-gate: invalid comparison ref unexpectedly succeeded" >&2
  exit 1
else
  status=$?
fi
[[ $status -eq 2 ]] || {
  echo "check-local-gate: invalid comparison ref exited $status, expected 2" >&2
  exit 1
}
assert_file_contains \
  "llmlint: 'invalid-comparison-ref' is not an existing commit; fetch it or pass a valid comparison ref" \
  "$tmp/invalid-ref" "missing invalid comparison ref diagnostic"

fixture_root="$tmp/local-gate-fixture"
mkdir -p "$fixture_root/scripts"
cp "$root/scripts/local-llmlint-gate.sh" "$root/scripts/local-llmlint-gate-lib.sh" \
  "$fixture_root/scripts/"
printf 'harnesses = ["../invalid"]\n' > "$fixture_root/oneharness.toml"
if "$fixture_root/scripts/local-llmlint-gate.sh" HEAD 2>"$tmp/invalid-harness"; then
  echo "check-local-gate: invalid primary harness unexpectedly succeeded" >&2
  exit 1
else
  status=$?
fi
[[ $status -eq 2 ]] || {
  echo "check-local-gate: invalid primary harness exited $status, expected 2" >&2
  exit 1
}
assert_file_contains \
  "llmlint: oneharness.toml must declare a valid first harness in 'harnesses'" \
  "$tmp/invalid-harness" "missing invalid primary harness diagnostic"

printf 'harnesses = ["fixture-harness"]\n' > "$fixture_root/oneharness.toml"
CALL_LOG="$log" PATH="$tmp/bin:$PATH" HOME="$tmp/home" OPENAI_API_KEY=test-key \
  "$fixture_root/scripts/local-llmlint-gate.sh" HEAD 2>"$tmp/unavailable-harness"
assert_file_contains \
  "llmlint: judge skipped locally (committed primary harness 'fixture-harness' unavailable)" \
  "$tmp/unavailable-harness" "missing unavailable primary harness diagnostic"
: > "$log"

CALL_LOG="$log" PATH="$tmp/bin:$PATH" HOME="$tmp/home" \
  "$root/scripts/local-llmlint-gate.sh" HEAD 2>"$tmp/skip"
assert_file_contains 'llmlint: judge skipped locally (OPENAI_API_KEY unavailable)' "$tmp/skip" \
  "missing no-key skip diagnostic"
assert_line_count 1 "$log" "no-key path should only validate"

CALL_LOG="$log" PATH="$tmp/bin:$PATH" HOME="$tmp/home" OPENAI_API_KEY=test-key \
  "$root/scripts/local-llmlint-gate.sh" HEAD
assert_file_contains "$primary_harness login --with-api-key" "$log" \
  "primary harness login was not invoked"
assert_file_contains 'llmlint --diff --diff-base HEAD' "$log" \
  "llmlint judge was not invoked with the comparison ref"

git init -q --bare "$tmp/remote.git"
git init -q -b main "$tmp/repo"
git -C "$tmp/repo" config user.email test@example.com
git -C "$tmp/repo" config user.name Test
git -C "$tmp/repo" commit --allow-empty -qm initial
git -C "$tmp/repo" remote add origin "$tmp/remote.git"
git -C "$tmp/repo" push -qu origin main
git -C "$tmp/repo" remote set-head origin main
resolved_base=$(cd "$tmp/repo" && "$root/scripts/comparison-base.sh" origin)
[[ $resolved_base == origin/main ]] || {
  echo "check-local-gate: comparison base discovery returned '$resolved_base', expected 'origin/main'" >&2
  exit 1
}

cat > "$tmp/bin/just" <<'STUB'
#!/usr/bin/env bash
[[ -z ${GIT_DIR:-} ]]
printf 'just %s\n' "$*" >> "$CALL_LOG"
STUB
chmod +x "$tmp/bin/just"
CALL_LOG="$log" PATH="$tmp/bin:$PATH" GIT_DIR=.git \
  ONEHARNESS_COMPARISON_BASE=main "$root/.githooks/pre-push" upstream
assert_file_contains 'just gate upstream main' "$log" \
  "pre-push did not forward the remote and configured comparison base"

# Exercise the real bootstrap recipe from an isolated checkout. Only external
# package/tool commands are stubbed; setup-llmlint.sh and Git's hooksPath write
# cross their real process and repository boundaries.
bootstrap_repo="$tmp/bootstrap-repo"
bootstrap_bin="$tmp/bootstrap-bin"
mkdir -p "$bootstrap_repo/scripts" "$bootstrap_repo/npm/oneharness-sdk" "$bootstrap_bin"
cp "$root/justfile" "$bootstrap_repo/justfile"
cp "$root/scripts/setup-llmlint.sh" "$bootstrap_repo/scripts/setup-llmlint.sh"
git -C "$bootstrap_repo" init -q
for tool in rustup cargo bun uv; do
  cat > "$bootstrap_bin/$tool" <<'STUB'
#!/usr/bin/env bash
printf '%s %s\n' "$(basename "$0")" "$*" >> "$CALL_LOG"
STUB
  chmod +x "$bootstrap_bin/$tool"
done
ln -s "$(command -v just)" "$bootstrap_bin/just"
CALL_LOG="$log" PATH="$bootstrap_bin:$PATH" HOME="$tmp/home" \
  just --justfile "$bootstrap_repo/justfile" --working-directory "$bootstrap_repo" bootstrap >/dev/null
assert_file_contains 'uv tool install --upgrade llmlint-cli>=0.3.17' "$log" \
  "bootstrap did not run the llmlint installer"
hooks_path=$(git -C "$bootstrap_repo" config --local --get core.hooksPath)
[[ $hooks_path == .githooks ]] || {
  echo "check-local-gate: bootstrap installed hooksPath '$hooks_path', expected '.githooks'" >&2
  exit 1
}
