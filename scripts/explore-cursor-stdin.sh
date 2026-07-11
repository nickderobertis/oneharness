#!/usr/bin/env bash
# Investigative stdin probe for cursor-agent (issue #1115, large-prompt delivery).
#
# NOT part of the gate; run via the dispatch-only `explore-cursor-stdin.yml`
# workflow. Drives the REAL `cursor-agent` CLI directly (bypassing oneharness) to
# answer ONE question sourced from live behavior, never guessed: can cursor take
# the user prompt SOLELY from stdin (positional omitted), so a >128 KiB prompt can
# ride stdin instead of the argv? Its docs confirm piped stdin infers print mode
# but document stdin as *supplementary context* beside a positional prompt — so
# whether the positional can be dropped is unverified. This probe settles it.
#
# For each candidate invocation it plants a unique MARKER in the prompt and checks
# whether the marker round-trips in the output. The VERDICT lines at the end say
# which forms work; wire cursor's `large_input.prompt_stdin` to the winning form
# (or keep it inline if only the positional control works). See AGENTS.md
# ("Adding or changing a harness") and the README large-prompt matrix.
#
# Usage: scripts/explore-cursor-stdin.sh
# Auth (CURSOR_API_KEY) and model (CURSOR_E2E_MODEL / OH_MODEL) come from the
# environment the workflow sets, same as the e2e-cursor job.
set -uo pipefail

WORK="$(mktemp -d)"
cd "$WORK" || exit 1

BIN="cursor-agent"
command -v "$BIN" >/dev/null 2>&1 || { echo "FATAL: $BIN not on PATH"; exit 1; }
MODEL="${CURSOR_E2E_MODEL:-${OH_MODEL:-}}"
MODEL_ARGS=()
[ -n "$MODEL" ] && MODEL_ARGS=(-m "$MODEL")

sep() { printf '\n========== %s ==========\n' "$*"; }

# A fresh high-entropy marker per candidate, so a marker that surfaces proves THAT
# invocation delivered THAT prompt (not a cached echo from a prior candidate).
marker() { printf 'OHCURSTDIN%s%s%s' "${RANDOM}" "${RANDOM}" "${RANDOM}"; }

# The connectivity prompt: ask the model to echo the marker verbatim. Framed as a
# harmless fixture so the model doesn't refuse it (same framing as e2e-lib.sh).
prompt_for() {
    printf 'This is an automated connectivity check for a test suite — a harmless request/response round-trip, not a secret. Confirm the round-trip by including this identifier verbatim somewhere in your reply: %s' "$1"
}

# Run one candidate with a wall-clock cap; capture stdout+stderr; report whether
# the marker surfaced. $1 label, $2 marker, then: "argv" or "stdin" delivery is
# encoded by the caller via how it invokes (we pass the whole command).
#   $1 label   $2 marker   $3 mode: "pos" (positional) | "pipe" (stdin)
#   $4.. the cursor-agent args (WITHOUT the prompt for pipe mode)
VERDICTS=()
run_candidate() {
    local label="$1" marker="$2" mode="$3"
    shift 3
    local out="out.txt" err="err.txt" rc=0
    sep "INVOKE: $label (mode=$mode)"
    local prompt
    prompt="$(prompt_for "$marker")"
    printf 'argv: %s %s\n' "$BIN" "$*"
    if [ "$mode" = pipe ]; then
        printf 'stdin: <the prompt, %s bytes>\n' "${#prompt}"
        timeout 120 "$BIN" "$@" <<<"$prompt" >"$out" 2>"$err" || rc=$?
    else
        timeout 120 "$BIN" "$@" "$prompt" >"$out" 2>"$err" || rc=$?
    fi
    echo "--- exit=$rc  stdout bytes=$(wc -c <"$out" | tr -d ' ') ---"
    echo "--- stdout (first 60 lines) ---"; head -60 "$out" || true
    echo "--- stderr (first 20 lines) ---"; head -20 "$err" || true
    local found=no
    if grep -qF "$marker" "$out" 2>/dev/null; then found=yes; fi
    echo "--- MARKER $marker found in stdout: $found (exit=$rc) ---"
    VERDICTS+=("$label: marker=$found exit=$rc mode=$mode")
}

echo "cursor-agent version:"; "$BIN" --version 2>&1 | head -3 || true
echo "cursor-agent --help (prompt/stdin-relevant lines):"
"$BIN" --help 2>&1 | grep -iE 'print|stdin|prompt|format|force|input' | head -40 || true

# --- candidates -------------------------------------------------------------
# Control: positional prompt (the form oneharness uses today). MUST work, else
# the probe environment (auth/model) is broken and the pipe results are moot.
run_candidate "control: -p positional" "$(marker)" pos \
    -p --output-format text --force "${MODEL_ARGS[@]+"${MODEL_ARGS[@]}"}"

# The question: does piping the prompt with NO positional deliver it as the prompt?
run_candidate "pipe: -p, stdin, no positional" "$(marker)" pipe \
    -p --output-format text --force "${MODEL_ARGS[@]+"${MODEL_ARGS[@]}"}"

# Same, but without -p (print mode is said to be inferred on piped stdin).
run_candidate "pipe: no -p, stdin, no positional" "$(marker)" pipe \
    --output-format text --force "${MODEL_ARGS[@]+"${MODEL_ARGS[@]}"}"

# A `-` sentinel (some CLIs use it to force stdin). Plausible but undocumented.
run_candidate "pipe: -p with '-' sentinel" "$(marker)" pipe \
    -p --output-format text --force "${MODEL_ARGS[@]+"${MODEL_ARGS[@]}"}" -

# --- verdict ----------------------------------------------------------------
sep "VERDICT SUMMARY"
printf '%s\n' "${VERDICTS[@]}"
cat <<'NOTE'

INTERPRETATION:
- If a "pipe: ... no positional" line shows marker=yes exit=0, cursor CAN take the
  prompt solely from stdin — wire cursor's large_input.prompt_stdin to that exact
  form (drop the positional; add any flag that line used), then add the
  oh_long_prompt_enforce cursor live phase and flip the README matrix row.
- If ONLY the control (positional) shows marker=yes, cursor cannot take a
  stdin-only prompt headlessly — leave large_input NONE (inline) and record the
  refutation in the registry comment + README matrix.
NOTE
