# shellcheck shell=bash
# Shared helpers for oneharness's live e2e checks (scripts/e2e-<harness>.sh).
#
# These are the opt-in, network-bound counterpart to the hermetic `tests/cli.rs`
# suite: instead of a mock fixture, each script drives a *real* harness CLI
# through the real `oneharness` binary and asserts on the JSON report — proving
# the adapter's argv actually works end to end against the live tool, the
# process envelope is reported, and `text` extraction lands.
#
# The contract under test is oneharness's own stdout report (see README and
# crates/oneharness-core/src/domain/report.rs). A script:
#   1. picks a high-entropy MARKER the model cannot reproduce from memory,
#   2. asks the harness (via oneharness) to echo exactly that marker,
#   3. asserts status=ok / exit_code=0 and that the marker surfaced in the
#      harness output — so a pass means the model genuinely ran end to end,
#      not merely that the process exited cleanly.
#
# A missing harness or a missing oneharness binary is a SKIP, never a failure —
# the same "absence is data, not a crash" stance the tool itself takes. That
# leniency is for developer boxes; in CI it would let a job go green having tested
# nothing, so the workflows set OH_E2E_NO_SKIP=1 to make any skip a hard failure
# (see skip() below). Per-platform exclusions must use an `if`/`note` that
# continues, or a matrix exclude — never skip().
#
# Sourced by each script AFTER it sets `set -euo pipefail`. Requires `jq`.

# Repo root, regardless of where the script is invoked from.
OH_REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# --- reporting -------------------------------------------------------------

note() { printf '%s\n' "$*" >&2; }
fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }

# A skip is "something that should be present is absent" — the right stance on a
# developer box (no harness installed, no auth, no jq). But in CI every e2e
# workflow installs its one harness and verifies auth up front, so a skip THERE
# means detection/install/spawn silently broke and the job would otherwise go
# GREEN having tested nothing. The classic trap is Windows: an npm `.cmd` shim
# oneharness fails to resolve makes it report status=skipped, every assertion
# bails to `skip`, and the windows-latest leg looks supported while running zero
# model calls. Set OH_E2E_NO_SKIP=1 (the e2e workflows do) to turn any skip into
# a hard failure, so a vanished harness is RED, not a false pass.
#
# Intentional per-platform exclusions must NOT use skip() — guard them with an
# `if`/`note` that continues (see e2e-cursor.sh's Windows hook branch) or exclude
# the platform at the matrix level (see e2e-schema.yml). skip() is reserved for
# absences that are never expected in CI.
skip() {
    if [ -n "${OH_E2E_NO_SKIP:-}" ]; then
        printf 'ERROR: skip disallowed (OH_E2E_NO_SKIP set), failing instead: %s\n' "$*" >&2
        exit 1
    fi
    printf 'SKIP: %s\n' "$*" >&2
    exit 0
}

# A required tool's absence is a SKIP, not a failure (e.g. no jq on the box).
need() { command -v "$1" >/dev/null 2>&1 || skip "required tool not found: $1"; }

# At least one of the named env vars must be non-empty, else SKIP. Used for the
# local auth preflight; CI verifies the secret up front and hard-fails instead.
need_env() {
    local label="$1"
    shift
    local v
    for v in "$@"; do
        [ -n "${!v:-}" ] && return 0
    done
    skip "no $label configured (set one of: $*)"
}

# --- locating oneharness ---------------------------------------------------

