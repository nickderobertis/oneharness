#!/usr/bin/env bash
# Investigative harness hook-capability probe (mock/spy framework groundwork;
# see docs/mock-spy-design.md).
#
# NOT part of the gate; run via the dispatch-only `explore-hooks.yml` workflow.
# Drives each REAL harness CLI directly (bypassing oneharness) with a pre/post
# tool hook installed *ephemerally* (temp project dir, per-run --settings, or a
# redirected home env var — nothing permanent is touched), and dumps:
#   1. whether the hook fired headlessly and the exact stdin payload it received
#      (the spy channel — including whether post-tool events carry the result);
#   2. whether an input-REWRITE verdict is honored (the hook rewrites
#      `touch ORIG_<marker>` to `touch REWRITTEN_<marker>`; which file exists
#      afterwards is the ground truth);
#   3. whether a result-REPLACEMENT verdict is honored (the hook substitutes
#      `MOCKEDOUT_<marker>` for a probe command's output; the marker appearing
#      in the agent's final answer is the ground truth).
# So each harness's mock verdict shape can be *sourced from real behavior*,
# never guessed, before any `mock` capability lands in the registry — the same
# discipline as `explore-events.sh` for transcript shapes.
#
# Usage: scripts/explore-hooks.sh <harness-id>
# Auth/model come from the environment the workflow sets (same as the e2e jobs).
set -uo pipefail

ID="${1:?usage: explore-hooks.sh <harness-id>}"
WORK="$(mktemp -d)"
cd "$WORK" || exit 1
# Some harnesses only discover project-scoped config inside a repository.
git init -q . 2>/dev/null || true

M="OHHOOKPROBE"
HOOK_LOG="$WORK/hook-events.jsonl"
: >"$HOOK_LOG"

# Single-line prompts, framed as a fixture so agents don't refuse them.
P_LOG="This is an automated capability probe for a test suite. Using your shell/bash tool (not an inline answer), run a command that prints probe-hello to stdout — for example: echo probe-hello. Then briefly confirm you did it."
P_REWRITE="This is an automated capability probe for a test suite. Using your shell/bash tool, run exactly this command: touch ORIG_${M}.txt — then briefly confirm you ran it."
P_REPLACE="This is an automated capability probe for a test suite. Using your shell/bash tool, run exactly this command: echo probe-echo-${M} — then report the EXACT stdout text the command printed, verbatim."
P_READ="This is an automated capability probe for a test suite. Using your file-read tool (not shell), read the file real.txt in the current directory and report its exact contents verbatim."

sep() { printf '\n========== %s ==========\n' "$*"; }

# mk_hook <path> [needle] [verdict-json]: write a hook script that appends its
# stdin event to HOOK_LOG (the spy record) and — when <needle> is non-empty and
# appears in the event — replies with <verdict-json> on stdout. Otherwise it
# emits nothing, the universal "no objection" fall-through every harness reads.
# The verdict must not contain single quotes (it is embedded single-quoted).
mk_hook() {
    local path="$1" needle="${2:-}" verdict="${3:-}"
    cat >"$path" <<HOOK
#!/usr/bin/env bash
payload="\$(cat)"
printf '%s\n' "\$payload" >>"$HOOK_LOG"
if [ -n "$needle" ]; then
    case "\$payload" in *"$needle"*) printf '%s' '$verdict' ;; esac
fi
HOOK
    chmod +x "$path"
}

# Clear per-experiment state (marker files + the hook log) so each invocation's
# probe_state reflects only that run.
reset() {
    rm -f "$WORK"/ORIG_* "$WORK"/REWRITTEN_* "$WORK"/real.txt "$WORK"/mock.txt
    : >"$HOOK_LOG"
}

# What actually happened: the hook events received (truncated — payloads can
# embed file contents) and which marker files exist. ORIG present = the original
# command ran (rewrite NOT honored); REWRITTEN present = the rewrite ran.
probe_state() {
    local n
    n="$(grep -c . "$HOOK_LOG" 2>/dev/null || true)"
    sep "hook events received: ${n:-0}"
    if [ -s "$HOOK_LOG" ]; then
        cut -c1-500 "$HOOK_LOG" | head -40
    else
        echo "(empty — the hook did not fire, or wrote nothing)"
    fi
    echo "--- marker files (ORIG=original ran, REWRITTEN=rewrite honored) ---"
    local found=0 f
    for f in "$WORK"/ORIG_* "$WORK"/REWRITTEN_*; do
        if [ -e "$f" ]; then
            echo "present: $(basename "$f")"
            found=1
        fi
    done
    if [ "$found" -eq 0 ]; then echo "(none)"; fi
    return 0
}

