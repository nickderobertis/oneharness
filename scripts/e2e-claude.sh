#!/usr/bin/env bash
# Live e2e: drive the real Claude Code CLI through oneharness and assert the
# JSON contract. Auth: CLAUDE_CODE_OAUTH_TOKEN (mint with `claude setup-token`)
# or ANTHROPIC_API_KEY. Model: $CLAUDE_E2E_MODEL (default: haiku).
set -euo pipefail
# shellcheck source=scripts/e2e-lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/e2e-lib.sh"

note "== oneharness live e2e: claude-code =="
need jq
need_env "Claude auth" CLAUDE_CODE_OAUTH_TOKEN ANTHROPIC_API_KEY

export OH_MODEL="${CLAUDE_E2E_MODEL:-haiku}"
marker="$(oh_marker)"
oh_run claude-code "$(oh_prompt "$marker")"
oh_assert_echoed claude-code "$marker"

# Multi-line argument: a rendered `--system` spans several lines. On Windows the
# harness is a `claude.cmd` npm shim, and std refuses to spawn a `.cmd` with a
# newline-bearing argument ("batch file arguments are invalid") unless oneharness
# bypasses the shim — so a one-line `--prompt` masks the bug. Drive the marker
# through a genuinely multi-line `--system` (the echo instruction lives there)
# and assert it still round-trips, proving the multi-line spawn path on every OS.
note "» multi-line --system must spawn and round-trip (Windows .cmd-shim regression)"
sysmarker="$(oh_marker)"
multiline_system="$(printf 'You are a connectivity-check fixture.\nFollow the next instruction exactly.\nReply with this single token verbatim and nothing else: %s' "$sysmarker")"
oh_run claude-code "Follow your system instructions." --system "$multiline_system"
oh_assert_echoed claude-code "$sysmarker"

# Cache-token reporting: Claude Code auto-caches its tools+system prefix, so a
# second run within the TTL reads it back. Assert oneharness surfaces the
# provider's cache_read_tokens (read from `cache_read_input_tokens`) — the live
# drift alarm for cache-token extraction.
note "» cache reporting: a second run must surface cache_read_tokens"
oh_cache_assert claude-code

# Normalized tool events: Claude Code's default single-document `json` result
# carries no transcript, so events require the streaming format. Under
# `--output-format stream-json` (oneharness adds the required `--verbose`) it
# emits the Anthropic content-block transcript, which normalizes to `tool_call` /
# `tool_result` events — the live drift alarm for that extraction path.
note "» events: a tool-using turn under stream-json must surface normalized events"
oh_events_assert claude-code "stream-json:content-blocks" --output-format stream-json

# Same-prefix batch mode (fork-based min-tokens): the batch warms prompt[0] as a
# session carrying the large shared --system, then FORKS it for the fan-out, so
# each fanned-out call reuses the warmed cached prefix and writes less than the
# warm-up. This is the end-to-end proof that min-tokens actually reduces tokens
# on a fork-capable harness (a static --system can't be reused across separate
# `claude -p` processes; session reuse is the realizable saving).
note "» batch min-tokens (fork): the fan-out must reuse the warmed session and save writes"
oh_batch_fork_enforce claude-code

# Sync enforcement: a policy synced into .claude/settings.json must govern the
# real CLI under --no-bypass — the allow rule lets the exact command run, the
# deny rule (and headless default-deny) keeps the other from running.
ok="$(oh_enforce_file ok)"
blocked="$(oh_enforce_file blocked)"
policy="[harness.claude-code]
allowed_tools = [\"Bash(touch $ok)\"]
denied_tools = [\"Bash(touch $blocked)\"]"
oh_sync_enforce claude-code "$policy" "$ok" present allow
oh_sync_enforce claude-code "$policy" "$blocked" absent deny
note "PASS: claude-code sync enforcement"

# Hook enforcement: a synced `[[hooks]]` gate (oneharness gate claude-code) must
# block a marked command and let an unmarked one through — the live proof the
# installed hook file is honored by the real CLI.
note "» hook enforcement: the synced gate must block a marked command"
oh_hook_enforce claude-code

# Approval-mode enforcement: the no-mutation modes (`read-only` =
# bypassPermissions with the mutating tools denied; `plan` = --permission-mode
# plan) must each block a write that `--mode bypass` allows.
note "» read-only / plan enforcement: each must block a write"
oh_mode_enforce claude-code read-only
oh_mode_enforce claude-code plan
