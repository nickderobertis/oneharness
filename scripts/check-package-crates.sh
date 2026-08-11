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
  if [[ ${BINARY_FAILURE_KIND:-core-mismatch} == missing-feature ]]; then
    # Captured verbatim from a real `cargo package` on a tree whose core had
    # gained the `mock-harness` feature the published version lacked. This
    # failure happens during RESOLUTION, so — unlike every other case here —
    # it names no registry source path; a detector requiring one misses it.
    echo 'error: failed to prepare local package for uploading' >&2
    echo '' >&2
    echo 'Caused by:' >&2
    echo '  failed to select a version for `oneharness-core`.' >&2
    echo '      ... required by package `oneharness v0.6.13 (/repo)`' >&2
    echo '  versions that meet the requirements `^0.6.11` are: 0.6.11' >&2
    echo '' >&2
    echo '  the package `oneharness` depends on `oneharness-core`, with features: `mock-harness` but `oneharness-core` does not have these features.' >&2
    echo '' >&2
    echo '  failed to select a version for `oneharness-core` which could resolve this conflict' >&2
  elif [[ ${BINARY_FAILURE_KIND:-core-mismatch} == version-select ]]; then
    echo 'error: failed to select a version for the requirement `oneharness-core = "^0.6.12"`' >&2
    echo 'candidate versions found which did not match: 0.6.11' >&2
    echo 'location searched: crates.io index' >&2
    echo 'required by package `oneharness v0.6.14`' >&2
  elif [[ ${BINARY_FAILURE_KIND:-core-mismatch} == core-mismatch ]]; then
    if [[ ${REGISTRY_PATH_KIND:-unix} == windows ]]; then
      echo '  --> C:\registry\src\oneharness-core-0.6.11\src\io\http_turn.rs:592:8' >&2
    else
      echo '  --> /registry/src/oneharness-core-0.6.11/src/io/http_turn.rs:592:8' >&2
    fi
    echo 'error: could not compile `oneharness` (lib) due to previous error' >&2
  else
    echo 'error: simulated unrelated binary package failure' >&2
  fi
  exit 101
fi
EOF

cat >"$work/bin/git" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
case "$1" in
  tag)
    # The glob asks which core releases are visible at all; an exact name asks
    # whether release-plz has already tagged the version the binary pins.
    if [[ $* == *'oneharness-core-v*'* ]]; then
      if [[ ${NO_TAG:-0} == 0 || -f ${CALL_LOG}.fetched ]]; then
        echo oneharness-core-v0.6.11
      fi
    elif [[ ${CORE_VERSION_TAGGED:-0} == 1 ]]; then
      echo "${*##* }"
    fi
    ;;
  fetch)
    if [[ $* != *--force* ]]; then
      echo "tag fetch omitted --force" >&2
      exit 2
    fi
    if [[ ${SHALLOW_REPOSITORY:-false} == true && $* != *--unshallow* ]]; then
      echo "shallow fetch omitted --unshallow" >&2
      exit 2
    fi
    case ${FETCH_MODE:-ok} in
      ok) ;;
      fail) exit 1 ;;
      recover) touch "${CALL_LOG}.fetched" ;;
      *) echo "invalid FETCH_MODE" >&2; exit 2 ;;
    esac
    ;;
  rev-parse)
    case ${SHALLOW_REPOSITORY:-false} in
      true|false) echo "${SHALLOW_REPOSITORY:-false}" ;;
      fail) exit 2 ;;
      invalid) echo unknown ;;
      *) echo "invalid SHALLOW_REPOSITORY" >&2; exit 2 ;;
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
if ! run_case just package-crates >"$work/out" 2>&1; then
  cat "$work/out" >&2
  fail "happy-path package verification failed; fix the diagnostic above and rerun 'bash scripts/check-package-crates.sh'"
fi
[[ $(wc -l <"$work/calls") -eq 2 ]] || fail "happy path did not package core then binary"
assert_contains 'package-crates: ok' "$work/out"

if run_case env CORE_PACKAGE=fail just package-crates >"$work/out" 2>&1; then
  fail "core package failure unexpectedly passed"
fi
assert_contains "core package verification failed" "$work/out"

# Release history is read only to classify a packaging failure, so a checkout
# that cannot reach it must still verify packaging. This is the shape that took
# a whole release down: a check whose own diagnostics failed closed.
: >"$work/calls"
if ! run_case env NO_TAG=1 FETCH_MODE=fail SHALLOW_REPOSITORY=fail just package-crates >"$work/out" 2>&1; then
  cat "$work/out" >&2
  fail "an unreadable tag state failed a packaging run that succeeded"
