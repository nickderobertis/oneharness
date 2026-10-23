#!/usr/bin/env bash
# Live e2e: drive the real OpenCode CLI through oneharness and assert the JSON
# contract. Auth: a provider key (ANTHROPIC_API_KEY or OPENAI_API_KEY). Model:
# $OPENCODE_E2E_MODEL (default: anthropic/claude-haiku-4-5) — OpenCode needs a
# fully-qualified provider/model id.
set -euo pipefail
# shellcheck source=scripts/e2e-lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/e2e-lib.sh"

note "== oneharness live e2e: opencode =="
need jq
# Every harness launch below is measured into target/e2e-telemetry/network/opencode.jsonl.
oh_telemetry_setup opencode
need_env "OpenCode provider key" ANTHROPIC_API_KEY OPENAI_API_KEY

export OH_MODEL="${OPENCODE_E2E_MODEL:-anthropic/claude-haiku-4-5}"
marker="$(oh_marker)"
oh_run opencode "$(oh_prompt "$marker")"
# OpenCode streams JSONL under `--format json`; oneharness must reconstruct the
# answer from its `text` parts, so require that exact extraction method here.
oh_assert_echoed opencode "$marker" "json:opencode-parts"

# Large prompt + system (issue #1115): oneharness pipes a >128 KiB prompt to
# `opencode run`'s stdin (positional omitted), with the system prepended — so it
# never trips the argv ceiling. The marker must still round-trip.
note "» long prompt: a >128 KiB prompt+system must round-trip off the argv"
oh_long_prompt_enforce opencode

# Cache-token reporting: with an Anthropic model OpenCode caches its tools+system
# prefix, so a second run reads it back. Assert oneharness surfaces the summed
# `part.tokens.cache.read` as cache_read_tokens — the live drift alarm for the
# per-step cache-token extraction.
note "» cache reporting: a second run must surface cache_read_tokens"
oh_cache_assert opencode

# Normalized tool events: OpenCode's `tool` parts become `tool_call` events, so a
# shell-tool turn must surface at least one via `json:opencode-parts` — the live
# drift alarm for event extraction against the real JSONL shape.
note "» events: a tool-using turn must surface normalized tool_call events"
oh_events_assert opencode "json:opencode-parts"

# Streaming: the same events must arrive incrementally under --stream, then a
# terminal result line — the live proof of the short-circuit path.
note "» stream: events must arrive incrementally, then a terminal result line"
oh_stream_assert opencode

# Batch min-tokens is deliberately NOT fork-accelerated for OpenCode: although
# OpenCode can fork, its `--fork` re-sends the branched conversation cold (measured
# live — the fan-out reads no cache and re-writes the whole prefix, so forking
# would RAISE tokens). `fork_reuses_cache` is false for it, so oneharness leaves
# OpenCode's min-tokens as order-only — no batch caching check here (only Claude
# Code's fork reuses the cache; see e2e-claude.sh's oh_batch_fork_enforce).

# Sync enforcement: OpenCode has no list-shaped rules, so the policy goes via
# the raw settings table into opencode.json's permission map. Without
# --dangerously-skip-permissions the deny pattern must block the command, and
# the explicit allow pattern is the positive control.
ok="$(oh_enforce_file ok)"
blocked="$(oh_enforce_file blocked)"
policy="[harness.opencode.settings.permission.bash]
\"touch $ok\" = \"allow\"
\"touch $blocked\" = \"deny\""
oh_sync_enforce opencode "$policy" "$ok" present allow
oh_sync_enforce opencode "$policy" "$blocked" absent deny
note "PASS: opencode sync enforcement"

note "» hook enforcement: the synced plugin gate must block a marked command"
oh_hook_enforce opencode

# Mock enforcement: `run --mock-rules` installs the plugin shim ephemerally
# (restored afterwards) and the shim must APPLY the mock's `updated_input`
# reply by merging it into the tool's mutable args before execution — the live
# proof of the shim's rewrite path (the hermetic suite pins only the shim text),
# and that the spy log preserves the original (pre-rewrite) event.
note "» mock enforcement: run --mock-rules must rewrite a marked command's input"
oh_mock_enforce opencode