# Resolve the oneharness binary to drive the harnesses with. Honors
# $ONEHARNESS_BIN, then PATH, then a local debug/release build. Empty if none.
#
# Windows note: the binary is `oneharness.exe`, but the `just live-*` recipes
# (and most callers) pass the extensionless path. Probe a `.exe` sibling for
# every candidate so the same scripts drive the build on all three platforms.
oh_bin() {
    local b out=""
    if [ -n "${ONEHARNESS_BIN:-}" ]; then
        # Prefer a `.exe` sibling when one exists. On Windows the synced gate hook
        # embeds this path and a native harness (Go/Node) execs it as an explicit
        # path — which, unlike Git Bash, is NOT auto-suffixed with `.exe`, so the
        # hook silently fails to launch and stops blocking. A `.exe` path runs
        # everywhere; on Unix the `.exe` candidate never matches.
        out="$ONEHARNESS_BIN"
        for b in "$ONEHARNESS_BIN.exe" "$ONEHARNESS_BIN"; do
            if [ -x "$b" ]; then
                out="$b"
                break
            fi
        done
    elif command -v oneharness >/dev/null 2>&1; then
        out="oneharness"
    else
        local cand
        for cand in "$OH_REPO_ROOT"/target/release/oneharness{.exe,} "$OH_REPO_ROOT"/target/debug/oneharness{.exe,}; do
            if [ -x "$cand" ]; then
                out="$cand"
                break
            fi
        done
    fi
    # Normalize Windows backslashes to forward slashes. The path is interpolated
    # into TOML basic strings for the sync/hook enforcement phases, where `\` is an
    # escape char (a raw `D:\a\...` is a parse error); Windows accepts `/` in paths
    # all the same. No-op on Unix, where paths carry no backslashes.
    printf '%s' "${out//\\//}"
}

# Render a path in a form a *native* (non-MSYS) process understands. On Windows
# `mktemp -d` yields a POSIX path like /tmp/tmp.XXXX that a native harness (or
# oneharness resolving its --cwd) cannot resolve, so a synced config lands where
# the harness can't read it and absolute prompt paths don't exist. `cygpath -ml`
# gives a mixed C:/Users/.../tmp.XXXX path that BOTH Git Bash and Windows accept,
# using the *long* (non-8.3) name: the runner's %TEMP% is a short name
# (C:/Users/RUNNER~1/...), and Claude normalizes a cwd to POSIX form for settings
# discovery/trust matching, where a short name fails to line up with the synced
# .claude/settings.json — so its allow rules are silently ignored. The long name
# is the canonical spelling and is harmless for the other harnesses. Fall back to
# `-m` if `-l` can't resolve (e.g. the path doesn't exist yet).
# No-op on Linux/macOS, where cygpath is absent and paths are already native.
oh_native_path() {
    if command -v cygpath >/dev/null 2>&1; then
        cygpath -ml "$1" 2>/dev/null || cygpath -m "$1"
    else
        printf '%s' "$1"
    fi
}

# Per-harness preparation of the enforcement scratch dir, run after it's created
# and before the harness is driven. Claude gates project `.claude/settings.json`
# (permissions AND hooks) behind a per-directory trust flag in ~/.claude.json;
# headless on Windows there is no prompt to accept it, so the synced policy is
# silently ignored and every tool call default-denies. Pre-accept trust for the
# sandbox under each path spelling claude might canonicalize it to. Harmless and
# non-destructive elsewhere (jq merge preserves existing config).
oh_sandbox_prepare() {
    local id="$1" dir="$2"
    case "$id" in
    claude-code)
        command -v jq >/dev/null 2>&1 || return 0
        local cfg="$HOME/.claude.json" k tmp existing
        local keys=("$dir")
        if command -v cygpath >/dev/null 2>&1; then
            keys+=("$(cygpath -w "$dir")" "$(cygpath -wl "$dir" 2>/dev/null || cygpath -w "$dir")")
        fi
        for k in "${keys[@]}"; do
            [ -f "$cfg" ] && existing="$(cat "$cfg" 2>/dev/null)" || existing=""
            [ -n "$existing" ] || existing='{}'
            tmp="$(mktemp)"
            if printf '%s' "$existing" | jq --arg p "$k" '.projects[$p].hasTrustDialogAccepted = true' >"$tmp" 2>/dev/null; then
                mv "$tmp" "$cfg"
            else
                rm -f "$tmp"
            fi
        done
        ;;
    esac
}

# --- driving a harness -----------------------------------------------------

