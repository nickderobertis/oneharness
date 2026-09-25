#!/usr/bin/env bash
# llmlint: ignore-file[new_code_lands_in_a_project] The rule presumes an Nx project graph; this repository has none by a recorded decision (`AGENTS.md`: root `just` delegates to Cargo/Bun without Nx because the two-package graph is static), so no project definition can cover this file and `just lint-workflows` is what runs it.
#
# Behavioral test of the MSRV half of the workflow drift gate.
#
# Both manifests inherit `rust-version` from `[workspace.package]`, so the gate
# must resolve each manifest's own `[package]` table rather than search the
# file. Each case drives the gate over a staged checkout; the green baseline
# keeps a refusal from passing for an unrelated reason.
#
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

work="$(mktemp -d)"
# Keep staged checkouts only on failure, when the diagnostic names one.
keep=0
cleanup() {
  if [ "$keep" = 1 ]; then
    echo "check-workflows-test: staged checkouts kept under $work" >&2
  else
    rm -rf "$work"
  fi
}
trap cleanup EXIT

fail_showing() {
  [ -z "${KEEP_FIXTURES:-}" ] || keep=1
  echo "check-workflows-test: $1" >&2
  echo "  what the gate said:" >&2
  sed 's/^/    /' "$work/out" >&2
  echo "  fix: what decides every case here is the MSRV block in scripts/check-workflows.sh — package_rust_version, which reads the [package] table alone, and the loop below it that resolves an 'inherit' through [workspace.package]. To see this case by hand, rerun as" >&2
  echo "         KEEP_FIXTURES=1 bash scripts/check-workflows-test.sh" >&2
  echo "       which leaves the staged checkout at ${root:-$work}, then drive the gate against it directly with" >&2
  echo "         bash ${root:-$work}/scripts/check-workflows.sh" >&2
  exit 1
}

# Everything the gate reads. Staged rather than mutated in place, so a case that
# leaves a manifest broken cannot reach the working tree.
staged=(
  scripts/check-workflows.sh
  scripts/ci-verdict.sh
  scripts/verify-published.sh
  rust-toolchain.toml
  Cargo.toml
  crates/oneharness-core/Cargo.toml
  pyproject.toml
  python/oneharness-sdk/pyproject.toml
  npm/oneharness/package.json
  npm/oneharness-sdk/package.json
  justfile
  release-plz.toml
  .github/actions/setup-just/action.yml
)

stage() {
  local root="$work/$1" file
  rm -rf "$root"
  for file in "${staged[@]}"; do
    mkdir -p "$root/$(dirname "$file")"
    cp "$file" "$root/$file"
  done
  mkdir -p "$root/.github/workflows"
  cp .github/workflows/*.yml "$root/.github/workflows/"
  printf '%s\n' "$root"
}

rewrite() {
  local root="$1" path="$2" program="$3"
  awk "$program" "$root/$path" >"$root/$path.rewritten"
  mv "$root/$path.rewritten" "$root/$path"
}

run_gate() {
  local root="$1"
  (cd "$root" && bash scripts/check-workflows.sh) >"$work/out" 2>&1
}

# The staged tree is the repository's own, so it must be green before any case
# mutates it — otherwise every refusal below could be this baseline failing.
root="$(stage baseline)"
if ! run_gate "$root"; then
  fail_showing "the unmodified staged checkout must pass"
fi

root="$(stage matching-literal)"
rewrite "$root" crates/oneharness-core/Cargo.toml '
  { sub(/^rust-version\.workspace = true$/, "rust-version = \"1.86\""); print }
'
if ! run_gate "$root"; then
  fail_showing "a package declaring the canonical rust-version literally must pass"
fi

root="$(stage missing-workspace-msrv)"
rewrite "$root" Cargo.toml '
  /^\[workspace\.package\]$/ { inside = 1; print; next }
  inside && /^\[/ { inside = 0 }
  !(inside && /^rust-version = /) { print }
'
if run_gate "$root"; then
  fail_showing "packages inheriting an absent workspace rust-version must be refused"
fi
for manifest in Cargo.toml crates/oneharness-core/Cargo.toml; do
  grep -Fxq "workflow drift: $manifest rust-version '' must match canonical toolchain '1.86.0'" "$work/out" ||
    fail_showing "an absent workspace rust-version must name $manifest, which inherits it"
done

# The inherited form resolves through [workspace.package]: a drift there reaches
# BOTH manifests, and each is named, because each is what a reader must fix.
root="$(stage workspace-drift)"
rewrite "$root" Cargo.toml '
  /^\[workspace\.package\]$/ { inside = 1; print; next }
  inside && /^\[/ { inside = 0 }
  inside && /^rust-version = / { print "rust-version = \"1.70\""; next }
  { print }
'
if run_gate "$root"; then
  fail_showing "a drifted [workspace.package] rust-version must be refused"
fi
for manifest in Cargo.toml crates/oneharness-core/Cargo.toml; do
  grep -Fxq "workflow drift: $manifest rust-version '1.70' must match canonical toolchain '1.86.0'" "$work/out" ||
    fail_showing "a drifted workspace rust-version must name $manifest, which inherits it"
done

root="$(stage literal-drift)"
rewrite "$root" crates/oneharness-core/Cargo.toml '
  { sub(/^rust-version\.workspace = true$/, "rust-version = \"1.70\""); print }
'
if run_gate "$root"; then
  fail_showing "a drifted literal rust-version must be refused"
fi
grep -Fq "crates/oneharness-core/Cargo.toml rust-version '1.70'" "$work/out" ||
  fail_showing "a drifted literal rust-version must name the manifest that states it"
# Anchored, and not -F: every diagnostic is prefixed `workflow drift: `, and
# the root manifest's path is a suffix of the core manifest's, so an unanchored
# search for it matches the refusal that just fired.
if grep -q "^workflow drift: Cargo\.toml rust-version" "$work/out"; then
  fail_showing "the root manifest still resolves its own MSRV and must not be named"
fi

# The regression case: the root [package] declares no MSRV while
# [workspace.package] in the same file still does. A whole-file search passes
# here; only a table-scoped read refuses it.
root="$(stage no-msrv-root)"
rewrite "$root" Cargo.toml '
  !/^rust-version\.workspace = true$/ { print }
'
if run_gate "$root"; then
  fail_showing "the root manifest declaring no rust-version must be refused, not satisfied by the [workspace.package] line in the same file"
fi
grep -Fxq "workflow drift: Cargo.toml declares no rust-version in its [package] table; state one, or inherit the workspace's with 'rust-version.workspace = true'" "$work/out" ||
  fail_showing "the root manifest declaring no rust-version must be named as such"

root="$(stage no-msrv)"
rewrite "$root" crates/oneharness-core/Cargo.toml '
  !/^rust-version\.workspace = true$/ { print }
'
if run_gate "$root"; then
  fail_showing "a manifest declaring no rust-version must be refused"
fi
grep -Fq "crates/oneharness-core/Cargo.toml declares no rust-version" "$work/out" ||
  fail_showing "a manifest declaring no rust-version must be named as such"

echo "check-workflows-test: ok"