fi
assert_contains 'package-crates: ok' "$work/out"
[[ $(wc -l <"$work/calls") -eq 2 ]] || fail "a successful packaging run consulted the release history"

if run_case env NO_TAG=1 BINARY_PACKAGE=fail just package-crates >"$work/out" 2>&1; then
  fail "missing-tag case unexpectedly passed"
fi
assert_contains 'no merged oneharness-core release tag found after fetching origin' "$work/out"

if run_case env NO_TAG=1 BINARY_PACKAGE=fail FETCH_MODE=fail just package-crates >"$work/out" 2>&1; then
  fail "failed tag fetch unexpectedly passed"
fi
assert_contains 'could not be fetched from origin' "$work/out"
# The Cargo diagnostics are the finding whatever the tag state turns out to be;
# a fetch that fails must never hide what it was classifying.
assert_contains '/registry/src/oneharness-core-0.6.11/src/io/http_turn.rs' "$work/out"

if run_case env NO_TAG=1 BINARY_PACKAGE=fail SHALLOW_REPOSITORY=fail just package-crates >"$work/out" 2>&1; then
  fail "failed shallow-repository probe unexpectedly passed"
fi
assert_contains 'could not determine whether the checkout is shallow' "$work/out"

if run_case env NO_TAG=1 BINARY_PACKAGE=fail SHALLOW_REPOSITORY=invalid just package-crates >"$work/out" 2>&1; then
  fail "invalid shallow-repository state unexpectedly passed"
fi
assert_contains "invalid shallow-repository state 'unknown'" "$work/out"

rm -f "$work/calls.fetched"
if ! run_case env NO_TAG=1 BINARY_PACKAGE=fail COMMIT_KIND=fix FETCH_MODE=recover \
  SHALLOW_REPOSITORY=true just package-crates >"$work/out" 2>&1; then
  cat "$work/out" >&2
  fail "tag fetch recovery did not continue to the release-transition decision"
fi
assert_contains "awaits release-plz's core version bump" "$work/out"
rm -f "$work/calls.fetched"

# release-plz has tagged the core version the binary pins, but release.yml has
# yet to publish it. Nothing in the working tree can bring that forward, so the
# window between a tag and its registry entry must not redden every PR.
if ! run_case env BINARY_PACKAGE=fail BINARY_FAILURE_KIND=version-select CORE_VERSION_TAGGED=1 \
  just package-crates >"$work/out" 2>&1; then
  cat "$work/out" >&2
  fail "an already-tagged, not-yet-published core version did not permit the pending publish"
fi
assert_contains 'awaits the crates.io publish of already-tagged oneharness-core' "$work/out"

# That tag excuses only a version the registry does not have. A core Cargo DID
# resolve and then failed to compile against is a source drift, and its tag must
# not wave it through.
if run_case env BINARY_PACKAGE=fail CORE_VERSION_TAGGED=1 just package-crates >"$work/out" 2>&1; then
  fail "a tagged core version excused a drift against the published core"
fi
assert_contains "commit the core API change as fix/feat/perf" "$work/out"

if run_case env BINARY_PACKAGE=fail just package-crates >"$work/out" 2>&1; then
  fail "incompatible published dependency unexpectedly passed"
fi
assert_contains "commit the core API change as fix/feat/perf" "$work/out"

if ! run_case env BINARY_PACKAGE=fail COMMIT_KIND=fix just package-crates >"$work/out" 2>&1; then
  cat "$work/out" >&2
  fail "release-worthy core fix did not permit the release-plz transition"
fi
assert_contains "awaits release-plz's core version bump" "$work/out"

if ! run_case env BINARY_PACKAGE=fail BINARY_FAILURE_KIND=version-select COMMIT_KIND=fix just package-crates >"$work/out" 2>&1; then
  cat "$work/out" >&2
  fail "unpublished release-plz core version did not permit the release transition"
fi
assert_contains "awaits release-plz's core version bump" "$work/out"

# A core change that adds a FEATURE fails at resolution rather than at compile,
# so it prints no registry source path. That shape shipped undetected once and
# turned a legitimate release transition into a red gate; pin both halves.
if ! run_case env BINARY_PACKAGE=fail BINARY_FAILURE_KIND=missing-feature COMMIT_KIND=fix just package-crates >"$work/out" 2>&1; then
  cat "$work/out" >&2
  fail "a core feature the published version lacks did not permit the release-plz transition"
fi
assert_contains "awaits release-plz's core version bump" "$work/out"

# The same diagnostic with NO release-worthy core change must still be red:
# release-plz would never bump core, so nothing would ever make it resolve.
if run_case env BINARY_PACKAGE=fail BINARY_FAILURE_KIND=missing-feature just package-crates >"$work/out" 2>&1; then
  fail "a missing core feature with no pending core release unexpectedly passed"
