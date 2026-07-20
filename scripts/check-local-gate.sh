#!/usr/bin/env bash
# Hermetic behavioral check for the local llmlint gate and comparison-base helper.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/bin"
log="$tmp/calls"
primary_harness=$(sed -nE 's/^harnesses = \["([a-z0-9][a-z0-9-]*)".*$/\1/p' "$root/oneharness.toml")

fail() {
  echo "check-local-gate: $*" >&2
  exit 1
}

assert_log_line() {
  local expected=$1
  grep -Fxq "$expected" "$log" \
    || fail "expected call '$expected' in $log; inspect the recorded local-gate calls"
}

cat > "$tmp/bin/llmlint" <<'STUB'
#!/usr/bin/env bash
printf 'llmlint %s\n' "$*" >> "$CALL_LOG"
STUB
[[ -n $primary_harness ]] \
  || fail "could not read the primary harness from $root/oneharness.toml"
cat > "$tmp/bin/$primary_harness" <<'STUB'
#!/usr/bin/env bash
read -r key
[[ $key == test-key ]]
printf '%s %s\n' "$(basename "$0")" "$*" >> "$CALL_LOG"
STUB
chmod +x "$tmp/bin/llmlint" "$tmp/bin/$primary_harness"

CALL_LOG="$log" PATH="$tmp/bin:$PATH" HOME="$tmp/home" \
  "$root/scripts/local-llmlint-gate.sh" origin/main 2>"$tmp/skip"
grep -Fq 'judge skipped locally (OPENAI_API_KEY unavailable)' "$tmp/skip" \
  || fail "missing no-API-key skip diagnostic; inspect $tmp/skip"
[[ $(wc -l < "$log") -eq 1 ]] \
  || fail "no-API-key run should only validate once; inspect $log"

CALL_LOG="$log" PATH="$tmp/bin:$PATH" HOME="$tmp/home" OPENAI_API_KEY=test-key \
  "$root/scripts/local-llmlint-gate.sh" origin/main
assert_log_line "$primary_harness login --with-api-key"
assert_log_line 'llmlint --diff --diff-base origin/main'

# Exercise config-boundary failures from an isolated copy so the committed
# authoritative config remains untouched.
gate_case="$tmp/gate-case"
mkdir -p "$gate_case/scripts"
cp "$root/scripts/local-llmlint-gate.sh" "$gate_case/scripts/local-llmlint-gate.sh"
printf 'harnesses = []\n' > "$gate_case/oneharness.toml"
if CALL_LOG="$log" PATH="$tmp/bin:$PATH" HOME="$tmp/home" \
  "$gate_case/scripts/local-llmlint-gate.sh" origin/main 2>"$tmp/bad-config"; then
  fail "malformed primary-harness config should fail; inspect $gate_case/oneharness.toml"
fi
grep -Fq 'fix its harnesses = ["<primary>", ...] entry' "$tmp/bad-config" \
  || fail "malformed config diagnostic lacks a recovery action; inspect $tmp/bad-config"

printf 'harnesses = ["missing-primary"]\n' > "$gate_case/oneharness.toml"
CALL_LOG="$log" PATH="$tmp/bin:$PATH" HOME="$tmp/home" OPENAI_API_KEY=test-key \
  "$gate_case/scripts/local-llmlint-gate.sh" origin/main 2>"$tmp/missing-primary"
grep -Fq "committed primary harness 'missing-primary' unavailable" "$tmp/missing-primary" \
  || fail "missing-primary run lacked its configured-harness diagnostic; inspect $tmp/missing-primary"

git init -q --bare "$tmp/remote.git"
git init -q -b main "$tmp/repo"
git -C "$tmp/repo" config user.email test@example.com
git -C "$tmp/repo" config user.name Test
git -C "$tmp/repo" commit --allow-empty -qm initial
git -C "$tmp/repo" remote add origin "$tmp/remote.git"
git -C "$tmp/repo" push -qu origin main
git -C "$tmp/repo" remote set-head origin main
[[ $(cd "$tmp/repo" && "$root/scripts/comparison-base.sh" origin) == origin/main ]] \
  || fail "comparison-base did not resolve origin/main; inspect the temporary repository"

cat > "$tmp/bin/just" <<'STUB'
#!/usr/bin/env bash
[[ -z ${GIT_DIR:-} ]]
printf 'just %s\n' "$*" >> "$CALL_LOG"
STUB
chmod +x "$tmp/bin/just"
CALL_LOG="$log" PATH="$tmp/bin:$PATH" GIT_DIR=.git \
  ONEHARNESS_COMPARISON_BASE=main "$root/.githooks/pre-push" upstream
assert_log_line 'just gate upstream main'

# Drive the clean-clone bootstrap recipe with hermetic prerequisite doubles. This
# proves both setup-llmlint delivery and the installed hooksPath through the same
# public command contributors run.
bootstrap="$tmp/bootstrap"
mkdir -p "$bootstrap/scripts" "$bootstrap/.githooks"
cp "$root/justfile" "$bootstrap/justfile"
cp "$root/.githooks/pre-push" "$bootstrap/.githooks/pre-push"
cat > "$bootstrap/scripts/setup-llmlint.sh" <<'STUB'
#!/usr/bin/env bash
printf 'setup-llmlint\n' >> "$CALL_LOG"
STUB
for tool in rustup cargo bun; do
  cat > "$tmp/bin/$tool" <<'STUB'
#!/usr/bin/env bash
printf '%s %s\n' "$(basename "$0")" "$*" >> "$CALL_LOG"
STUB
  chmod +x "$tmp/bin/$tool"
done
chmod +x "$bootstrap/scripts/setup-llmlint.sh"
git init -q "$bootstrap"

just_path=$(command -v just || true)
[[ -n $just_path ]] \
  || fail "just is required to test bootstrap; install it and rerun 'just lint-workflows'"
ln -s "$just_path" "$tmp/bin/just-real"
CALL_LOG="$log" PATH="$tmp/bin:$PATH" "$tmp/bin/just-real" \
  --justfile "$bootstrap/justfile" --working-directory "$bootstrap" bootstrap
assert_log_line 'setup-llmlint'
[[ $(git -C "$bootstrap" config --local core.hooksPath) == .githooks ]] \
  || fail "bootstrap did not install core.hooksPath=.githooks; inspect $bootstrap/.git/config"