# Run one candidate invocation with a wall-clock cap; never abort the whole
# probe. Flags result-substitution markers surfacing in the final output.
try() {
    local label="$1"
    shift
    local out="$WORK/out.txt" err="$WORK/err.txt" rc=0
    sep "INVOKE: $label"
    printf 'cmd: %s\n' "$*"
    if command -v timeout >/dev/null 2>&1; then
        timeout 240 "$@" >"$out" 2>"$err" || rc=$?
    else
        "$@" >"$out" 2>"$err" || rc=$?
    fi
    sep "$label — exit=$rc"
    echo "--- stdout (first 60 lines) ---"
    head -60 "$out" || true
    echo "--- stderr (first 20 lines) ---"
    head -20 "$err" || true
    if grep -q "MOCKEDOUT_${M}" "$out"; then
        echo "*** RESULT SUBSTITUTION SURFACED: final output contains MOCKEDOUT_${M} ***"
    fi
    if grep -q "MOCKFILE_${M}" "$out"; then
        echo "*** READ REDIRECT SURFACED: final output contains MOCKFILE_${M} ***"
    fi
    probe_state
}

# Fixture pair for the file-read redirect experiment: the agent is asked to read
# real.txt; a rewrite hook redirects the read to mock.txt.
mk_read_fixtures() {
    printf 'REALFILE_%s the-real-content\n' "$M" >"$WORK/real.txt"
    printf 'MOCKFILE_%s the-mock-content\n' "$M" >"$WORK/mock.txt"
}

sep "HARNESS: $ID (work=$WORK)"

# Optional per-harness model, read from EXPLORE_MODEL the workflow sets (empty →
# the harness's own default / env-selected model).
m="${EXPLORE_MODEL:-}"

