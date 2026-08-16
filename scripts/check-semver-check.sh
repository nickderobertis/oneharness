#!/usr/bin/env bash
# Subprocess-level coverage for semver-check's declared-vs-undeclared decision.
#
# The gate's whole value is the case it REJECTS, and a gate that only ever
# passes proves nothing — so every arm is driven here against stubbed `cargo`,
# `cargo-semver-checks` and `git`, with no network and no crates.io baseline.
set -euo pipefail

case $(uname -s) in
  MINGW* | MSYS* | CYGWIN*)
    echo "check-semver-check: skipped on Windows because this Unix behavioral harness relies on extensionless executable stubs" >&2
    exit 0
    ;;
esac

cd "$(dirname "$0")/.."

fail() {
  echo "check-semver-check: $1; ${2:-fix scripts/semver-check.sh and rerun 'bash scripts/check-semver-check.sh'}" >&2
  exit 1
}

assert_contains() {
  grep -Fq "$1" "$2" || {
    cat "$2" >&2
    fail "missing expected diagnostic '$1'"
  }
}

# release-plz.toml is the one source for what releases and how each crate is
# tagged; this script and semver-check.sh both hold copies of it. Reconcile every
# copy against that source here, the subject grammar included — two release
# decisions that disagree are worse than either alone.
grep -Fq 'release_commits = "(?s)^(feat|fix|perf)' release-plz.toml ||
  fail "release-worthy commit contract drifted from release-plz.toml"
grep -Fq "breaking_subject_re='^(feat|fix|perf)(\\([^)]+\\))?!:'" scripts/semver-check.sh ||
  fail "semver script breaking-subject grammar drifted from release-plz.toml's release_commits"
grep -Fq "release_subject_re='^(feat|fix|perf)" scripts/package-crates.sh ||
  fail "the two release-gate scripts disagree about which subjects release"
grep -Fq 'git_tag_name = "oneharness-core-v{{ version }}"' release-plz.toml ||
  fail "core tag namespace drifted from release-plz.toml"
grep -Fq -- "'oneharness-core-v*'" scripts/semver-check.sh ||
  fail "semver script core tag glob drifted from release-plz.toml"
grep -Fq "grep -q '^BREAKING CHANGE:'" scripts/semver-check.sh ||
  fail "semver script breaking-change body contract drifted from release-plz.toml"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin" "$work/bin-no-tool"

cat >"$work/bin/cargo" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ ${1:-} == +* ]]; then
  shift
fi
if [[ ${1:-} == --version ]]; then
  # The named toolchain is the one cargo-semver-checks needs; an absent one is
  # what a developer on the pinned channel alone has.
  [[ ${TOOLCHAIN_PRESENT:-1} == 1 ]] || exit 1
  echo 'cargo 1.97.1 (stubbed)'
  exit 0
fi
if [[ ${1:-} == semver-checks ]]; then
  printf '%s\n' "$*" >>"$CALL_LOG"
  # Bounded by spaces: `--package oneharness` is a prefix of
  # `--package oneharness-core`, and matching loosely would judge both.
  if [[ " $* " != *" --package ${SEMVER_PACKAGE:-oneharness-core} "* ]]; then
    echo 'Summary no semver update required'
    exit 0
  fi
  case ${SEMVER_RESULT:-ok} in
    ok)
      echo 'Summary no semver update required'
      ;;
    major)
      # Captured from a real run against the published baseline.
      echo '--- failure constructible_struct_adds_field: struct exhaustively constructible through public API adds field ---'
      echo '  field RunControls.supervisor in crates/oneharness-core/src/io/run.rs:141'
      echo 'Summary semver requires new major version: 1 major and 0 minor checks failed'
      exit 1
      ;;
    offline)
      # Not a verdict about the API: the tool could not run at all.
      echo 'error: failed to fetch the baseline from the crates.io registry'
      exit 1
      ;;
    *)
      echo 'invalid SEMVER_RESULT' >&2
      exit 2
      ;;
  esac
  exit 0
fi
exit 2
EOF

