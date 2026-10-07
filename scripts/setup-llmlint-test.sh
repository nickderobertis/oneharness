#!/usr/bin/env bash
#
# Behavioral test of the llmlint session installer's one promise: it always exits
# 0, because it runs from the SessionStart hook and a flaky install must never
# break session startup. It runs under `set -e`, so every fallible step has to
# be guarded, and an unguarded one would only show up as a failure here.
#
# Each case runs the real script against a scratch HOME whose `uv` and
# `llmlint` are stubs that record their calls, so reaching the final `llmlint
# doctor` proves the run continued past the failure under test.
#
# Quiet on success, one line. On failure it prints what the installer said.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
script="$repo_root/scripts/setup-llmlint.sh"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

fail() {
  echo "setup-llmlint-test: $1" >&2
  echo "  what the installer said:" >&2
  cat "$work/err" >&2 || true
  exit 1
}

# $1 = exit status of the stub `uv tool install`. Leaves a fresh HOME at
# $work/home whose bin dir (the one the installer puts first on PATH) holds the
# stubs, and clears the call log.
stage() {
  rm -rf "${work:?}/home"
  mkdir -p "$work/home/.local/bin"
  : > "$work/calls"
  cat > "$work/home/.local/bin/uv" <<EOF
#!/usr/bin/env bash
echo "uv \$*" >> "$work/calls"
exit $1
EOF
  cat > "$work/home/.local/bin/llmlint" <<EOF
#!/usr/bin/env bash
echo "llmlint \$*" >> "$work/calls"
EOF
  chmod +x "$work/home/.local/bin/uv" "$work/home/.local/bin/llmlint"
}

# Runs the installer with the rest of the arguments as extra environment, its
# stderr captured to $work/err; fails unless it exits 0 and reached the end.
run_installer() {
  local status=0
  env HOME="$work/home" "$@" bash "$script" 2> "$work/err" || status=$?
  [ "$status" -eq 0 ] || fail "exited $status; it must always exit 0"
  grep -q '^llmlint doctor' "$work/calls" || fail "stopped before 'llmlint doctor'"
}

# A failed install is logged and setup carries on.
stage 1
run_installer CLAUDE_ENV_FILE=
grep -q 'llmlint-cli install failed (continuing)' "$work/err" \
  || fail "a failed install was not reported as continuing"

# An env file that cannot be written is said so, never reported as exported.
stage 0
run_installer CLAUDE_ENV_FILE="$work/missing-dir/env"
grep -q "could not write $work/missing-dir/env (continuing)" "$work/err" \
  || fail "an unwritable CLAUDE_ENV_FILE was not reported"
if grep -q 'exported PATH' "$work/err"; then
  fail "claimed to export PATH after the env file write failed"
fi

# A writable env file is the quiet path.
stage 0
run_installer CLAUDE_ENV_FILE="$work/env"
grep -q 'exported PATH' "$work/err" || fail "a writable CLAUDE_ENV_FILE was not reported as exported"

# A diagnostic that cannot be written (stderr closed) must not end the run.
stage 1
status=0
env HOME="$work/home" CLAUDE_ENV_FILE="$work/env" bash "$script" 2>&- || status=$?
: > "$work/err"
[ "$status" -eq 0 ] || fail "exited $status with stderr closed; it must always exit 0"
grep -q '^llmlint doctor' "$work/calls" || fail "stopped before 'llmlint doctor' with stderr closed"

echo "setup-llmlint-test: ok"
