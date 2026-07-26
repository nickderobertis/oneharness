#!/usr/bin/env bash
#
# End-to-end smoke of the *built* oneharness binary — the artifact a user runs,
# not the test-compiled crate. Two modes:
#
#   (default) hermetic — drive the built binary through `list`, `detect --all`,
#       and `run --all --print-command`, then one real spawn+parse against the
#       mock-harness fixture. No network, no auth, fully deterministic. This is
#       what `just check` (and therefore CI) requires on every platform.
#
#   --live             — additionally fire a real prompt at whatever harnesses
#       are installed and authenticated, skipping cleanly when none are. Opt-in:
#       it needs binaries, auth, and network and makes real (paid) model calls,
#       so it is never part of the gate or CI. Run it via `just smoke-live`.
#
# Output is context the next agent reads: near-silent on success (one line),
# and on failure the exact step, command, captured output, and a suggested fix.
set -euo pipefail

LIVE=0
case "${1:-}" in
  --live) LIVE=1 ;;
  "") ;;
  *)
    echo "smoke: unknown argument: $1 (use --live, or no argument)" >&2
    exit 2
    ;;
esac

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

# Hermetic: the machine's real oneharness config (user-level or a project file
# above the repo) must never shape these assertions. The config feature itself
# is smoked in step 3b with explicitly planted files.
export ONEHARNESS_NO_CONFIG=1

PROMPT="oneharness smoke: reply with the single word pong"
LAST_CMD=""

# Resolve a built binary path, tolerating the Windows `.exe` suffix.
exe_path() {
  if [ -x "$1" ]; then printf '%s' "$1"; return 0; fi
  if [ -x "$1.exe" ]; then printf '%s' "$1.exe"; return 0; fi
  return 1
}

fail() {
  # $1 message, $2 command (optional), $3 captured output (optional), $4 fix (optional)
  echo "smoke: FAIL — $1" >&2
  [ -n "${2:-}" ] && echo "  command: $2" >&2
  if [ -n "${3:-}" ]; then
    echo "  output:" >&2
    printf '%s\n' "$3" >&2
  fi
  [ -n "${4:-}" ] && echo "  fix: $4" >&2
  exit 1
}

assert_contains() {
  # $1 haystack, $2 fixed-string needle, $3 fix hint (optional)
  if ! printf '%s' "$1" | grep -qF -- "$2"; then
    fail "expected output to contain: $2" "$LAST_CMD" "$1" "${3:-}"
  fi
}

count_matches() {
  # Count occurrences of a fixed string; cross-platform, and zero-safe so a
  # no-match grep (exit 1) doesn't trip `set -e`/`pipefail`.
  { printf '%s' "$1" | grep -oF -- "$2" || true; } | wc -l | tr -d '[:space:]'
}

# Resolve the oneharness binary to smoke. Prefer an explicit override, then the
# *freshest* built binary (release vs debug, by mtime). Preferring the newer of
# the two is deliberate: `just check` rebuilds debug right before smoke, so a
# just-built debug must win over a stale release left in target/ — otherwise the
# gate silently smokes an out-of-date artifact (the original footgun). Build
# debug if neither exists.
resolve_oneharness() {
  if [ -n "${ONEHARNESS_BIN:-}" ]; then printf '%s' "$ONEHARNESS_BIN"; return 0; fi
  local rel deb
  rel="$(exe_path target/release/oneharness || true)"
  deb="$(exe_path target/debug/oneharness || true)"
  if [ -n "$rel" ] && [ -n "$deb" ]; then
    # `-nt`: POSIX file-newer-than test, portable across Linux/macOS/Git-Bash.
    if [ "$rel" -nt "$deb" ]; then printf '%s' "$rel"; else printf '%s' "$deb"; fi
    return 0
  fi
  [ -n "$rel" ] && { printf '%s' "$rel"; return 0; }
  [ -n "$deb" ] && { printf '%s' "$deb"; return 0; }
  echo "smoke: building oneharness (debug)…" >&2
  cargo build --locked >&2
  exe_path target/debug/oneharness || fail "could not find oneharness after build" \
    "" "" "run 'just build' and retry"
}

