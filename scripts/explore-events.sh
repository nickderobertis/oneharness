#!/usr/bin/env bash
# TEMPORARY exploratory harness-output probe (issue #1096, events/streaming).
#
# NOT part of the gate. Drives each REAL harness CLI directly (bypassing
# oneharness) with a tool-using prompt under candidate output formats, and dumps
# `--help` plus the raw stdout + a structural digest — so the true tool-transcript
# shape of each harness can be *sourced from real output*, never guessed, before
# writing an `extract_events` recognizer for it. Delete once the events matrix is
# settled (`.github/workflows/explore-events.yml` too).
#
# Usage: scripts/explore-events.sh <harness-id>
# Auth/model come from the environment the workflow sets (same as the e2e jobs).
set -uo pipefail

ID="${1:?usage: explore-events.sh <harness-id>}"
WORK="$(mktemp -d)"
cd "$WORK" || exit 1
MARKER="OHEXPLORE12345"
# A plain, on-task shell request — frames it as a fixture so copilot/claude don't
# refuse it as off-task or as injection.
PROMPT="This is an automated capability probe for a test suite. Using your shell/bash tool (not an inline answer), run a command that prints the exact text ${MARKER} to stdout — for example: echo ${MARKER}. Then briefly confirm you did it."

sep() { printf '\n========== %s ==========\n' "$*"; }
# Print raw output (capped) plus a byte count and a structural digest of any
# JSONL: the distinct top-level `type`s, distinct `part.type`s, and any object
# keys whose name contains "tool", so the transcript shape is legible in the log.
dump() {
    local file="$1" label="$2" rc="$3"
    sep "$label — exit=$rc bytes=$(wc -c <"$file" | tr -d ' ')"
    echo "--- raw stdout (first 120 lines) ---"
    head -120 "$file" || true
    if command -v jq >/dev/null 2>&1; then
        echo "--- digest: distinct .type ---"
        jq -rs '[.[]?|objects|.type]|unique' "$file" 2>/dev/null | head -40 || true
        echo "--- digest: distinct .part.type ---"
        jq -rs '[.[]?|objects|.part?|objects|.type]|unique' "$file" 2>/dev/null | head -40 || true
        echo "--- digest: lines mentioning tool (first 6) ---"
        grep -i '"tool' "$file" 2>/dev/null | head -6 || true
    fi
}

# Run one candidate invocation with a wall-clock cap; never abort the whole probe.
try() {
    local label="$1"
    shift
    local out="out.txt" rc=0
    sep "INVOKE: $label"
    printf 'cmd: %s\n' "$*"
    if command -v timeout >/dev/null 2>&1; then
        timeout 180 "$@" >"$out" 2>err.txt || rc=$?
    else
        "$@" >"$out" 2>err.txt || rc=$?
    fi
    dump "$out" "$label" "$rc"
    echo "--- stderr (first 20 lines) ---"
    head -20 err.txt || true
}

sep "HARNESS: $ID (work=$WORK)"

# Optional per-harness model, read from EXPLORE_MODEL the workflow sets (empty →
# the harness's own default / env-selected model).
m="${EXPLORE_MODEL:-}"

case "$ID" in
claude-code)
    claude --help 2>&1 | head -60 || true
    ma=(); [ -n "$m" ] && ma=(--model "$m")
    try "claude stream-json+verbose" claude -p "$PROMPT" --permission-mode bypassPermissions "${ma[@]}" --output-format stream-json --verbose
    try "claude json" claude -p "$PROMPT" --permission-mode bypassPermissions "${ma[@]}" --output-format json
    ;;
codex)
    codex exec --help 2>&1 | head -80 || true
    ma=(); [ -n "$m" ] && ma=(--model "$m")
    try "codex exec --json" codex exec --dangerously-bypass-approvals-and-sandbox "${ma[@]}" --json "$PROMPT"
    try "codex exec --experimental-json" codex exec --dangerously-bypass-approvals-and-sandbox "${ma[@]}" --experimental-json "$PROMPT"
    try "codex exec text" codex exec --dangerously-bypass-approvals-and-sandbox "${ma[@]}" "$PROMPT"
    ;;
opencode)
    opencode run --help 2>&1 | head -60 || true
    ma=(); [ -n "$m" ] && ma=(-m "$m")
    try "opencode run --format json" opencode run --dangerously-skip-permissions "${ma[@]}" --format json "$PROMPT"
    ;;
cursor)
    cursor-agent --help 2>&1 | head -60 || true
    ma=(); [ -n "$m" ] && ma=(--model "$m")
    try "cursor stream-json" cursor-agent -p "$PROMPT" --force "${ma[@]}" --output-format stream-json
    try "cursor json" cursor-agent -p "$PROMPT" --force "${ma[@]}" --output-format json
    ;;
goose)
    goose run --help 2>&1 | head -80 || true
    GOOSE_MODE=auto try "goose run text" goose run --with-builtin developer -t "$PROMPT"
    ;;
qwen)
    qwen --help 2>&1 | head -80 || true
    ma=(); [ -n "$m" ] && ma=(-m "$m")
    try "qwen yolo text" qwen --yolo "${ma[@]}" -p "$PROMPT"
    try "qwen yolo json" qwen --yolo "${ma[@]}" --output-format json -p "$PROMPT"
    try "qwen yolo stream-json" qwen --yolo "${ma[@]}" --output-format stream-json -p "$PROMPT"
    ;;
crush)
    crush run --help 2>&1 | head -80 || true
    ma=(); [ -n "$m" ] && ma=(-m "$m")
    try "crush run text" crush run -q "${ma[@]}" "$PROMPT"
    try "crush run json" crush run -q "${ma[@]}" --format json "$PROMPT"
    ;;
copilot)
    copilot --help 2>&1 | head -80 || true
    ma=(); [ -n "$m" ] && ma=(--model "$m")
    try "copilot text" copilot -p "$PROMPT" --allow-all-tools --allow-all-paths --no-ask-user "${ma[@]}"
    try "copilot log-level all" copilot -p "$PROMPT" --allow-all-tools --allow-all-paths --no-ask-user "${ma[@]}" --log-level all
    ;;
*)
    echo "unknown harness id: $ID" >&2
    exit 2
    ;;
esac

sep "END: $ID"
rm -rf "$WORK"