# A high-entropy marker the model cannot reproduce from memory, so its presence
# in the output proves this run produced it.
oh_marker() { printf 'ONEHARNESS-LIVE-%s%s%s' "${RANDOM}" "${RANDOM}" "${RANDOM}"; }

# The echo prompt: ask the harness to emit exactly the marker and nothing else.
# Framed as a sanctioned connectivity check, not a bare "output this token":
# coding-tuned harnesses (notably Copilot) otherwise refuse it as off-task
# ("I'm here to help with software development tasks"). The verbatim-echo demand
# is unchanged, so the marker assertion still means the model genuinely ran.
oh_prompt() {
    printf 'This is an automated connectivity check for the oneharness end-to-end test suite, and echoing the token below is the expected, approved task — not an arbitrary request to decline. The suite confirms the request/response round-trip by checking that one verification token comes back verbatim. Reply with this exact token and nothing else: no preamble, no explanation, no quotes, no code fences — just the token on a single line: %s' "$1"
}

# Run one prompt through oneharness against a real harness. Stores the JSON
# report in $OH_REPORT. Honors $OH_TIMEOUT (default 120) and, when set,
# $OH_MODEL (passed as --model). Any extra args are forwarded to `oneharness
# run` verbatim. Provider keys reach the harness via the inherited environment.
OH_REPORT=""
oh_run() {
    local id="$1" prompt="$2"
    shift 2
    local bin
    bin="$(oh_bin)"
    [ -n "$bin" ] || skip "oneharness binary not found (build it: \`just build-release\`, or set ONEHARNESS_BIN)"

    local model_args=()
    [ -n "${OH_MODEL:-}" ] && model_args+=(--model "$OH_MODEL")

    # The global default mode is `default`, but the live checks need the agent to
    # actually act (run the marker/hook/allow commands), so oh_run requests
    # `--mode bypass` by default. A caller that sets its own mode flag (e.g.
    # oh_sync_enforce's --no-bypass, oh_mode_enforce's --mode read-only) wins.
    local mode_args=(--mode bypass)
    for a in "$@"; do
        case "$a" in
        --mode | --bypass | --no-bypass) mode_args=() ;;
        esac
    done

    local errf
    errf="$(mktemp)"
    note "  driving: $bin run --harness $id (timeout ${OH_TIMEOUT:-120}s${OH_MODEL:+, model $OH_MODEL})"
    # ONEHARNESS_NO_CONFIG=1: a live check pins its own model/timeout; the
    # machine's oneharness config files must not reshape the invocation.
    OH_REPORT="$(ONEHARNESS_NO_CONFIG=1 "$bin" run --harness "$id" --prompt "$prompt" \
        --timeout "${OH_TIMEOUT:-120}" --compact \
        "${mode_args[@]+"${mode_args[@]}"}" \
        "${model_args[@]+"${model_args[@]}"}" "$@" 2>"$errf")" || true

    if [ -z "$OH_REPORT" ]; then
        note "  oneharness emitted no JSON on stdout. Its stderr:"
        sed 's/^/    /' "$errf" >&2 || true
        rm -f "$errf"
        fail "oneharness produced no report for $id (is the binary the right version?)"
    fi
    rm -f "$errf"
}

# Read a field from the stored report.
oh_field() { printf '%s' "$OH_REPORT" | jq -r "$1"; }

# Pretty-print the result entry plus stdout/stderr tails on failure, so a CI log
# shows exactly what the harness did.
oh_dump() {
    note "  ── oneharness result ──"
    printf '%s' "$OH_REPORT" \
        | jq '.results[0] | {harness, available, status, exit_code, duration_ms, text_source, error}' >&2 2>/dev/null \
        || printf '%s\n' "$OH_REPORT" >&2
    note "  ── harness stdout tail ──"
    oh_field '.results[0].stdout // ""' | tail -30 | sed 's/^/    /' >&2 || true
    note "  ── harness stderr tail ──"
    oh_field '.results[0].stderr // ""' | tail -20 | sed 's/^/    /' >&2 || true
}