cat >"$work/bin/git" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
case "$1" in
  tag) [[ ${NO_TAG:-0} == 1 ]] || echo oneharness-core-v0.6.11 ;;
  rev-list) [[ ${COMMIT_KIND:-none} == none ]] || echo candidate ;;
  show)
    if [[ $* == *'%s'* ]]; then
      case ${COMMIT_KIND:-none} in
        none) echo 'test: no release' ;;
        additive) echo 'feat: add an entry point' ;;
        breaking | othercrate) echo 'feat!: change the API' ;;
        nonreleasing) echo 'docs!: change the API' ;;
        body) echo 'feat: change the API' ;;
        *) echo 'invalid COMMIT_KIND' >&2; exit 2 ;;
      esac
    elif [[ ${COMMIT_KIND:-none} == body ]]; then
      echo 'BREAKING CHANGE: the field is gone'
    else
      echo
    fi
    ;;
  diff)
    case ${GIT_DIFF_MODE:-clean} in
      clean) exit 0 ;;
      dirty) exit 1 ;;
      *) echo 'invalid GIT_DIFF_MODE' >&2; exit 2 ;;
    esac
    ;;
  diff-tree) [[ ${COMMIT_KIND:-none} == othercrate ]] || echo crates/oneharness-core/src/lib.rs ;;
  *) exit 2 ;;
esac
EOF

