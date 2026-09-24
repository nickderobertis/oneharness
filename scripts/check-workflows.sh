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

# The `rust-version` a manifest's `[package]` table states, read from that table
# alone. Prints `literal <value>` where one is stated, `inherit` where the
# manifest takes the workspace's with `rust-version.workspace = true`, and
# nothing where the table declares neither.
#
# Reading the table rather than the file is what makes the two answers
# distinguishable: `[workspace.package]` now carries a `rust-version = "..."`
# line of its own, and a whole-file search finds it for every manifest in the
# workspace — including one that declares no MSRV at all.
package_rust_version() {
  awk '
    $0 == "[package]" { inside = 1; next }
    inside && /^\[/ { exit }
    inside && match($0, /^rust-version[[:space:]]*=[[:space:]]*"[^"]+"/) {
      line = substr($0, RSTART, RLENGTH)
      sub(/^[^"]*"/, "", line)
      sub(/"$/, "", line)
      print "literal " line
      exit
    }
    inside && /^rust-version\.workspace[[:space:]]*=[[:space:]]*true[[:space:]]*$/ {
      print "inherit"
      exit
    }
  ' "$1"
}

# Like require_line, but for a line that may only run behind a condition: the
# guard must be the line immediately above it. A `run:` on its own is the shape
# this repository is moving away from — a check re-run on every release — so
# presence alone is not the contract. EVERY occurrence is checked, because one
# guarded copy says nothing about a second that runs unconditionally.
require_guarded() {
  local file="$1" line="$2" guard="$3" description="$4" numbers number previous
  numbers="$(grep -Fn -- "$line" "$file" | cut -d: -f1)"
  if [ -z "$numbers" ]; then
    fail "$file must $description"
    return
  fi
  for number in $numbers; do
    previous=
    [ "$number" -gt 1 ] && previous="$(sed -n "$((number - 1))p" "$file" | sed 's/^[[:space:]]*//')"
    [ "$previous" = "$guard" ] || fail "$file must $description"
  done
}

require_gate_dependency() {
  local job
  for job in publish-crates upload build-wheels build-python-sdk build-npm build-node-sdk; do
    awk -v job="$job" '
      $0 == "  " job ":" { inside=1; found=1; next }
      inside && /^  [a-z][a-z-]*:/ { exit }
      inside && /^    needs: gate$/ { gated=1 }
      END { if (!found || !gated) exit 1 }
    ' .github/workflows/release.yml ||
      fail "release.yml job $job must depend on gate before constructing or publishing an artifact"
  done
}

# rust-toolchain.toml is canonical. Cargo requires an MSRV in each publishable
# manifest, while actions-rust-lang/setup-rust-toolchain reads the committed
# toolchain file directly. Both manifests inherit theirs from
# `[workspace.package]`, so what is compared here is the value each one
# RESOLVES to — the workspace's where it is inherited, the manifest's own where
# it is stated — and an MSRV that reaches neither manifest is still named.
toolchain="$(sed -n 's/^channel = "\([^"]*\)"$/\1/p' rust-toolchain.toml)"
if ! [[ "$toolchain" =~ ^[0-9]+\.[0-9]+\.0$ ]]; then
  fail "rust-toolchain.toml must contain one stable x.y.0 channel"
else
  msrv="${toolchain%.0}"
  workspace_msrv="$(sed -n '/^\[workspace\.package\]$/,/^\[/ s/^rust-version = "\([^"]*\)"$/\1/p' Cargo.toml | head -n 1)"
  for manifest in Cargo.toml crates/oneharness-core/Cargo.toml; do
    stated="$(package_rust_version "$manifest")"
    case "$stated" in
      "literal "*) declared="${stated#literal }" ;;
      inherit) declared="$workspace_msrv" ;;
      *)
        fail "$manifest declares no rust-version in its [package] table; state one, or inherit the workspace's with 'rust-version.workspace = true'"
        continue
        ;;
    esac
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
require_line .github/workflows/release.yml 'run: scripts/publish-crates.sh' "use the validated crates.io publisher"
# Pin the release workflow to the verdict selector and the CI job names it
# consumes; check-ci-verdict.sh exercises the selector's status branches.
require_line .github/workflows/release.yml 'run: scripts/ci-verdict.sh' \
  "read CI's verdict for the tagged commit instead of re-running the gate CI already ran on it"
require_line .github/workflows/ci.yml 'os: [ubuntu-latest, macos-latest, windows-latest]' \
  "keep the declared check matrix matched to the release verdict selector"
require_line .github/workflows/ci.yml 'branches: [main]' \
  "keep the CI push branch matched to the release verdict selector"
require_line .github/workflows/ci.yml '  check:' \
  "keep the check job named as the release verdict selector expects"
require_line scripts/ci-verdict.sh 'event=push&branch=main' \
  "select CI's main-branch push runs"
require_line scripts/ci-verdict.sh '["check (macos-latest)", "check (ubuntu-latest)", "check (windows-latest)"]' \
  "require every CI check matrix job before skipping the release gate"
require_line pyproject.toml 'name = "oneharness-cli"' "keep the PyPI CLI name used by publication verification"
require_line python/oneharness-sdk/pyproject.toml 'name = "oneharness-sdk"' "keep the PyPI SDK name used by publication verification"
require_line npm/oneharness/package.json '"name": "oneharness-cli"' "keep the npm CLI name used by publication verification"
require_line npm/oneharness-sdk/package.json '"name": "@oneharness/sdk"' "keep the npm SDK name used by publication verification"
while IFS='|' read -r line description; do
  require_line scripts/verify-published.sh "$line" "$description"
done <<'INSTALL_LINES'
pip install --no-cache-dir "oneharness-cli==$version"|install the published PyPI CLI name
pip install --no-cache-dir "oneharness-sdk==$version"|install the published PyPI SDK name
npm install -g --prefer-online "oneharness-cli@$version"|install the published npm CLI name
npm install --prefer-online "@oneharness/sdk@$version"|install the published npm SDK name
INSTALL_LINES
require_gate_dependency
require_line .github/workflows/release.yml 'actions: read' \
  "hold the permission that lets it read CI's own result"
# This is a literal GitHub expression in YAML.
# shellcheck disable=SC2016
require_line .github/workflows/release.yml 'needs_check: ${{ steps.verdict.outputs.needs_check }}' \
  "publish that verdict to the jobs that would otherwise re-run a check"
require_guarded .github/workflows/release.yml 'run: just check' \
  "if: steps.verdict.outputs.needs_check == 'true'" \
  "run the complete repository gate only when CI reached no verdict for the tagged commit"
# `just check` contains both SDK gates, so the fallback above has already run
# them on this commit. A release job running either again is the same commit
# swept twice, and neither belongs here any more.
if grep -qE '^[[:space:]]*run: just (sdk-check|python-sdk-check)$' .github/workflows/release.yml; then
  fail "release.yml must not run an SDK gate; 'just check' in the gate job contains both, so running one here sweeps the same commit twice"
fi
# The gate builds the gitignored SDK dist on its way past; skipping the gate must
# not leave the pack with nothing to pack.
require_line .github/workflows/release.yml 'run: just sdk-build' \
  "build the Node SDK's publishable sources even on the release that skips the gate"
# Every post-publication wait is the consumer's own operation, retried to a
# bound. A registry answers its metadata API before the index a consumer reads —
# PyPI's JSON before the simple index, `npm view` before the per-platform package
# an optional dependency resolves — so a metadata probe standing in for the
# install reddens a release that published perfectly.
# The target list is verify-published.sh's own, read out of the allowlist it
# validates against rather than restated here — a target added there and not
# called from the workflow is exactly the drift this catches.
verify_targets="$(sed -n 's/^  \([a-z| -]*\)) ;;$/\1/p' scripts/verify-published.sh | head -1 | tr -d ' ' | tr '|' ' ')"
if [ -z "$verify_targets" ]; then
  fail "scripts/verify-published.sh must keep its accepted targets in one 'case' allowlist ending in ') ;;', which is what names the workflow calls this gate requires"
fi
for verify_target in $verify_targets; do
  require_line .github/workflows/release.yml \
    "run: scripts/verify-published.sh $verify_target \"\${GITHUB_REF_NAME#v}\"" \
    "verify the published $verify_target with the consumer's own install"
done
# Comment lines are exempt: the rule is about what the release RUNS, and the
# job comments have to be able to say what they replaced.
if grep -vE '^[[:space:]]*#' .github/workflows/release.yml | grep -qE 'pypi\.org/pypi/|npm view'; then
  fail "release.yml must not wait on a registry's metadata API; retry the consumer's own install through scripts/verify-published.sh instead"
fi
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