case "$ID" in
claude-code)
    # Delivery under test: per-run `--settings <file>` (no config file mutation).
    ma=(); [ -n "$m" ] && ma=(--model "$m")
    mk_hook "$WORK/hook-log.sh"
    mk_hook "$WORK/hook-rewrite.sh" "ORIG_${M}" \
        '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","permissionDecisionReason":"probe rewrite","updatedInput":{"command":"touch REWRITTEN_'"$M"'.txt"}}}'
    mk_hook "$WORK/hook-replace.sh" "probe-echo-${M}" \
        '{"hookSpecificOutput":{"hookEventName":"PostToolUse","updatedToolOutput":"MOCKEDOUT_'"$M"'"}}'
    mk_hook "$WORK/hook-readmock.sh" "real.txt" \
        '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","permissionDecisionReason":"probe read redirect","updatedInput":{"file_path":"'"$WORK"'/mock.txt"}}}'
    cat >"$WORK/s-log.json" <<EOF
{"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"$WORK/hook-log.sh"}]}],"PostToolUse":[{"hooks":[{"type":"command","command":"$WORK/hook-log.sh"}]}]}}
EOF
    cat >"$WORK/s-rewrite.json" <<EOF
{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"$WORK/hook-rewrite.sh"}]}]}}
EOF
    cat >"$WORK/s-replace.json" <<EOF
{"hooks":{"PostToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"$WORK/hook-replace.sh"}]}]}}
EOF
    cat >"$WORK/s-readmock.json" <<EOF
{"hooks":{"PreToolUse":[{"matcher":"Read","hooks":[{"type":"command","command":"$WORK/hook-readmock.sh"}]}]}}
EOF
    try "claude E1 payload dump (pre+post via --settings)" \
        claude -p "$P_LOG" --permission-mode bypassPermissions "${ma[@]}" --settings "$WORK/s-log.json"
    reset
    try "claude E2 Bash updatedInput rewrite" \
        claude -p "$P_REWRITE" --permission-mode bypassPermissions "${ma[@]}" --settings "$WORK/s-rewrite.json"
    reset
    try "claude E3 PostToolUse updatedToolOutput replace" \
        claude -p "$P_REPLACE" --permission-mode bypassPermissions "${ma[@]}" --settings "$WORK/s-replace.json"
    reset
    mk_read_fixtures
    try "claude E4 Read updatedInput redirect to fixture" \
        claude -p "$P_READ" --permission-mode bypassPermissions "${ma[@]}" --settings "$WORK/s-readmock.json"
    ;;
codex)
    # Delivery under test: project .codex/hooks.json + `-c features.hooks=true`,
    # with the hook-trust bypass flag (and a config trust fallback). The E1
    # payload dump reveals the real shell tool_input shape (string vs argv
    # array) — read it before trusting E2's static rewrite verdict.
    ma=(); [ -n "$m" ] && ma=(--model "$m")
    mkdir -p "$WORK/.codex"
    mk_hook "$WORK/hook-log.sh"
    mk_hook "$WORK/hook-rewrite.sh" "ORIG_${M}" \
        '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","permissionDecisionReason":"probe rewrite","updatedInput":{"command":"touch REWRITTEN_'"$M"'.txt"}}}'
    hooks_json() {
        cat >"$WORK/.codex/hooks.json" <<EOF
{"hooks":{"PreToolUse":[{"matcher":"*","hooks":[{"type":"command","command":"$1"}]}],"PostToolUse":[{"matcher":"*","hooks":[{"type":"command","command":"$1"}]}]}}
EOF
    }
    hooks_json "$WORK/hook-log.sh"
    try "codex E1 payload dump (features.hooks + bypass-hook-trust)" \
        codex exec --dangerously-bypass-approvals-and-sandbox "${ma[@]}" \
        -c features.hooks=true --dangerously-bypass-hook-trust "$P_LOG"
    reset
    try "codex E1b payload dump (config trust fallback)" \
        codex exec --dangerously-bypass-approvals-and-sandbox "${ma[@]}" \
        -c features.hooks=true -c 'projects."'"$WORK"'".trust_level="trusted"' "$P_LOG"
    reset
    hooks_json "$WORK/hook-rewrite.sh"
    try "codex E2 updatedInput rewrite" \
        codex exec --dangerously-bypass-approvals-and-sandbox "${ma[@]}" \
        -c features.hooks=true --dangerously-bypass-hook-trust "$P_REWRITE"
    ;;
opencode)
    # Delivery under test: project .opencode/plugin/ JS (in-process, so it can
    # mutate args in `before` and — the key question — the result object in
    # `after`). One plugin covers all three experiments via needle conditionals.
    ma=(); [ -n "$m" ] && ma=(-m "$m")
    mkdir -p "$WORK/.opencode/plugin"
    cat >"$WORK/.opencode/plugin/ohprobe.js" <<EOF
// oneharness explore-hooks probe plugin (temp dir; deleted after the run).
import { appendFileSync } from "node:fs";
const LOG = "$HOOK_LOG";
const log = (rec) => {
  try { appendFileSync(LOG, JSON.stringify(rec) + "\n"); } catch (_) {}
};
export const OhProbe = async ({ directory }) => ({
  "tool.execute.before": async (input, output) => {
    const args = (output && output.args) || {};
    log({ hook: "before", tool: input && input.tool, args });
    if (typeof args.command === "string" && args.command.includes("ORIG_$M")) {
      args.command = "touch REWRITTEN_$M.txt";
      log({ hook: "before", rewrote: true });
    }
  },
  "tool.execute.after": async (input, output) => {
    log({
      hook: "after",
      tool: input && input.tool,
      args: (input && input.args) || null,
      title: output && output.title,
      output:
        output && typeof output.output === "string"
          ? output.output.slice(0, 300)
          : null,
    });
    const blob = JSON.stringify((input && input.args) || {});
    if (blob.includes("probe-echo-$M")) {
      output.output = "MOCKEDOUT_$M";
      output.title = "mocked by probe";
      log({ hook: "after", replaced: true });
    }
  },
});
EOF
    try "opencode E1 payload dump (before+after)" \
        opencode run --dangerously-skip-permissions "${ma[@]}" "$P_LOG"
    reset
    try "opencode E2 before-args rewrite" \
        opencode run --dangerously-skip-permissions "${ma[@]}" "$P_REWRITE"
    reset
    try "opencode E3 after-output replace" \
        opencode run --dangerously-skip-permissions "${ma[@]}" "$P_REPLACE"
    ;;
