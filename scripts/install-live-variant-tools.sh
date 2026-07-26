#!/usr/bin/env bash
# Install the real CLIs used by e2e-variants.sh, validating downloaded installers.
set -euo pipefail

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
log="$tmp/install.log"
npm_bin="${OH_LIVE_VARIANT_NPM_BIN:-npm}"
if [ -z "${OH_LIVE_VARIANT_INSTALL_TEST:-}" ]; then
    npm_bin=npm
fi
if ! "$npm_bin" install -g \
    @anthropic-ai/claude-code@2.1.220 \
    @openai/codex@0.145.0 \
    opencode-ai@1.18.5 \
    @qwen-code/qwen-code@0.21.0 \
    @charmland/crush@0.87.0 >"$log" 2>&1; then
    cat "$log" >&2
    printf 'npm CLI installation failed; resolve the error above, then rerun just live-variants-tools\n' >&2
    exit 1
fi
sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}
platform="$(uname -s)"
[ -z "${OH_LIVE_VARIANT_INSTALL_TEST:-}" ] || platform="${OH_LIVE_VARIANT_TEST_OS:-$platform}"
case "$platform" in
MINGW* | MSYS* | CYGWIN*)
    installer="$tmp/download_cli.ps1"
    url="${OH_LIVE_VARIANT_WINDOWS_URL:-https://raw.githubusercontent.com/aaif-goose/goose/ce928f04e8352d570c0c525d11bcd23e46a03d12/download_cli.ps1}"
    expected="${OH_LIVE_VARIANT_WINDOWS_SHA256:-a979f5b92879954657d307a24bf9c2e0386fbf7d185be68c56d64c5403644489}"
    if [ -z "${OH_LIVE_VARIANT_INSTALL_TEST:-}" ]; then
        url="https://raw.githubusercontent.com/aaif-goose/goose/ce928f04e8352d570c0c525d11bcd23e46a03d12/download_cli.ps1"
        expected="a979f5b92879954657d307a24bf9c2e0386fbf7d185be68c56d64c5403644489"
    fi
    curl -fsSL "$url" -o "$installer" ||
        { printf 'Goose installer download failed; verify network access to raw.githubusercontent.com and rerun\n' >&2; exit 1; }
    [ "$(sha256 "$installer")" = "$expected" ] ||
        { printf 'Goose installer checksum mismatch: update the pinned commit and reviewed checksum\n' >&2; exit 1; }
    goose_runner="${OH_LIVE_VARIANT_GOOSE_RUNNER:-pwsh}"
    [ -n "${OH_LIVE_VARIANT_INSTALL_TEST:-}" ] || goose_runner=pwsh
    if ! CONFIGURE=false "$goose_runner" -NoProfile -File "$installer" >"$log" 2>&1; then
        cat "$log" >&2
        printf 'Goose installation failed; resolve the error above, then rerun just live-variants-tools\n' >&2
        exit 1
    fi
    ;;
*)
    installer="$tmp/download_cli.sh"
    url="${OH_LIVE_VARIANT_UNIX_URL:-https://github.com/aaif-goose/goose/releases/download/stable/download_cli.sh}"
    expected="${OH_LIVE_VARIANT_UNIX_SHA256:-54d64de9b10befba030d3fdc4f6c316de55557c203abeaa9525c04f450c34280}"
    if [ -z "${OH_LIVE_VARIANT_INSTALL_TEST:-}" ]; then
        url="https://github.com/aaif-goose/goose/releases/download/stable/download_cli.sh"
        expected="54d64de9b10befba030d3fdc4f6c316de55557c203abeaa9525c04f450c34280"
    fi
    curl -fsSL "$url" -o "$installer" ||
        { printf 'Goose installer download failed; verify network access to github.com and rerun\n' >&2; exit 1; }
    [ "$(sha256 "$installer")" = "$expected" ] ||
        { printf 'Goose installer checksum mismatch: review the stable installer and update its checksum\n' >&2; exit 1; }
    goose_runner="${OH_LIVE_VARIANT_GOOSE_RUNNER:-bash}"
    [ -n "${OH_LIVE_VARIANT_INSTALL_TEST:-}" ] || goose_runner=bash
    if ! CONFIGURE=false "$goose_runner" "$installer" >"$log" 2>&1; then
        cat "$log" >&2
        printf 'Goose installation failed; resolve the error above, then rerun just live-variants-tools\n' >&2
        exit 1
    fi
    ;;
esac
[ -z "${GITHUB_PATH:-}" ] || printf '%s\n' "$HOME/.local/bin" >>"$GITHUB_PATH"

printf 'live-variants-tools: installed Claude Code, Codex, OpenCode, Qwen Code, Crush, and Goose\n'