resolve_mock() {
  local c
  for c in target/release/oneharness-mock-harness target/debug/oneharness-mock-harness; do
    if p="$(exe_path "$c")"; then printf '%s' "$p"; return 0; fi
  done
  echo "smoke: building mock-harness fixture…" >&2
  cargo build --locked --features mock-harness --bin oneharness-mock-harness >&2
  exe_path target/debug/oneharness-mock-harness || fail \
    "could not find mock-harness fixture after build" \
    "" "" "run 'cargo build --features mock-harness --bin oneharness-mock-harness'"
}

oh="$(resolve_oneharness)"

# Belt-and-suspenders against a stale artifact the freshest-binary pick can't
# catch (e.g. a leftover release of a different version): the binary under test
# must report the crate version. This is exactly the failure mode that motivated
# the guard — a 0.1.0 release binary shadowing a 0.1.1 source tree.
crate_ver="$(grep -m1 -E '^version[[:space:]]*=' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"
bin_ver="$("$oh" --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)"
if [ -n "$crate_ver" ] && [ -n "$bin_ver" ] && [ "$crate_ver" != "$bin_ver" ]; then
  fail "binary under test is v$bin_ver but Cargo.toml is v$crate_ver (stale build)" \
    "$oh --version" "" "rebuild with 'just build' (or 'just build-release'), then retry"
fi

# 0. Installer e2e: package the binary under test as a release-shaped archive,
#    install it through scripts/install.sh from a local URL, and prove the
#    installed binary runs. This keeps the installer covered in `just check`
#    without touching the network or depending on an already-published release.
install_dir="$(mktemp -d)"
LAST_CMD="bash scripts/install-e2e.sh <oneharness-bin> <install-dir>"
if ! out="$(bash scripts/install-e2e.sh "$oh" "$install_dir" 2>&1)"; then
  fail "installer e2e failed" "$LAST_CMD" "$out" \
    "inspect scripts/install.sh and scripts/install-e2e.sh"
fi
if ! installed="$(exe_path "$install_dir/oneharness")"; then
  fail "installer did not create an executable oneharness" "$LAST_CMD" "$out"
fi
installed_ver="$("$installed" --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)"
if [ -n "$crate_ver" ] && [ -n "$installed_ver" ] && [ "$crate_ver" != "$installed_ver" ]; then
  fail "installed binary is v$installed_ver but Cargo.toml is v$crate_ver" \
    "$installed --version" "$out"
fi
rm -rf "$install_dir"

# 0b. npm packaging e2e: assemble the host's per-platform npm package from the
#     binary under test, stage it under the launcher exactly as npm's optional-
#     dependency resolution would, and prove the `oneharness-cli` launcher shim
#     resolves and execs it. Node-gated like the other external tools the gate
#     uses: GitHub runners ship Node so CI covers every platform; a node-less
#     clone skips with a notice rather than failing the gate.
if command -v node >/dev/null 2>&1; then
  LAST_CMD="bash scripts/npm-e2e.sh <oneharness-bin>"
  if ! out="$(bash scripts/npm-e2e.sh "$oh" 2>&1)"; then
    fail "npm packaging e2e failed" "$LAST_CMD" "$out" \
      "inspect scripts/npm-e2e.sh, scripts/npm-build.mjs, and npm/oneharness/"
  fi
else
  echo "smoke: node not found; npm packaging e2e skipped (install Node to run it)" >&2
fi

# 1. `list` — the registry, with each adapter's example command.
LAST_CMD="$oh list --compact"
out="$($oh list --compact)" || fail "list exited non-zero" "$LAST_CMD"
assert_contains "$out" '"schema_version"'
assert_contains "$out" '"claude-code"'
n_list="$(count_matches "$out" '"default_bin"')"
[ "$n_list" -ge 8 ] || fail "list reported $n_list harness(es), expected >= 8" "$LAST_CMD" "$out"

# 2. `detect --all` — probe availability without requiring any to be present.
LAST_CMD="$oh detect --all --compact"
out="$($oh detect --all --compact)" || fail "detect exited non-zero" "$LAST_CMD"
assert_contains "$out" '"detected"'
assert_contains "$out" '"available"'

# 3. `run --all --print-command` — build every adapter's argv, execute nothing.
LAST_CMD="$oh run --all --print-command --prompt <prompt> --compact"
out="$($oh run --all --print-command --prompt "$PROMPT" --compact)" \
  || fail "print-command dry run exited non-zero" "$LAST_CMD"