fi
assert_contains "cannot be packaged against its published oneharness-core dependency" "$work/out"

if ! run_case env BINARY_PACKAGE=fail REGISTRY_PATH_KIND=windows COMMIT_KIND=fix just package-crates >"$work/out" 2>&1; then
  cat "$work/out" >&2
  fail "Windows registry path did not permit the release-plz transition"
fi
assert_contains "awaits release-plz's core version bump" "$work/out"

if run_case env BINARY_PACKAGE=fail BINARY_FAILURE_KIND=unrelated COMMIT_KIND=fix just package-crates >"$work/out" 2>&1; then
  fail "unrelated binary failure unexpectedly passed during a pending core release"
fi
assert_contains "failed for a reason other than its registry-resolved oneharness-core transition" "$work/out"

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

# The release checkout, against real Git. actions/checkout fetches a tag by SHA
# at depth 1, so refs/tags/<tag> is a LIGHTWEIGHT ref at the commit while origin
# carries an annotated tag object — and the deepening fetch that discovers the
# core release tags is then rejected as "would clobber existing tag", silently,
# because --quiet suppresses the rejection line. That is how a checkout with a
# perfectly reachable origin reported its tags as unfetchable. Build exactly that
# repository twice: once to prove the fixture still reproduces the hazard, once
# to drive the script through it.
release_bin="$work/release-bin"
mkdir -p "$release_bin"
cp "$work/bin/cargo" "$release_bin/cargo"

origin="$work/release-origin.git"
seed="$work/release-seed"
git init -q --bare "$origin"
git init -q -b main "$seed"
git -C "$seed" config user.email test@example.com
git -C "$seed" config user.name Test
mkdir -p "$seed/scripts" "$seed/crates/oneharness-core/src"
cp scripts/package-crates.sh "$seed/scripts/package-crates.sh"
printf '[package]\nname = "oneharness-core"\nversion = "0.6.11"\n' \
  > "$seed/crates/oneharness-core/Cargo.toml"
printf 'pub fn seed() {}\n' > "$seed/crates/oneharness-core/src/lib.rs"
git -C "$seed" add -A
git -C "$seed" commit -qm 'chore: seed the release fixture'
git -C "$seed" tag -a -m 'core release' oneharness-core-v0.6.11
printf 'pub fn seed() {}\npub fn added() {}\n' > "$seed/crates/oneharness-core/src/lib.rs"
git -C "$seed" commit -qam 'fix(core): change the packaged API'
git -C "$seed" tag -a -m 'release' v0.6.14
git -C "$seed" remote add origin "$origin"
git -C "$seed" push -q --tags origin main
release_sha="$(git -C "$seed" rev-parse HEAD)"

# One depth-1 checkout of refs/tags/v0.6.14, exactly as actions/checkout builds it.
release_checkout() {
  local target="$1"
  git init -q "$target"
  git -C "$target" remote add origin "$origin"
  git -C "$target" -c protocol.version=2 fetch -q --no-tags --prune \
    --no-recurse-submodules --depth=1 origin "+$release_sha:refs/tags/v0.6.14"
  git -C "$target" checkout -q --force refs/tags/v0.6.14
  [[ $(git -C "$target" rev-parse --is-shallow-repository) == true ]] ||
    fail "the release-checkout fixture is not shallow; it no longer reproduces the release gate's repository"
}

release_checkout "$work/release-hazard"
if git -C "$work/release-hazard" fetch --tags --unshallow origin >"$work/out" 2>&1; then
  fail "the release-checkout fixture no longer rejects an unforced tag fetch, so it cannot prove the fix"
fi
assert_contains 'would clobber existing tag' "$work/out"

release_checkout "$work/release-run"
if ! CALL_LOG="$work/release-calls" PATH="$release_bin:$PATH" BINARY_PACKAGE=fail \
  bash "$work/release-run/scripts/package-crates.sh" >"$work/out" 2>&1; then
  cat "$work/out" >&2
  fail "package-crates could not read the release history from a depth-1 tag checkout" \
    "restore --force on the tag fetch in scripts/package-crates.sh"
fi
# Reaching this verdict from a depth-1 tag checkout of an annotated tag proves
# both halves: the clobbering fetch delivered the core tags, and --unshallow
# delivered the history `git rev-list <core tag>..HEAD` has to walk to find the
# release-worthy core commit. Neither is reachable in the repository as checked out.
assert_contains "awaits release-plz's core version bump" "$work/out"

echo "check-package-crates: ok"
