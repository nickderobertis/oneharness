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
if [[ ${1:-} == --diff && ${JUDGE_FAIL:-0} == 1 ]]; then
  echo 'judge found an issue' >&2
  exit 4
fi
STUB
cat > "$tmp/bin/oneharness" <<'STUB'
#!/usr/bin/env bash
printf 'oneharness %s\n' "$*" >> "$CALL_LOG"
case ${PROBE_MODE:-available} in
  available)
    echo '{"fallback":{"ran":"codex"}}'
    exit 0
    ;;
  unavailable)
    echo '{"fallback":{"ran":null}}'
    exit 1
    ;;
  error)
    echo '{"fallback":{"ran":"codex"}}'
    echo 'oneharness: fallback harness ran but did not succeed' >&2
    exit 1
    ;;
  invalid)
    echo 'not-json'
    exit 0
    ;;
esac
STUB
cat > "$tmp/bin/$primary_harness" <<'STUB'
#!/usr/bin/env bash
read -r key
[[ $key == test-key ]]
printf '%s %s\n' "$(basename "$0")" "$*" >> "$CALL_LOG"
STUB
chmod +x "$tmp/bin/llmlint" "$tmp/bin/oneharness" "$tmp/bin/$primary_harness"

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
  "$root/scripts/local-llmlint-gate.sh" HEAD
assert_file_contains \
  "oneharness run --config $root/oneharness.toml --compact --prompt Reply with exactly: available" \
  "$log" \
  "no-key path did not probe the configured fallback"
assert_file_contains 'llmlint --diff --diff-base HEAD' "$log" \
  "no-key path did not invoke the judge after a successful probe"
: > "$log"

CALL_LOG="$log" PATH="$tmp/bin:$PATH" HOME="$tmp/home" PROBE_MODE=unavailable \
  "$root/scripts/local-llmlint-gate.sh" HEAD 2>"$tmp/skip"
assert_file_contains \
  'llmlint: judge skipped locally (no configured harness is available and authenticated)' \
  "$tmp/skip" "missing unavailable fallback diagnostic"
assert_line_count 2 "$log" "unavailable path should validate and probe without judging"
: > "$log"

if CALL_LOG="$log" PATH="$tmp/bin:$PATH" HOME="$tmp/home" PROBE_MODE=error \
  "$root/scripts/local-llmlint-gate.sh" HEAD 2>"$tmp/probe-error"; then
  echo "check-local-gate: genuine probe error unexpectedly succeeded" >&2
  exit 1
else
  status=$?
fi
[[ $status -eq 1 ]] || {
  echo "check-local-gate: genuine probe error exited $status, expected 1" >&2
  exit 1
}
assert_file_contains \
  "llmlint: oneharness availability probe failed; check harness authentication and run 'oneharness run --config oneharness.toml --prompt test'" \
  "$tmp/probe-error" \
  "genuine probe error was treated as an unavailable skip"
: > "$log"

if CALL_LOG="$log" PATH="$tmp/bin:$PATH" HOME="$tmp/home" PROBE_MODE=invalid \
  "$root/scripts/local-llmlint-gate.sh" HEAD 2>"$tmp/invalid-report"; then
  echo "check-local-gate: invalid probe report unexpectedly succeeded" >&2
  exit 1
fi
assert_file_contains \
  "llmlint: oneharness availability probe returned an invalid report; run 'oneharness run --config oneharness.toml --prompt test' to diagnose" \
  "$tmp/invalid-report" "invalid probe report was not rejected"
: > "$log"

mv "$tmp/bin/oneharness" "$tmp/oneharness"
CALL_LOG="$log" PATH="$tmp/bin:/usr/bin:/bin" HOME="$tmp/home" \
  "$root/scripts/local-llmlint-gate.sh" HEAD 2>"$tmp/no-oneharness"
assert_file_contains 'llmlint: judge skipped locally (oneharness unavailable)' \
  "$tmp/no-oneharness" "missing oneharness did not skip clearly"
mv "$tmp/oneharness" "$tmp/bin/oneharness"
: > "$log"

if (cd "$tmp" && PATH="bin:/usr/bin:/bin" llmlint_judge_available "$root/oneharness.toml") \
  2>"$tmp/relative-oneharness"; then
  echo "check-local-gate: relative oneharness path unexpectedly succeeded" >&2
  exit 1
fi
assert_file_contains \
  "llmlint: resolved oneharness path is not an absolute executable: bin/oneharness; fix PATH or run 'just setup-llmlint'" \
  "$tmp/relative-oneharness" "relative oneharness path was not rejected"

if PATH="$tmp/bin" llmlint_judge_available "$root/oneharness.toml" 2>"$tmp/no-jq"; then
  echo "check-local-gate: missing jq unexpectedly succeeded" >&2
  exit 1
fi
assert_file_contains \
  'llmlint: jq is required to validate the oneharness availability probe; install jq and retry' \
  "$tmp/no-jq" "missing jq was not rejected clearly"

if CALL_LOG="$log" PATH="$tmp/bin:$PATH" HOME="$tmp/home" JUDGE_FAIL=1 \
  "$root/scripts/local-llmlint-gate.sh" HEAD 2>"$tmp/judge-error"; then
  echo "check-local-gate: judge finding unexpectedly succeeded" >&2
  exit 1
else
  status=$?
fi
[[ $status -eq 4 ]] || {
  echo "check-local-gate: judge finding exited $status, expected 4" >&2
  exit 1
}
: > "$log"

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
