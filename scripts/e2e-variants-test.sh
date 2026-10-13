#!/usr/bin/env bash
# Hermetic boundary coverage for e2e-variants.sh; no provider API is contacted.
# llmlint: ignore-file[e2e_not_mocked] This deterministic test drives the real built oneharness CLI and the complete live-variants shell workflow. Only paid model-provider processes are replaced by the repository's shipped subprocess double, which is necessary to exercise provider failure and malformed-report recovery paths reliably without credentials, network calls, or billable CI; scripts/e2e-variants.sh remains the separate real-provider drift alarm.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/bin"
mkdir -p "$tmp/live"

fail() {
  echo "e2e-variants-test: $1; rerun with 'bash -x scripts/e2e-variants-test.sh' to inspect the failing boundary" >&2
  exit 1
}

assert_contains() {
  local file="$1" pattern="$2" description="$3"
  grep -q "$pattern" "$file" ||
    fail "$description; inspect the captured output by running: bash -x scripts/e2e-variants-test.sh"
}

resolve_executable() {
  local path="$1"
  if [ -x "$path" ]; then
    printf '%s' "$path"
  elif [ -x "$path.exe" ]; then
    printf '%s' "$path.exe"
  else
    return 1
  fi
}

oh="$(resolve_executable "$root/target/debug/oneharness" || true)"
mock="$(resolve_executable "$root/target/debug/oneharness-mock-harness" || true)"
[ -x "$oh" ] || fail "built oneharness binary is missing; run 'just check' and retry"
[ -x "$mock" ] ||
  fail "provider subprocess double is missing; run 'just check' and retry"

# The real oneharness process launches these provider-boundary adapters. Each
# adapter delegates to the repository's shipped subprocess double with the
# provider's real output shape, derived from the prompt oneharness delivered.
if [ "${OS:-}" = "Windows_NT" ]; then
  provider="$tmp/bin/e2e-variants-provider-double.sh"
  cp "$root/scripts/e2e-variants-provider-double.sh" "$provider"
  chmod +x "$provider"
  bash_windows="$(cygpath -w "$(command -v bash)")"
  provider_windows="$(cygpath -w "$provider")"
  for tool in claude codex; do
    # Git Bash's command lookup needs an extensionless executable, while
    # the native Rust child lookup selects the PATHEXT-backed .cmd shim.
    cp "$provider" "$tmp/bin/$tool"
    chmod +x "$tmp/bin/$tool"
    printf '@"%s" "%s" "--provider-tool=%s" %%*\r\n' \
      "$bash_windows" "$provider_windows" "$tool" >"$tmp/bin/$tool.cmd"
  done
else
  for tool in claude codex; do
    cp "$root/scripts/e2e-variants-provider-double.sh" "$tmp/bin/$tool"
    chmod +x "$tmp/bin/$tool"
  done
fi

common_env=(
  PATH="$tmp/bin:$PATH"
  ONEHARNESS_BIN="$oh"
  OH_E2E_PROVIDER_DOUBLE="$mock"
  TMPDIR="$tmp/live"
  ANTHROPIC_API_KEY="test-anthropic-material"
  OPENAI_API_KEY="test-openai-material"
  OH_E2E_VARIANTS_API_ONLY=1
  OH_E2E_VARIANTS_CORE_ONLY=1
  OH_E2E_TELEMETRY_DIR="$tmp/telemetry"
)

for selector in OH_E2E_VARIANTS_RUN_OPENCODE OH_E2E_VARIANTS_RUN_QWEN; do
  if env "${common_env[@]}" "$selector=invalid" \
    bash "$root/scripts/e2e-variants.sh" >"$tmp/selector.out" 2>"$tmp/selector.err"; then
    fail "invalid $selector value unexpectedly succeeded"
  fi
  assert_contains "$tmp/selector.err" \
    'OH_E2E_VARIANTS_RUN_OPENCODE and OH_E2E_VARIANTS_RUN_QWEN must each be 0 or 1' \
    "invalid $selector value omitted its validation diagnostic"
done

evidence="$tmp/evidence.log"
if ! env "${common_env[@]}" OH_E2E_EVIDENCE_FILE="$evidence" \
  bash "$root/scripts/e2e-variants.sh" >"$tmp/success.out" 2>"$tmp/success.err"; then
  fail "successful boundary run failed: $(tail -1 "$tmp/success.err")"
fi
assert_contains "$evidence" 'IDENTITY claude-code:apikey' \
  "successful run omitted Claude API identity evidence"
assert_contains "$evidence" 'ASSERT fallback: first_failure_kind=auth' \
  "successful run omitted same-harness fallback evidence"
assert_contains "$tmp/success.err" 'live variants: ok' \
  "successful run omitted its completion diagnostic"