assert_contains "$out" '"dry_run":true'
assert_contains "$out" '"status":"planned"'
n_run="$(count_matches "$out" '"harness":')"
[ "$n_run" = "$n_list" ] \
  || fail "print-command planned $n_run harness(es) but list has $n_list" "$LAST_CMD" "$out"

# 3b. Unified config: a planted project oneharness.toml supplies the selection
#     and model with no flags. ONEHARNESS_NO_CONFIG is suspended just for this
#     step, and the user-level file is pinned to a planted empty one so the
#     developer's real config never leaks in.
cfg_dir="$(mktemp -d)"
printf 'harnesses = ["claude-code"]\nmodel = "smoke-model"\n' > "$cfg_dir/oneharness.toml"
: > "$cfg_dir/user.toml"
LAST_CMD="ONEHARNESS_CONFIG=$cfg_dir/user.toml $oh run --prompt <prompt> --cwd $cfg_dir --print-command --compact"
out="$(ONEHARNESS_NO_CONFIG='' ONEHARNESS_CONFIG="$cfg_dir/user.toml" \
  "$oh" run --prompt "$PROMPT" --cwd "$cfg_dir" --print-command --compact)" \
  || fail "config-driven dry run exited non-zero" "$LAST_CMD" "" \
       "the oneharness.toml project config layer is broken"
assert_contains "$out" '"--model","smoke-model"' "project config model was not applied"
n_cfg="$(count_matches "$out" '"harness":')"
[ "$n_cfg" = "1" ] \
  || fail "config selection planned $n_cfg harness(es), expected 1 (claude-code)" "$LAST_CMD" "$out"

# 3b-env. The ONEHARNESS_<FIELD> environment overrides layer above the files and
#     below the flags: ONEHARNESS_MODEL must beat the planted project model.
LAST_CMD="ONEHARNESS_CONFIG=$cfg_dir/user.toml ONEHARNESS_MODEL=env-model $oh run --harness claude-code --prompt <prompt> --cwd $cfg_dir --print-command --compact"
out="$(ONEHARNESS_NO_CONFIG='' ONEHARNESS_CONFIG="$cfg_dir/user.toml" ONEHARNESS_MODEL="env-model" \
  "$oh" run --harness claude-code --prompt "$PROMPT" --cwd "$cfg_dir" --print-command --compact)" \
  || fail "env-override dry run exited non-zero" "$LAST_CMD" "" \
       "the ONEHARNESS_* environment override layer is broken"
assert_contains "$out" '"--model","env-model"' "ONEHARNESS_MODEL did not override the project config model"

# 3c. `config` — the layering debug surface: the planted model must be shown
#     with the project file attributed as its source.
LAST_CMD="ONEHARNESS_CONFIG=$cfg_dir/user.toml $oh config --cwd $cfg_dir --compact"
out="$(ONEHARNESS_NO_CONFIG='' ONEHARNESS_CONFIG="$cfg_dir/user.toml" \
  "$oh" config --cwd "$cfg_dir" --compact)" \
  || fail "config command exited non-zero" "$LAST_CMD"
assert_contains "$out" '"value":"smoke-model"' "config value reporting is broken"
assert_contains "$out" 'oneharness.toml' "config source attribution is broken"

# 3d. `sync` — materialize a permission rule into a harness's own config file
#     (the file-based delivery for allow/deny/hooks), and prove idempotency.
printf 'allowed_tools = ["Bash(echo *)"]\n' >> "$cfg_dir/oneharness.toml"
LAST_CMD="ONEHARNESS_CONFIG=$cfg_dir/user.toml $oh sync --harness claude-code --cwd $cfg_dir --compact"
out="$(ONEHARNESS_NO_CONFIG='' ONEHARNESS_CONFIG="$cfg_dir/user.toml" \
  "$oh" sync --harness claude-code --cwd "$cfg_dir" --compact)" \
  || fail "sync exited non-zero" "$LAST_CMD"
assert_contains "$out" '"status":"created"' "sync did not create the harness config file"
grep -qF 'Bash(echo *)' "$cfg_dir/.claude/settings.json" \
  || fail "synced rule missing from .claude/settings.json" "$LAST_CMD" \
       "$(cat "$cfg_dir/.claude/settings.json" 2>/dev/null || echo '<missing>')"
