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
need_env "OpenCode provider key" ANTHROPIC_API_KEY OPENAI_API_KEY

export OH_MODEL="${OPENCODE_E2E_MODEL:-anthropic/claude-haiku-4-5}"
marker="$(oh_marker)"
oh_run opencode "$(oh_prompt "$marker")"
# OpenCode streams JSONL under `--format json`; oneharness must reconstruct the
# answer from its `text` parts, so require that exact extraction method here.
oh_assert_echoed opencode "$marker" "json:opencode-parts"

# Cache-token reporting: with an Anthropic model OpenCode caches its tools+system
# prefix, so a second run reads it back. Assert oneharness surfaces the summed
# `part.tokens.cache.read` as cache_read_tokens — the live drift alarm for the
# per-step cache-token extraction.
note "» cache reporting: a second run must surface cache_read_tokens"
oh_cache_assert opencode

# Batch min-tokens (fork): OpenCode is the other fork-capable, cache-reporting
# harness, so it completes the support matrix's "supported" side. The batch warms
# prompt[0] as a session, then forks it for the fan-out, which must reuse the
# warmed cached prefix (read it, and write less than the warm-up).
note "» batch min-tokens (fork): the fan-out must reuse the warmed session and save writes"
oh_batch_fork_enforce opencode

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
    local bin sandbox out status denyfile allowfile
    bin="$(oh_bin)"
    [ -n "$bin" ] || skip "oneharness binary not found (build it: \`just build-release\`, or set ONEHARNESS_BIN)"

    local rules='Rules: you MUST actually invoke your shell tool with that exact command — never decide on your own that it is not permitted; attempt it. Use only the shell tool, and do NOT create the file by any other means.'

    # Sync a single-hook gate (deny needle $2) into a fresh sandbox and drive the
    # real OpenCode through one `touch $1`, under bypass so the gate is the sole
    # decider. Echoes the sandbox path; the caller checks for the file.
    _oh_oc_gate_run() {
        local file="$1" needle="$2" sb cfg
        sb="$(mktemp -d)"
        sb="$(oh_native_path "$sb")"
        git init -q "$sb" 2>/dev/null || true
        cat > "$sb/oneharness.toml" <<TOML
[[hooks]]
command = "$bin gate opencode --deny-if-contains $needle"
harnesses = ["opencode"]
plugin_name = "ohsess"
TOML
        if ! cfg="$(ONEHARNESS_NO_CONFIG='' "$bin" sync --harness opencode \
            --cwd "$sb" --config "$sb/oneharness.toml" --compact 2>&1)"; then
            printf '%s\n' "$cfg" >&2
            rm -rf "$sb"
            fail "opencode: oneharness sync failed to install the session_id gate"
        fi
        oh_run opencode "You are a non-interactive test fixture in a scratch directory. Execute exactly this shell command, then stop: touch $sb/$file. $rules" --cwd "$sb"
        printf '%s' "$sb"
    }

    note "  session-id[control]: a needle absent from the payload must let the touch run"
    allowfile="ohsess-allow-${RANDOM}${RANDOM}.txt"
    sandbox="$(_oh_oc_gate_run "$allowfile" "OHSESSNEVER${RANDOM}${RANDOM}")"
    status="$(oh_field '.results[0].status')"
    if [ "$status" = "skipped" ]; then
        rm -rf "$sandbox"
        skip "opencode is not installed (oneharness reported status=skipped); nothing to verify"
    fi
    if [ ! -e "$sandbox/$allowfile" ]; then
        oh_dump
        rm -rf "$sandbox"
        fail "opencode: the positive control never ran ($allowfile absent) — cannot trust the session_id deny as a real block (does OpenCode run shell headlessly here?)"
    fi
    note "  ok[control]: the unmatched command ran"
    rm -rf "$sandbox"

    note "  session-id[proof]: denying on the session_id key must block the touch"
    denyfile="ohsess-deny-${RANDOM}${RANDOM}.txt"
    sandbox="$(_oh_oc_gate_run "$denyfile" "session_id")"
    if [ -e "$sandbox/$denyfile" ]; then
        oh_dump
        rm -rf "$sandbox"
        fail "opencode: the gate did NOT fire on the session_id key — the shim's payload is missing session_id, so input.sessionID is not being forwarded (session_id regression)"
    fi
    note "  ok[proof]: the payload carried session_id, so the gate fired"
    rm -rf "$sandbox"
    note "PASS: opencode session_id forwarding"
}

note "» session_id forwarding: the shim must put input.sessionID on its payload"
oh_opencode_session_id_forwarded
