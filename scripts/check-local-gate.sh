#!/usr/bin/env bash
# Hermetic behavioral check for the local llmlint gate and comparison-base helper.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/bin"
log="$tmp/calls"

expect_failure() {
  local expected=$1
  shift
  if "$@" >"$tmp/stdout" 2>"$tmp/stderr"; then
    echo "expected command to fail: $*" >&2
    exit 1
  fi
  grep -Fq -- "$expected" "$tmp/stderr"
}

expect_failure_in() {
  local directory=$1
  shift
  (cd "$directory" && expect_failure "$@")
}

cat > "$tmp/bin/llmlint" <<'STUB'
#!/usr/bin/env bash
printf 'llmlint %s\n' "$*" >> "$CALL_LOG"
STUB
cat > "$tmp/bin/codex" <<'STUB'
#!/usr/bin/env bash
read -r key
[[ $key == test-key ]]
printf 'codex %s\n' "$*" >> "$CALL_LOG"
STUB
chmod +x "$tmp/bin/llmlint" "$tmp/bin/codex"
ln -s "$(command -v just)" "$tmp/bin/just"

expect_failure 'usage: local-llmlint-gate.sh <remote/base>' \
  "$root/scripts/local-llmlint-gate.sh" ''
expect_failure "'not-a-ref' is not a valid remote/base ref" \
  "$root/scripts/local-llmlint-gate.sh" not-a-ref

PATH=/usr/bin:/bin HOME="$tmp/home" \
  "$root/scripts/local-llmlint-gate.sh" origin/main 2>"$tmp/unavailable"
grep -Fq 'llmlint unavailable' "$tmp/unavailable"

CALL_LOG="$log" PATH="$tmp/bin:/usr/bin:/bin" HOME="$tmp/home" \
  "$root/scripts/local-llmlint-gate.sh" origin/main 2>"$tmp/skip"
grep -Fq 'judge skipped locally (OPENAI_API_KEY unavailable)' "$tmp/skip"
[[ $(wc -l < "$log") -eq 1 ]]

mv "$tmp/bin/codex" "$tmp/codex"
CALL_LOG="$log" PATH="$tmp/bin:/usr/bin:/bin" HOME="$tmp/home" OPENAI_API_KEY=test-key \
  "$root/scripts/local-llmlint-gate.sh" origin/main 2>"$tmp/no-codex"
grep -Fq "primary harness 'codex' unavailable" "$tmp/no-codex"
mv "$tmp/codex" "$tmp/bin/codex"

cat > "$tmp/bin/llmlint" <<'STUB'
#!/usr/bin/env bash
[[ ${1:-} != validate ]] || { echo 'invalid llmlint setup' >&2; exit 9; }
STUB
chmod +x "$tmp/bin/llmlint"
expect_failure 'invalid llmlint setup' env PATH="$tmp/bin:/usr/bin:/bin" HOME="$tmp/home" \
  "$root/scripts/local-llmlint-gate.sh" origin/main

cat > "$tmp/bin/llmlint" <<'STUB'
#!/usr/bin/env bash
printf 'llmlint %s\n' "$*" >> "$CALL_LOG"
STUB
chmod +x "$tmp/bin/llmlint"

CALL_LOG="$log" PATH="$tmp/bin:/usr/bin:/bin" HOME="$tmp/home" OPENAI_API_KEY=test-key \
  "$root/scripts/local-llmlint-gate.sh" origin/main
grep -Fxq 'codex login --with-api-key' "$log"
grep -Fxq 'llmlint --diff --diff-base origin/main' "$log"

cat > "$tmp/bin/codex" <<'STUB'
#!/usr/bin/env bash
echo 'codex login failed' >&2
exit 8
STUB
chmod +x "$tmp/bin/codex"
expect_failure 'codex login failed' env CALL_LOG="$log" PATH="$tmp/bin:/usr/bin:/bin" \
  HOME="$tmp/home" OPENAI_API_KEY=test-key \
  "$root/scripts/local-llmlint-gate.sh" origin/main

