#!/usr/bin/env bash
#
# Hold the capability manifest's Rust column to what `tests/library_surface.rs`
# actually exercises.
#
# `domain::capability::CAPABILITIES` names, per capability, the `oneharness-core`
# entry point a Rust consumer calls instead of spawning the CLI — and that claim
# is what `docs/sdk-parity.md` publishes. A named path nobody drives is a claim
# nothing checks, which is the failure mode this whole gate exists to remove. So
# every capability must carry a `// capability: <method>` marker in one of the
# library test binaries, on a test that calls its entry point.
#
# Quiet on success, one line. On failure it names the capabilities with no test
# and what to add.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

target="${CARGO_TARGET_DIR:-target/sdk-schema-generator}"
generator=(cargo run -q -p oneharness-core --features sdk-schema
  --example generate_core_sdk_schema)
# `set -e` would abort here with cargo's raw output and no statement of which
# gate wanted the bundle or what to run next. Say all three, and keep the quoted
# cause to the tail — a cargo failure runs long, and the error is at the end.
generator_err="$(mktemp)"
trap 'rm -f "$generator_err"' EXIT
if ! bundle="$(CARGO_TARGET_DIR="$target" "${generator[@]}" 2>"$generator_err")"; then
  echo "check-capability-surface: the Rust schema generator failed, so there is no capability manifest to check against." >&2
  if [ -s "$generator_err" ]; then
    echo "  cargo said:" >&2
    tail -n 20 "$generator_err" | sed 's/^/    /' >&2
  fi
  echo "  fix: make the bundle build, then rerun 'bash scripts/check-capability-surface.sh'." >&2
  echo "       See it in full with: ${generator[*]}" >&2
  exit 1
fi

# The methods declared in the manifest, and the ones marked in the test file.
declared="$(printf '%s' "$bundle" | node -e '
  let raw = "";
  process.stdin.on("data", (chunk) => (raw += chunk));
  process.stdin.on("end", () => {
    for (const c of JSON.parse(raw).capabilities) console.log(c.method);
  });
')"
marked="$(grep -hoE '^// capability: [A-Za-z]+' tests/library.rs tests/library_surface.rs \
  | sed 's|^// capability: ||' | sort -u)"

missing=""
while IFS= read -r method; do
  [ -n "$method" ] || continue
  printf '%s\n' "$marked" | grep -qx "$method" || missing="$missing $method"
done <<< "$declared"

if [ -n "$missing" ]; then
  echo "check-capability-surface: no Rust test exercises:$missing" >&2
  echo "  fix: add a test to tests/library_surface.rs (or tests/library.rs, for the" >&2
  echo "       run capabilities) calling that capability's" >&2
  echo "       \`rust\` entry point, tagged with a '// capability: <method>' line." >&2
  echo "       The manifest's Rust column is published in docs/sdk-parity.md, so" >&2
  echo "       an unexercised entry point is an unverified claim." >&2
  exit 1
fi

echo "check-capability-surface: every capability's Rust entry point is exercised"