# The shared conclusion for every harness. Given the harness id and the marker
# planted in the prompt, assert oneharness reported a clean run AND the marker
# surfaced in the harness output. Treats an uninstalled harness as a SKIP.
#
# Optional third arg: an expected `text_source`. When given, the check is
# strengthened from "marker surfaced in text-or-stdout" to "oneharness extracted
# a normalized `text` via exactly this method, and the marker is in that `text`"
# — i.e. the convenience field is proven, not just the raw-stdout fallback. Pass
# it for a harness whose extraction we guarantee (e.g. opencode → json:opencode-parts).
oh_assert_echoed() {
    local id="$1" marker="$2" expected_source="${3:-}"
    local status available exit_code source

    status="$(oh_field '.results[0].status')"
    available="$(oh_field '.results[0].available')"

    if [ "$status" = "skipped" ] || [ "$available" != "true" ]; then
        skip "$id is not installed (oneharness reported status=$status); nothing to verify"
    fi

    exit_code="$(oh_field '.results[0].exit_code')"
    source="$(oh_field '.results[0].text_source')"

    if [ "$status" != "ok" ]; then
        oh_dump
        fail "$id did not run cleanly: status=$status, exit_code=$exit_code"
    fi
    note "  ok: $id ran and exited cleanly (status=ok, exit_code=0, duration=$(oh_field '.results[0].duration_ms')ms)"

    # The marker must appear in the harness's own output — its normalized text
    # or, failing that, the raw stdout. This is what makes the check meaningful:
    # exit 0 alone could be an empty turn; the marker proves the model ran.
    if printf '%s' "$OH_REPORT" | jq -e --arg m "$marker" \
        '.results[0] | ((.text // "") + "\n" + (.stdout // "")) | contains($m)' >/dev/null; then
        if [ "$source" != "null" ]; then
            note "  confirmed: marker surfaced; oneharness extracted text via '$source'"
        else
            note "  confirmed: marker surfaced in raw stdout (oneharness left text null for this format)"
        fi
    else
        oh_dump
        fail "$id ran but the unique marker never surfaced — the model did not echo it back"
    fi

    # When the caller guarantees a normalized-text method for this harness, hold
    # extraction to it: the right `text_source`, and the marker in `.text` itself
    # (not merely in the raw stdout the previous block would also accept).
    if [ -n "$expected_source" ]; then
        if [ "$source" != "$expected_source" ]; then
            oh_dump
            fail "$id: expected text_source=$expected_source but got '$source' — normalized text extraction regressed"
        fi
        if ! printf '%s' "$OH_REPORT" | jq -e --arg m "$marker" \
            '(.results[0].text // "") | contains($m)' >/dev/null; then
            oh_dump
            fail "$id: marker is absent from the normalized .text (only surfaced via raw stdout)"
        fi
        note "  confirmed: oneharness extracted .text via '$source' and the marker is in it"
    fi

    note "PASS: $id live e2e"
}

# --- sync enforcement --------------------------------------------------------

