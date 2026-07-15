#!/usr/bin/env bash
# Hermetic behavioral coverage for scripts/publish-crates.sh. The script itself
# and every local boundary are real; only cargo/crates.io are deterministic test
# doubles because publishing to the public registry is irreversible.
set -euo pipefail

cd "$(dirname "$0")/.."

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin"

cat >"$work/bin/cargo" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
case "$1" in
  pkgid)
    case " $* " in
      *" --package oneharness-core "*) printf 'path+file:///repo/crates/oneharness-core#%s\n' "${CORE_VERSION:-0.4.4}" ;;
      *" --package oneharness "*) printf 'path+file:///repo#oneharness@%s\n' "${CLI_VERSION:-0.3.21}" ;;
      *) exit 2 ;;
    esac
    ;;
  publish)
    printf '%s\n' "$*" >>"$PUBLISH_LOG"
    ;;
  *) exit 2 ;;
esac
EOF

cat >"$work/bin/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
url="${*: -1}"
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
export PUBLISH_LOG="$work/published"

reset_case() {
  : >"$PUBLISH_LOG"
  unset CORE_VERSION CLI_VERSION GITHUB_REF_NAME
  export CORE_HTTP=200 CLI_HTTP=200
}

expect_failure() {
  local expected="$1"
  shift
  if "$@" >"$work/stdout" 2>"$work/stderr"; then
    printf 'publish-crates-e2e: expected failure containing %q\n' "$expected" >&2
    exit 1
  fi
  if ! grep -Fq "$expected" "$work/stderr"; then
    printf 'publish-crates-e2e: missing error %q; got:\n' "$expected" >&2
    cat "$work/stderr" >&2
    exit 1
  fi
}

reset_case
GITHUB_REF_NAME=v0.3.21 scripts/publish-crates.sh
[ ! -s "$PUBLISH_LOG" ] || { echo "publish-crates-e2e: published an existing crate" >&2; exit 1; }

reset_case
CORE_HTTP=404 scripts/publish-crates.sh
grep -Fxq 'publish --locked --manifest-path crates/oneharness-core/Cargo.toml' "$PUBLISH_LOG"
[ "$(wc -l <"$PUBLISH_LOG")" -eq 1 ]

reset_case
CLI_HTTP=404 scripts/publish-crates.sh
grep -Fxq 'publish --locked --manifest-path Cargo.toml' "$PUBLISH_LOG"
[ "$(wc -l <"$PUBLISH_LOG")" -eq 1 ]

reset_case
expect_failure "release tag 'v9.9.9' does not match" env GITHUB_REF_NAME=v9.9.9 scripts/publish-crates.sh
[ ! -s "$PUBLISH_LOG" ]

reset_case
expect_failure "crates.io returned HTTP 503" env CORE_HTTP=503 scripts/publish-crates.sh
[ ! -s "$PUBLISH_LOG" ]

reset_case
expect_failure "could not query crates.io" env CORE_HTTP=error scripts/publish-crates.sh
[ ! -s "$PUBLISH_LOG" ]

reset_case
expect_failure "cargo returned an invalid version" env CLI_VERSION=not-a-version scripts/publish-crates.sh
[ ! -s "$PUBLISH_LOG" ]

echo "publish-crates-e2e: ok"
