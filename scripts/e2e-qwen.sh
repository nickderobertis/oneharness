#!/usr/bin/env bash
# Live e2e: drive the real Qwen Code CLI through oneharness and assert the JSON
# contract. Qwen speaks an OpenAI-compatible API: set OPENAI_API_KEY (and
# typically OPENAI_BASE_URL + OPENAI_MODEL for the provider you point it at).
# Model: $QWEN_E2E_MODEL (default: the CLI's own default).
set -euo pipefail
# shellcheck source=scripts/e2e-lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/e2e-lib.sh"

note "== oneharness live e2e: qwen =="
need jq
need_env "Qwen (OpenAI-compatible) auth" OPENAI_API_KEY DASHSCOPE_API_KEY

export OH_MODEL="${QWEN_E2E_MODEL:-}"
marker="$(oh_marker)"
oh_run qwen "$(oh_prompt "$marker")"
oh_assert_echoed qwen "$marker"

# Sync enforcement is NOT provable headlessly for qwen — live findings:
# without -y/--yolo on the CLI, qwen never auto-approves tools from settings.
# permissions.allow rules left the tool excluded or the call waiting for an
# approval that headless mode can't grant (one run hung until oneharness's
# timeout killed it), and permissions.defaultMode = "yolo" did not unlock
# execution either. The synced mapping (.qwen/settings.json
# permissions.allow/deny) matches qwen's documented settings and governs
# interactive approval; headless runs are gated by the CLI flag alone.
#
# What IS provable headlessly: the real CLI accepts the synced file — a
# malformed write would break this run, so it is the format drift alarm.
sandbox="$(mktemp -d)"
printf '[harness.qwen]\nallowed_tools = ["run_shell_command(ls)"]\ndenied_tools = ["run_shell_command(rm)"]\n' \
    > "$sandbox/oneharness.toml"
ONEHARNESS_NO_CONFIG='' "$(oh_bin)" sync --harness qwen --cwd "$sandbox" \
    --config "$sandbox/oneharness.toml" --compact >/dev/null \
    || fail "qwen: oneharness sync failed"
marker="$(oh_marker)"
oh_run qwen "$(oh_prompt "$marker")" --cwd "$sandbox"
oh_assert_echoed qwen "$marker"
rm -rf "$sandbox"
note "PASS: qwen accepts the synced settings file (headless enforcement is flag-gated in qwen itself; see comments)"

# Hook enforcement DOES hold headlessly — unlike permission rules. Qwen fires a
# PreToolUse hook in every approval mode, but only a *user*-scoped one (project
# hooks sit behind folder trust), so the gate is synced with `--global` into an
# isolated HOME the run also reads. This is also the live proof of `sync --global`.
note "» hook enforcement (global scope): the synced gate must block a marked command"
oh_hook_enforce qwen global

# Approval-mode enforcement: qwen's `read-only` and `plan` both map to
# `--approval-mode plan` (read-only — no execution), so a write must be blocked
# that `--mode bypass` (`--yolo`) allows. This is also the live proof qwen's
# approval-mode flag is honored headlessly (its allow/deny rules are not — above).
note "» read-only / plan enforcement: each must block a write"
oh_mode_enforce qwen read-only
oh_mode_enforce qwen plan

# Edit enforcement: `--mode edit` (--approval-mode auto-edit) auto-applies file
# edits but gates shell — an edit must apply while a `touch` is blocked.
note "» edit enforcement: edits apply, shell gated under --mode edit"
oh_edit_enforce qwen
