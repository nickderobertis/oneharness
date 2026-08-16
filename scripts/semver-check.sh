#!/usr/bin/env bash
# Refuse an UNDECLARED API break, in the one window where it is still cheap.
#
# `cargo semver-checks check-release` compares the working tree against the last
# version on crates.io. The tree always carries that same version — release-plz
# bumps only inside its own release PR — so "requires new major version" is what
# EVERY breaking change looks like here, the legitimate ones included. The
# finding is therefore not the verdict: a break the release-driving subject
# DECLARES (`!` or a `BREAKING CHANGE:` body) is one release-plz will bump the
# major for. A break nobody declared is the defect this catches — it would
# otherwise ship as a patch or a minor and break consumers at `cargo update`.
#
# This is the gate `release-plz.toml` defers to. release-plz's own `semver_check`
# stays off because it cannot tell those two apart and would block every
# breaking release; here the conventional-commit type decides, exactly as that
# file says it should.
set -euo pipefail

cd "$(dirname "$0")/.."

# rust-toolchain.toml pins the channel the product is built with, which is older
# than cargo-semver-checks can read the baseline's rustdoc JSON on. Name a newer
# one rather than moving the product's pin for a lint.
toolchain="${SEMVER_TOOLCHAIN:-stable}"

# An absent tool is a skip locally and a failure in CI, the same split the live
# e2e suites use: a developer can run the pre-push gate on a clean box, while the
# tier that is supposed to enforce this cannot quietly prove nothing.
unavailable() {
  if [ "${OH_SEMVER_NO_SKIP:-}" = "1" ]; then
    echo "semver-check: $1" >&2
    exit 1
  fi
  echo "semver-check: skipped ($1)" >&2
  exit 0
}

if ! command -v cargo-semver-checks >/dev/null 2>&1; then
  unavailable "cargo-semver-checks not installed: 'cargo install cargo-semver-checks' / https://github.com/obi1kenobi/cargo-semver-checks"
fi
if ! cargo "+$toolchain" --version >/dev/null 2>&1; then
  unavailable "the '$toolchain' toolchain is not installed: 'rustup toolchain install $toolchain' (cargo-semver-checks needs a newer rustc than rust-toolchain.toml pins; set SEMVER_TOOLCHAIN to name another)"
fi

# A breaking change is declared by whatever release-plz will actually read. On a
# pull request that is the squash subject — the PR title, which
# `scripts/check-pr-title.sh` already holds to Conventional Commits — and
# everywhere else it is the commits themselves.
breaking_subject_re='^(feat|fix|perf|refactor|docs|test|chore|ci|build|style|revert)(\([^)]+\))?!:'

declared_in_title() {
  [ -n "${PR_TITLE:-}" ] && [[ "$PR_TITLE" =~ $breaking_subject_re ]]
}

# Every commit since this crate's own release tag, since "the last release" is a
# different commit per crate (see release-plz.toml's tag namespaces).
declared_in_commits() {
  local tag_glob="$1" path="$2" last_tag subject body
  last_tag="$(git tag --merged HEAD --list "$tag_glob" --sort=-version:refname | head -n 1)"
  if [ -z "$last_tag" ]; then
    return 1
  fi
  while IFS= read -r commit; do
    subject="$(git show -s --format=%s "$commit")"
    body="$(git show -s --format=%b "$commit")"
    if [[ "$subject" =~ $breaking_subject_re ]] || grep -q '^BREAKING CHANGE:' <<<"$body"; then
      if git diff-tree --no-commit-id --name-only -r "$commit" -- "$path" | grep -q .; then
        return 0
      fi
    fi
  done < <(git rev-list "$last_tag..HEAD")
  return 1
}

# A developer has to be able to run the gate before committing the change that
# declares the break; the commit subject is enforced once it exists.
uncommitted() {
  local path="$1"
  ! git diff --quiet -- "$path" || ! git diff --cached --quiet -- "$path"
}

output="$(mktemp)"
trap 'rm -f "$output"' EXIT

check_crate() {
  local package="$1" tag_glob="$2" path="$3"
  if cargo "+$toolchain" semver-checks check-release --package "$package" >"$output" 2>&1; then
    return 0
  fi
  if ! grep -q 'requires new major version' "$output"; then
    # Not a verdict about the API at all: an unreachable crates.io index, a
    # baseline that was never published, a build that does not compile. The
    # diagnostics are the finding.
    cat "$output" >&2
    echo "semver-check: cargo-semver-checks could not judge \`$package\`; fix the diagnostics above and rerun 'just semver-check'" >&2
    exit 1
  fi
  if declared_in_title || declared_in_commits "$tag_glob" "$path" || uncommitted "$path"; then
    echo "semver-check: \`$package\` has a DECLARED breaking change; release-plz will bump its major" >&2
    return 0
  fi
  cat "$output" >&2
  echo "semver-check: \`$package\` breaks its published API and nothing declares it; land the change as 'feat!: …' (or with a 'BREAKING CHANGE:' body) so release-plz bumps the major, or keep the API additive" >&2
  exit 1
}

# Both published crates, each against its own tag namespace. The binary crate's
# `src/` is what a `vX.Y.Z` release covers; the engine has its own.
check_crate oneharness-core 'oneharness-core-v*' crates/oneharness-core
check_crate oneharness 'v*' src

echo "semver-check: ok"