out="$(ONEHARNESS_NO_CONFIG='' ONEHARNESS_CONFIG="$cfg_dir/user.toml" \
  "$oh" sync --harness claude-code --cwd "$cfg_dir" --compact)" \
  || fail "re-sync exited non-zero" "$LAST_CMD"
assert_contains "$out" '"status":"unchanged"' "sync is not idempotent"
rm -rf "$cfg_dir"

# 4. Real spawn + capture + extract, hermetically, via the mock-harness fixture.
#    The mock emits a Claude-shaped result so this also proves the normalized
#    envelope (text, usage, session_id) is lifted out of harness-specific stdout.
mock="$(resolve_mock)"
mock_stdout='{"type":"result","result":"pong","session_id":"smoke-sess","total_cost_usd":0.0012,"usage":{"input_tokens":42,"output_tokens":1}}'
LAST_CMD="ONEHARNESS_BIN_CLAUDE_CODE=$mock MOCK_STDOUT=<claude-json> $oh run --harness claude-code --prompt <prompt> --compact"
out="$(ONEHARNESS_BIN_CLAUDE_CODE="$mock" MOCK_STDOUT="$mock_stdout" \
  "$oh" run --harness claude-code --prompt "$PROMPT" --compact)" \
  || fail "mock run exited non-zero" "$LAST_CMD"
assert_contains "$out" '"status":"ok"' "the mock spawn/parse path is broken"
assert_contains "$out" '"text":"pong"' "json:result extraction is broken"
assert_contains "$out" '"usage_source":"json"' "usage normalization is broken"
assert_contains "$out" '"session_id":"smoke-sess"' "session_id surfacing is broken"

# 4b. The other usage shape: OpenCode's JSONL reports per-step tokens/cost under
#     `part` plus a camelCase `sessionID`, which oneharness sums (source
#     `json:summed-steps`) and surfaces — a different code path than step 4's
#     single-event Claude shape, so the built binary proves both.
oc_stdout='{"type":"step_start","sessionID":"ses_smoke","part":{}}
{"type":"step_finish","sessionID":"ses_smoke","part":{"cost":0.001,"tokens":{"input":40,"output":2}}}
{"type":"step_finish","sessionID":"ses_smoke","part":{"cost":0.002,"tokens":{"input":3,"output":5}}}'
LAST_CMD="ONEHARNESS_BIN_OPENCODE=$mock MOCK_STDOUT=<opencode-jsonl> $oh run --harness opencode --prompt <prompt> --compact"
out="$(ONEHARNESS_BIN_OPENCODE="$mock" MOCK_STDOUT="$oc_stdout" \
  "$oh" run --harness opencode --prompt "$PROMPT" --compact)" \
  || fail "opencode mock run exited non-zero" "$LAST_CMD"
assert_contains "$out" '"usage_source":"json:summed-steps"' "opencode per-step usage summing is broken"
assert_contains "$out" '"input_tokens":43' "opencode token summing is broken"
assert_contains "$out" '"session_id":"ses_smoke"' "camelCase sessionID surfacing is broken"

# 5. Structured output: constrain the final answer to a JSON Schema, validate it,
#    and surface the parsed value. Prompt-based delivery (crush) proves the
#    portable path that works for every harness; the mock returns a conforming
#    object so the validator passes and `structured`/`schema_valid` are emitted.
schema_dir="$(mktemp -d)"
printf '{"type":"object","properties":{"name":{"type":"string"},"age":{"type":"integer"}},"required":["name","age"],"additionalProperties":false}' > "$schema_dir/person.json"
LAST_CMD="ONEHARNESS_BIN_CRUSH=$mock MOCK_STDOUT=<conforming-json> $oh run --harness crush --prompt <prompt> --schema $schema_dir/person.json --compact"
out="$(ONEHARNESS_BIN_CRUSH="$mock" MOCK_STDOUT='{"name":"Ada","age":36}' \
  "$oh" run --harness crush --prompt "$PROMPT" --schema "$schema_dir/person.json" --compact)" \
  || fail "structured-output run exited non-zero" "$LAST_CMD" "$out" \
       "the --schema validate path is broken"