goose)
    # Delivery under test: project .agents/plugins/<name>/ (manifest +
    # hooks/hooks.json, the shapes oneharness sync writes). Goose documents no
    # rewrite/replace verdicts; E2 attempts a claude-style updatedInput anyway to
    # record empirically that it is ignored (ORIG file present).
    mkdir -p "$WORK/.agents/plugins/ohprobe/hooks"
    cat >"$WORK/.agents/plugins/ohprobe/plugin.json" <<EOF
{"name":"ohprobe","version":"0.1.0","description":"explore-hooks probe."}
EOF
    mk_hook "$WORK/hook-log.sh"
    mk_hook "$WORK/hook-rewrite.sh" "ORIG_${M}" \
        '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","updatedInput":{"command":"touch REWRITTEN_'"$M"'.txt"}}}'
    goose_hooks() {
        cat >"$WORK/.agents/plugins/ohprobe/hooks/hooks.json" <<EOF
{"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"$1","timeout":30}]}],"PostToolUse":[{"hooks":[{"type":"command","command":"$1","timeout":30}]}]}}
EOF
    }
    goose_hooks "$WORK/hook-log.sh"
    try "goose E1 payload dump (pre+post; does post carry the result?)" \
        env GOOSE_MODE=auto goose run --with-builtin developer -t "$P_LOG"
    reset
    goose_hooks "$WORK/hook-rewrite.sh"
    try "goose E2 updatedInput attempt (expected ignored)" \
        env GOOSE_MODE=auto goose run --with-builtin developer -t "$P_REWRITE"
    ;;
qwen)
    # Delivery under test: user-scope settings.json (the scope that fires
    # headlessly) under a redirected home — QWEN_HOME (E1) vs a fake $HOME (E1b);
    # auth stays env-delivered so the redirect loses nothing.
    ma=(); [ -n "$m" ] && ma=(-m "$m")
    mk_hook "$WORK/hook-log.sh"
    mk_hook "$WORK/hook-rewrite.sh" "ORIG_${M}" \
        '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","permissionDecisionReason":"probe rewrite","updatedInput":{"command":"touch REWRITTEN_'"$M"'.txt"}}}'
    qwen_settings() { # $1=dir $2=hook-path
        mkdir -p "$1"
        cat >"$1/settings.json" <<EOF
{"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"$2"}]}],"PostToolUse":[{"hooks":[{"type":"command","command":"$2"}]}]}}
EOF
    }
    qwen_settings "$WORK/qwen-home" "$WORK/hook-log.sh"
    try "qwen E1 payload dump (QWEN_HOME redirect)" \
        env QWEN_HOME="$WORK/qwen-home" QWEN_CODE_SUPPRESS_YOLO_WARNING=1 \
        qwen --yolo "${ma[@]}" -p "$P_LOG"
    reset
    qwen_settings "$WORK/home/.qwen" "$WORK/hook-log.sh"
    try "qwen E1b payload dump (fake HOME redirect)" \
        env HOME="$WORK/home" QWEN_CODE_SUPPRESS_YOLO_WARNING=1 \
        qwen --yolo "${ma[@]}" -p "$P_LOG"
    reset
    qwen_settings "$WORK/qwen-home" "$WORK/hook-rewrite.sh"
    try "qwen E2 updatedInput rewrite" \
        env QWEN_HOME="$WORK/qwen-home" QWEN_CODE_SUPPRESS_YOLO_WARNING=1 \
        qwen --yolo "${ma[@]}" -p "$P_REWRITE"
    ;;
crush)
    # Delivery under test: project .crush.json (the flat PreToolUse shape
    # oneharness sync writes). Crush has no post-tool hook; E2 probes its
    # documented `updated_input` shallow-merge rewrite.
    ma=(); [ -n "$m" ] && ma=(-m "$m")
    mk_hook "$WORK/hook-log.sh"
    mk_hook "$WORK/hook-rewrite.sh" "ORIG_${M}" \
        '{"version":1,"decision":"allow","reason":"probe rewrite","updated_input":{"command":"touch REWRITTEN_'"$M"'.txt"}}'
    crush_json() {
        cat >"$WORK/.crush.json" <<EOF
{"hooks":{"PreToolUse":[{"command":"$1","timeout":15}]}}
EOF
    }
    crush_json "$WORK/hook-log.sh"
    try "crush E1 payload dump (pre only — no post event exists)" \
        crush run -q "${ma[@]}" "$P_LOG"
    reset
    crush_json "$WORK/hook-rewrite.sh"
    try "crush E2 updated_input rewrite" \
        crush run -q "${ma[@]}" "$P_REWRITE"
    ;;