[ ! -s "$tmp/success.out" ] ||
  fail "successful live workflow wrote unexpected stdout; keep machine-readable evidence in OH_E2E_EVIDENCE_FILE"

env "${common_env[@]}" OH_E2E_EVIDENCE=1 \
  bash "$root/scripts/e2e-variants.sh" >"$tmp/legacy.out" 2>"$tmp/legacy.err"
assert_contains "$tmp/legacy.out" 'IDENTITY claude-code:apikey' \
  "legacy OH_E2E_EVIDENCE mode omitted its sanitized stdout evidence"

if env "${common_env[@]}" OH_E2E_EVIDENCE_FILE=$'bad\npath' \
  bash "$root/scripts/e2e-variants.sh" >"$tmp/path.out" 2>"$tmp/path.err"; then
  fail "multiline evidence target unexpectedly succeeded"
fi
assert_contains "$tmp/path.err" 'must be a single-line file path' \
  "multiline evidence target omitted its validation diagnostic"

if env "${common_env[@]}" OH_E2E_EVIDENCE_FILE="$tmp/bin" \
  bash "$root/scripts/e2e-variants.sh" >"$tmp/path.out" 2>"$tmp/path.err"; then
  fail "directory evidence target unexpectedly succeeded"
fi
assert_contains "$tmp/path.err" 'remove the non-file target or choose a new file path' \
  "invalid evidence target omitted its recovery action"

touch "$tmp/read-only-evidence"
chmod 400 "$tmp/read-only-evidence"
if env "${common_env[@]}" OH_E2E_EVIDENCE_FILE="$tmp/read-only-evidence" \
  bash "$root/scripts/e2e-variants.sh" >"$tmp/path.out" 2>"$tmp/path.err"; then
  fail "unwritable evidence target unexpectedly succeeded"
fi
assert_contains "$tmp/path.err" 'run chmod u+w on it or choose another path' \
  "unwritable evidence target omitted its recovery action"

if env "${common_env[@]}" OH_E2E_EVIDENCE_FILE="$tmp/missing/evidence.log" \
  bash "$root/scripts/e2e-variants.sh" >"$tmp/path.out" 2>"$tmp/path.err"; then
  fail "missing evidence parent unexpectedly succeeded"
fi
assert_contains "$tmp/path.err" 'create it with mkdir -p' \
  "missing evidence parent omitted its recovery action"

if env "${common_env[@]}" OH_E2E_DOUBLE_VALID_FAILURE=1 \
  bash "$root/scripts/e2e-variants.sh" >"$tmp/report.out" 2>"$tmp/report.err"; then
  fail "nonzero provider report unexpectedly succeeded"
fi
assert_contains "$tmp/report.err" 'exited nonzero (status=nonzero exit_code=7' \
  "valid nonzero report omitted its structured status diagnostic"

if env "${common_env[@]}" \
  ONEHARNESS_BIN="$root/scripts/e2e-variants-invalid-report-double.sh" \
  bash "$root/scripts/e2e-variants.sh" >"$tmp/failure.out" 2>"$tmp/failure.err"; then
  fail "invalid provider report unexpectedly succeeded"
fi
assert_contains "$tmp/failure.err" 'before producing a valid report' \
  "invalid provider report omitted its exit diagnostic"

if env "${common_env[@]}" OH_E2E_VARIANTS_API_ONLY= \
  OH_CLAUDE_CONFIG_A="$tmp/missing" OH_CLAUDE_CONFIG_B="$tmp" \
  bash "$root/scripts/e2e-variants.sh" >"$tmp/config.out" 2>"$tmp/config.err"; then
  fail "missing Claude config directory unexpectedly succeeded"
fi
assert_contains "$tmp/config.err" 'must name an existing directory' \
  "missing Claude config directory omitted its validation diagnostic"

if env "${common_env[@]}" OH_E2E_VARIANTS_API_ONLY= \
  OH_CLAUDE_CONFIG_A=$'bad\npath' OH_CLAUDE_CONFIG_B="$tmp" \
  bash "$root/scripts/e2e-variants.sh" >"$tmp/config.out" 2>"$tmp/config.err"; then
  fail "multiline Claude config directory unexpectedly succeeded"
fi
assert_contains "$tmp/config.err" 'must be a non-empty single-line directory path' \
  "multiline Claude config directory omitted its validation diagnostic"

# Where the recorder measures — Linux with pasta and unprivileged user
# namespaces — the opted-in phase demands a measured line; a Linux box without
# them skips the journey below, saying why, unless OH_NET_NO_SKIP=1 (CI's
# Linux leg) makes that a failure. macOS and Windows run it on the fallback.
recorder_probe="$tmp/recorder-probe.jsonl"
if [ "$(uname -s)" = Linux ] &&
  ! { python3 "$root/scripts/e2e-netmeasure.py" --line-file "$recorder_probe" --launch-id probe -- true 2>/dev/null &&
    jq -e '.method == "netns-pasta"' "$recorder_probe" >/dev/null; }; then
  why="$(jq -r '.reason // "no measurement"' "$recorder_probe" 2>/dev/null || echo "the recorder could not run")"
  [ -z "${OH_NET_NO_SKIP:-}" ] || fail "the network recorder cannot measure on this Linux host ($why) and OH_NET_NO_SKIP is set"
  echo "e2e-variants-test: ok (opted-in plugins journey skipped: $why)"
  exit 0
