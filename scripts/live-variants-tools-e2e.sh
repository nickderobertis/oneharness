#!/usr/bin/env bash
# Hermetic command-boundary test for `just live-variants-tools`.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/bin"
cat >"$tmp/bin/npm" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >"$OH_INSTALL_ARGS_FILE"
if [ -n "${OH_INSTALL_FAIL:-}" ]; then
    printf 'synthetic npm install failure\n' >&2
    exit 42
fi
EOF
chmod 700 "$tmp/bin/npm"

args_file="$tmp/args"
success="$tmp/success"
PATH="$tmp/bin:$PATH" OH_INSTALL_ARGS_FILE="$args_file" \
    just --justfile "$repo_root/justfile" live-variants-tools >"$success"
grep -Fxq 'install -g @anthropic-ai/claude-code@2.1.220 @openai/codex@0.145.0 opencode-ai@1.18.5 @qwen-code/qwen-code@0.21.0 @charmland/crush@0.87.0' "$args_file"
grep -Fq 'installed Claude Code, Codex, OpenCode, Qwen Code, and Crush' "$success"

failure="$tmp/failure"
if PATH="$tmp/bin:$PATH" OH_INSTALL_ARGS_FILE="$args_file" OH_INSTALL_FAIL=1 \
    just --justfile "$repo_root/justfile" live-variants-tools >"$failure" 2>&1; then
    echo "live-variants-tools-e2e: expected npm failure to fail the recipe" >&2
    exit 1
fi
grep -Fq 'synthetic npm install failure' "$failure"
grep -Fq 'npm install failed; resolve the reported error' "$failure"
echo "live-variants-tools-e2e: ok"
