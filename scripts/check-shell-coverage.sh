#!/usr/bin/env bash
#
# Behavioral test of the shell coverage floor through the real collector:
# scripts/shell-test.sh running a step under the pinned kcov, and
# scripts/shell-coverage.sh merging the reports and enforcing the floor.
#
# A staged git checkout carries the four coverage scripts, the pin file and a
# `demo` project whose `test` runs one step, scripts/demo-test.sh, which drives
# scripts/demo.sh three ways: directly, from a `bash -eu -c` (as just runs a
# recipe), and from a bash whose kcov trace descriptor was closed (as a Node,
# Rust or just spawn leaves it). The step must pass with nothing of kcov's on
# its stderr, its report must count the demo script's lines it ran and not the
# ones it did not, and the floor must pass below the measured rate, fail above
# it, fail when a second untested script joins the measured set, and refuse a
# step whose report is missing, undeclared or misnamed, and a merged report
# kcov wrote wrongly. A failing step keeps its own exit status through kcov.
#
# kcov is built on Linux only (scripts/shell-tools.sh); elsewhere the step must
# run uninstrumented and the floor must say it was skipped.
#
# Quiet on success, one line. On failure it names the case and what it saw.
set -Eeuo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

fail() {
  echo "check-shell-coverage: $1" >&2
  shift
  for line in "$@"; do echo "  $line" >&2; done
  echo "  fix: restore that behaviour in scripts/shell-test.sh or scripts/shell-coverage.sh, then rerun 'bash scripts/check-shell-coverage.sh'" >&2
  exit 1
}

# A setup step that fails (a scratch directory, a copy, a write) says which,
# rather than ending the run on the bare error.
setup_failed() {
  # Once, from the shell the step ran in, not again from each enclosing one.
  [ "$BASH_SUBSHELL" = 0 ] || return 0
  echo "check-shell-coverage: \`$1\` failed at line $2 (above); fix: check that ${TMPDIR:-/tmp} and target/ are writable and have room, then rerun 'bash scripts/check-shell-coverage.sh'" >&2
}
trap 'setup_failed "$BASH_COMMAND" "$LINENO"' ERR

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
stage="$work/repo"
mkdir -p "$stage/scripts" "$stage/demo"
cp scripts/shell-test.sh scripts/shell-coverage.sh scripts/shell-tools.sh scripts/shell-trace-env.sh "$stage/scripts/"
cp .shell-tool-versions "$stage/"
cat >"$stage/scripts/demo.sh" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
case "${1:-}" in
  covered)
    echo "demo: covered"
    ;;
  untested)
    echo "demo: never run"
    echo "demo: by any step"
    echo "demo: so these lines"
    echo "demo: stay at zero"
    ;;
esac
SH
cat >"$stage/scripts/demo-test.sh" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[ "$(bash scripts/demo.sh covered)" = "demo: covered" ]
bash -eu -o pipefail -c 'bash scripts/demo.sh covered >/dev/null'
if [ -n "${KCOV_BASH_XTRACEFD:-}" ]; then
  eval "bash scripts/demo.sh covered >/dev/null ${KCOV_BASH_XTRACEFD}>&-"
fi
exit "${DEMO_EXIT:-0}"
SH
cat >"$stage/demo/project.json" <<'JSON'
{
  "name": "demo",
  "targets": {
    "test": {
      "options": { "commands": ["bash scripts/shell-test.sh demo scripts/demo-test.sh"] },
      "outputs": ["{workspaceRoot}/target/coverage/shell/demo"]
    }
  }
}
JSON
git -C "$stage" init --quiet
git -C "$stage" add -A

step() {
  status=0
  (cd "$stage" && bash scripts/shell-test.sh demo scripts/demo-test.sh) >"$work/step.out" 2>"$work/step.err" || status=$?
}
floor() {
  status=0
  (cd "$stage" && SHELL_COVERAGE_MIN="$1" bash scripts/shell-coverage.sh) >"$work/out" 2>&1 || status=$?
}
expect() {
  local want="$1" text="$2" case="$3"
  [ "$status" = "$want" ] || fail "$case: exited $status, not $want" "$(cat "$work/out")"
  grep -Fq -- "$text" "$work/out" || fail "$case: did not say '$text'" "$(cat "$work/out")"
}

if [ "$(uname -s)" != Linux ]; then
  step
  [ "$status" = 0 ] && [ ! -s "$work/step.err" ] || fail "the step did not run cleanly uninstrumented (exit $status)" "$(cat "$work/step.err")"
  floor 95
  expect 0 "shell-coverage: skipped on $(uname -s)" "the floor off Linux"
  echo "check-shell-coverage: ok (uninstrumented on $(uname -s); the floor is measured on Linux)"
  exit 0
fi

step
[ "$status" = 0 ] || fail "the demo step failed under kcov (exit $status)" "$(cat "$work/step.out" "$work/step.err")"
[ ! -s "$work/step.err" ] || fail "the demo step wrote to stderr under kcov, so tracing changed what a test observes" "$(cat "$work/step.err")"
report="$stage/target/coverage/shell/demo/demo-test"
[ -n "$(find "$report" -name coverage.json -print -quit)" ] || fail "the step left no kcov report in $report"

floor 1
expect 0 "shell-coverage: ok (" "a floor below the measured rate"
summary="$stage/target/coverage/shell-coverage.txt"
demo="$(grep -E ' scripts/demo\.sh$' "$summary")" || fail "the summary has no row for scripts/demo.sh" "$(cat "$summary")"
read -r _ counts _ <<<"$demo"
covered="${counts%/*}"
total="${counts#*/}"
[ "$covered" -ge 3 ] && [ "$total" -ge $((covered + 4)) ] ||
  fail "scripts/demo.sh reads $counts: the step ran its 'covered' arm, and its four 'untested' lines must count, unrun" "$(cat "$summary")"
