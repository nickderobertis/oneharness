#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

printf '#!/usr/bin/env bash\nexit 7\n' >"$tmp/failing-rustc-wrapper"
chmod +x "$tmp/failing-rustc-wrapper"

if CARGO_TARGET_DIR="$tmp/target" RUSTC_WRAPPER="$tmp/failing-rustc-wrapper" \
    just --justfile "$root/justfile" build-mock-harness >"$tmp/stdout" 2>"$tmp/stderr"; then
    echo "check-build-mock-harness: injected compiler failure unexpectedly succeeded; verify the recipe still propagates cargo failures" >&2
    exit 1
fi

grep -q "mock-harness build failed; fix the compiler diagnostics above and rerun 'just build-mock-harness'" "$tmp/stderr" || {
    echo "check-build-mock-harness: failure omitted its recovery action; run the recipe with a failing RUSTC_WRAPPER and inspect stderr" >&2
    exit 1
}

echo "check-build-mock-harness: ok"