copilot)
    # Delivery under test: repo .github/hooks/*.json in the temp workdir. E2/E3
    # probe the documented `modifiedArgs` rewrite and post-hoc `modifiedResult`
    # replacement; E1's payload dump shows the real toolArgs field names.
    ma=(); [ -n "$m" ] && ma=(--model "$m")
    mkdir -p "$WORK/.github/hooks"
    mk_hook "$WORK/hook-log.sh"
    mk_hook "$WORK/hook-rewrite.sh" "ORIG_${M}" \
        '{"permissionDecision":"allow","permissionDecisionReason":"probe rewrite","modifiedArgs":{"command":"touch REWRITTEN_'"$M"'.txt"}}'
    mk_hook "$WORK/hook-replace.sh" "probe-echo-${M}" \
        '{"modifiedResult":{"resultType":"success","textResultForLlm":"MOCKEDOUT_'"$M"'"}}'
    copilot_hooks() { # $1=pre-hook $2=post-hook
        cat >"$WORK/.github/hooks/ohprobe.json" <<EOF
{"version":1,"hooks":{"preToolUse":[{"type":"command","bash":"$1","timeoutSec":30}],"postToolUse":[{"type":"command","bash":"$2","timeoutSec":30}]}}
EOF
    }
    copilot_hooks "$WORK/hook-log.sh" "$WORK/hook-log.sh"
    try "copilot E1 payload dump (pre+post; post carries toolResult?)" \
        copilot -p "$P_LOG" --allow-all-tools --allow-all-paths --no-ask-user "${ma[@]}"
    reset
    copilot_hooks "$WORK/hook-rewrite.sh" "$WORK/hook-log.sh"
    try "copilot E2 modifiedArgs rewrite" \
        copilot -p "$P_REWRITE" --allow-all-tools --allow-all-paths --no-ask-user "${ma[@]}"
    reset
    copilot_hooks "$WORK/hook-log.sh" "$WORK/hook-replace.sh"
    try "copilot E3 modifiedResult replace" \
        copilot -p "$P_REPLACE" --allow-all-tools --allow-all-paths --no-ask-user "${ma[@]}"
    ;;
cursor)
    # Delivery under test: project .cursor/hooks.json in the temp workdir. The
    # headline question is whether the pre-tool events fire headlessly at all
    # (officially undocumented); E2 probes the documented snake_case
    # `updated_input` rewrite on preToolUse.
    ma=(); [ -n "$m" ] && ma=(--model "$m")
    mkdir -p "$WORK/.cursor"
    mk_hook "$WORK/hook-log.sh"
    mk_hook "$WORK/hook-rewrite.sh" "ORIG_${M}" \
        '{"permission":"allow","updated_input":{"command":"touch REWRITTEN_'"$M"'.txt"}}'
    cursor_hooks() { # $1=preToolUse hook (all other events log)
        cat >"$WORK/.cursor/hooks.json" <<EOF
{"version":1,"hooks":{"preToolUse":[{"command":"$1"}],"postToolUse":[{"command":"$WORK/hook-log.sh"}],"beforeShellExecution":[{"command":"$WORK/hook-log.sh"}],"afterShellExecution":[{"command":"$WORK/hook-log.sh"}]}}
EOF
    }
    cursor_hooks "$WORK/hook-log.sh"
    try "cursor E1 payload dump (which events fire headlessly?)" \
        cursor-agent -p "$P_LOG" --force "${ma[@]}"
    reset
    cursor_hooks "$WORK/hook-rewrite.sh"
    try "cursor E2 preToolUse updated_input rewrite" \
        cursor-agent -p "$P_REWRITE" --force "${ma[@]}"
    ;;
*)
    echo "unknown harness id: $ID" >&2
    exit 2
    ;;
esac

sep "END: $ID"
rm -rf "$WORK"