grep -Eq '^ *100\.00% .* scripts/demo-test\.sh$' "$summary" ||
  fail "the step's own script is not fully covered, so kcov missed lines it ran" "$(cat "$summary")"
rate="$(sed -n 's/^shell line coverage: \([0-9.]*\)%.*/\1/p' "$summary")"

floor 100
expect 1 "below the 100% floor" "a floor above the measured rate"
grep -Fq "scripts/demo.sh" "$work/out" || fail "the floor failure did not list the least-covered scripts" "$(cat "$work/out")"

# An untested script joining the measured set lowers the rate.
printf '#!/usr/bin/env bash\necho one\necho two\necho three\n' >"$stage/scripts/untested.sh"
floor "$rate"
expect 1 "below the $rate% floor" "an untested script added at the measured rate"
rm "$stage/scripts/untested.sh"
floor "$rate"
expect 0 "shell-coverage: ok (" "the measured rate once the untested script is gone"

# A step that left no report is a run that never happened.
mv "$stage/target/coverage/shell/demo" "$work/demo-report"
floor 1
expect 1 "left no kcov report" "a step with no report"
mv "$work/demo-report" "$stage/target/coverage/shell/demo"

# A step named for another project writes where its target's cache never looks.
sed 's/shell-test.sh demo /shell-test.sh other /' "$stage/demo/project.json" >"$work/project.json"
cp "$work/project.json" "$stage/demo/project.json"
floor 1
expect 1 "runs a step as project other" "a step naming another project"
git -C "$stage" checkout --quiet -- demo/project.json

floor abc
expect 2 "is not a percentage" "a malformed floor"

# A step whose target does not declare its report as an output, or that names
# no Nx project, is refused: the cache would never restore its report.
cp "$stage/demo/project.json" "$work/project.json"
sed 's/"outputs": \[[^]]*\]/"outputs": []/' "$work/project.json" >"$stage/demo/project.json"
floor 1
expect 1 "does not declare {workspaceRoot}/target/coverage/shell/demo as an output" "a step whose report is no output"
sed 's/shell-test.sh demo /shell-test.sh ..\/demo /' "$work/project.json" >"$stage/demo/project.json"
floor 1
expect 1 'runs a step as project "../demo", which is not an Nx project name' "a step naming a path, not a project"
git -C "$stage" checkout --quiet -- demo/project.json

# A merged report kcov wrote wrongly is refused, never read as a rate: a kcov
# double in a tools directory of its own stands in for the merge.
double_tools="$work/double-tools"
pinned_kcov="$(awk '$1 == "kcov" { print $2 }' .shell-tool-versions)"
mkdir -p "$double_tools/kcov-$pinned_kcov/bin"
cat >"$double_tools/kcov-$pinned_kcov/bin/kcov" <<'SH'
#!/usr/bin/env bash
# The first non-option argument is the output directory; a merge writes the
# report KCOV_DOUBLE_REPORT holds, with ROOT standing for the checkout.
set -euo pipefail
merge=0
for arg in "$@"; do
  case "$arg" in
    --merge) merge=1 ;;
    -*) ;;
    *)
      mkdir -p "$arg/kcov-merged"
      [ "$merge" = 0 ] || printf '%s\n' "${KCOV_DOUBLE_REPORT//ROOT/$PWD}" >"$arg/kcov-merged/coverage.json"
      exit 0
      ;;
  esac
done
SH
chmod +x "$double_tools/kcov-$pinned_kcov/bin/kcov"
while IFS='|' read -r report says; do
  status=0
  (cd "$stage" && KCOV_DOUBLE_REPORT="$report" ONEHARNESS_TOOLS_DIR="$double_tools" SHELL_COVERAGE_MIN=1 \
    bash scripts/shell-coverage.sh) >"$work/out" 2>&1 || status=$?
  expect 1 "could not read line counts from the merged report" "a merged report of $report"
  grep -Fq -- "$says" "$work/out" || fail "a merged report of $report was refused without saying '$says'" "$(cat "$work/out")"
done <<'REPORTS'
{"files": 3}|has no list of scripts
{"files": [{"covered_lines": "1", "total_lines": "2"}]}|has no list of scripts
{"files": []}|lists 0 scripts
{"files": [{"file": "ROOT/scripts/a.sh", "covered_lines": "5", "total_lines": "2"}]}|unreadable line counts for: scripts/a.sh
{"files": [{"file": "ROOT/scripts/b.sh", "covered_lines": "-1", "total_lines": "2"}]}|unreadable line counts for: scripts/b.sh
{"files": [{"file": "/elsewhere/c.sh", "covered_lines": "1", "total_lines": "2"}]}|counts files outside scripts/ and .githooks/: /elsewhere/c.sh
REPORTS

# A failing step keeps its own exit status through kcov.
status=0
(cd "$stage" && DEMO_EXIT=3 bash scripts/shell-test.sh demo scripts/demo-test.sh) >"$work/out" 2>&1 || status=$?
[ "$status" = 3 ] || fail "a step exiting 3 under kcov exited $status" "$(cat "$work/out")"

# A step must be one of scripts/'s own files, not a path or link out of it.
ln -s "$stage/demo/project.json" "$stage/scripts/linked.sh"
for outside in scripts/../demo/project.json scripts/linked.sh scripts/missing.sh; do
  status=0
  (cd "$stage" && bash scripts/shell-test.sh demo "$outside") >"$work/out" 2>&1 || status=$?
  expect 2 "is not a script in scripts/" "a step naming $outside"
done

echo "check-shell-coverage: ok"
