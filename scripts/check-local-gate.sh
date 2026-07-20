#!/usr/bin/env bash
# Hermetic behavioral check for the local llmlint gate and comparison-base helper.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/bin"
log="$tmp/calls"

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

CALL_LOG="$log" PATH="$tmp/bin:$PATH" HOME="$tmp/home" \
  "$root/scripts/local-llmlint-gate.sh" origin/main 2>"$tmp/skip"
grep -Fq 'judge skipped locally (OPENAI_API_KEY unavailable)' "$tmp/skip"
[[ $(wc -l < "$log") -eq 1 ]]

CALL_LOG="$log" PATH="$tmp/bin:$PATH" HOME="$tmp/home" OPENAI_API_KEY=test-key \
  "$root/scripts/local-llmlint-gate.sh" origin/main
grep -Fxq 'codex login --with-api-key' "$log"
grep -Fxq 'llmlint --diff --diff-base origin/main' "$log"

git init -q --bare "$tmp/remote.git"
git init -q -b main "$tmp/repo"
git -C "$tmp/repo" config user.email test@example.com
git -C "$tmp/repo" config user.name Test
git -C "$tmp/repo" commit --allow-empty -qm initial
git -C "$tmp/repo" remote add origin "$tmp/remote.git"
git -C "$tmp/repo" push -qu origin main
git -C "$tmp/repo" remote set-head origin main
[[ $(cd "$tmp/repo" && "$root/scripts/comparison-base.sh" origin) == origin/main ]]

grep -Fq 'just gate' "$root/.githooks/pre-push"