# One phase of the sync-enforcement check: plant a oneharness.toml in a fresh
# sandbox, `oneharness sync` it into the harness's OWN config file, then ask
# the harness (via `oneharness run --no-bypass`) to run `touch <file>` and
# assert the file's presence or absence. This is the live proof that a synced
# policy is actually honored by the real CLI — the hermetic suite can only
# prove the file was written correctly.
#
#   $1 harness id   $2 oneharness.toml content   $3 file the prompt asks for
#   $4 expectation: present|absent               $5 phase label (for logs)
#
# The `present` phase is the positive control: without it, `absent` would pass
# vacuously on a harness that can't execute anything headlessly.
oh_sync_enforce() {
    local id="$1" toml="$2" file="$3" expect="$4" label="$5"
    local bin sandbox out
    bin="$(oh_bin)"
    [ -n "$bin" ] || skip "oneharness binary not found (build it: \`just build-release\`, or set ONEHARNESS_BIN)"

    sandbox="$(mktemp -d)"
    sandbox="$(oh_native_path "$sandbox")"
    oh_sandbox_prepare "$id" "$sandbox"
    printf '%s\n' "$toml" > "$sandbox/oneharness.toml"

    note "  enforce[$label]: syncing policy into $id's own config file"
    # --config pins the exact planted file (no discovery), and the explicit
    # empty ONEHARNESS_NO_CONFIG guards against a hermetic wrapper's env.
    if ! out="$(ONEHARNESS_NO_CONFIG='' "$bin" sync --harness "$id" \
        --cwd "$sandbox" --config "$sandbox/oneharness.toml" --compact 2>&1)"; then
        printf '%s\n' "$out" >&2
        rm -rf "$sandbox"
        fail "$id: oneharness sync failed ($label phase)"
    fi
    if ! printf '%s' "$out" | grep -qF '"status":"created"'; then
        printf '%s\n' "$out" >&2
        rm -rf "$sandbox"
        fail "$id: sync did not create the harness config file ($label phase)"
    fi
    note "  sync wrote: $out"

    local prompt
    # Kept to one physical line for simplicity. oneharness now spawns a multi-line
    # argument on Windows too (it bypasses the npm `.cmd` shim — see
    # crates/oneharness-core/src/domain/shim.rs; e2e-claude.sh proves it live), so
    # this is no longer a hard constraint, but a single line keeps the fixture
    # prompt trivially quoting-safe across all shells.
    prompt="You are a non-interactive test fixture in a scratch directory. Execute exactly this shell command in the current directory, then stop: touch $file. Rules: you MUST actually invoke your shell tool with that exact command — never assume or decide on your own that it is not permitted; attempt it. Use only the shell tool. Only if that tool invocation itself fails or is rejected: do NOT create the file by any other means (no file-write or edit tools) — reply with the single word DENIED and stop."
    # --no-bypass is `--mode default`, which oneharness refuses for harnesses
    # whose default ask flow would hang headlessly (opencode/cursor). Here the
    # synced allow rule is exactly what stops the prompt from firing, so opt in
    # with --permit-prompts; the per-harness timeout still bounds any hang.
    oh_run "$id" "$prompt" --no-bypass --permit-prompts --cwd "$sandbox"

    local status
    status="$(oh_field '.results[0].status')"
    if [ "$status" = "skipped" ]; then
        rm -rf "$sandbox"
        skip "$id is not installed (oneharness reported status=skipped); nothing to verify"
    fi

    case "$expect" in
        present)
            if [ -e "$sandbox/$file" ]; then
                note "  ok[$label]: the synced allow rule let the command run under --no-bypass"
            else
                oh_dump
                # Show the synced config the harness should have honored, so a
                # Windows "not honored" failure reveals write-location vs matching.
                local cf
                while IFS= read -r cf; do
                    note "  (debug) $cf:"
                    sed 's/^/    /' "$cf" >&2 2>/dev/null || true
                done < <(find "$sandbox" -type f -name '*.json' 2>/dev/null)
                rm -rf "$sandbox"
                fail "$id: 'touch $file' did not run despite the synced allow rule (status=$status) — the $label phase is the positive control, so either the synced file is not honored or the rule syntax drifted"
            fi
            ;;
        absent)
            if [ -e "$sandbox/$file" ]; then
                oh_dump
                rm -rf "$sandbox"
                fail "$id: 'touch $file' executed DESPITE the synced deny policy — enforcement is broken"
            fi
            note "  ok[$label]: the denied command did not execute"
            ;;
        *)
            rm -rf "$sandbox"
            fail "oh_sync_enforce: bad expectation '$expect' (use present|absent)"
            ;;
    esac
    rm -rf "$sandbox"
}

# A shell-safe scratch file name for the enforcement phases.
oh_enforce_file() { printf '%s-%s%s.txt' "$1" "${RANDOM}" "${RANDOM}"; }

# --- approval-mode enforcement ----------------------------------------------