cat > "$tmp/bin/codex" <<'STUB'
#!/usr/bin/env bash
read -r key
[[ $key == test-key ]]
STUB
cat > "$tmp/bin/llmlint" <<'STUB'
#!/usr/bin/env bash
[[ ${1:-} == validate ]] && exit 0
echo 'llmlint judge failed' >&2
exit 7
STUB
chmod +x "$tmp/bin/codex" "$tmp/bin/llmlint"
expect_failure 'llmlint judge failed' env PATH="$tmp/bin:/usr/bin:/bin" HOME="$tmp/home" \
  OPENAI_API_KEY=test-key "$root/scripts/local-llmlint-gate.sh" origin/main

cat > "$tmp/bin/llmlint" <<'STUB'
#!/usr/bin/env bash
printf 'llmlint %s\n' "$*" >> "$CALL_LOG"
STUB
cat > "$tmp/bin/codex" <<'STUB'
#!/usr/bin/env bash
read -r key
[[ $key == test-key ]]
printf 'codex %s\n' "$*" >> "$CALL_LOG"
STUB
chmod +x "$tmp/bin/llmlint" "$tmp/bin/codex"

git init -q --bare "$tmp/remote.git"
git init -q -b main "$tmp/repo"
git -C "$tmp/repo" config user.email test@example.com
git -C "$tmp/repo" config user.name Test
git -C "$tmp/repo" commit --allow-empty -qm initial
git -C "$tmp/repo" remote add origin "$tmp/remote.git"
git -C "$tmp/repo" push -qu origin main
git -C "$tmp/repo" remote set-head origin main
[[ $(cd "$tmp/repo" && "$root/scripts/comparison-base.sh" origin) == origin/main ]]

expect_failure_in "$tmp/repo" "'bad name' is not a valid remote name" \
  "$root/scripts/comparison-base.sh" 'bad name'
expect_failure_in "$tmp/repo" "remote 'upstream' does not exist" \
  "$root/scripts/comparison-base.sh" upstream

git -C "$tmp/repo" remote add empty "$tmp/remote.git"
expect_failure_in "$tmp/repo" "cannot discover the base for 'empty'" \
  "$root/scripts/comparison-base.sh" empty
expect_failure_in "$tmp/repo" "'bad..branch' is not a valid branch name" \
  "$root/scripts/comparison-base.sh" origin 'bad..branch'
expect_failure_in "$tmp/repo" "'origin/missing' is missing" \
  "$root/scripts/comparison-base.sh" origin missing

# Exercise the repository's gate recipe through `just gate`, replacing only its
# expensive prerequisite recipes with no-ops in a generated test justfile.
awk '
  /^gate remote=/ { capture=1; sub(/: check deps-check/, ": check deps-check"); }
  /^lint-llm-local base:/ { capture=2 }
  capture == 1 { print; if ($0 ~ /^    @comparison=/) capture=0 }
  capture == 2 { print; if ($0 ~ /^    scripts\//) capture=0 }
' "$root/justfile" > "$tmp/gate.just"
cat >> "$tmp/gate.just" <<'JUST'
check:
    @:
deps-check:
    @:
JUST
PATH="$tmp/bin:/usr/bin:/bin" HOME="$tmp/home" CALL_LOG="$log" OPENAI_API_KEY=test-key \
  just --justfile "$tmp/gate.just" --working-directory "$root" gate origin main
grep -Fxq 'llmlint validate --diff-base origin/main' "$log"
grep -Fxq 'llmlint --diff --diff-base origin/main' "$log"

rm "$tmp/bin/just"
cat > "$tmp/bin/just" <<'STUB'
#!/usr/bin/env bash
[[ -z ${GIT_DIR:-} ]]
printf 'just %s\n' "$*" >> "$CALL_LOG"
STUB
chmod +x "$tmp/bin/just"
CALL_LOG="$log" PATH="$tmp/bin:/usr/bin:/bin" GIT_DIR=.git \
  ONEHARNESS_COMPARISON_BASE=main "$root/.githooks/pre-push" upstream
grep -Fxq 'just gate upstream main' "$log"
