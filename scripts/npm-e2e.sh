#!/usr/bin/env bash
# Hermetic npm-packaging e2e: assemble the per-platform npm package for the host
# from a just-built oneharness binary, wire it under the launcher package exactly
# as npm's optional-dependency resolution would, and prove `oneharness-cli`'s
# launcher shim (npm/oneharness/bin/oneharness.js) resolves and execs the binary.
#
# No network, no `npm install`, no publish — just Node running the committed
# launcher against a locally-built platform package. Requires `node`; the caller
# (smoke.sh) skips this step cleanly when Node is absent.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

say() { printf '%s\n' "$*" >&2; }
fail() { printf 'npm-e2e: FAIL: %s\n' "$*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

usage() {
    cat >&2 <<EOF
Usage: npm-e2e.sh <oneharness-bin>

Builds the host's @oneharness/cli-<platform>-<arch> package from <oneharness-bin>,
stages it under the launcher package, and runs the launcher end to end.
EOF
}

exe_path() {
    if [ -x "$1" ]; then printf '%s' "$1"; return 0; fi
    if [ -x "$1.exe" ]; then printf '%s' "$1.exe"; return 0; fi
    return 1
}

# Map `uname` to the Rust target triple the npm-build script keys on (the same
# matrix as release.yml). Mirrors scripts/install-e2e.sh's detection.
detect_target() {
    local os arch os_part arch_part
    os="$(uname -s)"
    arch="$(uname -m)"
    case "$os" in
        Linux) os_part="unknown-linux-gnu" ;;
        Darwin) os_part="apple-darwin" ;;
        MINGW* | MSYS* | CYGWIN* | Windows_NT) os_part="pc-windows-msvc" ;;
        *) fail "unsupported operating system: $os" ;;
    esac
    case "$arch" in
        x86_64 | amd64) arch_part="x86_64" ;;
        arm64 | aarch64) arch_part="aarch64" ;;
        *) fail "unsupported architecture: $arch" ;;
    esac
    if [ "$os_part" = "pc-windows-msvc" ] && [ "$arch_part" != "x86_64" ]; then
        fail "no prebuilt Windows npm package for $arch"
    fi
    TARGET="${arch_part}-${os_part}"
}

[ $# -ge 1 ] || { usage; exit 2; }
bin="$1"
have node || fail "node not found on PATH"
bin_resolved="$(exe_path "$bin")" || fail "oneharness binary not found: $bin"

cd "$repo_root"
detect_target

# node's own platform/arch is the source of truth for the package dir name, so it
# always matches the launcher's resolution key.
key="$(node -e 'process.stdout.write(process.platform+"-"+process.arch)')"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# Build the platform package and the version-stamped launcher into $tmp/dist.
plat_dir="$(node scripts/npm-build.mjs platform --target "$TARGET" --binary "$bin_resolved" --out "$tmp/dist")" \
    || fail "npm-build platform failed"
launcher_dir="$(node scripts/npm-build.mjs launcher --out "$tmp/dist")" \
    || fail "npm-build launcher failed"

# Stage the platform package where npm would put the resolved optional
# dependency: node_modules/@oneharness/cli-<platform>-<arch> beside the launcher.
mkdir -p "$launcher_dir/node_modules/@oneharness"
cp -R "$plat_dir" "$launcher_dir/node_modules/@oneharness/cli-$key"

# Run the launcher shim exactly as the installed `oneharness` bin would.
out="$(node "$launcher_dir/bin/oneharness.js" list --compact)" \
    || fail "launcher run exited non-zero"
printf '%s' "$out" | grep -qF '"claude-code"' \
    || fail "launcher output missing expected harness (\"claude-code\"): $out"

# The launcher must run the SAME version as the source tree (a stale binary would
# surface here just like the installer e2e's version guard).
crate_ver="$(grep -m1 -E '^version[[:space:]]*=' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"
bin_ver="$(node "$launcher_dir/bin/oneharness.js" --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)"
if [ -n "$crate_ver" ] && [ -n "$bin_ver" ] && [ "$crate_ver" != "$bin_ver" ]; then
    fail "launcher ran v$bin_ver but Cargo.toml is v$crate_ver (stale binary)"
fi

say "npm-e2e: ok (launcher resolved @oneharness/cli-$key and ran oneharness $bin_ver)"