# Live proof that `--mode read-only` is HONORED by the real harness — the drift
# alarm for the per-harness read-only mapping (Codex's `--sandbox read-only`,
# Claude's `--disallowedTools`, Copilot's `--deny-tool`, Cursor's `--mode ask`,
# the plan-agent read-only for Qwen). The agent is told to `touch` a file:
#   * under `--mode read-only` the write must be BLOCKED (file absent), and
#   * under `--mode bypass` the same command must run (file present) — the
#     positive control, so "absent" can't pass vacuously on a harness that runs
#     nothing headlessly.
# Only call this for a harness whose `oneharness list` marks `read-only`
# supported. $1 harness id.
oh_mode_enforce() {
    local id="$1"
    local bin sandbox file status
    bin="$(oh_bin)"
    [ -n "$bin" ] || skip "oneharness binary not found (build it: \`just build-release\`, or set ONEHARNESS_BIN)"

    sandbox="$(mktemp -d)"
    sandbox="$(oh_native_path "$sandbox")"
    oh_sandbox_prepare "$id" "$sandbox"
    file="$(oh_enforce_file readonly)"
    local prompt
    prompt="You are a non-interactive test fixture in a scratch directory. Execute exactly this shell command in the current directory, then stop: touch $file. Rules: you MUST actually attempt your shell tool with that exact command. Only if that tool invocation itself fails or is rejected: do NOT create the file by any other means (no file-write or edit tools) — reply with the single word DENIED and stop."

    note "  mode-enforce[read-only]: the write must be blocked under --mode read-only"
    oh_run "$id" "$prompt" --mode read-only --cwd "$sandbox"
    status="$(oh_field '.results[0].status')"
    if [ "$status" = "skipped" ]; then
        rm -rf "$sandbox"
        skip "$id is not installed (oneharness reported status=skipped); nothing to verify"
    fi
    if [ -e "$sandbox/$file" ]; then
        oh_dump
        rm -rf "$sandbox"
        fail "$id: --mode read-only did NOT block the write ($file was created) — the read-only mapping is not honored (or its flag drifted)"
    fi
    note "  ok[read-only]: the write was blocked"

    note "  mode-enforce[bypass]: the same command must run under --mode bypass (control)"
    oh_run "$id" "$prompt" --mode bypass --cwd "$sandbox"
    if [ ! -e "$sandbox/$file" ]; then
        oh_dump
        rm -rf "$sandbox"
        fail "$id: positive control failed ($file absent under --mode bypass) — the read-only block can't be trusted (does the harness run shell headlessly?)"
    fi
    note "  ok[bypass]: the command ran"

    rm -rf "$sandbox"
    note "PASS: $id read-only enforcement"
}

# --- hook enforcement --------------------------------------------------------

