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

    # The same correction install.sh makes for a shell emulating x64 on a
    # Windows ARM64 host, so the archive staged here is the one it will ask for.
    if [ "$EXT" = "zip" ] && [ "$arch_part" = "x86_64" ]; then
        case "${PROCESSOR_ARCHITECTURE:-}:${PROCESSOR_IDENTIFIER:-}" in
            ARM64:* | *:ARMv8* | *:ARM64*) arch_part="aarch64" ;;
        esac
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
            elif have python3; then
                python3 -c 'import sys, zipfile
with zipfile.ZipFile(sys.argv[1], "w") as z: z.write(sys.argv[2], sys.argv[3])' \
                    "$archive" "$stage/$BIN_FILE" "$BIN_FILE"
            else
                fail "need zip, PowerShell Compress-Archive or python3 to create $archive"
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

# Prove install.sh picks the right artifact for every host it supports, not
# only for the one this runs on. The host is substituted at the installer's own
# seams — `uname` (a stub first on PATH) and the PROCESSOR_* variables Windows
# sets — and the release source at ONEHARNESS_RELEASE_BASE_URL, which serves an
# archive for EVERY platform release-platforms.toml declares, each carrying a
# binary that names its own target. So a host mapped to the wrong platform
# installs a binary naming the wrong target, and one mapped to an unpublished
# platform fails its download.
verify_platform_selection() {
    local mirror trust stubdir probe declared target ext bin_file name expected
    local uname_s uname_m proc_arch proc_id want installed
    mirror="$work/platform-mirror"
    trust="$work/platform-trust"
    stubdir="$work/platform-uname"
    mkdir -p "$mirror/$version" "$trust/$version" "$stubdir"

    declared="$(awk '
        /^\[\[platform\]\]$/ { if (t != "") print t, a; t = ""; a = ""; next }
        /^target = "/ { t = $3; gsub(/"/, "", t) }
        /^archive = "/ { a = $3; gsub(/"/, "", a) }
        END { if (t != "") print t, a }
    ' "$repo_root/release-platforms.toml")"
    [ -n "$declared" ] || fail "release-platforms.toml declares no platform to stage"

    while read -r target ext; do
        case "$target" in
            *-windows-*) bin_file="oneharness.exe" ;;
            *) bin_file="oneharness" ;;
        esac
        rm -rf "$work/platform-stage"
        mkdir -p "$work/platform-stage"
        printf 'oneharness fixture for %s\n' "$target" >"$work/platform-stage/$bin_file"
        name="oneharness-${version}-${target}"
        BIN_FILE="$bin_file" make_archive "$work/platform-stage" "$mirror/$version/${name}.${ext}"
        printf '%s  %s\n' "$(sha256_of "$mirror/$version/${name}.${ext}")" "${name}.${ext}" \
            >"$trust/$version/${name}.sha256"
    done <<<"$declared"

    # llmlint: ignore[e2e_not_mocked] uname is the installer's own host seam: this suite runs on one host, and proving the six hosts install.sh serves (Windows ARM64 above all, which no hosted job here runs it on) means answering uname as each would, while install.sh itself, its download, checksum and unpack run for real.
    cat >"$stubdir/uname" <<'STUB'
#!/bin/sh
case "${1:-}" in
    -s) printf '%s\n' "$STUB_UNAME_S" ;;
    -m) printf '%s\n' "$STUB_UNAME_M" ;;
    *) printf '%s\n' "$STUB_UNAME_S" ;;
esac
STUB
    chmod +x "$stubdir/uname"

    # uname -s | uname -m | PROCESSOR_ARCHITECTURE | PROCESSOR_IDENTIFIER | target.
    # Every case sets both PROCESSOR_* variables, so the runner's own (a Windows
    # job has them) can never decide one.
    while IFS='|' read -r uname_s uname_m proc_arch proc_id want; do
        probe="$work/platform-probe/$want-$uname_m"
        rm -rf "$probe"
        if ! PATH="$stubdir:$PATH" STUB_UNAME_S="$uname_s" STUB_UNAME_M="$uname_m" \
            PROCESSOR_ARCHITECTURE="$proc_arch" PROCESSOR_IDENTIFIER="$proc_id" \
            ONEHARNESS_RELEASE_BASE_URL="$mirror" ONEHARNESS_CHECKSUM_BASE_URL="$trust" \
            sh "$repo_root/scripts/install.sh" --version "$version" --to "$probe" \
            >"$work/platform.out" 2>&1; then
            cat "$work/platform.out" >&2
            fail "install.sh refused a $uname_s $uname_m host (PROCESSOR_ARCHITECTURE='$proc_arch'); it should have installed $want — fix detect_target in scripts/install.sh, whose refusal is printed above"
        fi
        case "$want" in
            *-windows-*) bin_file="oneharness.exe" ;;
            *) bin_file="oneharness" ;;
        esac
        [ -f "$probe/$bin_file" ] ||
            fail "install.sh on a $uname_s $uname_m host installed no $bin_file under $probe (found: $(ls "$probe" 2>/dev/null)); check the binary name detect_target in scripts/install.sh picks for $want"
        expected="oneharness fixture for $want"
        installed="$(cat "$probe/$bin_file")"
        [ "$installed" = "$expected" ] ||
            fail "install.sh on a $uname_s $uname_m host (PROCESSOR_ARCHITECTURE='$proc_arch', PROCESSOR_IDENTIFIER='$proc_id') installed the artifact saying '$installed'; it should have installed $want — fix the target detect_target in scripts/install.sh maps this host to"
    done <<'CASES'
Linux|x86_64|||x86_64-unknown-linux-gnu
Linux|aarch64|||aarch64-unknown-linux-gnu
Darwin|x86_64|||x86_64-apple-darwin
Darwin|arm64|||aarch64-apple-darwin
MINGW64_NT-10.0-26100|x86_64|AMD64|Intel64 Family 6 Model 85 Stepping 7, GenuineIntel|x86_64-pc-windows-msvc
MINGW64_NT-10.0-26100|aarch64|ARM64|ARMv8 (64-bit) Family 8 Model 1 Revision 201, Qualcomm Technologies Inc|aarch64-pc-windows-msvc
MINGW64_NT-10.0-26100|x86_64|AMD64|ARMv8 (64-bit) Family 8 Model 1 Revision 201, Qualcomm Technologies Inc|aarch64-pc-windows-msvc
MINGW64_NT-10.0-26100|x86_64|ARM64|ARMv8 (64-bit) Family 8 Model 1 Revision 201, Qualcomm Technologies Inc|aarch64-pc-windows-msvc
MSYS_NT-10.0-26100|x86_64|AMD64|ARM64 Family 8 Model 1 Revision 201, Qualcomm Technologies Inc|aarch64-pc-windows-msvc
CASES
}

verify_trust_root_independence
verify_attestation_gate
verify_platform_selection
say "install-e2e: ok"
