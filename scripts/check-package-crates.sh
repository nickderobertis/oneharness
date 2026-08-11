#!/usr/bin/env bash
# Subprocess-level coverage for package-crates' release transition decisions.
set -euo pipefail

cd "$(dirname "$0")/.."

fail() {
  echo "check-package-crates: $1; ${2:-fix scripts/package-crates.sh and rerun 'bash scripts/check-package-crates.sh'}" >&2
  exit 1
}

assert_contains() {
  grep -Fq "$1" "$2" || fail "missing expected diagnostic '$1'"
}

# These values intentionally duplicate release-plz's contract; pin the copy so
# an automation edit cannot silently make the pre-release gate disagree.
grep -Fq 'release_commits = "(?s)^(feat|fix|perf)' release-plz.toml ||
  fail "release-worthy commit contract drifted from release-plz.toml"
grep -Fq 'git_tag_name = "oneharness-core-v{{ version }}"' release-plz.toml ||
  fail "core tag namespace drifted from release-plz.toml"
grep -Fq -- "--list 'oneharness-core-v*'" scripts/package-crates.sh ||
  fail "package script core tag glob drifted from release-plz.toml"
grep -Fq "release_subject_re='^(feat|fix|perf)" scripts/package-crates.sh ||
  fail "package script release subject regex drifted from release-plz.toml"
grep -Fq "grep -q '^BREAKING CHANGE:'" scripts/package-crates.sh ||
  fail "package script breaking-change regex drifted from release-plz.toml"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin"

cat >"$work/bin/cargo" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"$CALL_LOG"
if [[ $* == *"crates/oneharness-core/Cargo.toml"* && ${CORE_PACKAGE:-ok} == fail ]]; then
  echo "simulated core package failure" >&2
  exit 101
fi
if [[ $* == *"Cargo.toml"* && $* != *"crates/oneharness-core/Cargo.toml"* && ${BINARY_PACKAGE:-ok} == fail ]]; then
  echo "simulated registry dependency mismatch" >&2
  exit 101
fi
EOF

cat >"$work/bin/git" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
case "$1" in
  tag)
    if [[ ${NO_TAG:-0} == 0 || -f ${CALL_LOG}.fetched ]]; then
      echo oneharness-core-v0.6.11
    fi
    ;;
  fetch)
    case ${FETCH_MODE:-ok} in
      ok) ;;
      fail) exit 1 ;;
      recover) touch "${CALL_LOG}.fetched" ;;
      *) echo "invalid FETCH_MODE" >&2; exit 2 ;;
    esac
    ;;
  diff)
    case ${GIT_DIFF_MODE:-clean} in
      clean) exit 0 ;;
      dirty) exit 1 ;;
      *) echo "invalid GIT_DIFF_MODE" >&2; exit 2 ;;
    esac
    ;;
  rev-list)
    case ${COMMIT_KIND:-none} in
      none) ;;
      fix|breaking|noncore) echo candidate ;;
      *) echo "invalid COMMIT_KIND" >&2; exit 2 ;;
    esac
    ;;
  show)
    if [[ $* == *'%s'* ]]; then
      case ${COMMIT_KIND:-none} in
        fix|noncore) echo 'fix(core): publish changed API' ;;
        breaking) echo 'chore: publish changed API' ;;
        none) echo 'test: no release' ;;
        *) echo "invalid COMMIT_KIND" >&2; exit 2 ;;
      esac
    elif [[ ${COMMIT_KIND:-none} == breaking ]]; then
      echo 'BREAKING CHANGE: changed core API'
    else
      echo
    fi
    ;;
  diff-tree) if [[ ${COMMIT_KIND:-none} != noncore ]]; then echo crates/oneharness-core/src/lib.rs; fi ;;
  *) exit 2 ;;
esac
EOF
chmod +x "$work/bin/cargo" "$work/bin/git"

run_case() {
  CALL_LOG="$work/calls" PATH="$work/bin:$PATH" "$@"
}

: >"$work/calls"
run_case just package-crates >"$work/out"
[[ $(wc -l <"$work/calls") -eq 2 ]] || fail "happy path did not package core then binary"
assert_contains 'package-crates: ok' "$work/out"

if run_case env CORE_PACKAGE=fail just package-crates >"$work/out" 2>&1; then
  fail "core package failure unexpectedly passed"
fi
assert_contains "core package verification failed" "$work/out"

if run_case env NO_TAG=1 just package-crates >"$work/out" 2>&1; then
  fail "missing-tag case unexpectedly passed"
fi
assert_contains 'no merged oneharness-core release tag found after fetching origin' "$work/out"

if run_case env NO_TAG=1 FETCH_MODE=fail just package-crates >"$work/out" 2>&1; then
  fail "failed tag fetch unexpectedly passed"
fi
assert_contains 'could not be fetched from origin' "$work/out"

rm -f "$work/calls.fetched"
if ! run_case env NO_TAG=1 FETCH_MODE=recover just package-crates >"$work/out" 2>&1; then
  cat "$work/out" >&2
  fail "tag fetch recovery did not continue to package verification"
fi
assert_contains 'package-crates: ok' "$work/out"

if run_case env BINARY_PACKAGE=fail just package-crates >"$work/out" 2>&1; then
  fail "incompatible published dependency unexpectedly passed"
fi
assert_contains "commit the core API change as fix/feat/perf" "$work/out"

if ! run_case env BINARY_PACKAGE=fail COMMIT_KIND=fix just package-crates >"$work/out" 2>&1; then
  cat "$work/out" >&2
  fail "release-worthy core fix did not permit the release-plz transition"
fi
assert_contains "awaits release-plz's core version bump" "$work/out"

if ! run_case env BINARY_PACKAGE=fail COMMIT_KIND=breaking just package-crates >"$work/out" 2>&1; then
  cat "$work/out" >&2
  fail "breaking core change did not permit the release-plz transition"
fi
assert_contains "awaits release-plz's core version bump" "$work/out"

if run_case env BINARY_PACKAGE=fail COMMIT_KIND=noncore just package-crates >"$work/out" 2>&1; then
  fail "release-worthy non-core commit unexpectedly excused an incompatible dependency"
fi
assert_contains "cannot be packaged against its published oneharness-core dependency" "$work/out"

if ! run_case env BINARY_PACKAGE=fail GIT_DIFF_MODE=dirty just package-crates >"$work/out" 2>&1; then
  cat "$work/out" >&2
  fail "dirty core checkpoint did not permit pre-commit package verification"
fi
assert_contains "awaits release-plz's core version bump" "$work/out"

echo "check-package-crates: ok"
