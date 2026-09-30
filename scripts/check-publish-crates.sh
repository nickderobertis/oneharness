#!/usr/bin/env bash
# Deterministic contract coverage for scripts/publish-crates.sh. Registry and
# cargo responses are simulated because a real public publish is irreversible;
# the last case hands version resolution to the real toolchain over this
# workspace, doubling only the crates.io query and `cargo publish`.
set -euo pipefail

cd "$(dirname "$0")/.."

real_cargo="$(command -v cargo)"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin"

cat >"$work/bin/cargo" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
case "$1" in
  pkgid)
    case " $* " in
      *" --manifest-path crates/oneharness-core/Cargo.toml "*) package=oneharness-core ;;
      *" --manifest-path Cargo.toml "*) package=oneharness ;;
      *) exit 2 ;;
    esac
    if [ "$package" = "${PKGID_FAILURE:-}" ]; then
      echo 'error: simulated unreadable manifest' >&2
      exit 101
    fi
    if [ -n "${PKGID_NAME:-}" ]; then
      printf 'path+file:///repo#%s@1.0.0\n' "$PKGID_NAME"
    elif [ "$package" = oneharness-core ]; then
      printf 'path+file:///repo/crates/oneharness-core#%s\n' "${CORE_VERSION:-0.4.4}"
    else
      printf 'path+file:///repo#oneharness@%s\n' "${CLI_VERSION:-0.3.21}"
    fi
    ;;
  publish)
    printf '%s\n' "$*" >>"$PUBLISH_LOG"
    if [ "${PUBLISH_FAILURE:-}" = cargo ]; then
      echo 'simulated cargo publish failure' >&2
      exit 101
    fi
    ;;
  *) exit 2 ;;
esac
EOF

cat >"$work/bin/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
expected_user_agent="oneharness-release/${CLI_VERSION:-0.3.21} (https://github.com/nickderobertis/oneharness)"
user_agent=
for arg in "$@"; do
  if [ "${previous:-}" = --user-agent ]; then
    user_agent="$arg"
  fi
  previous="$arg"
done
if [ "$user_agent" != "$expected_user_agent" ]; then
  printf 'curl: expected User-Agent %q, got %q\n' "$expected_user_agent" "$user_agent" >&2
  exit 2
