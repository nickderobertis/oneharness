#!/usr/bin/env bash
# Drive both halves of the setup-just action's install: the cache hit that must
# reach no network, and the cache miss that must install the pin and be held to
# it. Stubbed `cargo` and `just` keep it hermetic and let the miss path assert
# the exact install arguments, which no CI run can show without a cold cache.
set -euo pipefail

cd "$(dirname "$0")/.."

# This check drives three files, so the remediation `fail` falls back to names
# whichever one the failing case ran: every case goes through a driver below, and
# each driver claims the file it drives. A failure pointing at the wrong one
# costs the reader exactly the debugging time this check exists to save.
subject=

fail() {
  echo "check-setup-just: $1; ${2:-fix $subject and rerun 'bash scripts/check-setup-just.sh'}" >&2
  exit 1
}

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin"

# `just` reports whatever version the fixture last wrote for it.
cat >"$work/bin/just" <<'EOF'
#!/usr/bin/env bash
cat "$JUST_VERSION_FILE"
EOF
# `cargo` records its arguments and installs the version the fixture tells it to,
# which is how a mismatch between the pin and what lands on PATH is staged. It is
# as chatty as the real one, so a successful install's noise is visible here.
cat >"$work/bin/cargo" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"$CARGO_LOG"
echo "    Updating crates.io index"
echo "   Compiling just v$CARGO_INSTALLS_VERSION" >&2
if [ "${CARGO_INSTALL_FAILS:-0}" = 1 ]; then
  echo "error: could not compile just" >&2
  exit 101
fi
printf 'just %s\n' "$CARGO_INSTALLS_VERSION" >"$JUST_VERSION_FILE"
EOF
chmod +x "$work/bin/just" "$work/bin/cargo"

run_case() {
  subject=scripts/setup-just-install.sh
  : >"$work/cargo-calls"
  env PATH="$work/bin:$PATH" JUST_VERSION_FILE="$work/just-version" \
    CARGO_LOG="$work/cargo-calls" CARGO_INSTALLS_VERSION="${3:-1.99.0}" \
    CARGO_INSTALL_FAILS="${4:-0}" \
    scripts/setup-just-install.sh "${2-1.99.0}" "$1" >"$work/out" 2>&1
}

# Cache hit: the restored binary is the pin, so nothing is installed. This is
# the path every warm run takes, and reaching crates.io on it would put a
# required check back at the mercy of a registry outage.
printf 'just 1.99.0\n' >"$work/just-version"
if ! run_case true; then
  cat "$work/out" >&2
  fail "a cache hit carrying the pinned version was rejected"
fi
[ ! -s "$work/cargo-calls" ] ||
  fail "a cache hit installed anyway: $(cat "$work/cargo-calls")" \
    "restore the cache-hit branch in scripts/setup-just-install.sh"

# Cache miss: the pin is installed, and pinned exactly — `--force` because
# rust-cache restores a `just` of its own, `--locked` so the install itself is
# reproducible.
printf 'just 0.0.1\n' >"$work/just-version"
if ! run_case false; then
  cat "$work/out" >&2
  fail "a cache miss did not install and verify the pinned version"
fi
grep -Fq -- 'install just --locked --version 1.99.0 --force' "$work/cargo-calls" ||
  fail "the cache-miss install lost its pinning arguments: $(cat "$work/cargo-calls")"
# An install that worked has nothing to say. Cargo's progress is what a reader
# has to scroll past to find the step that actually failed.
[ ! -s "$work/out" ] ||
  fail "a successful install was not quiet: $(cat "$work/out")" \
    "capture cargo's output in scripts/setup-just-install.sh and report it only on failure"

# ...and a failed one says everything: cargo's own diagnostics are the finding.
printf 'just 0.0.1\n' >"$work/just-version"
if run_case false 1.99.0 1.99.0 1; then
  fail "a failed install was reported as success"
fi
grep -Fq 'could not compile just' "$work/out" ||
  fail "a failed install swallowed cargo's diagnostics: $(cat "$work/out")"
grep -Fq 'could not install just 1.99.0' "$work/out" ||
  fail "a failed install lacked its own diagnostic: $(cat "$work/out")"

