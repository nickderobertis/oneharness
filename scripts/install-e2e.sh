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
# The mirror mirrors GitHub's <base>/<tag>/<asset> layout, so the archive lives
# under a version subdirectory just as install.sh (and a real mirror) expects.
mkdir -p "$release_dir/$version" "$stage"
cp "$source_bin" "$stage/$BIN_FILE"
chmod 0755 "$stage/$BIN_FILE"

archive="oneharness-${version}-${TARGET}.${EXT}"
sumfile="oneharness-${version}-${TARGET}.sha256"
bundlefile="oneharness-${version}-${TARGET}.sigstore.json"
archive_path="$release_dir/$version/$archive"

make_archive "$stage" "$archive_path"
good_sum="$(sha256_of "$archive_path")"

# An independent checksum trust root (a separate directory from the archive
# mirror), so a valid checksum vouches for the archive without any verifier or
# network — the sum_trusted=yes path.
trust_dir="$work/trust"
mkdir -p "$trust_dir/$version"
printf '%s  %s\n' "$good_sum" "$archive" > "$trust_dir/$version/$sumfile"

# place_sum <dir> — write the good checksum into <dir>/<version>/ so tests can
# stand up their own trust roots.
place_sum() {
    mkdir -p "$1/$version"
    printf '%s  %s\n' "$good_sum" "$archive" > "$1/$version/$sumfile"
}

say "install-e2e: installing oneharness from local mirror ${archive}"
# Archive from the local mirror; checksum from a SEPARATE local trust root, so
# the install verifies hermetically (no verifier, no network) via a checksum the
# mirror does not control.
ONEHARNESS_RELEASE_BASE_URL="$release_dir" \
    ONEHARNESS_CHECKSUM_BASE_URL="$trust_dir" \
    sh "$repo_root/scripts/install.sh" --version "$version" --to "$install_dir" >&2

installed="$(exe_path "$install_dir/oneharness")" \
    || fail "installer did not create oneharness under $install_dir"
"$installed" --version >&2

# Prove the checksum trust root is independent of the archive mirror: an archive
# is accepted only against a checksum from a *separate* source, a tampered mirror
# archive is rejected, and a checksum that shares the mirror's origin is refused
# outright (it is no trust root at all).
verify_trust_root_independence() {
    local mirror probe
    mirror="$work/mirror"
    probe="$work/probe"
    mkdir -p "$mirror/$version" "$probe"
    cp "$archive_path" "$mirror/$version/$archive"

    # (1) Good archive from the mirror + checksum from the independent root installs.
    if ! ONEHARNESS_RELEASE_BASE_URL="$mirror" ONEHARNESS_CHECKSUM_BASE_URL="$trust_dir" \
        sh "$repo_root/scripts/install.sh" --version "$version" --to "$probe/ok" >/dev/null 2>&1; then
        fail "independent trust root rejected a valid archive"
    fi
    exe_path "$probe/ok/oneharness" >/dev/null \
        || fail "trust-root install produced no binary"

    # (2) Tamper the mirror's archive; the trusted checksum is unchanged -> must fail.
    printf 'tampered\n' >> "$mirror/$version/$archive"
    if ONEHARNESS_RELEASE_BASE_URL="$mirror" ONEHARNESS_CHECKSUM_BASE_URL="$trust_dir" \
        sh "$repo_root/scripts/install.sh" --version "$version" --to "$probe/bad" >/dev/null 2>&1; then
        fail "installer accepted a tampered mirror archive against an independent checksum"
    fi

    # (3) A checksum that shares the mirror's origin is refused, not trusted:
    #     restore the good archive, put the checksum in the *same* mirror dir, and
    #     ship no attestation bundle. With nothing independent to vouch for it,
    #     the install must abort rather than trust the mirror's own checksum.
    cp "$archive_path" "$mirror/$version/$archive"
    place_sum "$mirror"
    if ONEHARNESS_RELEASE_BASE_URL="$mirror" ONEHARNESS_CHECKSUM_BASE_URL="$mirror" \
        sh "$repo_root/scripts/install.sh" --version "$version" --to "$probe/self" >/dev/null 2>&1; then
        fail "installer trusted a checksum sharing the mirror's origin"
    fi
    say "install-e2e: trust-root independence verified (tampered + mirror-origin checksums rejected)"
}

# Prove install.sh runs a Sigstore verifier and gates on its verdict. A real
# offline verification needs a genuine signed bundle, so stub every verifier
# (cosign/sigstore/gh) on PATH: the stub records the call and passes or fails on
# demand. The checksum here shares the mirror's origin (refused), so the Sigstore
# attestation is the ONLY thing that can authorize the install — isolating the
# gate. Pass -> installs; fail -> aborts.
verify_attestation_gate() {
    local mirror probe stubdir log tool
    mirror="$work/att-mirror"
    probe="$work/att-probe"
    stubdir="$work/stub"
    mkdir -p "$mirror/$version" "$probe" "$stubdir"

    cp "$archive_path" "$mirror/$version/$archive"
    place_sum "$mirror"                                   # mirror-origin -> refused
    printf '{}' > "$mirror/$version/$bundlefile"          # placeholder; stub judges it

    for tool in cosign sigstore gh; do
        cat > "$stubdir/$tool" <<STUB
#!/bin/sh
echo "$tool \$*" >> "\$STUB_LOG"
exit "\${STUB_EXIT:-0}"
STUB
        chmod +x "$stubdir/$tool"
    done
    log="$work/verifier-calls.log"
    : > "$log"

    # (1) Verifier passes -> install succeeds AND a verifier actually ran.
    if ! PATH="$stubdir:$PATH" STUB_LOG="$log" STUB_EXIT=0 \
        ONEHARNESS_RELEASE_BASE_URL="$mirror" ONEHARNESS_CHECKSUM_BASE_URL="$mirror" \
        sh "$repo_root/scripts/install.sh" --version "$version" --to "$probe/ok" >/dev/null 2>&1; then
        fail "install rejected an archive whose Sigstore attestation verified"
    fi
    grep -q "verify" "$log" || fail "install did not invoke a Sigstore verifier"

    # (2) Verifier fails -> no independent root remains -> install must abort.
    if PATH="$stubdir:$PATH" STUB_LOG="$log" STUB_EXIT=1 \
        ONEHARNESS_RELEASE_BASE_URL="$mirror" ONEHARNESS_CHECKSUM_BASE_URL="$mirror" \
        sh "$repo_root/scripts/install.sh" --version "$version" --to "$probe/bad" >/dev/null 2>&1; then
        fail "installer accepted an archive whose Sigstore attestation failed"
    fi
    say "install-e2e: Sigstore attestation gate verified (a verifier runs; a failed attestation aborts)"
}

verify_trust_root_independence
verify_attestation_gate
say "install-e2e: ok"