fi

# The opted-in Codex launch (codex:plugins), through the real measuring shim
# with the mock codex harness standing for `codex`: the double records the
# argv it was launched with, the sleep its turn ran, and the mock's plugins
# record of each launch as the evidence the phase reads in Codex's log's place.
plugins_bin="$tmp/plugins-bin"
plugins_home="$tmp/plugins-home"
mkdir -p "$plugins_bin" "$plugins_home/.codex/plugins/fixture-plugin"
printf '#!/usr/bin/env bash\nexec bash "%s" --double-tool=codex "$@"\n' \
  "$root/tools/network-telemetry/harness_double.sh" >"$plugins_bin/codex"
chmod +x "$plugins_bin/codex"
if [ "${OS:-}" = "Windows_NT" ]; then
  printf '@"%s" "%s" "--double-tool=codex" %%*\r\n' "$bash_windows" \
    "$(cygpath -w "$root/tools/network-telemetry/harness_double.sh")" >"$plugins_bin/codex.cmd"
fi
plugins_run() {
  env "${common_env[@]}" PATH="$plugins_bin:$tmp/bin:$PATH" HOME="$plugins_home" \
    OH_E2E_CODEX_SUBSCRIPTION=1 OH_E2E_PLUGINS_SLEEP=2 OH_TEST_MOCK="$mock" \
    OH_E2E_PLUGINS_EVIDENCE="$tmp/plugins-evidence.jsonl" OH_TEST_PLUGINS_EVIDENCE="$tmp/plugins-evidence.jsonl" \
    OH_TEST_ARGV_LOG="$tmp/plugins-argv.jsonl" OH_TEST_RAN_LOG="$tmp/plugins-ran.log" \
    bash "$root/scripts/e2e-variants.sh" >"$tmp/plugins.out" 2>"$tmp/plugins.err"
}
rm -rf "$tmp/telemetry"
plugins_run || fail "the opted-in plugins launch failed: $(tail -3 "$tmp/plugins.err")"
telemetry="$tmp/telemetry/variants.jsonl"
launch="$(jq -c 'select(.phase == "codex-plugins-opt-in" and .kind == "exec" and .argv)' "$telemetry")"
[ "$(printf '%s\n' "$launch" | grep -c launch_id)" = 1 ] ||
  fail "the opted-in phase must record exactly one exec launch in variants.jsonl, got: $launch"
launch_id="$(printf '%s' "$launch" | jq -r .launch_id)"
[ "$(jq -c --arg id "$launch_id" 'select(.launch_id == $id and (.argv | not))' "$telemetry" | grep -c .)" = 1 ] ||
  fail "the opted-in launch $launch_id must have exactly one measurement line"
[ ! -s "$tmp/telemetry/codex.jsonl" ] ||
  fail "the opted-in launch wrote to codex.jsonl, which only live-codex's default-config launches may"
launched="$(jq -c 'select(.tool == "codex" and (.argv | index(["exec"]) != null) and (.argv | join(" ") | contains("_pl")))' \
  "$tmp/plugins-argv.jsonl")"
printf '%s' "$launched" | jq -e '.argv | index(["-c", "model_reasoning_effort=\"low\""]) != null' >/dev/null ||
  fail "the double's argv lacks the variant's own args: $launched"
printf '%s' "$launched" | jq -e '(.argv | index(["-c", "features.plugins=false"]) == null) and (.argv | index(["--disable", "plugins"]) == null)' >/dev/null ||
  fail "the double was launched with a plugins-off flag despite plugins = true: $launched"
grep -qx 'sleep 2' "$tmp/plugins-ran.log" || fail "the opted-in turn never ran its sustained shell command"
jq -e --arg id "$launch_id" 'select(.launch_id == $id and (.argv | not)) | .seconds >= 2' "$telemetry" >/dev/null ||
  fail "the recorder measured the opted-in launch shorter than the sleep its turn ran"