fi
url="${*: -1}"
printf '%s\n' "$url" >>"$QUERY_LOG"
case "$url" in
  */oneharness-core/*) status="${CORE_HTTP:-200}" ;;
  */oneharness/*) status="${CLI_HTTP:-200}" ;;
  *) exit 2 ;;
esac
if [ "$status" = error ]; then
  exit 7
fi
printf '%s' "$status"
EOF

chmod +x "$work/bin/cargo" "$work/bin/curl"
export PATH="$work/bin:$PATH"
export PUBLISH_LOG="$work/published" QUERY_LOG="$work/queried"

reset_case() {
  : >"$PUBLISH_LOG"
  : >"$QUERY_LOG"
  unset CORE_VERSION CLI_VERSION GITHUB_REF_NAME PKGID_FAILURE PKGID_NAME PUBLISH_FAILURE
  export CORE_HTTP=200 CLI_HTTP=200
}

expect_failure() {
  local expected="$1"
  shift
  if "$@" >"$work/stdout" 2>"$work/stderr"; then
    printf 'check-publish-crates: expected failure containing %q\n' "$expected" >&2
    exit 1
  fi
  if ! grep -Fq "$expected" "$work/stderr"; then
    printf 'check-publish-crates: missing error %q; got:\n' "$expected" >&2
    cat "$work/stderr" >&2
    exit 1
  fi
}

expect_publish_log() {
  local expected="$1" count
  count="$(wc -l <"$PUBLISH_LOG")"
  if [ "$count" -ne 1 ] || ! grep -Fxq "$expected" "$PUBLISH_LOG"; then
    printf 'check-publish-crates: expected one publish %q; got:\n' "$expected" >&2
    cat "$PUBLISH_LOG" >&2
    exit 1
  fi
}

expect_publish_order() {
  local actual expected
  actual="$(cat "$PUBLISH_LOG")"
  expected=$'publish --locked --manifest-path crates/oneharness-core/Cargo.toml\npublish --locked --manifest-path Cargo.toml'
  if [ "$actual" != "$expected" ]; then
    printf 'check-publish-crates: expected core then CLI publish; got:\n' >&2
    cat "$PUBLISH_LOG" >&2
    exit 1
  fi
}

expect_no_publish() {
  local context="$1"
  if [ -s "$PUBLISH_LOG" ]; then
    printf 'check-publish-crates: %s unexpectedly published:\n' "$context" >&2
    cat "$PUBLISH_LOG" >&2
    exit 1
  fi
}

reset_case
GITHUB_REF_NAME=v0.3.21 scripts/publish-crates.sh
expect_no_publish "existing versions"

reset_case
CORE_HTTP=404 scripts/publish-crates.sh
expect_publish_log 'publish --locked --manifest-path crates/oneharness-core/Cargo.toml'

reset_case
CLI_HTTP=404 scripts/publish-crates.sh
expect_publish_log 'publish --locked --manifest-path Cargo.toml'

reset_case
CORE_HTTP=404 CLI_HTTP=404 scripts/publish-crates.sh
expect_publish_order

reset_case
expect_failure "publish-crates: cargo could not publish oneharness-core 0.4.4" env CORE_HTTP=404 PUBLISH_FAILURE=cargo scripts/publish-crates.sh
grep -Fq 'simulated cargo publish failure' "$work/stderr" || {
  echo 'check-publish-crates: cargo publish failure output was hidden' >&2
  exit 1
}

reset_case
expect_failure "release tag 'v9.9.9' does not match" env GITHUB_REF_NAME=v9.9.9 scripts/publish-crates.sh
expect_no_publish "a mismatched release tag"

reset_case
expect_failure "publish-crates: cannot validate oneharness-core's version in crates/oneharness-core/Cargo.toml; run 'cargo metadata --no-deps' and fix the manifest" env PKGID_FAILURE=oneharness-core scripts/publish-crates.sh
expect_no_publish "a cargo pkgid failure"
grep -Fq 'error: simulated unreadable manifest' "$work/stderr" || {
  echo "check-publish-crates: cargo's own pkgid error was discarded; keep cargo pkgid's stderr in manifest_version's failure message" >&2
  exit 1
}

reset_case
expect_failure "publish-crates: crates/oneharness-core/Cargo.toml declares package 'other', not oneharness-core" env PKGID_NAME=other scripts/publish-crates.sh
expect_no_publish "a manifest naming another package"

reset_case
expect_failure "crates.io returned HTTP 503" env CORE_HTTP=503 scripts/publish-crates.sh
expect_no_publish "an HTTP 503"

reset_case
expect_failure "could not query crates.io" env CORE_HTTP=error scripts/publish-crates.sh
expect_no_publish "a registry connection error"

reset_case
expect_failure "cargo returned an invalid version" env CLI_VERSION=not-a-version scripts/publish-crates.sh
expect_no_publish "an invalid manifest version"

# Real toolchain over this workspace. history-compat resolves the published
# oneharness-core beside the member, the state that made a `--package` spec
# ambiguous and failed the v0.19.0 release; the precondition keeps this case
# proving that state rather than an easier one.
manifest_package_version() {
  sed -n '/^\[package\]/,/^\[/s/^version = "\(.*\)"$/\1/p' "$1"
}
core_manifest_version="$(manifest_package_version crates/oneharness-core/Cargo.toml)"
cli_manifest_version="$(manifest_package_version Cargo.toml)"
if ! grep -A1 -Fx 'name = "oneharness-core"' Cargo.lock | grep -Fxq 'version = "0.19.0"'; then
  echo 'check-publish-crates: the workspace no longer resolves the published oneharness-core 0.19.0; the real-toolchain case no longer proves the ambiguous-name release, so restate it' >&2
  exit 1
fi

reset_case
cat >"$work/bin/cargo" <<EOF
#!/usr/bin/env bash
set -euo pipefail
if [ "\$1" = publish ]; then
  printf '%s\\n' "\$*" >>"\$PUBLISH_LOG"
  exit 0
fi
exec "$real_cargo" "\$@"
EOF
CORE_HTTP=404 CLI_HTTP=404 CLI_VERSION="$cli_manifest_version" GITHUB_REF_NAME="v$cli_manifest_version" \
  scripts/publish-crates.sh
expect_publish_order
expected_queries="https://crates.io/api/v1/crates/oneharness-core/$core_manifest_version
https://crates.io/api/v1/crates/oneharness/$cli_manifest_version"
if [ "$(cat "$QUERY_LOG")" != "$expected_queries" ]; then
  printf 'check-publish-crates: expected the real toolchain to resolve core %s then CLI %s; fix manifest_version in scripts/publish-crates.sh (run it by hand with cargo pkgid over this workspace). Queried:\n' \
    "$core_manifest_version" "$cli_manifest_version" >&2
  cat "$QUERY_LOG" >&2
  exit 1
fi

echo "check-publish-crates: ok"
