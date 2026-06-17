#!/usr/bin/env bash
# Hermetic installer e2e: package a just-built oneharness binary into the same
# archive/checksum shape release.yml publishes, then install it through
# scripts/install.sh from a local release directory.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

say() { printf '%s\n' "$*" >&2; }
fail() { printf 'install-e2e: FAIL: %s\n' "$*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

usage() {
    cat >&2 <<EOF
Usage: install-e2e.sh <oneharness-bin> <install-dir>

Packages <oneharness-bin> as a local release asset and invokes scripts/install.sh
to install it into <install-dir>.
EOF
}

exe_path() {
    if [ -x "$1" ]; then printf '%s' "$1"; return 0; fi
    if [ -x "$1.exe" ]; then printf '%s' "$1.exe"; return 0; fi
    return 1
}

detect_target() {
    local os arch os_part arch_part
    os="$(uname -s)"
    arch="$(uname -m)"

    case "$os" in
        Linux) os_part="unknown-linux-gnu"; EXT="tar.gz"; BIN_FILE="oneharness" ;;
        Darwin) os_part="apple-darwin"; EXT="tar.gz"; BIN_FILE="oneharness" ;;
        MINGW* | MSYS* | CYGWIN* | Windows_NT)
            os_part="pc-windows-msvc"; EXT="zip"; BIN_FILE="oneharness.exe" ;;
        *) fail "unsupported operating system: $os" ;;
    esac

    case "$arch" in
        x86_64 | amd64) arch_part="x86_64" ;;
        arm64 | aarch64) arch_part="aarch64" ;;
        *) fail "unsupported architecture: $arch" ;;
    esac

    if [ "$EXT" = "zip" ] && [ "$arch_part" != "x86_64" ]; then
        fail "no prebuilt Windows binary target for $arch"
    fi

    TARGET="${arch_part}-${os_part}"
}

sha256_of() {
    local f="$1"
    if have sha256sum; then
        sha256sum "$f" | awk '{print $1}'
    elif have shasum; then
        shasum -a 256 "$f" | awk '{print $1}'
    elif have openssl; then
        openssl dgst -sha256 "$f" | awk '{print $NF}'
    else
        fail "no SHA-256 tool found"
    fi
}

make_archive() {
    local stage="$1" archive="$2"
    case "$archive" in
        *.tar.gz)
            tar -czf "$archive" -C "$stage" "$BIN_FILE"
            ;;
        *.zip)
            if have zip; then
                (cd "$stage" && zip -q "$archive" "$BIN_FILE")
            elif have powershell.exe && have cygpath; then
                local win_bin win_archive
                win_bin="$(cygpath -w "$stage/$BIN_FILE")"
                win_archive="$(cygpath -w "$archive")"
                powershell.exe -NoProfile -Command \
                    "Compress-Archive -LiteralPath '$win_bin' -DestinationPath '$win_archive' -Force"
            else
                fail "need zip or PowerShell Compress-Archive to create $archive"
            fi
            ;;
        *) fail "unknown archive type: $archive" ;;
    esac
}

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
    usage
    exit 0
fi

[ "$#" -eq 2 ] || { usage; exit 2; }

source_bin="$(exe_path "$1")" || fail "binary not found or not executable: $1"
install_dir="$2"
version="${ONEHARNESS_INSTALL_E2E_VERSION:-v0.0.0-e2e}"

detect_target

work="$(mktemp -d 2>/dev/null || mktemp -d -t oneharness-install-e2e)" \
    || fail "could not create temporary directory"
trap 'rm -rf "$work"' EXIT INT TERM

release_dir="$work/release"
stage="$work/stage"
mkdir -p "$release_dir" "$stage"
cp "$source_bin" "$stage/$BIN_FILE"
chmod 0755 "$stage/$BIN_FILE"

archive="oneharness-${version}-${TARGET}.${EXT}"
sumfile="oneharness-${version}-${TARGET}.sha256"
archive_path="$release_dir/$archive"

make_archive "$stage" "$archive_path"
printf '%s  %s\n' "$(sha256_of "$archive_path")" "$archive" > "$release_dir/$sumfile"

say "install-e2e: installing oneharness from local ${archive}"
ONEHARNESS_RELEASE_BASE_URL="$release_dir" \
    sh "$repo_root/scripts/install.sh" --version "$version" --to "$install_dir" >&2

installed="$(exe_path "$install_dir/oneharness")" \
    || fail "installer did not create oneharness under $install_dir"
"$installed" --version >&2
say "install-e2e: ok"
