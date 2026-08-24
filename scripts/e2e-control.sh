#!/usr/bin/env bash
# Live e2e: out-of-band turn control (`run --control` + `oneharness interrupt`).
#
# Drives a REAL harness through a turn that produces one file per step,
# interrupts it from a separate process mid-flight, and proves the file count is
# frozen for 15 seconds afterwards. The filesystem is the assertion on purpose:
# some harnesses report a normal `end_turn` after a real cancellation, so a
# check that trusted the harness's own stop reason would pass for the wrong
# reason.
#
# Then a named handle across two turns, which must either continue ONE
# conversation or refuse to — the half no mock can alarm, since the resume
# request is the real CLI's own protocol.
#
# Then the same turn again with `interrupt --input`, which must do BOTH halves:
# freeze the original work AND carry out the redirected work, on the same
# dispatch. An interrupt that stopped the turn and quietly dropped the message
# passes the first phase and fails this one.
#
# This is a per-FEATURE live suite (like `e2e-schema.sh`), deliberately separate
# from the per-harness `e2e-<id>.sh` scripts: each phase drives a real
# multi-step turn and then waits out a 15-second freeze window, which is far too
# slow to add to a suite that runs on every PR. Run it on demand
# (`just live-control`) and after any change to the control path.
#
# One phase per harness that DECLARES a control mechanism, read from
# `oneharness list` — so a newly declared harness is exercised here without
# editing this script, and a harness whose capability was never proven cannot
# quietly skip.
#
# Auth and model mirror the per-harness scripts. A missing CLI or auth is a
# SKIP, never a failure.
#
# llmlint: ignore-file[tool_output_is_signal] Every live e2e script in this repo
# announces its phases (see e2e-claude.sh, e2e-schema.sh): the transcript is what
# attributes a later failure — or a hang inside a 15s freeze window — to a phase
# in a CI log where nobody can attach a debugger. Silence here would make a
# timeout indistinguishable from a harness that never started.
set -euo pipefail
# shellcheck source=scripts/e2e-lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/e2e-lib.sh"

note "== oneharness live e2e: out-of-band turn control (--control / interrupt [--input]) =="
need jq

# Wall-clock, formatted. Every phase drives a real multi-step turn, waits out a
# 15s freeze window, and retries an inconclusive attempt up to three times, so
# what this suite costs in time is not something a reader can infer from the
# transcript — a phase that started retrying, or a newly proven harness, shows
# up here as minutes that were not there before. `$SECONDS` is the whole run;
# pass a difference for one phase.
elapsed() { printf '%dm%02ds' "$(($1 / 60))" "$(($1 % 60))"; }

case "$(uname -s 2>/dev/null || echo unknown)" in
MINGW* | MSYS* | CYGWIN*)
    note "control sockets are unix-only; nothing to verify on Windows"
    exit 0
    ;;
esac

BIN="$(oh_bin)"
[ -n "$BIN" ] || skip "oneharness binary not found (build it: \`just build-release\`, or set ONEHARNESS_BIN)"

# The capability matrix is the script's input: every harness declaring a
# mechanism must prove it here. `default_bin` rides along so a phase that probes
# the CLI directly (copilot's, below) can only ever probe the binary oneharness
# would spawn.
mapfile -t CONTROLLABLE < <(ONEHARNESS_NO_CONFIG=1 "$BIN" list --compact \
    | jq -r '.harnesses[] | select(.control != null) | [.id, .control, .default_bin] | @tsv')

if [ "${#CONTROLLABLE[@]}" -eq 0 ]; then
    fail "no harness declares a control mechanism, so this suite would prove nothing"
fi
note "harnesses declaring turn control: ${CONTROLLABLE[*]}"

# Auth is per harness here, so a missing credential must retire ONE phase, not
# the suite: `need_env` exits, which would leave every harness after the
# unauthenticated one silently unexercised — exactly the quiet skip the
# capability-driven loop above exists to prevent. This returns instead, and the
# suite reports at the end whether anything actually ran.
#
# Each phase also accepts a `<HARNESS>_E2E_AUTH` sentinel alongside the provider
# keys CI supplies. Several of these CLIs authenticate from their own on-disk
# credentials (`~/.claude/.credentials.json`, `~/.codex/auth.json`) with no key
# in the environment at all, so on a developer host the key check alone would
# retire a phase whose harness is in fact ready to run. The sentinel is how that
# host says so; its value is never read. Copilot needs no sentinel — its phase
# asks copilot itself instead (see below), which is what a sentinel only
# approximates.
have_env() {
    local label="$1"
    shift
    local v
    for v in "$@"; do
        [ -n "${!v:-}" ] && return 0
    done
    note "  SKIP $label: none of $* is set"
    return 1
}

proven=()
skipped=()
failed=()
refused=()
gaps=()

