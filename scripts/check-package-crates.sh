#!/usr/bin/env bash
# Subprocess-level coverage for package-crates' release transition decisions.
set -euo pipefail

cd "$(dirname "$0")/.."

# These values intentionally duplicate release-plz's contract; pin the copy so
# an automation edit cannot silently make the pre-release gate disagree.
grep -Fq 'release_commits = "(?s)^(feat|fix|perf)' release-plz.toml
grep -Fq 'git_tag_name = "oneharness-core-v{{ version }}"' release-plz.toml

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin"

cat >"$work/bin/cargo" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"$CALL_LOG"
if [[ $* == *"Cargo.toml"* && $* != *"crates/oneharness-core/Cargo.toml"* && ${BINARY_PACKAGE:-ok} == fail ]]; then
  echo "simulated registry dependency mismatch" >&2
  exit 101
fi
EOF

cat >"$work/bin/git" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
case "$1" in
  tag) if [[ ${NO_TAG:-0} == 0 ]]; then echo oneharness-core-v0.6.11; fi ;;
  diff) exit "${DIRTY_CORE:-0}" ;;
  rev-list) if [[ ${PENDING_FIX:-0} == 1 ]]; then echo fixcommit; fi ;;
  show)
    if [[ $* == *'%s'* ]]; then echo 'fix(core): publish changed API'; else echo; fi
    ;;
  diff-tree) if [[ ${PENDING_FIX:-0} == 1 ]]; then echo crates/oneharness-core/src/lib.rs; fi ;;
  *) exit 2 ;;
esac
EOF
chmod +x "$work/bin/cargo" "$work/bin/git"

run_case() {
  CALL_LOG="$work/calls" PATH="$work/bin:$PATH" "$@"
}

: >"$work/calls"
run_case scripts/package-crates.sh >/dev/null
[[ $(wc -l <"$work/calls") -eq 2 ]]

if run_case env NO_TAG=1 scripts/package-crates.sh >"$work/out" 2>&1; then
  echo "check-package-crates: missing-tag case unexpectedly passed" >&2
  exit 1
fi
grep -Fq 'fetch tags from origin and retry' "$work/out"

if run_case env BINARY_PACKAGE=fail scripts/package-crates.sh >"$work/out" 2>&1; then
  echo "check-package-crates: incompatible published dependency unexpectedly passed" >&2
  exit 1
fi
grep -Fq "commit the core API change as fix/feat/perf" "$work/out"

run_case env BINARY_PACKAGE=fail PENDING_FIX=1 scripts/package-crates.sh >"$work/out" 2>&1
grep -Fq "awaits release-plz's core version bump" "$work/out"

if run_case env ONEHARNESS_BIN="$work/not-executable" scripts/smoke.sh >"$work/out" 2>&1; then
  echo "check-package-crates: invalid ONEHARNESS_BIN unexpectedly passed smoke" >&2
  exit 1
fi
grep -Fq 'ONEHARNESS_BIN is not an executable file' "$work/out"

echo "check-package-crates: ok"