# The OpenCode plugin shim is the only harness path that BUILDS its own JSON
# payload (every other harness pipes its native hook event straight to the gate),
# so it is the only place session_id forwarding can silently regress — and the
# hermetic suite can only prove the rendered shim *text* carries the field, never
# that the live runtime actually sends it. Prove the round-trip through the real
# CLI: install a gate that denies whenever the payload carries the literal
# `session_id` key. OpenCode always sets input.sessionID, so a forwarding shim
# makes the gate fire on the touch (file ABSENT); a regressed shim that dropped
# the field leaves `session_id` out of the payload, the gate no longer matches,
# and the file appears — failing here. The control phase denies on a token that
# never occurs (file PRESENT); the only difference between the two phases is the
# deny needle, which isolates the session_id field as the cause of the block.
oh_opencode_session_id_forwarded() {
  local bin sandbox status denyfile allowfile report attempt
  bin="$(oh_bin)"
  [ -n "$bin" ] || skip "oneharness binary not found (build it: \`just build-release\`, or set ONEHARNESS_BIN)"

  local rules='Rules: you MUST actually invoke your shell tool with that exact command — never decide on your own that it is not permitted; attempt it. Use only the shell tool, and do NOT create the file by any other means.'

  # Sync a single-hook gate (deny needle $1) into a fresh sandbox. Echoes the
  # sandbox path. It runs no turn: `oh_run` must run in this shell, not the
  # caller's `$(...)`, or `oh_field`/`oh_dump` read the previous phase's report
  # (a failed control once dumped the mock-stub turn before it).
  _oh_oc_gate_sandbox() {
    local needle="$1" sb cfg
    sb="$(mktemp -d)"
    sb="$(oh_native_path "$sb")"
    git init -q "$sb" 2>/dev/null || true
    cat >"$sb/oneharness.toml" <<TOML
[[hooks]]
command = "$bin gate opencode --deny-if-contains $needle"
harnesses = ["opencode"]
plugin_name = "ohsess"
TOML
    if ! cfg="$(ONEHARNESS_NO_CONFIG='' "$bin" sync --harness opencode \
      --cwd "$sb" --config "$sb/oneharness.toml" --compact 2>&1)"; then
      printf '%s\n' "$cfg" >&2
      rm -rf "$sb" || note "  could not remove the scratch directory $sb — delete it by hand"
      fail "opencode: oneharness sync failed to install the session_id gate"
    fi
    printf '%s' "$sb"
  }

  # Drive the real OpenCode through one `touch $2` in sandbox $1, under bypass
  # so the gate is the sole decider; the caller checks for the file.
  _oh_oc_gate_run() {
    oh_run opencode "You are a non-interactive test fixture in a scratch directory. Execute exactly this shell command, then stop: touch $1/$2. $rules" --cwd "$1"
  }

  note "  session-id[control]: a needle absent from the payload must let the touch run"
  allowfile="ohsess-allow-${RANDOM}${RANDOM}.txt"
  sandbox="$(_oh_oc_gate_sandbox "OHSESSNEVER${RANDOM}${RANDOM}")"
  report="$sandbox/.oh-report.json"
  # Up to two turns, as `oh_mock_enforce` allows: a model occasionally answers
  # without invoking its shell tool at all (no `touch` tool call in its events),
  # which is agent flakiness, not a verdict. Only that case is retried; a touch
  # the agent attempted that still left no file fails hard.
  # llmlint: ignore-block[tool_output_is_signal] WHICH attempt won is the signal, not chatter, as in `oh_control_mode_enforce`: the retry is a backstop for a model that declined the turn, so a control that passes only on its second attempt every night is one about to fail, and only this line can say so in a CI-only suite.
  for attempt in 1 2; do
    _oh_oc_gate_run "$sandbox" "$allowfile"
    status="$(oh_field '.results[0].status')"
    if [ "$status" = "skipped" ]; then
      rm -rf "$sandbox" || note "  could not remove the scratch directory $sandbox — delete it by hand"
      skip "opencode is not installed (oneharness reported status=skipped); nothing to verify"
    fi
    [ ! -e "$sandbox/$allowfile" ] || break
    if ! printf '%s' "$OH_REPORT" >"$report"; then
      rm -rf "$sandbox" || note "  could not remove the scratch directory $sandbox — delete it by hand"
      fail "opencode: could not write the control turn's report to $report — check that the host temp dir is writable and has space"
    fi
    if _oh_touch_attempted "$report" "$allowfile"; then
      break
    fi
  done
  if [ ! -e "$sandbox/$allowfile" ]; then
    oh_dump
    rm -rf "$sandbox" || note "  could not remove the scratch directory $sandbox — delete it by hand"
    fail "opencode: the positive control never ran ($allowfile absent after $attempt attempt(s)), so the session_id deny cannot be trusted as a real block. Next: read the events dumped above — a bash tool call that errored means the gate blocked a needle the payload never carries (pipe that payload to \`oneharness gate opencode --deny-if-contains <needle>\` by hand); no bash tool call on either attempt means the model declined the turn, so tighten the prompt"
  fi
  note "  ok[control]: the unmatched command ran (attempt $attempt of 2)"
  # llmlint: ignore-end[tool_output_is_signal]
  rm -rf "$sandbox" || note "  could not remove the scratch directory $sandbox — delete it by hand"

  note "  session-id[proof]: denying on the session_id key must block the touch"
  denyfile="ohsess-deny-${RANDOM}${RANDOM}.txt"
  sandbox="$(_oh_oc_gate_sandbox "session_id")"
  _oh_oc_gate_run "$sandbox" "$denyfile"
  if [ -e "$sandbox/$denyfile" ]; then
    oh_dump
    rm -rf "$sandbox" || note "  could not remove the scratch directory $sandbox — delete it by hand"
    fail "opencode: the gate did NOT fire on the session_id key — the shim's payload is missing session_id, so input.sessionID is not being forwarded (session_id regression)"
  fi
  note "  ok[proof]: the payload carried session_id, so the gate fired"
  rm -rf "$sandbox" || note "  could not remove the scratch directory $sandbox — delete it by hand"
  note "PASS: opencode session_id forwarding"
}

note "» session_id forwarding: the shim must put input.sessionID on its payload"
oh_opencode_session_id_forwarded
