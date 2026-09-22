#!/usr/bin/env bash
# llmlint: ignore-file[new_code_lands_in_a_project] The rule presumes an Nx project graph; this repository has none by a recorded decision (`AGENTS.md`: root `just` delegates to Cargo/Bun without Nx because the two-package graph is static), so no project definition can cover this file and `just lint-workflows` is what runs it.
#
# Behavioral test of the MSRV half of the workflow drift gate.
#
# `scripts/check-workflows.sh` holds every publishable manifest's `rust-version`
# to the canonical `rust-toolchain.toml` channel. Both manifests now inherit
# that field from `[workspace.package]`, which means the gate no longer reads a
# value out of the manifest it is judging — and a gate that reads the wrong file
# passes for a reason unrelated to what it checks. That already happened once:
# a whole-file search for `rust-version = "…"` found the workspace table's line
# while looking at the root manifest, so the root "passed" whatever its own
# [package] table said, including nothing at all.
#
# So the resolution is driven here against a staged checkout, once per way a
# manifest can state its MSRV and once per way that MSRV can drift, and the gate
# is asserted to go red naming the manifest it read. The green baseline matters
# as much as the refusals: without it, "the gate accepts the inherited form" and
# "the gate is red for some unrelated reason" would look identical.
#
# Quiet on success, one line. On failure it prints what the gate said.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

work="$(mktemp -d)"
# The staged checkouts are what a failure is diagnosed from, and they are gone
# by the time anyone reads the diagnostic. KEEP_FIXTURES leaves them, so the
# `fix:` line below can name a checkout that will still be there.
if [ -n "${KEEP_FIXTURES:-}" ]; then
  trap 'echo "check-workflows-test: staged checkouts kept under $work" >&2' EXIT
else
  trap 'rm -rf "$work"' EXIT
fi

fail_showing() {
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
  rust-toolchain.toml
  Cargo.toml
  crates/oneharness-core/Cargo.toml
  justfile
  release-plz.toml
  .github/actions/setup-just/action.yml
)

# $1 = fixture name. Leaves a fresh staged checkout at $work/$1 and prints it.
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

# Rewrite a staged file through an awk program. $1 = fixture root,
# $2 = repository-relative path, $3 = awk program.
rewrite() {
  local root="$1" path="$2" program="$3"
  awk "$program" "$root/$path" >"$root/$path.rewritten"
  mv "$root/$path.rewritten" "$root/$path"
}

# Runs the staged gate. Prints nothing; leaves its combined output in $work/out
# and returns the gate's own exit status.
run_gate() {
  local root="$1"
  bash "$root/scripts/check-workflows.sh" >"$work/out" 2>&1
}

# The staged tree is the repository's own, so it must be green before any case
# mutates it — otherwise every refusal below could be this baseline failing.
root="$(stage baseline)"
if ! run_gate "$root"; then
  fail_showing "the unmodified staged checkout must pass"
fi

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
  grep -Fq "$manifest rust-version '1.70'" "$work/out" ||
    fail_showing "a drifted workspace rust-version must name $manifest, which inherits it"
done

# A manifest that states its own is still read from its own [package] table.
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

# The case the whole-file search could not see: a manifest declaring no MSRV at
# all, while [workspace.package] still carries one for the search to find.
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
