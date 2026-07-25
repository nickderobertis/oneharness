#!/usr/bin/env bash
# Live e2e: structured output (`run --schema`). Drives the real Claude Code CLI
# through oneharness with a JSON Schema and asserts a schema-VALID round-trip —
# the live drift alarm for Claude's native `--json-schema` flag and its
# `structured_output` field. The hermetic suite (tests/cli.rs) mocks both, so
# only this catches Claude renaming/removing the flag or moving the field.
#
# claude-code is the right harness here: it is the one with NATIVE schema
# delivery, the part most likely to drift. The portable prompt-based path is
# harness-agnostic (schema appended to the prompt, value recovered + validated by
# oneharness itself) and is already proven hermetically; any per-harness script
# can add a live prompt-based leg by calling `oh_schema_enforce <id>`.
#
# Auth and model mirror e2e-claude.sh: CLAUDE_CODE_OAUTH_TOKEN (mint with
# `claude setup-token`) or ANTHROPIC_API_KEY; model $CLAUDE_E2E_MODEL (default
# haiku). A missing CLI or auth is a SKIP, never a failure.
set -euo pipefail
# shellcheck source=scripts/e2e-lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/e2e-lib.sh"

note "== oneharness live e2e: structured output (--schema) =="
need jq
need_env "Claude auth" CLAUDE_CODE_OAUTH_TOKEN ANTHROPIC_API_KEY
oh_sandbox_prepare claude-code "$PWD"

export OH_MODEL="${CLAUDE_E2E_MODEL:-haiku}"

note "» native delivery: claude-code --json-schema → structured_output"
oh_schema_enforce claude-code
