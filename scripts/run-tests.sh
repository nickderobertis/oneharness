#!/usr/bin/env bash
#
# Run the workspace suite (core unit tests + binary unit and integration tests),
# preferring nextest and falling back to cargo test. `just test` runs this under
# scripts/check-temp-leaks.sh, which is what refuses a run that leaked scratch
# space; this script only runs the suite.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

features="${FEATURES:-mock-harness}"
if command -v cargo-nextest >/dev/null 2>&1; then
  exec cargo nextest run --workspace --features "$features" --locked
fi
exec cargo test --workspace --features "$features" --locked