# A phase this suite REPORTS rather than runs, with the concrete reason. A gap
# left in the verdict is a hole a reader can see; a phase quietly dropped is
# indistinguishable from coverage, and this feature has already lost a night to
# that difference.
#
# Empty for every (harness, phase) not named here, so a harness added later
# arrives with no gaps and has to earn its verdict.
#   $1 harness id, $2 phase key
known_gap() {
    case "$1:$2" in
    opencode:control-mode)
        printf '%s' "a controlled turn under \`--mode default\` does not end (status=timeout) — opencode failed this phase in four consecutive CI cycles, and the mode delivery it was added with was reverted rather than fixed a fifth time (see \`control_mode_parity\`'s known-gap cell). Its interrupt and redirection are still proven below"
        ;;
    *) printf '' ;;
    esac
}

for declaration in "${CONTROLLABLE[@]}"; do
    IFS=$'\t' read -r id mechanism default_bin <<<"$declaration"
    case "$id" in
    claude-code)
        have_env "Claude auth" CLAUDE_E2E_AUTH CLAUDE_CODE_OAUTH_TOKEN ANTHROPIC_API_KEY || {
            skipped+=("$id")
            continue
        }
        export OH_MODEL="${CLAUDE_E2E_MODEL:-haiku}"
        ;;
    codex)
        have_env "Codex auth" CODEX_E2E_AUTH OPENAI_API_KEY || {
            skipped+=("$id")
            continue
        }
        export OH_MODEL="${CODEX_E2E_MODEL:-}"
        ;;
    opencode)
        have_env "OpenCode auth" OPENCODE_E2E_AUTH ANTHROPIC_API_KEY || {
            skipped+=("$id")
            continue
        }
        # The same fully-qualified id `e2e-opencode.sh` pins, and pinned for a
        # sharper reason here: this phase ran with NO model at all, because a
        # controlled opencode turn dropped `--model` on the floor. It then ran on
        # whatever the pooled server chose by itself — live, a free model
        # answering `Provider request failed with HTTP 401` — which is what made
        # this the one harness needing all three attempts (22m39s on
        # 2026-08-10) or running out of them. The model now reaches the session
        # the turn opens, so this is an ordinary `OH_MODEL` again.
        export OH_MODEL="${OPENCODE_E2E_MODEL:-anthropic/claude-haiku-4-5}"
        ;;
    goose)
        # Goose reads its provider/model from the environment (no --model flag
        # is mapped for it), exactly as `e2e-goose.sh` does — and `goose acp`
        # resolves them the same way an ordinary `goose run` would, so a missing
        # GOOSE_PROVIDER fails `session/new` rather than the turn.
        #
        # The MODEL is load-bearing here in a way it is not in the other phases:
        # this one needs a turn still in flight when the interrupt lands, so the
        # agent has to spread its work over time. `gpt-4o-mini` does not — it
        # ignores "one file per tool call" and writes all 60 in a single batched
        # command (measured: zero files for 15s, then 60 at once, turn over), and
        # work that is atomic can never be caught mid-flight. `claude-haiku`
        # honors it (~1 file every 2s), so prefer Anthropic where its key is
        # present. Either way an explicit GOOSE_MODEL/GOOSE_E2E_MODEL still wins.
        if [ -n "${ANTHROPIC_API_KEY:-}" ]; then
            export GOOSE_PROVIDER="${GOOSE_PROVIDER:-anthropic}"
            export GOOSE_MODEL="${GOOSE_MODEL:-${GOOSE_E2E_MODEL:-claude-haiku-4-5-20251001}}"
        else
            export GOOSE_PROVIDER="${GOOSE_PROVIDER:-openai}"
            export GOOSE_MODEL="${GOOSE_MODEL:-${GOOSE_E2E_MODEL:-gpt-4o-mini}}"
        fi
        # `GOOSE_E2E_AUTH` is the same sentinel every other phase accepts, and
        # goose needs one for the same reason: several of its providers take no
        # key from the environment at all, so the key check alone retires a phase
        # on a host whose goose is in fact ready to run. A host saying so
        # supplies its own GOOSE_PROVIDER/GOOSE_MODEL, which the defaults above
        # already defer to.
        #
        # NOT every provider is a usable substrate for THIS suite, and the trap
        # is worth naming: goose's own `claude-code` provider authenticates from
        # the local Claude Code CLI (so it needs no key) but does NOT honor
        # `session/cancel` — the ACP session is cancelled while the provider's
        # own subprocess carries on, and the phase below fails on the freeze
        # assertion having proven nothing about the mechanism (measured twice on
        # one host: 5 step files became 16, then 20, in the 15s after a served
        # interrupt). The same `acp-cancel` code path passes on copilot. Pick a
        # provider goose talks to over the wire.
        have_env "Goose provider key" GOOSE_E2E_AUTH OPENAI_API_KEY ANTHROPIC_API_KEY GOOGLE_API_KEY || {
            skipped+=("$id")
            continue
        }
        export OH_MODEL=""
        ;;
    # Crush resolves its provider from the ambient environment, and an
    # ANTHROPIC_API_KEY reaching the pooled server is what picks one it can
    # actually call: without it a host carrying AWS selectors falls through to
    # Bedrock, where a role lacking `bedrock:InvokeModelWithResponseStream`
    # answers `403` and the turn does no work there is anything to freeze.
    crush)
        have_env "Crush auth" CRUSH_E2E_AUTH ANTHROPIC_API_KEY || {
            skipped+=("$id")
            continue
        }
        export OH_MODEL="${CRUSH_E2E_MODEL:-}"
        ;;
    # Copilot is the one phase that does NOT ask the environment. Its login is
    # stored by `copilot login` in the OS credential store, so a host with no
    # GH_TOKEN/GITHUB_TOKEN runs copilot perfectly well — and the token gate
    # this replaces retired the phase on exactly such a host, which is how
    # copilot's control path came to ship with no live alarm. `oh_copilot_login_ready`
    # asks copilot instead (a zero-turn ACP `session/new`, no AI credits), and
    # answers for an environment token too, since copilot reads those as well.
    #
    # A note on what the bypass phases below do and no longer do for copilot.
    # Its control launch used to be a bare `copilot --acp`, so every shell call
    # raised `session/request_permission` and the step files existed only
    # because oneharness answered it — the ACP permission path came free with
    # the freeze assertion. It no longer does: a controlled run now carries the
    # mode's own flags, so under `--mode bypass` copilot is told allow-all up
    # front and asks nothing. That is the point (the controlled run is under the
    # same policy an uncontrolled one is), but it moves the permission path's
    # proof to `oh_control_mode_enforce`, which drives a turn under the gating
    # `--mode default` and requires it to end.
    copilot)
        oh_copilot_login_ready "$default_bin" || {
            note "  SKIP Copilot auth: copilot cannot open a session (run \`copilot login\`, or set COPILOT_GITHUB_TOKEN)"
            skipped+=("$id")
            continue
        }
        export OH_MODEL="${COPILOT_E2E_MODEL:-}"
        ;;
    *)
        export OH_MODEL="${OH_MODEL:-}"
        ;;
    esac
    phase_started=$SECONDS
    # Where a phase says the provider refused, it says so in a FILE: every phase
    # below runs in a subshell, so a variable it set would be gone by the time
    # the verdict is written.
    OH_NOT_RUN_FILE="$(mktemp)"
    export OH_NOT_RUN_FILE
    mode_gap="$(known_gap "$id" control-mode)"
    # Both phases run in a subshell so this harness's verdict cannot end the run:
    # `fail` exits, and aborting here would leave every harness after this one
    # unexercised with nothing saying so — the same silent partial coverage
    # OH_E2E_NO_SKIP exists to prevent, arrived at from the other direction. One
    # CI run should say what all six harnesses did, not just the first to break.
    #
    # `|| exit $?` on each phase, and not for style: a subshell that is itself
    # the left side of an `||` runs with `set -e` SUPPRESSED, so a phase's plain
    # non-zero return would be ignored and the phases after it would run anyway.
    # A `fail` still exits by itself, but a provider refusal returns — and
    # without this every later phase spent another refused call, and the first
    # refusal would have masked a genuine failure after it.
    phase_status=0
    (
        note "» $id: a real turn must actually STOP when interrupted"
        oh_control_enforce "$id" "$mechanism" || exit $?
        note "» $id: an interrupt carrying --input must STOP the turn and DO the new work"
        oh_control_redirect_enforce "$id" || exit $?
        note "» $id: a named handle must CONTINUE one conversation, or refuse to — never start over quietly"
        oh_control_session_enforce "$id" "$mechanism" || exit $?
        if [ -n "$mode_gap" ]; then
            note "» $id: KNOWN GAP, not run — a controlled turn under the mode's OWN policy"
            note "    $mode_gap"
        else
            note "» $id: a controlled turn must run under the mode's OWN policy, not one invented for the wire"
            oh_control_mode_enforce "$id"
        fi
    ) || phase_status=$?
    refusal="$(cat "$OH_NOT_RUN_FILE" 2>/dev/null || true)"
    rm -f "$OH_NOT_RUN_FILE"
    unset OH_NOT_RUN_FILE
    if [ "$phase_status" -eq 0 ] && [ -n "$mode_gap" ]; then
        # Everything this harness CAN prove, it proved — and the phase it cannot
        # is named in the verdict rather than left out of it.
        gaps+=("$id (control-mode: a controlled turn under --mode default does not end)")
        note "  $id: known gap reported after $(elapsed $((SECONDS - phase_started)))"
    elif [ "$phase_status" -eq 0 ]; then
        proven+=("$id")
        note "  $id proven in $(elapsed $((SECONDS - phase_started)))"
    elif [ -n "$refusal" ]; then
        # Not a verdict on this feature at all: the harness's own provider
        # answered the request and declined it, so no turn ever ran.
        refused+=("$refusal")
        note "  NOT RUN: $id after $(elapsed $((SECONDS - phase_started))) — $refusal"
    else
        failed+=("$id")
        note "  FAILED: $id after $(elapsed $((SECONDS - phase_started))) — see its evidence above; continuing so this run reports every harness"
    fi
done

oh_control_report_outcome "${proven[*]-}" "${skipped[*]-}" "$(elapsed $SECONDS)" \
    "${failed[*]-}" "${refused[*]-}" "${gaps[*]-}"
