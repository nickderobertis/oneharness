#!/usr/bin/env bash
# Deterministic contract coverage for scripts/publish-npm.sh. Registry responses
# are simulated because a real public publish cannot be rolled back.
set -euo pipefail

cd "$(dirname "$0")/.."

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin"

cat >"$work/bin/npm" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
case "$1" in
  pack)
    package="${4:-}"
    case "$package" in
      existing.tgz) identity='@oneharness/existing@1.2.3' ;;
      missing.tgz) identity='@oneharness/missing@1.2.3' ;;
      failing.tgz) identity='@oneharness/failing@1.2.3' ;;
      outage.tgz) identity='@oneharness/outage@1.2.3' ;;
      unreadable.tgz) echo 'npm error could not read package metadata' >&2; exit 2 ;;
      invalid.tgz) printf '{}\n'; exit 0 ;;
      *) exit 2 ;;
    esac
    name="${identity%@*}"
    version="${identity##*@}"
    printf '[{"name":"%s","version":"%s"}]\n' "$name" "$version"
    ;;
  view)
    case "$2" in
      @oneharness/existing@1.2.3) printf '1.2.3\n' ;;
      @oneharness/missing@1.2.3) echo 'npm error code E404' >&2; exit 1 ;;
      @oneharness/failing@1.2.3) echo 'npm error code E404' >&2; exit 1 ;;
      @oneharness/outage@1.2.3) echo 'npm error code E503' >&2; exit 1 ;;
      *) exit 2 ;;
    esac
    ;;
  publish)
    printf '%s\n' "$2" >>"$PUBLISH_LOG"
    if [ "$2" = failing.tgz ]; then
      echo 'simulated npm publish failure' >&2
      exit 1
    fi
    ;;
  *) exit 2 ;;
esac
EOF
chmod +x "$work/bin/npm"

export PATH="$work/bin:$PATH"
export PUBLISH_LOG="$work/published"
: >"$PUBLISH_LOG"

expect_publish_count() {
  local expected="$1" context="$2" actual
  actual="$(wc -l <"$PUBLISH_LOG")"
  if [ "$actual" -ne "$expected" ]; then
    printf 'check-publish-npm: expected %s publish(es) after %s; got:\n' "$expected" "$context" >&2
    cat "$PUBLISH_LOG" >&2
    exit 1
  fi
}

expect_failure() {
  local expected="$1"
  shift
  if "$@" >"$work/stdout" 2>"$work/stderr"; then
    printf 'check-publish-npm: expected failure containing %q\n' "$expected" >&2
    exit 1
  fi
  if ! grep -Fqx "$expected" "$work/stderr"; then
    printf 'check-publish-npm: missing error %q; got:\n' "$expected" >&2
    cat "$work/stderr" >&2
    exit 1
  fi
}

expect_failure "publish-npm: pass at least one package directory or tarball" scripts/publish-npm.sh
expect_publish_count 0 "a missing package argument"

scripts/publish-npm.sh existing.tgz missing.tgz >"$work/stdout"
if [ -s "$work/stdout" ] || ! grep -Fxq 'missing.tgz' "$PUBLISH_LOG"; then
  echo "check-publish-npm: existing/missing registry decisions were incorrect" >&2
  exit 1
fi
expect_publish_count 1 "existing and missing versions"

if scripts/publish-npm.sh failing.tgz >"$work/stdout" 2>"$work/stderr"; then
  echo 'check-publish-npm: failing publish unexpectedly succeeded' >&2
  exit 1
fi
grep -Fq 'simulated npm publish failure' "$work/stderr" || {
  echo 'check-publish-npm: npm publish failure output was hidden' >&2
  exit 1
}
grep -Fq "publish-npm: npm could not publish '@oneharness/failing@1.2.3'" "$work/stderr" || {
  echo 'check-publish-npm: npm publish failure lacked a concise diagnostic' >&2
  exit 1
}
expect_publish_count 2 "a failed npm publish"

if scripts/publish-npm.sh outage.tgz >"$work/stdout" 2>"$work/stderr"; then
  echo "check-publish-npm: registry outage unexpectedly permitted publishing" >&2
  exit 1
fi
if ! grep -Fq "cannot query '@oneharness/outage@1.2.3'" "$work/stderr"; then
  echo "check-publish-npm: registry outage lacked a concise diagnostic" >&2
  exit 1
fi
expect_publish_count 2 "a registry outage"

expect_failure "publish-npm: cannot read package metadata from 'unreadable.tgz'; rebuild the npm artifact" scripts/publish-npm.sh unreadable.tgz
expect_publish_count 2 "an npm pack metadata-read failure"

if scripts/publish-npm.sh invalid.tgz >"$work/stdout" 2>"$work/stderr"; then
  echo "check-publish-npm: invalid metadata unexpectedly permitted publishing" >&2
  exit 1
fi
if ! grep -Fq "npm returned invalid metadata" "$work/stderr"; then
  echo "check-publish-npm: invalid metadata lacked a concise diagnostic" >&2
  exit 1
fi
expect_publish_count 2 "invalid package metadata"

echo "check-publish-npm: ok"
