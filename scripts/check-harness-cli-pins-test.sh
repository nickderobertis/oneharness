#!/usr/bin/env bash
# Hermetic proof that check-harness-cli-pins.sh still fires. No network: each
# case writes a pin file and a workflow into scratch space (or drifts a copy of
# the real ci.yml) and drives the real gate over them.
#
# The gate is the only thing between a floating harness CLI install and a
# required check that fails a passing tree mid-release; a green CI log cannot
# show that it would have caught one, so it needs its own proof.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
gate="$root/scripts/check-harness-cli-pins.sh"

tmp="$(mktemp -d "${TMPDIR:-/tmp}/check-harness-cli-pins-test.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT

fail() {
  echo "check-harness-cli-pins-test: $1" >&2
  echo "  rerun with 'bash -x scripts/check-harness-cli-pins-test.sh' to inspect the failing case" >&2
  exit 1
}

pins() { printf '%s\n' "$@" >"$tmp/pins"; }
workflow() { printf '%s\n' "$@" >"$tmp/wf.yml"; }

# $1 description, $2 expected exit (0|1), $3 substring the output must carry;
# the rest are the gate's arguments (default: the scratch pin file and workflow).
run_case() {
  local description="$1" expected="$2" needle="$3" out status=0
  shift 3
  [ "$#" -gt 0 ] || set -- "$tmp/pins" "$tmp/wf.yml"
  out="$(bash "$gate" "$@" 2>&1)" || status=$?
  [ "$status" -eq "$expected" ] ||
    fail "$description: exit $status, want $expected; output: $out"
  case "$out" in
    *"$needle"*) ;;
    *) fail "$description: output lacks '$needle': $out" ;;
  esac
}

pins '# comment' '' '@github/copilot 1.0.114' 'opencode-ai 1.19.6' 'goose 1.60.2'
goose_ok='curl -fsSL https://github.com/block/goose/releases/download/v1.60.2/download_cli.sh | GOOSE_VERSION=1.60.2 CONFIGURE=false bash'

workflow 'run: |' \
  '  npm install -g @github/copilot@1.0.114 opencode-ai@1.19.6 && copilot --version' \
  "  $goose_ok"
run_case "every install at its pin" 0 "check-harness-cli-pins: ok"

# The incident: a scoped package with no version takes whatever is latest.
workflow '  npm install -g @github/copilot' '  npm i -g opencode-ai@1.19.6' "  $goose_ok"
run_case "an unpinned scoped package" 1 \
  "wf.yml:1: npm installs @github/copilot unpinned; install @github/copilot@1.0.114"

workflow '  npm install --global @github/copilot@1.0.115 opencode-ai@1.19.6' "  $goose_ok"
run_case "a version other than the pin" 1 \
  "npm installs @github/copilot@1.0.115 but $tmp/pins pins 1.0.114"

workflow '  npm install -g @github/copilot@latest opencode-ai@1.19.6' "  $goose_ok"
run_case "a floating dist-tag" 1 "npm installs @github/copilot@latest but"

workflow '  npm install -g @github/copilot@1.0.114 opencode-ai@1.19.6 some-new-harness' "  $goose_ok"
run_case "an install the pin file does not know" 1 \
  "npm installs some-new-harness, which $tmp/pins does not pin"

# A package continued onto the next line is still an install.
workflow "  npm install -g @github/copilot@1.0.114 \\" '    opencode-ai' "  $goose_ok"
run_case "an unpinned package on a continuation line" 1 \
  "wf.yml:1: npm installs opencode-ai unpinned"

workflow '  npm install -g @github/copilot@1.0.114 opencode-ai@1.19.6' \
  '  curl -fsSL https://github.com/block/goose/releases/download/stable/download_cli.sh | CONFIGURE=false bash'
run_case "Goose's floating stable installer" 1 \
  "fetches a Goose installer other than $tmp/pins's v1.60.2"
run_case "Goose with no version input" 1 "installs Goose without GOOSE_VERSION=1.60.2"

workflow '  npm install -g @github/copilot@1.0.114 opencode-ai@1.19.6' \
  '  curl -fsSL https://github.com/block/goose/releases/download/v1.60.2/download_cli.sh | GOOSE_VERSION=1.60.1 CONFIGURE=false bash'
run_case "Goose at another version" 1 "installs Goose without GOOSE_VERSION=1.60.2"

workflow '  npm install -g @github/copilot@1.0.114'
run_case "a pin nothing installs" 1 \
  "pins opencode-ai but no checked workflow installs it"

# The pin file is held to exactly one x.y.z per package, once.
workflow '  npm install -g @github/copilot@1.0.114'
for bad in '@github/copilot 1.0.114 1.0.115' '@github/copilot latest' '@github/copilot 1.0' '@github/copilot'; do
  pins "$bad"
  run_case "the pin '$bad'" 1 "must pin one package to exactly one x.y.z version"
done
pins '@github/copilot 1.0.114' '@github/copilot 1.0.114'
run_case "a package pinned twice" 1 "pins @github/copilot twice"

run_case "a missing pin file" 1 "no pin file at $tmp/absent" "$tmp/absent" "$tmp/wf.yml"

# The real ci.yml, drifted by one version, must fail at the line that drifted
# and name the fix; the undrifted copy must pass.
cp "$root/.github/workflows/ci.yml" "$tmp/ci.yml"
run_case "the real ci.yml" 0 "check-harness-cli-pins: ok" "$root/.harness-cli-versions" "$tmp/ci.yml"
pin="$(awk '$1 == "@github/copilot" { print $2 }' "$root/.harness-cli-versions")"
sed "s#@github/copilot@$pin\$#@github/copilot@0.0.1#" "$root/.github/workflows/ci.yml" >"$tmp/ci.yml"
line="$(awk '/@github\/copilot@0\.0\.1$/ { print NR; exit }' "$tmp/ci.yml")"
[ -n "$line" ] || fail "the real ci.yml no longer installs @github/copilot@$pin on a line of its own"
run_case "the real ci.yml with Copilot drifted" 1 \
  "ci.yml:$line: npm installs @github/copilot@0.0.1 but $root/.harness-cli-versions pins $pin" \
  "$root/.harness-cli-versions" "$tmp/ci.yml"

echo "check-harness-cli-pins-test: ok"
