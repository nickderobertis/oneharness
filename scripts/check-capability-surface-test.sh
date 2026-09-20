#!/usr/bin/env bash
#
# Behavioral test of where the capability-surface check builds its generator.
#
# The check names no target directory of its own: its `cargo run` builds
# wherever `.cargo/config.toml` sends every cargo invocation in this clone, so
# the example lands under `<clone>/target` and not under the sub-directory the
# check once pinned. The example is removed first so that what is present
# afterwards is this run's doing, not a prior one's — a restored CI cache or an
# older checkout's build can leave the legacy sub-directory standing. An
# inherited override is stripped for the same reason: the claim under test is
# what the config file does, not what a caller's environment did.
#
# Quiet on success, one line. On failure it says which location was wrong.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
unset CARGO_TARGET_DIR CARGO_BUILD_TARGET_DIR

example="target/debug/examples/generate_core_sdk_schema"
legacy="target/sdk-schema-generator/debug/examples/generate_core_sdk_schema"
case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*)
    example="$example.exe"
    legacy="$legacy.exe"
    ;;
esac
rm -f "$example" "$legacy"

fail() {
  echo "check-capability-surface-test: $1" >&2
  exit 1
}

if ! out="$(bash scripts/check-capability-surface.sh 2>&1)"; then
  fail "the check should pass over the checked-in tree; it said:"$'\n'"$out"
fi
[ -f "$example" ] ||
  fail "the generator did not build into the clone-root target directory ($example is absent)"
[ ! -e "$legacy" ] ||
  fail "the generator built into the legacy sub-directory ($legacy exists)"

echo "check-capability-surface-test: the capability-surface check builds its generator into the clone-root target directory"