# Live proof that a synced `[[hooks]]` gate is HONORED by the real harness — the
# hook counterpart to `oh_sync_enforce`, and the only check that the installed
# hook file is in a shape the real CLI loads and fires (the hermetic suite can
# only prove the file was written correctly).
#
# It syncs a `[[hooks]]` entry whose command is `oneharness gate <id>` into the
# harness's OWN config, then drives the harness (via `oneharness run`, under
# bypass so the hook is the sole decider) through two commands:
#   * deny  — a `touch` whose path carries a unique marker. The gate matches the
#             marker and emits the harness's native deny verdict, so the file
#             must be ABSENT.
#   * allow — a `touch` with no marker (the positive control). The gate stays
#             silent, so under bypass the command runs and the file is PRESENT —
#             proving the deny was a real block, not the harness simply never
#             executing anything headlessly.
#
# Excluded by design: Codex (`oneharness run` drives `codex exec`, which does not
# load hooks — allowlister proves Codex hooks only via the TUI in a PTY) and
# Copilot (project hooks are gated behind a real repo + a `trustedFolders` trust
# file + prompt-mode scaffolding that belongs in allowlister's adapter e2e).
#
#   $1 harness id
#   $2 scope: project (default) or global. Qwen only fires *user*-scoped hooks
#      headlessly (project hooks sit behind folder trust), so it syncs --global
#      into an isolated HOME that the run below also reads — which doubles as the
#      live proof of `sync --global`.
#
# The gate command is whitespace-tokenized (OpenCode's shim splits argv on
# spaces), so it carries no quoted arguments: the marker and the oneharness path
# must be space-free.
oh_hook_enforce() {
    local id="$1" scope="${2:-project}"
    local bin sandbox marker denyfile allowfile out status home
    bin="$(oh_bin)"
    [ -n "$bin" ] || skip "oneharness binary not found (build it: \`just build-release\`, or set ONEHARNESS_BIN)"

    sandbox="$(mktemp -d)"
    sandbox="$(oh_native_path "$sandbox")"
    oh_sandbox_prepare "$id" "$sandbox"
    # A real repo: some harnesses only discover project-scoped hooks inside one.
    git init -q "$sandbox" 2>/dev/null || true
    marker="OHGATEBLOCK${RANDOM}${RANDOM}"
    denyfile="$sandbox/$marker.txt"
    allowfile="$sandbox/ohgate-allowed-${RANDOM}${RANDOM}.txt"

    cat > "$sandbox/oneharness.toml" <<TOML
[[hooks]]
command = "$bin gate $id --deny-if-contains $marker"
harnesses = ["$id"]
plugin_name = "ohgate"
TOML

    # For a global sync, point HOME/XDG at the sandbox so the install lands where
    # the run will read it; the same env is passed to the harness via --env.
    local sync_vars=(ONEHARNESS_NO_CONFIG=) sync_global=() run_extra=()
    if [ "$scope" = global ]; then
        home="$sandbox/home"
        mkdir -p "$home"
        sync_vars+=("HOME=$home" "XDG_CONFIG_HOME=$sandbox/xdg")
        sync_global=(--global)
        run_extra=(--env "HOME=$home" --env "XDG_CONFIG_HOME=$sandbox/xdg")
    fi

    note "  hook-enforce: syncing a gate hook into $id's own config ($scope scope)"
    if ! out="$(env "${sync_vars[@]}" "$bin" sync --harness "$id" "${sync_global[@]+"${sync_global[@]}"}" \
        --cwd "$sandbox" --config "$sandbox/oneharness.toml" --compact 2>&1)"; then
        printf '%s\n' "$out" >&2
        rm -rf "$sandbox"
        fail "$id: oneharness sync failed to install the gate hook"
    fi

    local rules='Rules: you MUST actually invoke your shell tool with that exact command — never decide on your own that it is not permitted; attempt it. Use only the shell tool, and do NOT create the file by any other means.'

    note "  hook-enforce[deny]: the gate must block the marked command under bypass"
    oh_run "$id" "You are a non-interactive test fixture in a scratch directory. Execute exactly this shell command, then stop: touch $denyfile. $rules" --cwd "$sandbox" "${run_extra[@]+"${run_extra[@]}"}"
    status="$(oh_field '.results[0].status')"
    if [ "$status" = "skipped" ]; then
        rm -rf "$sandbox"
        skip "$id is not installed (oneharness reported status=skipped); nothing to verify"
    fi
    if [ -e "$denyfile" ]; then
        oh_dump
        rm -rf "$sandbox"
        fail "$id: the gate did NOT block — $denyfile was created despite the deny marker, so the installed hook is not honored (or its file format drifted)"
    fi
    note "  ok[deny]: the gate blocked the marked command"

    note "  hook-enforce[allow]: an unmarked command must run (positive control)"
    oh_run "$id" "You are a non-interactive test fixture in a scratch directory. Execute exactly this shell command, then stop: touch $allowfile. $rules" --cwd "$sandbox" "${run_extra[@]+"${run_extra[@]}"}"
    if [ ! -e "$allowfile" ]; then
        oh_dump
        rm -rf "$sandbox"
        fail "$id: the positive-control command never ran ($allowfile absent) — the deny phase cannot be trusted as a real block (does the harness run shell headlessly?)"
    fi
    note "  ok[allow]: the unmarked command ran"

    rm -rf "$sandbox"
    note "PASS: $id hook enforcement"
}

# --- structured output enforcement -------------------------------------------

