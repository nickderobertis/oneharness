#!/usr/bin/env bash
# Deterministic drift gate for repository-wide workflow contracts that GitHub
# Actions cannot derive directly from Cargo metadata.
set -euo pipefail

cd "$(dirname "$0")/.."

fails=0
fail() {
  printf 'workflow drift: %s\n' "$1" >&2
  fails=$((fails + 1))
}

require_line() {
  local file="$1" line="$2" description="$3"
  grep -Fq -- "$line" "$file" || fail "$file must $description"
}

# rust-toolchain.toml is canonical. Cargo requires an MSRV in each publishable
# manifest, while actions-rust-lang/setup-rust-toolchain reads the committed
# toolchain file directly.
toolchain="$(sed -n 's/^channel = "\([^"]*\)"$/\1/p' rust-toolchain.toml)"
if ! [[ "$toolchain" =~ ^[0-9]+\.[0-9]+\.0$ ]]; then
  fail "rust-toolchain.toml must contain one stable x.y.0 channel"
else
  msrv="${toolchain%.0}"
  for manifest in Cargo.toml crates/oneharness-core/Cargo.toml; do
    declared="$(sed -n 's/^rust-version = "\([^"]*\)"$/\1/p' "$manifest")"
    [ "$declared" = "$msrv" ] || fail "$manifest rust-version '$declared' must match canonical toolchain '$toolchain'"
  done

fi
if grep -q 'dtolnay/rust-toolchain@' .github/workflows/*.yml; then
  fail "workflows must read rust-toolchain.toml through actions-rust-lang/setup-rust-toolchain"
fi
for workflow in .github/workflows/ci.yml .github/workflows/e2e-*.yml .github/workflows/release-plz.yml .github/workflows/release.yml; do
  require_line "$workflow" 'uses: actions-rust-lang/setup-rust-toolchain@v1' "install Rust from rust-toolchain.toml"
done

# Tagging and release-PR creation must share one guarded workflow and one
# release-plz version. The published Release then enters the complete project
# gate and validated crates publisher through release.yml.
[ ! -e .github/workflows/tag-release.yml ] || fail "tag-release.yml must stay consolidated into release-plz.yml"
require_line release-plz.toml 'publish = false' "leave registry publishing to release.yml"
# These are literal GitHub/shell expressions in YAML.
# shellcheck disable=SC2016
require_line .github/workflows/release-plz.yml 'run: release-plz release --git-token "$GITHUB_TOKEN"' "tag in the guarded release-plz lifecycle"
# shellcheck disable=SC2016
require_line .github/workflows/release-plz.yml 'run: release-plz release-pr ${{ steps.baseline.outputs.arg }} --git-token "$GITHUB_TOKEN"' "create the next release PR after tagging"
tag_line="$(grep -nF 'run: release-plz release --git-token' .github/workflows/release-plz.yml | cut -d: -f1)"
pr_line="$(grep -nF 'run: release-plz release-pr ' .github/workflows/release-plz.yml | cut -d: -f1)"
if [ -n "$tag_line" ] && [ -n "$pr_line" ] && [ "$tag_line" -ge "$pr_line" ]; then
  fail "release-plz.yml must tag an already-bumped version before computing the next release PR"
fi

require_line .github/workflows/release.yml 'types: [published]' "start distribution from a published GitHub Release"
require_line .github/workflows/release.yml 'run: just check' "run the complete repository gate before publishing"
require_line .github/workflows/release.yml 'run: scripts/publish-crates.sh' "use the validated crates.io publisher"
require_line .github/workflows/release.yml 'run: just sdk-check' "use the Node SDK command surface"
# This is a literal shell expression in YAML.
# shellcheck disable=SC2016
require_line .github/workflows/release.yml 'if ! [[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+([+-][0-9A-Za-z.-]+)?$ ]]; then' "validate release-event tags before using them in paths"
require_line .github/workflows/ci.yml 'run: just deps-check' "use the dependency-audit command surface"
require_line .github/workflows/ci.yml 'run: scripts/check-pr-title.sh' "validate the release-driving PR title"
if grep -qE 'run: just (lint|lint-sh|test)$|run: bun run --cwd npm/oneharness-sdk (generate:check|build)$' .github/workflows/release.yml; then
  fail "release.yml must use just check/sdk-check instead of re-listing their stages"
fi

if [ "$fails" -ne 0 ]; then
  printf 'check-workflows: %d contract drift(s)\n' "$fails" >&2
  exit 1
fi
echo "check-workflows: ok"
