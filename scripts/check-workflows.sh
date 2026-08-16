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
require_line .github/workflows/release.yml 'run: just python-sdk-check' "use the Python SDK command surface"
require_line .github/workflows/release.yml 'needs: [publish-pypi, build-python-sdk]' "publish the Python SDK only after its exact CLI dependency"
require_line .github/workflows/release.yml 'name: python-sdk' "retain the Python SDK release artifact"
require_line .github/workflows/release.yml 'packages-dir: python-sdk-artifact' "publish the Python SDK through PyPI Trusted Publishing"
require_line .github/workflows/ci.yml 'uses: astral-sh/setup-uv@v6' "install the Python SDK toolchain"
# This is a literal shell expression in YAML.
# shellcheck disable=SC2016
require_line .github/workflows/release.yml 'if ! [[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+([+-][0-9A-Za-z.-]+)?$ ]]; then' "validate release-event tags before using them in paths"
require_line .github/workflows/ci.yml 'run: just deps-check' "use the dependency-audit command surface"
# Packaging is verified from the PR that precedes a release, never from the
# release: at a tag the binary already pins the core version that same run
# publishes, so `just package-crates` is structurally red there and a release
# gate carrying it skips every publish job (which is how v0.6.14 reached no
# registry at all). Keep it out of release.yml and out of `check`, which
# release.yml runs.
require_line .github/workflows/ci.yml 'run: just package-crates' "verify packaging from the PR gate"
if grep -q 'package-crates' .github/workflows/release.yml; then
  fail "release.yml must not run package-crates; it cannot pass at a release tag and would skip every publish job"
fi
if grep -qE '^check:.*\bpackage-crates\b' justfile; then
  fail "justfile 'check' must not depend on package-crates; release.yml runs check at the tag, where it cannot pass"
fi
require_line justfile 'gate remote="origin" base="": check deps-check package-crates semver-check' \
  "verify packaging and published-API compatibility in the pre-push gate instead"
# release-plz.toml's `semver_check` settles the version at release time but says
# nothing about the subject that drove it, so a break can still reach the
# changelog undeclared. This tier refuses that, and belongs to the PR gate for
# the same reason packaging does: there the subject is still one edit from right.
require_line .github/workflows/ci.yml 'run: just semver-check' "detect an undeclared API break from the PR gate"
require_line .github/workflows/ci.yml 'OH_SEMVER_NO_SKIP: "1"' "make absent cargo-semver-checks tooling red in CI rather than a silent skip"
if grep -q 'semver-check' .github/workflows/release.yml; then
  fail "release.yml must not run semver-check; the PR before a release proves it, and nothing may sit between a published Release and its publish jobs"
fi
require_line .github/workflows/ci.yml 'run: scripts/check-pr-title.sh' "validate the release-driving PR title"

# No workflow may install `just` from a third-party setup-just action: that
# fetches from a service outside this repo on every run, and an outage there
# takes a required check down for a reason unrelated to the change. The
# repository-local cached action is what replaced it. (Jobs that install a
# whole tool BUNDLE through taiki-e/install-action are a separate, deliberate
# choice and are not what this rule is about.)
if grep -rq 'setup-just@' .github/workflows/*.yml; then
  fail "workflows must install just through ./.github/actions/setup-just, not a third-party setup-just action"
fi
require_line .github/workflows/ci.yml 'uses: ./.github/actions/setup-just' "install just from the repository-local cached action"
[ -f .github/actions/setup-just/action.yml ] || fail ".github/actions/setup-just/action.yml must exist for the workflows that use it"

# API breaking-change detection must be both ENABLED and RUNNABLE. release-plz
# shells out to a `cargo-semver-checks` binary and merely warns when it is
# missing, so the setting alone is a check that can silently do nothing.
require_line release-plz.toml 'semver_check = true' "detect API breaking changes rather than trusting the commit subject"
require_line .github/workflows/release-plz.yml 'tool: cargo-semver-checks' "install the binary release-plz's semver check shells out to"
# A version probe passes on a tool that cannot build rustdoc at all — the exact
# pairing this repo has, since cargo-semver-checks resolves dependencies afresh
# and refuses a rustc below 1.93 while the workspace pins 1.86.0. Only running
# the analysis proves the gate works, and only a self-baseline keeps that run
# from deciding the release it is checking.
require_line .github/workflows/release-plz.yml \
  'run: cargo-semver-checks check-release --workspace --baseline-rev HEAD' \
  "run the semver analysis itself, since a version probe passes on a tool that cannot build rustdoc"
require_line .github/workflows/release-plz.yml 'RUSTUP_TOOLCHAIN=stable' \
  "give cargo-semver-checks the toolchain it needs; the pinned channel cannot build its rustdoc"
if grep -qE 'run: just (lint|lint-sh|test)$|run: bun run --cwd npm/oneharness-sdk (generate:check|build)$' .github/workflows/release.yml; then
  fail "release.yml must use just check/sdk-check instead of re-listing their stages"
fi

if [ "$fails" -ne 0 ]; then
  printf 'check-workflows: %d contract drift(s)\n' "$fails" >&2
  exit 1
fi
echo "check-workflows: ok"