# Both arguments are workflow expressions, and neither is taken on trust: a
# version reaches a `cargo install` argument, and a cache-hit that is neither
# `true` nor `false` nor empty would otherwise read as a cold cache — a silent
# reinstall on every warm run instead of a failure anyone sees.
printf 'just 1.99.0\n' >"$work/just-version"
for bad_version in '1.99' '1.99.0; rm -rf /' ''; do
  if run_case true "$bad_version"; then
    fail "the version '$bad_version' passed validation" \
      "restore the x.y.z check in scripts/setup-just-install.sh"
  fi
done
if run_case 'True'; then
  fail "a cache-hit of 'True' passed validation" \
    "restore the cache-hit check in scripts/setup-just-install.sh"
fi
grep -Fq "got 'True'" "$work/out" ||
  fail "the cache-hit refusal did not name the value: $(cat "$work/out")"
# Empty is what actions/cache emits when it never reached the cache service, so
# it is a miss and not a malformed value.
printf 'just 0.0.1\n' >"$work/just-version"
if ! run_case ''; then
  cat "$work/out" >&2
  fail "an empty cache-hit was refused rather than treated as a miss"
fi
grep -Fq -- 'install just --locked --version 1.99.0 --force' "$work/cargo-calls" ||
  fail "an empty cache-hit did not install: $(cat "$work/cargo-calls")"

# ...and the install is not taken on trust. A `just` of another version on PATH
# after it is the failure this verification exists for: every recipe in the gate
# would then run an unpinned binary.
printf 'just 0.0.1\n' >"$work/just-version"
if run_case false 1.99.0 0.0.1; then
  fail "a just of the wrong version passed verification" \
    "restore the version assertion in scripts/setup-just-install.sh"
fi
grep -Fq 'but .tool-versions pins just 1.99.0' "$work/out" ||
  fail "the version mismatch lacked a diagnostic naming the pin: $(cat "$work/out")"

# The pin the whole action is keyed on. asdf's format lets a tool carry several
# versions on its line and appear on several lines, so "the version" is only
# well defined once exactly one of each is proven — and the emitted line is the
# step's output verbatim, so it is what the test reads.
read_pin() {
  subject=scripts/setup-just-pin.sh
  scripts/setup-just-pin.sh "$@" >"$work/pin-out" 2>&1
}

pin_case() {
  printf '%s\n' "$1" >"$work/tool-versions"
  read_pin "$work/tool-versions"
}

if ! pin_case 'nodejs 22.0.0
just 1.99.0
python 3.12.0'; then
  cat "$work/pin-out" >&2
  fail "a single exact pin among other tools was refused"
fi
[ "$(cat "$work/pin-out")" = "version=1.99.0" ] ||
  fail "the pin step emitted '$(cat "$work/pin-out")', not the workflow output line 'version=1.99.0'"

# A second version on the line is the shape a field-two reader turns into a
# confident, wrong pin — and then caches under a key claiming otherwise.
for bad in 'just 1.99.0 2.0.0' 'just' 'just 1.99' 'just latest' 'nodejs 22.0.0' 'just 1.99.0
just 2.0.0'; do
  if pin_case "$bad"; then
    fail "the pin '$bad' was accepted as 'exactly one x.y.z': $(cat "$work/pin-out")" \
      "restore the whole-line validation in scripts/setup-just-pin.sh"
  fi
  grep -Fq 'must pin just as exactly one x.y.z version' "$work/pin-out" ||
    fail "refusing '$bad' lacked the actionable diagnostic: $(cat "$work/pin-out")"
done

# The repo's own file must satisfy its own reader; otherwise every workflow using
# the action fails at its first step.
read_pin ||
  fail "this repo's .tool-versions is not readable by its own pin step: $(cat "$work/pin-out")"

# The action must actually go through both scripts, or none of the above covers it.
delegates_to() {
  subject=.github/actions/setup-just/action.yml
  grep -Fq "run: $1" .github/actions/setup-just/action.yml
}

delegates_to scripts/setup-just-pin.sh ||
  fail "the setup-just action no longer reads its pin through scripts/setup-just-pin.sh" \
    "restore the delegation in .github/actions/setup-just/action.yml"
delegates_to scripts/setup-just-install.sh ||
  fail "the setup-just action no longer installs through scripts/setup-just-install.sh" \
    "restore the delegation in .github/actions/setup-just/action.yml"

echo "check-setup-just: ok"
