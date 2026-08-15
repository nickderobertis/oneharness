#!/usr/bin/env bash
# Drive both halves of the setup-just action's install: the cache hit that must
# reach no network, and the cache miss that must install the pin and be held to
# it. Stubbed `cargo` and `just` keep it hermetic and let the miss path assert
# the exact install arguments, which no CI run can show without a cold cache.
set -euo pipefail

cd "$(dirname "$0")/.."

fail() {
  echo "check-setup-just: $1; ${2:-fix scripts/setup-just-install.sh and rerun 'bash scripts/check-setup-just.sh'}" >&2
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
# which is how a mismatch between the pin and what lands on PATH is staged.
cat >"$work/bin/cargo" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"$CARGO_LOG"
printf 'just %s\n' "$CARGO_INSTALLS_VERSION" >"$JUST_VERSION_FILE"
EOF
chmod +x "$work/bin/just" "$work/bin/cargo"

run_case() {
  : >"$work/cargo-calls"
  env PATH="$work/bin:$PATH" JUST_VERSION_FILE="$work/just-version" \
    CARGO_LOG="$work/cargo-calls" CARGO_INSTALLS_VERSION="${2:-1.99.0}" \
    scripts/setup-just-install.sh 1.99.0 "$1" >"$work/out" 2>&1
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

# ...and the install is not taken on trust. A `just` of another version on PATH
# after it is the failure this verification exists for: every recipe in the gate
# would then run an unpinned binary.
printf 'just 0.0.1\n' >"$work/just-version"
if run_case false 0.0.1; then
  fail "a just of the wrong version passed verification" \
    "restore the version assertion in scripts/setup-just-install.sh"
fi
grep -Fq 'but .tool-versions pins just 1.99.0' "$work/out" ||
  fail "the version mismatch lacked a diagnostic naming the pin: $(cat "$work/out")"

# The action must actually go through the script, or none of the above covers it.
grep -Fq 'run: scripts/setup-just-install.sh' .github/actions/setup-just/action.yml ||
  fail "the setup-just action no longer installs through scripts/setup-just-install.sh" \
    "restore the delegation in .github/actions/setup-just/action.yml"

echo "check-setup-just: ok"
