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
# (debug, temporary) Dump the real `claude` shim(s) so domain::shim::parse_cmd_shim
# can be matched to their exact layout — the multi-line spawn bypass depends on it.
if command -v cygpath >/dev/null 2>&1 && command -v where >/dev/null 2>&1; then
    note "(debug) resolving claude shim files via where:"
    where claude 2>/dev/null | tr -d '\r' | while IFS= read -r winp; do
        up="$(cygpath -u "$winp" 2>/dev/null || printf '%s' "$winp")"
        note "(debug) === $winp ==="
        if [ -f "$up" ]; then sed 's/^/(debug)   | /' "$up" >&2 2>/dev/null || true; fi
    done || true
fi
sysmarker="$(oh_marker)"
multiline_system="$(printf 'You are a connectivity-check fixture.\nFollow the next instruction exactly.\nReply with this single token verbatim and nothing else: %s' "$sysmarker")"
oh_run claude-code "Follow your system instructions." --system "$multiline_system"
oh_assert_echoed claude-code "$sysmarker"

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
