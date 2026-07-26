#!/usr/bin/env bash
# Hermetic boundary/recovery check for install-live-variant-tools.sh.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
printf '#!/usr/bin/env bash\nexit 0\n' >"$tmp/goose.sh"
printf 'exit 0\n' >"$tmp/goose.ps1"
sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}
unix_sha="$(sha256 "$tmp/goose.sh")"
windows_sha="$(sha256 "$tmp/goose.ps1")"
cat >"$tmp/npm" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$*" >"$OH_INSTALL_NPM_LOG"
EOF
chmod 700 "$tmp/npm"

run_install() {
    local os="$1" source="$2" sha="$3"
    local out="$tmp/$os.out"
    OH_LIVE_VARIANT_NPM_BIN="$tmp/npm" \
        OH_LIVE_VARIANT_INSTALL_TEST=1 \
        OH_INSTALL_NPM_LOG="$tmp/npm.log" \
        OH_LIVE_VARIANT_TEST_OS="$os" \
        OH_LIVE_VARIANT_GOOSE_RUNNER=/bin/true \
        OH_LIVE_VARIANT_UNIX_URL="file://$source" \
        OH_LIVE_VARIANT_UNIX_SHA256="$sha" \
        OH_LIVE_VARIANT_WINDOWS_URL="file://$source" \
        OH_LIVE_VARIANT_WINDOWS_SHA256="$sha" \
        GITHUB_PATH="$tmp/github-path" \
        bash "$root/scripts/install-live-variant-tools.sh" >"$out"
    [ "$(wc -l <"$out")" -eq 1 ]
    grep -Fq '@anthropic-ai/claude-code@2.1.220' "$tmp/npm.log"
    grep -Fq 'installed Claude Code, Codex, OpenCode, Qwen Code, Crush, and Goose' "$out"
}
run_install Linux "$tmp/goose.sh" "$unix_sha"
run_install MINGW64_NT "$tmp/goose.ps1" "$windows_sha"

if OH_LIVE_VARIANT_NPM_BIN="$tmp/npm" \
    OH_LIVE_VARIANT_INSTALL_TEST=1 \
    OH_INSTALL_NPM_LOG="$tmp/npm.log" \
    OH_LIVE_VARIANT_UNIX_URL="file://$tmp/missing" \
    bash "$root/scripts/install-live-variant-tools.sh" >"$tmp/fail.out" 2>"$tmp/fail.err"; then
    printf 'installer unexpectedly accepted a missing download\n' >&2
    exit 1
fi
grep -Fq 'verify network access to github.com and rerun' "$tmp/fail.err"
printf 'install-live-variant-tools-e2e: ok\n'