# `command -v cargo-semver-checks` is the availability probe; the stub only has
# to exist on PATH, since the real invocation goes through `cargo semver-checks`.
printf '#!/usr/bin/env bash\nexit 2\n' >"$work/bin/cargo-semver-checks"
cp "$work/bin/cargo" "$work/bin/git" "$work/bin-no-tool/"
chmod +x "$work/bin"/* "$work/bin-no-tool"/*

run_case() {
  CALL_LOG="$work/calls" PATH="$work/bin:$PATH" "$@"
}

# Absence has to be real absence: prepending a stub directory cannot hide a tool
# the developer (or the CI image) has installed further down PATH. This PATH
# holds the stubs, the recipe runner, and the system utilities — nothing else.
ln -s "$(command -v just)" "$work/bin-no-tool/just"
no_tool_path="$work/bin-no-tool:/usr/bin:/bin"

: >"$work/calls"
if ! run_case just semver-check >"$work/out" 2>&1; then
  cat "$work/out" >&2
  fail "an additive API failed the gate"
fi
assert_contains 'semver-check: ok' "$work/out"
[[ $(wc -l <"$work/calls") -eq 2 ]] || fail "both published crates must be judged"

# The case the gate exists for: a break nobody declared would ship as a patch or
# a minor and break consumers at `cargo update`.
if run_case env SEMVER_RESULT=major just semver-check >"$work/out" 2>&1; then
  fail "an undeclared breaking change passed the gate"
fi
assert_contains 'breaks its published API and nothing declares it' "$work/out"
# The tool's own finding is the evidence; a verdict without it cannot be acted on.
assert_contains 'constructible_struct_adds_field' "$work/out"

# Declared three ways, each of which release-plz really does read.
for declaration in breaking body; do
  if ! run_case env SEMVER_RESULT=major COMMIT_KIND="$declaration" just semver-check >"$work/out" 2>&1; then
    cat "$work/out" >&2
    fail "a breaking change declared by commit ($declaration) was rejected"
  fi
  assert_contains 'ok (declared breaking change in oneharness-core' "$work/out"
done
if ! run_case env SEMVER_RESULT=major PR_TITLE='feat!: change the API' just semver-check >"$work/out" 2>&1; then
  cat "$work/out" >&2
  fail "a breaking change declared by the squash subject was rejected"
fi
# Recipes are quiet on success: a declared break is said IN that one line, never
# as a second.
[[ $(wc -l <"$work/out") -eq 1 ]] || fail "a passing run said more than its one line"
assert_contains 'ok (declared breaking change in oneharness-core' "$work/out"

# A developer has to be able to run the gate before committing the declaration.
if ! run_case env SEMVER_RESULT=major GIT_DIFF_MODE=dirty just semver-check >"$work/out" 2>&1; then
  cat "$work/out" >&2
  fail "an uncommitted breaking change was rejected"
fi

# The declaration has to be about THIS crate: a `feat!` elsewhere in the tree is
# not what bumps the engine's major.
if run_case env SEMVER_RESULT=major COMMIT_KIND=othercrate just semver-check >"$work/out" 2>&1; then
  fail "a breaking change declared by a commit touching another crate passed"
fi
assert_contains 'breaks its published API and nothing declares it' "$work/out"

# A non-breaking commit type does not declare anything either.
if run_case env SEMVER_RESULT=major COMMIT_KIND=additive just semver-check >"$work/out" 2>&1; then
  fail "a breaking change under a plain 'feat:' passed"
fi

# Nor does a `!` on a type release-plz never releases: it opens no release PR, so
# it bumps nothing and the break would ship on someone else's commit.
if run_case env SEMVER_RESULT=major COMMIT_KIND=nonreleasing just semver-check >"$work/out" 2>&1; then
  fail "a breaking change under a non-releasing type passed"
fi

# Neither does a break in the BINARY crate, whose own tag namespace is `v*`.
if run_case env SEMVER_PACKAGE=oneharness SEMVER_RESULT=major just semver-check >"$work/out" 2>&1; then
  fail "an undeclared breaking change in the binary crate passed"
fi
# The backticks are the diagnostic's own quoting of the crate name, not a shell
# substitution.
# shellcheck disable=SC2016
assert_contains '`oneharness` breaks its published API' "$work/out"

# An absent release history is its own answer, not "nothing declares it": the
# commits that would declare it cannot be enumerated at all, so the diagnostic
# has to name the checkout rather than send the reader to rewrite a subject.
if run_case env SEMVER_RESULT=major NO_TAG=1 just semver-check >"$work/out" 2>&1; then
  fail "a break judged against no release history passed as undeclared"
fi
assert_contains 'release tag is visible to say whether a commit declares it' "$work/out"
assert_contains 'fetch-depth: 0' "$work/out"
# The declarations that need no history still stand there.
if ! run_case env SEMVER_RESULT=major NO_TAG=1 PR_TITLE='feat!: change the API' just semver-check >"$work/out" 2>&1; then
  cat "$work/out" >&2
  fail "a squash subject declaring the break was rejected for want of release tags"
fi

# A tool that could not judge at all is never a pass: its diagnostics are the
# finding, and no declaration excuses them.
if run_case env SEMVER_RESULT=offline COMMIT_KIND=breaking just semver-check >"$work/out" 2>&1; then
  fail "a cargo-semver-checks failure that judged nothing passed"
fi
assert_contains 'could not judge' "$work/out"
assert_contains 'failed to fetch the baseline' "$work/out"

# Absent tooling: a skip on a developer's box, red in the tier that enforces it.
if ! CALL_LOG="$work/calls" PATH="$no_tool_path" just semver-check >"$work/out" 2>&1; then
  cat "$work/out" >&2
  fail "an absent cargo-semver-checks failed a local gate run"
fi
assert_contains 'semver-check: skipped' "$work/out"
if CALL_LOG="$work/calls" PATH="$no_tool_path" OH_SEMVER_NO_SKIP=1 just semver-check >"$work/out" 2>&1; then
  fail "an absent cargo-semver-checks passed with OH_SEMVER_NO_SKIP=1"
fi
assert_contains 'cargo-semver-checks not installed' "$work/out"

if ! run_case env TOOLCHAIN_PRESENT=0 just semver-check >"$work/out" 2>&1; then
  cat "$work/out" >&2
  fail "an absent toolchain failed a local gate run"
fi
assert_contains 'semver-check: skipped' "$work/out"
if run_case env TOOLCHAIN_PRESENT=0 OH_SEMVER_NO_SKIP=1 just semver-check >"$work/out" 2>&1; then
  fail "an absent toolchain passed with OH_SEMVER_NO_SKIP=1"
fi
assert_contains 'toolchain is not installed' "$work/out"

echo 'check-semver-check: ok'