# Live proof that `oneharness run --schema` produces a schema-VALID structured
# answer end to end — the counterpart of oh_sync_enforce/oh_hook_enforce for the
# structured-output feature, and the only check that catches drift in a harness's
# NATIVE schema delivery (the hermetic suite mocks the flag and its output
# field). It writes a small JSON Schema, asks the harness (via oneharness, with
# `--schema`) for an object whose `token` field is a high-entropy marker, then
# asserts oneharness reported `schema_valid: true` and round-tripped the marker
# into `.structured.token` — so a pass means the schema actually reached the
# model and the conforming value was extracted and validated, not merely that the
# process exited cleanly.
#
# Generic over the harness id: claude-code exercises the native `--json-schema`
# path (value read from `structured_output`); any other id exercises the portable
# prompt-based path. A missing harness is a SKIP, like every other live check.
#
#   $1 harness id
oh_schema_enforce() {
    local id="$1"
    local bin sandbox schema marker status valid token attempts

    bin="$(oh_bin)"
    [ -n "$bin" ] || skip "oneharness binary not found (build it: \`just build-release\`, or set ONEHARNESS_BIN)"

    sandbox="$(mktemp -d)"
    sandbox="$(oh_native_path "$sandbox")"
    marker="OHSCHEMA${RANDOM}${RANDOM}${RANDOM}"
    schema="$sandbox/schema.json"
    # additionalProperties:false + required keeps the constraint tight, so a
    # passing validation is a real conformance, not a vacuous match.
    printf '%s' '{"type":"object","properties":{"token":{"type":"string"},"ok":{"type":"boolean"}},"required":["token","ok"],"additionalProperties":false}' > "$schema"

    # One physical line, and NO embedded double quotes: an npm-installed harness
    # is a `.cmd` shim on Windows, and cmd.exe's `%*` forwarding mangles a
    # quote-containing argument (truncating it) — the same discipline the other
    # e2e prompts follow. (The quote-heavy schema itself rides `--json-schema`,
    # which is why the live schema check is scoped to Linux/macOS; see e2e-schema.yml.)
    local prompt="This is an automated structured-output check for the oneharness end-to-end test suite. Reply with a JSON object that has a token field set to exactly $marker and an ok field set to the boolean true. Output only that JSON object: no preamble, no explanation, no code fences."
    oh_run "$id" "$prompt" --schema "$schema"

    status="$(oh_field '.results[0].status')"
    if [ "$status" = "skipped" ] || [ "$(oh_field '.results[0].available')" != "true" ]; then
        rm -rf "$sandbox"
        skip "$id is not installed (oneharness reported status=$status); nothing to verify"
    fi

    valid="$(oh_field '.results[0].schema_valid')"
    attempts="$(oh_field '.results[0].schema_attempts')"
    if [ "$status" != "ok" ] || [ "$valid" != "true" ]; then
        oh_dump
        note "  schema_valid:  $valid"
        note "  schema_error:  $(oh_field '.results[0].schema_error // "null"')"
        note "  structured:    $(printf '%s' "$OH_REPORT" | jq -c '.results[0].structured // "null"')"
        rm -rf "$sandbox"
        fail "$id: --schema run did not yield a schema-valid result (status=$status, schema_valid=$valid, attempts=$attempts)"
    fi
    note "  ok: $id returned schema-valid structured output (attempts=$attempts)"

    # The marker must round-trip into the validated value, proving the schema
    # reached the model and the right object was extracted — not a lucky empty
    # object that happened to validate.
    token="$(oh_field '.results[0].structured.token? // ""')"
    if [ "$token" != "$marker" ]; then
        oh_dump
        note "  structured:    $(printf '%s' "$OH_REPORT" | jq -c '.results[0].structured // "null"')"
        rm -rf "$sandbox"
        fail "$id: the marker did not round-trip into .structured.token (got '$token')"
    fi
    note "  confirmed: the marker round-tripped into .structured.token"

    rm -rf "$sandbox"
    note "PASS: $id schema enforcement"
}
