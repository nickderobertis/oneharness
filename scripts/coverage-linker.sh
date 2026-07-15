#!/usr/bin/env bash
# Normalize rustc's profiling-runtime symbol option for clang-compatible `cc`.
set -euo pipefail

real_cc=$(command -v cc)
args=()
while (($#)); do
    if [[ "$1" == "-u" ]]; then
        shift
        args+=("-Wl,-u,$1")
    else
        args+=("$1")
    fi
    shift
done
exec "$real_cc" "${args[@]}"