# The phase's assertions over that run's own evidence and telemetry, each
# copy changed in the one way a case names.
plugins_marker="$(printf '%s' "$launch" | jq -r '.argv | join(" ")' | grep -oE 'OH_VARIANT_[0-9]+_[0-9]+_pl' | head -1)"
assert_case() {
  local expect="$1" description="$2" pattern="$3"
  shift 3
  if bash -c 'set -euo pipefail; source "$1"; shift; "$@"' _ "$root/scripts/e2e-lib.sh" "$@" \
    >"$tmp/case.out" 2>"$tmp/case.err"; then
    [ "$expect" = pass ] || fail "$description unexpectedly passed"
  else
    [ "$expect" = fail ] || fail "$description failed: $(cat "$tmp/case.err")"
    grep -q -- "$pattern" "$tmp/case.err" || fail "$description failed without naming why: $(cat "$tmp/case.err")"
  fi
}
evidence="$tmp/plugins-evidence.jsonl"
assert_case pass "the opted-in launch's own evidence" "" oh_codex_plugins_evidence_assert "$plugins_marker" "$evidence"
jq -c --arg m "$plugins_marker" 'select(.text | contains($m)) | .catalog_request = false' "$evidence" >"$tmp/ev-no-catalog.jsonl"
assert_case fail "evidence without a catalog request" "never requested the plugin catalog" \
  oh_codex_plugins_evidence_assert "$plugins_marker" "$tmp/ev-no-catalog.jsonl"
jq -c --arg m "$plugins_marker" 'select(.text | contains($m)) | .plugins_loaded = false' "$evidence" >"$tmp/ev-no-load.jsonl"
assert_case fail "evidence without a plugin load" "never loaded its plugins" \
  oh_codex_plugins_evidence_assert "$plugins_marker" "$tmp/ev-no-load.jsonl"
jq -c --arg m "$plugins_marker" 'if (.text | contains($m)) then .text = "another launch" else . end' \
  "$evidence" >"$tmp/ev-other.jsonl"
assert_case fail "evidence that belongs only to another launch" "belongs to the opted-in launch" \
  oh_codex_plugins_evidence_assert "$plugins_marker" "$tmp/ev-other.jsonl"
jq -c --arg m "$plugins_marker" 'select(.text | contains($m) | not)' "$evidence" >"$tmp/ev-absent.jsonl"
assert_case fail "evidence with the launch's records absent" "belongs to the opted-in launch" \
  oh_codex_plugins_evidence_assert "$plugins_marker" "$tmp/ev-absent.jsonl"
assert_case fail "no evidence at all" "no Codex plugins evidence" \
  oh_codex_plugins_evidence_assert "$plugins_marker" "$tmp/missing-evidence.jsonl"
# The measurement cases are the Linux leg's, where the budgets apply.
check_lines() {
  OH_E2E_OS=Linux assert_case "$1" "$2" "$3" oh_launch_measured_assert "$4" codex-plugins-opt-in exec 2
}
measurement="$(jq -c --arg id "$launch_id" 'select(.launch_id == $id and (.argv | not))' "$telemetry")"
others="$(jq -c --arg id "$launch_id" 'select(.launch_id != $id)' "$telemetry")"
measured_line="$(printf '%s' "$measurement" | jq -c '.method = "netns-pasta" | .rx_bytes = 5000 | .tx_bytes = 900')"
printf '%s\n%s\n%s\n' "$others" "$launch" "$measured_line" >"$tmp/lines-ok.jsonl"
check_lines pass "a complete measured line" "" "$tmp/lines-ok.jsonl"
printf '%s\n%s\n' "$others" "$launch" >"$tmp/lines-missing.jsonl"
check_lines fail "a missing measurement line" "has no measurement line" "$tmp/lines-missing.jsonl"
printf '%s\n%s\n%s\n' "$others" "$launch" \
  "$(printf '%s' "$measurement" | jq -c '{launch_id, method: "unavailable", reason: "no pasta on PATH", seconds, exit}')" \
  >"$tmp/lines-unavailable.jsonl"
check_lines fail "an unavailable measurement line" "was not measured" "$tmp/lines-unavailable.jsonl"
printf '%s\n%s\n%s\n' "$others" "$launch" "$(printf '%s' "$measured_line" | jq -c 'del(.tx_bytes)')" \
  >"$tmp/lines-incomplete.jsonl"
check_lines fail "an incomplete measurement line" "incomplete measurement line" "$tmp/lines-incomplete.jsonl"
printf '%s\n%s\n%s\n%s\n' "$others" "$launch" "$measured_line" "$measured_line" >"$tmp/lines-duplicate.jsonl"
check_lines fail "a duplicated measurement line" "more than one measurement line" "$tmp/lines-duplicate.jsonl"

# A Codex home that turns plugins off itself fails the phase, naming the file.
printf '[features]\nplugins = false\n' >"$plugins_home/.codex/config.toml"
if plugins_run; then
  fail "a Codex home whose config.toml disables plugins unexpectedly passed the opted-in phase"
fi
assert_contains "$tmp/plugins.err" "$plugins_home/.codex/config.toml disables Codex plugins" \
  "a plugins-off Codex home did not name its config.toml"

echo "e2e-variants-test: ok"