assert_contains "$out" '"schema_valid":true' "schema validation is broken"
# serde_json serializes object keys sorted, so `age` precedes `name`.
assert_contains "$out" '"structured":{"age":36,"name":"Ada"}' "structured value extraction is broken"
assert_contains "$out" '"schema_attempts":1' "schema attempt count is broken"
rm -rf "$schema_dir"

# 6. History: an opt-in `run --history` streams one normalized record to a
#    session file, and the `history` verb lists it back as JSON. Proves the
#    shipped binary's history write + view path end to end, hermetically.
hist_dir="$(mktemp -d)/hist"
LAST_CMD="ONEHARNESS_BIN_CODEX=$mock $oh run --harness codex --prompt <prompt> --history --history-dir $hist_dir --bypass --compact"
out="$(ONEHARNESS_BIN_CODEX="$mock" MOCK_STDOUT='{"type":"turn.started"}
{"type":"item.completed","item":{"id":"m1","type":"agent_message","text":"hi"}}
{"type":"turn.completed"}' \
  "$oh" run --harness codex --prompt "$PROMPT" --history --history-dir "$hist_dir" --bypass --compact)" \
  || fail "history run exited non-zero" "$LAST_CMD" "$out" "the --history write path is broken"
assert_contains "$out" '"history_file":' "history_file was not reported"
LAST_CMD="$oh history list --all-projects --history-dir $hist_dir --compact"
out="$("$oh" history list --all-projects --history-dir "$hist_dir" --compact)" \
  || fail "history list exited non-zero" "$LAST_CMD" "$out"
assert_contains "$out" '"harnesses":["codex"]' "history list did not surface the recorded session"
rm -rf "$hist_dir"

# 7. `init` — scaffold a starter oneharness.toml, prove it parses via `config`,
#    and prove the safe-by-default overwrite refusal (and --force).
init_dir="$(mktemp -d)"
init_path="$init_dir/oneharness.toml"
LAST_CMD="$oh init $init_path"
out="$("$oh" init "$init_path")" || fail "init exited non-zero" "$LAST_CMD" "$out"
assert_contains "$out" "wrote" "init did not confirm the written path"
grep -qF 'run_mode = "fallback"' "$init_path" \
  || fail "scaffolded config missing run_mode" "$LAST_CMD" "$(cat "$init_path" 2>/dev/null)"
# The scaffold must be a config the loader accepts (round-trip through `config`).
LAST_CMD="$oh config --config $init_path --compact"
out="$(ONEHARNESS_NO_CONFIG='' ONEHARNESS_RUN_MODE='' "$oh" config --config "$init_path" --compact)" \
  || fail "scaffolded config does not parse via 'oneharness config'" "$LAST_CMD" "$out"
assert_contains "$out" '"value":"fallback"' "scaffolded run_mode did not load"
# Safe by default: a second init without --force is refused (exit 2), with --force it succeeds.
LAST_CMD="$oh init $init_path"
if "$oh" init "$init_path" >/dev/null 2>&1; then
  fail "init overwrote an existing file without --force" "$LAST_CMD"
fi
"$oh" init "$init_path" --force >/dev/null \
  || fail "init --force did not overwrite" "$oh init $init_path --force"
rm -rf "$init_dir"

if [ "$LIVE" -eq 0 ]; then
  echo "smoke: ok (hermetic — install, list, detect, print-command, config, sync, mock run, schema, history, init)"
  exit 0
fi

# --live: exercise the real adapters against installed, authenticated harnesses.
LAST_CMD="$oh detect --all --compact"
det="$($oh detect --all --compact)" || fail "detect exited non-zero" "$LAST_CMD"
if ! printf '%s' "$det" | grep -qF '"available":true'; then
  echo "smoke: ok (hermetic) — no harnesses installed/authenticated, live step skipped"
  exit 0
fi

LAST_CMD="$oh run --all --prompt <prompt> --timeout 90 --compact"
out="$($oh run --all --prompt "$PROMPT" --timeout 90 --compact)" || true
if ! printf '%s' "$out" | grep -qF '"status":"ok"'; then
  fail "no installed harness returned an ok result" "$LAST_CMD" "$out" \
    "check each harness's auth/network; run 'oneharness detect --all' to see availability"
fi
echo "smoke: ok (hermetic + live — at least one installed harness returned ok)"
