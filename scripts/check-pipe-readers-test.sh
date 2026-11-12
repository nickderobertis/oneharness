#!/usr/bin/env bash
#
# Behavioral test of scripts/check-pipe-readers.sh: it runs the real check over
# fixtures — every early-exiting reader shape refused and named by line, every
# rewrite it suggests accepted — and over a missing file, a usage refusal.
#
# It proves the rule by the script's shape, never by running the hazard: whether
# a writer takes SIGPIPE depends on whether the reader exits before the writer's
# last write, which is scheduling. A pipefail script piping 256 KiB into an awk
# that exits on its first line ended 141 locally and 0 on a Linux CI runner
# (job 131151078773), so a test that runs it gives no verdict that holds.
#
# Quiet on success, one line. On failure it names the case and what to change.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)" || {
  echo "check-pipe-readers-test: could not resolve the repository root from ${BASH_SOURCE[0]}; run it from a checkout as 'bash scripts/check-pipe-readers-test.sh'" >&2
  exit 2
}
cd "$root" || {
  echo "check-pipe-readers-test: could not enter the repository root $root; check that it exists and is readable, then re-run" >&2
  exit 2
}

check="$root/scripts/check-pipe-readers.sh"
tmp="$(mktemp -d)" || {
  echo "check-pipe-readers-test: could not create a scratch directory under ${TMPDIR:-/tmp}; check that it is writable and has room, then re-run" >&2
  exit 2
}
cleanup() {
  rm -rf "$tmp" || echo "check-pipe-readers-test: could not remove scratch $tmp; delete it by hand" >&2
}
trap cleanup EXIT

fail() {
  echo "check-pipe-readers-test: $1" >&2
  [ -s "$tmp/err" ] && sed 's/^/  stderr: /' "$tmp/err" >&2
  echo "  Next: restore that behaviour in scripts/check-pipe-readers.sh, then re-run 'bash scripts/check-pipe-readers-test.sh'." >&2
  exit 1
}

run() {
  status=0
  bash "$check" "$@" >"$tmp/out" 2>"$tmp/err" || status=$?
}

# Old shapes, one per line, each a reader that can stop before its writer. They
# are written with PIPE for the pipe so this test's own source passes the check.
sed 's/ PIPE / | /' >"$tmp/bad.sh" <<'EOF' || fail "could not write the refused fixture under $tmp; check that ${TMPDIR:-/tmp} is writable and has room"
# a comment naming PIPE grep -q is not a pipeline
x="$(printf '%s\n' "$job" PIPE awk '/a/ { on = 1; next } on && /b/ { exit } on')"
printf '%s\n' "$affected" PIPE grep -qxF 'NX_BASE' || fail
first="$(sed -n 's/a/b/p' file PIPE head -n 1)"
if tr -d '\r' <file PIPE grep -Fxq 'x'; then :; fi
grep -v '#' file PIPE grep -m1 x
grep -v '#' file PIPE grep --quiet x
find . -type f PIPE grep -l x
EOF
run "$tmp/bad.sh"
[ "$status" -eq 1 ] || fail "a file of early-exiting readers exited $status, expected a refusal (exit 1)"
for line in 2 3 4 5 6 7 8; do
  grep -qF "check-pipe-readers: $tmp/bad.sh:$line:" "$tmp/err" ||
    fail "the refusal does not name $tmp/bad.sh line $line ($(sed -n "${line}p" "$tmp/bad.sh"))"
done
grep -qF "$tmp/bad.sh:1:" "$tmp/err" && fail "a comment line was reported as a pipeline"
grep -qF "Next: " "$tmp/err" || fail "the refusal does not say what to do next"

# The rewrites the refusal suggests, plus shapes that only look alike.
cat >"$tmp/good.sh" <<'EOF' || fail "could not write the accepted fixture under $tmp; check that ${TMPDIR:-/tmp} is writable and has room"
x="$(awk '/a/ { on = 1; next } on && /b/ { exit } on' <<<"$job")"
grep -qxF 'NX_BASE' <<<"$affected" || fail
first="$(sed -n 's/a/b/p' file | sed -n 1p)"
if tr -d '\r' <file | grep -E 'x' >/dev/null; then :; fi
grep -q x file || grep -q y file
reported="$(head -n 2 <<<"$out" | tr '\n' ' ')"
names="$(tr -d '\r' <file | awk 'done { next } /^jobs:/ { on = 1 } on && /^x/ { done = 1 }')"
EOF
run "$tmp/good.sh"
[ "$status" -eq 0 ] || fail "the suggested rewrites exited $status, expected ok"
grep -qx "check-pipe-readers: ok" "$tmp/out" || fail "a clean file did not print the one ok line"

run "$tmp/absent.sh"
[ "$status" -eq 2 ] || fail "a missing file exited $status, expected a usage refusal (exit 2)"
grep -qF "$tmp/absent.sh does not exist" "$tmp/err" || fail "the missing-file refusal does not name the file"

echo "check-pipe-readers-test: ok"
