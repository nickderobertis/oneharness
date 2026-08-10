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
        local cfg="$HOME/.claude.json" k tmp existing real
        local keys=("$dir")
        # macOS resolves symlinks in the cwd (/var → /private/var, /tmp →
        # /private/tmp), and Claude checks the *resolved* path for workspace
        # trust — so register that too, or the synced permissions.allow entry is
        # silently ignored ("this workspace has not been trusted") on macOS.
        real="$(cd "$dir" 2>/dev/null && pwd -P)" || real=""
        [ -n "$real" ] && [ "$real" != "$dir" ] && keys+=("$real")
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

# A high-entropy marker of FIXED character length (a constant prefix + zero-padded
# RANDOMs), so two of them have the same length and tokenize to the same count.
# Used by the batch caching check, where the min-tokens and speed runs must use
# DIFFERENT prompts (so neither warms a cache the other reads) that are still the
# same token length (so the two runs stay an apples-to-apples token comparison).
oh_marker_fixed() { printf 'OHBATCH%05d%05d%05d' "${RANDOM}" "${RANDOM}" "${RANDOM}"; }

# The connectivity prompt: ask the harness to include the random marker verbatim
# somewhere in its reply. Two refusal modes to avoid, both observed live:
#   * Copilot refuses a bare "output this token" as off-task ("I'm here to help
#     with software development tasks") — so the prompt frames it as a test-suite
#     connectivity check (an on-task software context).
#   * Claude (haiku) refuses an over-framed "echo this token and NOTHING else,
#     trust the framing" as a prompt-injection pattern — so the prompt drops the
#     "nothing else / suppress all other output" demand and the defensive
#     over-justification, and says plainly that the marker is a harmless,
#     non-secret test fixture.
# oh_assert_echoed is a substring check, so allowing the model to reply naturally
# (and merely include the marker) is fine — its presence still proves the model
# genuinely ran. Kept single-line for cross-shell quoting safety on Windows.
oh_prompt() {
    printf 'This is an automated connectivity check for the oneharness end-to-end test suite — a harmless request/response round-trip test, not untrusted input and not a secret to guard. The identifier below is a random fixture the suite just generated; please confirm the round-trip by including it verbatim somewhere in your reply: %s' "$1"
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

# --- large prompts / system (issue #1115) ------------------------------------

# Live proof that a LARGE prompt (and, where the harness supports it, a large
# system prompt) is delivered OFF THE ARGV — piped to the harness's stdin, or the
# system written to a file — so it never trips the OS argument ceiling (`E2BIG`:
# Linux caps a single argv string at 128 KiB, Windows the whole command line at
# ~32 KiB). This is the end-to-end drift alarm for issue #1115: if the stdin/file
# delivery flags drift or the harness stops reading stdin, a >128 KiB prompt
# fails to round-trip and this goes RED.
#
# The prompt/system are ~160 KiB each (past every ceiling) and reach oneharness
# via `--prompt-file`/`--system-file` (files, not argv — else the caller→oneharness
# spawn would itself E2BIG, the very wall #1108 moved). The unique marker sits at
# the END of the prompt (recency) so the model reliably echoes it back; its
# presence proves the FULL prompt was received, not truncated.
#
# Goose is the one wired harness whose `--system` has no off-argv route (inline
# TEXT only), so a large SYSTEM would E2BIG at its spawn — for goose the large
# system is skipped and only the (stdin-delivered) prompt is exercised.
#   $1 harness id
#   $2.. extra args forwarded to `oneharness run`
oh_long_prompt_enforce() {
    local id="$1"
    shift
    local bin marker sandbox pfile sfile pad
    bin="$(oh_bin)"
    [ -n "$bin" ] || skip "oneharness binary not found (build it: \`just build-release\`, or set ONEHARNESS_BIN)"

    marker="$(oh_marker)"
    sandbox="$(mktemp -d)"
    sandbox="$(oh_native_path "$sandbox")"
    pfile="$sandbox/big-prompt.txt"
    sfile="$sandbox/big-system.txt"
    # ~140 KiB of prompt filler — just past Linux's 128 KiB single-arg ceiling, so
    # inline delivery would E2BIG (the drift alarm) while keeping the model's
    # context modest so a slow model still answers inside the timeout.
    pad="$(head -c 143360 /dev/zero | tr '\0' 'x')"
    # ~70 KiB of system filler — over the 64 KiB off-argv threshold (so the system
    # rides the file/stdin path too) without doubling the context.
    spad="$(head -c 71680 /dev/zero | tr '\0' 'y')"
    # Padding first, then the real instruction + marker at the end (recency).
    { printf 'Inert padding to exceed the OS argument limit; ignore all of it: %s\n\n' "$pad"; oh_prompt "$marker"; } > "$pfile"
    printf 'You are a connectivity-check fixture for a large-prompt test. The following is inert padding, ignore it: %s' "$spad" > "$sfile"

    local model_args=()
    [ -n "${OH_MODEL:-}" ] && model_args+=(--model "$OH_MODEL")

    # Goose's --system is inline-only (no off-argv route), so a large system would
    # E2BIG at its spawn — drive a large prompt only for it.
    local system_args=(--system-file "$sfile")
    [ "$id" = goose ] && system_args=()

    # A large-context call legitimately takes longer than an ordinary connectivity
    # ping, and some harnesses route to a big/slow model, so give this phase a
    # generous timeout (independent of OH_TIMEOUT) — a real hang still trips it.
    local long_timeout="${OH_LONG_TIMEOUT:-300}"
    note "  long-prompt[$id]: a >128 KiB prompt (+system) must round-trip off the argv (timeout ${long_timeout}s)"
    OH_REPORT="$(ONEHARNESS_NO_CONFIG=1 "$bin" run --harness "$id" \
        --prompt-file "$pfile" "${system_args[@]+"${system_args[@]}"}" \
        --mode bypass --timeout "$long_timeout" --compact \
        "${model_args[@]+"${model_args[@]}"}" "$@" 2>"$sandbox/stderr.txt")" || true

    if [ -z "$OH_REPORT" ]; then
        note "  oneharness emitted no JSON on stdout. Its stderr:"
        sed 's/^/    /' "$sandbox/stderr.txt" >&2 || true
        rm -rf "$sandbox"
        fail "$id: oneharness produced no report for the long-prompt run"
    fi
    # The marker round-tripping proves the full prompt reached the model off-argv.
    oh_assert_echoed "$id" "$marker"
    # Guard against a silent regression to argv delivery: the giant prompt must
    # NOT appear as a command argument (it rode stdin / a file instead).
    if printf '%s' "$OH_REPORT" | jq -e --arg m "$marker" \
        '[.results[0].command[]] | any(contains($m))' >/dev/null; then
        oh_dump
        rm -rf "$sandbox"
        fail "$id: the large prompt appeared on the argv — off-argv delivery regressed"
    fi
    note "  confirmed: the large prompt was delivered off the argv (not in .command)"
    rm -rf "$sandbox"
    note "PASS: $id long-prompt enforcement"
}

# --- usage / cache-token reporting -------------------------------------------

# Live proof that oneharness surfaces provider prompt-cache counts in the
# normalized `usage` — the drift alarm for cache-token extraction against the
# REAL harness output shape (the hermetic suite can only mock the JSON). Only
# Claude Code and OpenCode report cache counts today (see the README `usage`
# field docs); call this only for those two.
#
# Provider prompt caching keys on the byte-exact request *prefix* (tools +
# system) the harness assembles, which is identical across two runs with the
# same model — the user message comes after the cache breakpoint, so it may
# differ. So a first "warm" run writes that prefix into the provider cache, and a
# second run (well within the ~5-min TTL) reads it back. We assert the second run
# reports cache_read_tokens > 0: the only end-to-end proof the count is real and
# lifted from the live harness's own usage block, not a mock.
#   $1 harness id
oh_cache_assert() {
    local id="$1"
    local status write1 read1 read2

    note "  cache[warm]: a first run writes the cacheable tools+system prefix"
    oh_run "$id" "$(oh_prompt "$(oh_marker)")"
    status="$(oh_field '.results[0].status')"
    if [ "$status" = "skipped" ] || [ "$(oh_field '.results[0].available')" != "true" ]; then
        skip "$id is not installed (oneharness reported status=$status); nothing to verify"
    fi
    [ "$status" = "ok" ] || { oh_dump; fail "$id: cache warm-up run did not complete (status=$status)"; }
    write1="$(oh_field '.results[0].usage.cache_write_tokens // 0')"
    read1="$(oh_field '.results[0].usage.cache_read_tokens // 0')"
    note "  warm run usage: cache_write_tokens=$write1 cache_read_tokens=$read1"

    note "  cache[read]: a second run with the same prefix must read it back"
    oh_run "$id" "$(oh_prompt "$(oh_marker)")"
    status="$(oh_field '.results[0].status')"
    [ "$status" = "ok" ] || { oh_dump; fail "$id: cache-read run did not complete (status=$status)"; }
    read2="$(oh_field '.results[0].usage.cache_read_tokens // 0')"
    note "  second run usage: cache_read_tokens=$read2 input_tokens=$(oh_field '.results[0].usage.input_tokens // "null"')"

    if [ "${read2:-0}" -gt 0 ] 2>/dev/null; then
        note "PASS: $id surfaced cache_read_tokens ($read2) on the cached second run"
    else
        oh_dump
        note "  usage: $(printf '%s' "$OH_REPORT" | jq -c '.results[0].usage')"
        fail "$id: the second run reported no cache_read_tokens (> 0) — either provider caching did not land (prefix unstable, under the cache-size threshold, or cold beyond the TTL) or oneharness's cache-token extraction drifted from the live shape"
    fi
}

# Live proof that the zero-turn `usage` probe still gets an ANSWER out of the
# real harness — the drift alarm for the probe's *exchange*, which the hermetic
# suite can only mock. Call it for every harness whose `usage` support is a probe
# (see the README support matrix); a harness that reports no headroom at all has
# nothing to drift.
#
# What is asserted is that the harness answered, not what it answered: CI
# authenticates with an API key, where the honest reading is `unavailable` with a
# reason, while a subscription box reports `available` windows. Both are the
# harness replying. `unknown` is the failure — it means oneharness asked and got
# nothing back.
#   $1 harness id
oh_usage_enforce() {
    local id="$1" bin report state reason detail errf rc
    bin="$(oh_bin)"
    [ -n "$bin" ] || skip "oneharness binary not found (build it: \`just build-release\`, or set ONEHARNESS_BIN)"

    errf="$(mktemp)"
    report="$(ONEHARNESS_NO_CONFIG=1 "$bin" usage --harness "$id" \
        --timeout "${OH_TIMEOUT:-120}" --compact 2>"$errf")" && rc=0 || rc=$?
    if [ -z "$report" ]; then
        note "  oneharness exited $rc and emitted no JSON on stdout. Its stderr:"
        sed 's/^/    /' "$errf" >&2 || true
        rm -f "$errf"
        note "  Next, in order:"
        note "    1. Exit 2 is a usage/config error in the call above, not a harness fault:"
        note "       read the stderr line, then fix the flags in oh_usage_enforce"
        note "       (scripts/e2e-lib.sh) and re-run:"
        note "         just live-$id"
        note "    2. Any other non-zero exit is oneharness aborting mid-probe, which its"
        note "       contract forbids — a missing binary, an unauthenticated harness, and a"
        note "       timeout are all meant to come back as an identity. File it against the"
        note "       probe (crates/oneharness-core/src/io/usage.rs) and its containment"
        note "       (probe_all in src/commands/usage.rs)."
        note "    3. Exit 0 with empty stdout means the report stopped reaching stdout —"
        note "       diagnostics belong on stderr and the report on stdout. Check print_json"
        note "       in src/commands/usage.rs, then rebuild and re-run:"
        note "         just build-release && just live-$id"
        fail "$id: 'oneharness usage' produced no report"
    fi
    rm -f "$errf"

    state="$(printf '%s' "$report" | jq -r '.identities[0].availability.state')"
    reason="$(printf '%s' "$report" | jq -r '
        .identities[0].availability.reason as $r
        | if ($r | type) == "object" then ($r.kind // "") else ($r // "") end')"
    case "$state:$reason" in
    available:*)
        detail="headroom $(printf '%s' "$report" |
            jq -r '[.identities[0].availability.windows[]
                    | "\(.id) \(.usage.used_percent // .usage.kind)"] | join(", ")')"
        ;;
    unavailable:*) detail="unavailable ($reason)" ;;
    unknown:binary_missing)
        skip "$id is not installed (oneharness reported binary_missing); nothing to probe"
        ;;
    *)
        note "  report: $report"
        note "  Next, in order:"
        note "    1. Replay the probe and read the harness's own errors:"
        note "         ONEHARNESS_NO_CONFIG=1 $bin usage --harness $id --timeout ${OH_TIMEOUT:-120} --format text"
        note "       An auth or plan error there is the account, not a drift."
        note "    2. Send $id's recorded request lines (docs/harness-usage.md) to the CLI by"
        note "       hand twice: once holding stdin open past the last request, once closing"
        note "       it straight away. Answered only when held open means the probe must wait"
        note "       for it — give $id StdinAfterRequests::HoldUntilAnswered in"
        note "       crates/oneharness-core/src/io/usage.rs."
        note "    3. Answered in an unrecognized shape means the payload moved: update $id's"
        note "       parser and its drift guard in crates/oneharness-core/src/domain/usage.rs"
        note "       (record the new payload in docs/harness-usage.md), then re-run:"
        note "         just live-$id"
        fail "$id: the usage probe got no answer out of the harness (state=$state, reason=$reason)"
        ;;
    esac
    # The reading itself is the evidence, and this log is its only record: a live
    # phase that passed silently is indistinguishable from one that never ran.
    note "PASS: $id answered its usage probe — $detail"
}

# --- normalized tool-call / action events ------------------------------------

# Live proof that oneharness surfaces normalized tool-call events in the
# `events` array — the drift alarm for event extraction against the REAL harness
# output shape (the hermetic suite can only mock the JSON). Only harnesses whose
# oneharness output format carries a machine-readable tool transcript expose
# events (OpenCode's `tool` parts, the Anthropic content-block stream — see the
# README `events` docs); call this only for those.
#
# Drives a prompt that forces a shell tool call — run a command that prints the
# marker — then asserts oneharness lifted at least one `tool_call` event into the
# normalized `events` array (with a non-null `events_source`, matching the caller's
# expected method when given). This is the only end-to-end proof the event
# extraction matches the live harness shape, not a mock.
#   $1 harness id
#   $2 expected events_source (optional; when given, held exactly; "" to skip)
#   $3.. extra args forwarded to `oneharness run` (e.g. --output-format
#        stream-json for a harness whose transcript needs a richer format)
oh_events_assert() {
    local id="$1"
    shift
    local expected_source="${1:-}"
    [ $# -gt 0 ] && shift
    local marker status count source calls

    marker="$(oh_marker)"
    note "  events: a run that must use a shell tool to print the marker"
    # Bypass so the agent may actually run the command; ask plainly for a shell
    # tool so a tool_call is emitted (not just an inline text answer). Any extra
    # args ($@) select the events-capable output format for this harness.
    oh_run "$id" "Using your shell/bash tool, run a command that prints the exact text ${marker} to stdout, then tell me you did it." "$@"
    status="$(oh_field '.results[0].status')"
    if [ "$status" = "skipped" ] || [ "$(oh_field '.results[0].available')" != "true" ]; then
        skip "$id is not installed (oneharness reported status=$status); nothing to verify"
    fi
    [ "$status" = "ok" ] || { oh_dump; fail "$id: events run did not complete (status=$status)"; }

    source="$(oh_field '.results[0].events_source')"
    count="$(printf '%s' "$OH_REPORT" | jq '(.results[0].events // []) | length')"
    calls="$(printf '%s' "$OH_REPORT" | jq '[(.results[0].events // [])[] | select(.kind == "tool_call")] | length')"
    note "  events: events_source=$source count=$count tool_calls=$calls"

    if [ "${count:-0}" -eq 0 ] 2>/dev/null || [ "$source" = "null" ]; then
        oh_dump
        note "  events: $(printf '%s' "$OH_REPORT" | jq -c '.results[0].events')"
        fail "$id: oneharness surfaced no normalized events for a tool-using turn — either the model answered without a tool call or event extraction drifted from the live shape"
    fi
    if [ "${calls:-0}" -lt 1 ] 2>/dev/null; then
        oh_dump
        fail "$id: events surfaced but none was a tool_call (got kinds: $(printf '%s' "$OH_REPORT" | jq -c '[.results[0].events[].kind]'))"
    fi
    if [ -n "$expected_source" ] && [ "$source" != "$expected_source" ]; then
        oh_dump
        fail "$id: expected events_source=$expected_source but got '$source' — event extraction regressed"
    fi
    # Input/output fidelity: the model ran `echo <marker>`, so the marker must
    # appear in a tool_call's structured `input` (and usually a tool_result's
    # `output`). This proves oneharness extracted the *real* args/observation into
    # the normalized shape — not merely that some empty event object surfaced.
    if ! printf '%s' "$OH_REPORT" | jq -e --arg m "$marker" \
        '[(.results[0].events // [])[] | (.input // {} | tostring) + (.output // "")] | any(contains($m))' >/dev/null; then
        oh_dump
        note "  events: $(printf '%s' "$OH_REPORT" | jq -c '.results[0].events')"
        fail "$id: the marker never appeared in any event's input/output — the command/observation was not lifted into the normalized event (structured extraction drifted)"
    fi
    note "PASS: $id surfaced $calls normalized tool_call event(s) via '$source' (marker present in event input/output)"
}

# Live proof of the STREAMING path: `oneharness run --stream` must emit at least
# one incremental `{"type":"event",...}` line for a tool-using turn, then a
# terminal `{"type":"result",...}` line — the end-to-end drift alarm for
# streaming + the consumer short-circuit contract. Extra args ($2..) are
# forwarded (e.g. --events to select a transcript-capable format).
#   $1 harness id
#   $2.. extra run args
oh_stream_assert() {
    local id="$1"
    shift
    local bin marker out events_lines result_lines
    bin="$(oh_bin)"
    [ -n "$bin" ] || skip "oneharness binary not found (build it: \`just build-release\`, or set ONEHARNESS_BIN)"

    marker="$(oh_marker)"
    local model_args=()
    [ -n "${OH_MODEL:-}" ] && model_args+=(--model "$OH_MODEL")

    note "  stream: a --stream run must emit event lines then a result line"
    out="$(ONEHARNESS_NO_CONFIG=1 "$bin" run --harness "$id" \
        --prompt "Using your shell/bash tool, run a command that prints the exact text ${marker} to stdout, then confirm." \
        --stream --mode bypass --timeout "${OH_TIMEOUT:-120}" \
        "${model_args[@]+"${model_args[@]}"}" "$@" 2>/dev/null)" || true

    if [ -z "$out" ]; then
        fail "$id: --stream produced no output"
    fi
    # Each line is a JSON object with a "type" discriminator.
    events_lines="$(printf '%s\n' "$out" | jq -rc 'select(.type == "event") | .event.kind' 2>/dev/null | grep -c . || true)"
    result_lines="$(printf '%s\n' "$out" | jq -rc 'select(.type == "result") | .report.results[0].status' 2>/dev/null | grep -c . || true)"
    note "  stream: event lines=$events_lines result lines=$result_lines"

    if [ "${result_lines:-0}" -lt 1 ] 2>/dev/null; then
        printf '%s\n' "$out" | tail -5 | sed 's/^/    /' >&2
        fail "$id: --stream emitted no terminal result line"
    fi
    if [ "${events_lines:-0}" -lt 1 ] 2>/dev/null; then
        printf '%s\n' "$out" | tail -8 | sed 's/^/    /' >&2
        fail "$id: --stream emitted no incremental event line for a tool-using turn"
    fi
    note "PASS: $id streamed $events_lines event line(s) then a result line"
}

# --- same-prefix batch caching ----------------------------------------------

# Live proof that a fork-capable `min-tokens` batch actually REDUCES tokens via
# session reuse — the end-to-end drift alarm for the feature (the hermetic suite
# can only pin the argv/scheduling against a mock). On a fork-capable, cache-
# reporting harness (Claude Code), `min-tokens` warms prompt[0] as a session that
# carries the large shared --system, then FORKS that session for the fan-out, so
# each fanned-out call reuses the warmed cached prefix instead of re-writing it.
# (A static --system can't be reused across separate `claude -p` processes — it's
# re-created each time; session reuse is the realizable saving, hence fork.)
#
# Asserts, from one batch run:
#   * the batch ran — one result per prompt, all clean, `batch.strategy` =
#     min-tokens, and `batch.forked` = true (the fan-out actually forked);
#   * the warm-up WROTE the shared prefix (cache_write > 0) — the positive control
#     that it is cacheable, and the in-batch baseline;
#   * every fanned-out fork READ the warmed prefix (cache_read > 0) and WROTE LESS
#     than the warm-up (cache_write < warm-up's) — i.e. it did not re-write the
#     shared --system, the token saving the mode promises.
# $1 harness id.
oh_batch_fork_enforce() {
    local id="$1" bin status count warm_write fan_write_max fan_read_min
    bin="$(oh_bin)"
    [ -n "$bin" ] || skip "oneharness binary not found (build it: \`just build-release\`, or set ONEHARNESS_BIN)"

    local prompts=()
    for _ in 1 2 3 4; do prompts+=("$(oh_prompt "$(oh_marker_fixed)")"); done
    note "  batch[min-tokens/fork]: warm prompt[0] as a session, fork it for the fan-out"
    _oh_batch_run "$id" min-tokens "$(_oh_batch_system)" "${prompts[@]}"
    status="$(oh_field '.results[0].status')"
    if [ "$status" = "skipped" ] || [ "$(oh_field '.results[0].available')" != "true" ]; then
        skip "$id is not installed (oneharness reported status=$status); nothing to verify"
    fi
    [ "$(oh_field '.batch.strategy')" = "min-tokens" ] || { oh_dump; fail "$id: report is not a batch (.batch missing)"; }
    count="$(printf '%s' "$OH_REPORT" | jq '.results | length')"
    [ "$count" = "${#prompts[@]}" ] || { oh_dump; fail "$id: batch returned $count results for ${#prompts[@]} prompts"; }
    _oh_batch_all_ok "$id" "min-tokens"
    if [ "$(oh_field '.batch.forked')" != "true" ]; then
        oh_dump
        fail "$id: the batch did not fork (.batch.forked != true) — the warm-up exposed no session id, or the harness is not fork-capable"
    fi

    warm_write="$(oh_field '.results[0].usage.cache_write_tokens // 0')"
    fan_write_max="$(printf '%s' "$OH_REPORT" | jq '[.results[1:][].usage.cache_write_tokens // 0] | max')"
    fan_read_min="$(printf '%s' "$OH_REPORT" | jq '[.results[1:][].usage.cache_read_tokens // 0] | min')"
    note "  usage: warm-up cache_write=$warm_write; fan-out max cache_write=$fan_write_max, min cache_read=$fan_read_min"
    note "  per-result usage: $(printf '%s' "$OH_REPORT" | jq -c '[.results[].usage]')"

    # Positive control: the warm-up must have written the cacheable prefix.
    if ! [ "${warm_write:-0}" -gt 0 ] 2>/dev/null; then
        oh_dump
        fail "$id: the warm-up wrote no cache (cache_write=$warm_write) — the shared prefix is not cacheable, so fork reuse cannot be measured"
    fi
    # Every fanned-out fork must have READ the warmed prefix...
    if ! [ "${fan_read_min:-0}" -gt 0 ] 2>/dev/null; then
        oh_dump
        fail "$id: a fanned-out fork read no cache (min cache_read=$fan_read_min) — the fork did not reuse the warmed session"
    fi
    # ...and WRITTEN LESS than the warm-up (it did not re-write the shared --system).
    if [ "${fan_write_max:-0}" -lt "${warm_write:-0}" ] 2>/dev/null; then
        note "PASS: $id min-tokens forked the warmed session and saved writes — fan-out cache_write (max $fan_write_max) < warm-up cache_write ($warm_write)"
    else
        oh_dump
        fail "$id: a fanned-out fork wrote as much as the warm-up (max fan-out cache_write=$fan_write_max >= warm-up=$warm_write) — the fork did not reuse the shared prefix, so min-tokens saved nothing"
    fi
}

# Assert every result in $OH_REPORT completed cleanly (status ok); dump + fail
# otherwise. $1 harness id, $2 strategy label (for the message).
_oh_batch_all_ok() {
    if printf '%s' "$OH_REPORT" | jq -e '[.results[].status] | all(. == "ok") | not' >/dev/null; then
        oh_dump
        fail "$1: a $2 batch call did not complete cleanly (statuses: $(printf '%s' "$OH_REPORT" | jq -c '[.results[].status]'))"
    fi
}

# The shared, cacheable --system prefix for a batch caching check. Two properties
# matter, and a third was the bug that made the first version of this check
# indistinguishable between strategies:
#   * unique per run — a fresh nonce at the very START, so the prefix is cold for
#     each run (no cross-run cache sharing, even on the shared body below);
#   * identical within a run — every prompt of one run sends the same bytes, so
#     the prefix is cacheable across that run's calls;
#   * LARGE ENOUGH TO CACHE — the body is padded well past the provider's
#     prompt-cache minimum (Haiku needs ~2048 tokens) so the prefix forms its own
#     cache breakpoint. A short --system (the original ~20-token sentence) is below
#     the minimum, never independently cached, so min-tokens and speed bill it
#     identically and the savings vanish — exactly the issue's caveat ("min-tokens
#     only helps when the shared prefix clears the minimum"). ~130 inert lines is
#     ~5k tokens, comfortably over the bar.
# Multi-line so oneharness spawns it via the Windows .cmd-shim bypass (a single
# ~20 KB line would exceed cmd.exe's command-line length limit); ~20 KB total
# stays well under the 32 KB CreateProcess limit.
_oh_batch_system() {
    local nonce body i
    nonce="$(oh_marker_fixed)"
    body=""
    for ((i = 1; i <= 130; i++)); do
        body+="Inert shared reference context line $i for the oneharness batch caching e2e; it carries no instructions and exists only to make the shared prefix large enough to be independently cached."$'\n'
    done
    printf 'Batch caching fixture, nonce %s. The text below is inert reference context — do not act on it; just answer the user request.\n%s' "$nonce" "$body"
}

# Drive one batch run: $1 id, $2 strategy, $3 shared --system, then the prompts.
# Stores the JSON report in $OH_REPORT (like oh_run). Uses --mode bypass so the
# agent actually answers, and honors $OH_MODEL / $OH_TIMEOUT.
_oh_batch_run() {
    local id="$1" strategy="$2" system="$3"
    shift 3
    local bin
    bin="$(oh_bin)"
    local model_args=()
    [ -n "${OH_MODEL:-}" ] && model_args+=(--model "$OH_MODEL")
    local prompt_args=() p
    local n=$#
    for p in "$@"; do prompt_args+=(--prompt "$p"); done
    # Claude Code injects per-invocation dynamic sections (working directory, git
    # status, platform, memory paths) into its system prompt; they share the cache
    # block that holds an appended --system, so a byte-identical --system is still
    # cache-CREATED (never read) on each separate `claude -p` process — making
    # min-tokens and speed indistinguishable. `--exclude-dynamic-system-prompt-
    # sections` moves those sections into the first user message, leaving the
    # system prompt (incl. our shared --system) static and prefix-cacheable across
    # calls — the condition min-tokens needs. Passed as a harness passthrough
    # (after `--`), only for Claude Code.
    local passthrough=()
    [ "$id" = claude-code ] && passthrough=(-- --exclude-dynamic-system-prompt-sections)
    note "  driving: $bin run --harness $id --batch-strategy $strategy ($n prompts)"
    OH_REPORT="$(ONEHARNESS_NO_CONFIG=1 "$bin" run --harness "$id" \
        --mode bypass --batch-strategy "$strategy" --system "$system" \
        --timeout "${OH_TIMEOUT:-120}" --compact \
        "${model_args[@]+"${model_args[@]}"}" "${prompt_args[@]}" \
        "${passthrough[@]+"${passthrough[@]}"}" 2>/dev/null)" || true
    [ -n "$OH_REPORT" ] || fail "$id: oneharness produced no report for the $strategy batch"
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

# Live proof that a NO-MUTATION mode (`read-only` or `plan`) is HONORED by the
# real harness — the drift alarm for the per-harness mapping (Codex's `--sandbox
# read-only`, Claude's `--disallowedTools` / `--permission-mode plan`, Copilot's
# `--deny-tool` / `--mode plan`, Cursor's `--mode ask` / `--mode plan`, Qwen's
# `--approval-mode plan`). The agent is told to `touch` a file:
#   * under `--mode <mode>` the write must be BLOCKED (file absent), and
#   * under `--mode bypass` the same command must run (file present) — the
#     positive control, so "absent" can't pass vacuously on a harness that runs
#     nothing headlessly.
# Only call this for a (harness, mode) that `oneharness list` marks supported.
#   $1 harness id   $2 mode to test (read-only | plan; default read-only)
oh_mode_enforce() {
    local id="$1" mode="${2:-read-only}"
    local bin sandbox file status
    bin="$(oh_bin)"
    [ -n "$bin" ] || skip "oneharness binary not found (build it: \`just build-release\`, or set ONEHARNESS_BIN)"

    sandbox="$(mktemp -d)"
    sandbox="$(oh_native_path "$sandbox")"
    oh_sandbox_prepare "$id" "$sandbox"
    file="$(oh_enforce_file "$mode")"
    local prompt
    prompt="You are a non-interactive test fixture in a scratch directory. Execute exactly this shell command in the current directory, then stop: touch $file. Rules: you MUST actually attempt your shell tool with that exact command. Only if that tool invocation itself fails or is rejected: do NOT create the file by any other means (no file-write or edit tools) — reply with the single word DENIED and stop."

    note "  mode-enforce[$mode]: the write must be blocked under --mode $mode"
    oh_run "$id" "$prompt" --mode "$mode" --cwd "$sandbox"
    status="$(oh_field '.results[0].status')"
    if [ "$status" = "skipped" ]; then
        rm -rf "$sandbox"
        skip "$id is not installed (oneharness reported status=skipped); nothing to verify"
    fi
    if [ -e "$sandbox/$file" ]; then
        oh_dump
        rm -rf "$sandbox"
        fail "$id: --mode $mode did NOT block the write ($file was created) — the $mode mapping is not honored (or its flag drifted)"
    fi
    note "  ok[$mode]: the write was blocked"

    note "  mode-enforce[bypass]: the same command must run under --mode bypass (control)"
    oh_run "$id" "$prompt" --mode bypass --cwd "$sandbox"
    if [ ! -e "$sandbox/$file" ]; then
        oh_dump
        rm -rf "$sandbox"
        fail "$id: positive control failed ($file absent under --mode bypass) — the $mode block can't be trusted (does the harness run shell headlessly?)"
    fi
    note "  ok[bypass]: the command ran"

    rm -rf "$sandbox"
    note "PASS: $id $mode enforcement"
}

# Live proof that `--mode edit` AUTO-APPROVES file edits — the reliably-testable
# half of edit's "auto-approve edits, gate shell" contract. The agent is asked to
# create a file with ONLY its file-writing/editing tool; under `--mode edit` that
# file must EXIST, because edit mode auto-approves an edit the harness would
# otherwise gate (copilot's `-p` auto-denies an un-allowed `write`; qwen's
# `default` deny-continues a write) — so its presence proves the edit mapping is
# honored live, not just that the harness can write.
#
# The "gate shell" half is deliberately NOT asserted live: it is not reliably
# testable. Told to create a file via shell, the model treats a gated shell as an
# obstacle and routes around it through whatever path the harness still allows —
# copilot auto-approves "safe" shell (`echo > file`), opencode delegates to a
# `task` subagent that runs the command, qwen's auto-edit ran it outright — so
# "file absent" is not a proxy for "shell blocked". That half stays argv/env-pinned
# hermetically (see `domain::harness` tests). Call only for a harness whose `edit`
# would otherwise gate the write (copilot/qwen); opencode's default permission is
# already `allow`, so the check can't distinguish its env mapping and it is omitted
# (its `OPENCODE_CONFIG_CONTENT` injection is pinned hermetically instead). $1 id.
oh_edit_enforce() {
    local id="$1"
    local bin sandbox editfile status
    bin="$(oh_bin)"
    [ -n "$bin" ] || skip "oneharness binary not found (build it: \`just build-release\`, or set ONEHARNESS_BIN)"

    sandbox="$(mktemp -d)"
    sandbox="$(oh_native_path "$sandbox")"
    oh_sandbox_prepare "$id" "$sandbox"
    editfile="$(oh_enforce_file edited)"
    local prompt
    prompt="You are a non-interactive test fixture in a scratch directory. Using your file-writing/editing tool (do NOT use the shell), create a file named $editfile containing the single word ok, then stop. If that tool call is rejected, reply with the single word DENIED and stop — do not use the shell or any other tool or workaround."

    note "  edit-enforce: a file edit must be auto-approved under --mode edit"
    oh_run "$id" "$prompt" --mode edit --cwd "$sandbox"
    status="$(oh_field '.results[0].status')"
    if [ "$status" = "skipped" ]; then
        rm -rf "$sandbox"
        skip "$id is not installed (oneharness reported status=skipped); nothing to verify"
    fi
    if [ ! -e "$sandbox/$editfile" ]; then
        oh_dump
        rm -rf "$sandbox"
        fail "$id: --mode edit did NOT auto-approve the file edit ($editfile absent) — edit mode must auto-approve edits (its argv/env mapping may have drifted)"
    fi
    note "  ok[edit]: the file edit was auto-approved"

    rm -rf "$sandbox"
    note "PASS: $id edit enforcement"
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
# Excluded by design: Codex (`codex exec` loads hooks only when the invocation
# opts in with `-c features.hooks=true --dangerously-bypass-hook-trust` —
# probe-verified 2026-07-06; oh_mock_enforce passes those flags and is the live
# proof its hooks load, so this plain gate phase stays omitted) and Copilot
# (probe-refuted: its repo `.github/hooks` produced ZERO events headlessly in
# `-p` even while the agent used its shell tool — nothing to gate).
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

# Live proof that out-of-band turn control actually STOPS work: drive a real
# turn that produces one observable artifact per step, interrupt it from a
# SEPARATE process mid-flight, and prove the artifact count is frozen afterwards.
#
# The assertion is deliberately at the filesystem, not at the harness's own
# report: goose and copilot both report a normal `end_turn` after a real
# cancellation, so a report-trusting check passes for the wrong reason. Files on
# disk are the one signal a harness cannot fake.
#
# Also asserts the contract around the stop: the socket exists at the documented
# path while the run lives and is gone after it, the interrupt response carries
# the harness's mechanism, the run's own report records the served interrupt,
# and the SESSION survives (the whole point — the dispatch is redirected, not
# destroyed).
#
# Only call this for a harness `oneharness list` reports a `control` mechanism
# for. Retries when the turn ended before the interrupt landed (a model that
# refused or finished early is flakiness, not a control regression) — which
# includes a turn that produced EVERY step it was asked for: a count pinned at
# the ceiling stays pinned whether or not anything was interrupted, so that
# reading is inconclusive rather than a pass.
#
#   $1 harness id
#   $2 mechanism reported by that harness's `oneharness list` entry
# llmlint: ignore-block[tool_output_is_signal] The phase's progress lines are how a
# CI log attributes a failure inside a multi-minute turn plus a 15s freeze window
# to the step that produced it — the same contract every other oh_*_enforce helper
# here follows.
oh_control_enforce() {
    local id="$1" expected_mechanism="$2"
    # Three attempts, not two: an inconclusive attempt is a model that refused or
    # raced, and opencode declines this fixture outright often enough (answers
    # `ok` in under ten seconds having run no tool at all) that two attempts
    # retire the phase on model mood rather than on control behavior. Retrying
    # cannot hide a regression — every real contract violation below calls
    # `fail` on the spot, and only a turn that proved NOTHING comes back here.
    local attempt attempts=3
    for attempt in $(seq 1 "$attempts"); do
        if _oh_control_enforce_once "$id" "$attempt" "$expected_mechanism"; then
            note "PASS: $id control enforcement"
            return 0
        fi
        note "  control-enforce: attempt $attempt was inconclusive (the turn ended, or finished every step, before the interrupt landed); retrying"
    done
    fail "$id: the turn never stayed in flight long enough to interrupt across $attempts attempts"
}

# One attempt. Returns 0 on a proven interrupt, 1 when the turn ended too early
# to prove anything (retryable). Any real contract violation calls fail().
_oh_control_enforce_once() {
    local id="$1" attempt="$2" expected_mechanism="$3"
    local bin sandbox store name socket count frozen frozen_after report
    bin="$(oh_bin)"
    [ -n "$bin" ] || skip "oneharness binary not found (build it: \`just build-release\`, or set ONEHARNESS_BIN)"

    sandbox="$(mktemp -d)"
    sandbox="$(oh_native_path "$sandbox")"
    oh_sandbox_prepare "$id" "$sandbox"
    git init -q "$sandbox" 2>/dev/null || true
    store="$sandbox/sessions"
    name="ohctl${attempt}${RANDOM}"
    socket="$store/control/$name.sock"
    report="$sandbox/report.json"

    local model_args=()
    [ -n "${OH_MODEL:-}" ] && model_args+=(--model "$OH_MODEL")

    # Many small steps with a pause between each: the turn must still be running
    # when the interrupt arrives, and each step must be individually observable.
    # `steps` is the ceiling the assertion below reads too, so the prompt and the
    # "did it merely finish?" test can never drift apart.
    local steps=60 last
    last="$(printf 'step-%03d.txt' "$steps")"
    local prompt="You are a non-interactive test fixture in a scratch directory. Using your shell tool, create $steps files named step-001.txt through $last in the current directory, ONE PER TOOL CALL, sleeping 1 second between each (for example: sleep 1 && touch step-001.txt). Do not use a loop and do not create them in one command — make a separate tool call for every file. Start now and keep going."

    # The turn must actually run shell commands for there to be work to stop, so a
    # narrower mode would make the freeze assertion vacuous. Confined to a fresh
    # mktemp sandbox, like every other oh_*_enforce phase.
    local grant=(--mode bypass) # llmlint: ignore[least_privilege_grants] see above
    note "  control-enforce: starting a controlled run ($id, session $name)"
    ONEHARNESS_NO_CONFIG=1 "$bin" run --harness "$id" --prompt "$prompt" \
        --control --session "$name" --session-dir "$store" --cwd "$sandbox" \
        "${grant[@]}" --timeout "${OH_TIMEOUT:-300}" --compact \
        "${model_args[@]+"${model_args[@]}"}" >"$report" 2>"$sandbox/run.err" &
    local run_pid=$!

    # The socket must appear at the documented path while the run is alive.
    if ! _oh_wait_for 60 test -S "$socket"; then
        kill "$run_pid" 2>/dev/null || true
        wait "$run_pid" 2>/dev/null || true
        sed 's/^/    /' "$sandbox/run.err" >&2 || true
        rm -rf "$sandbox"
        fail "$id: no control socket appeared at $socket (--control did not open one)"
    fi
    note "  ok: control socket present at $socket"

    # Wait until the agent is demonstrably working: at least two steps done, so
    # a frozen count afterwards means something real stopped.
    if ! _oh_wait_for 180 _oh_steps_at_least "$sandbox" 2; then
        kill "$run_pid" 2>/dev/null || true
        wait "$run_pid" 2>/dev/null || true
        rm -rf "$sandbox"
        note "  control-enforce: the agent never produced two steps"
        return 1
    fi
    if ! kill -0 "$run_pid" 2>/dev/null; then
        rm -rf "$sandbox"
        return 1
    fi
    # A turn that already wrote every file it was asked for has nothing left to
    # stop, so a frozen count afterwards is what finishing looks like, not what
    # an interrupt looks like. Inconclusive, never a pass.
    count="$(_oh_step_count "$sandbox")"
    if [ "$count" -ge "$steps" ]; then
        kill "$run_pid" 2>/dev/null || true
        wait "$run_pid" 2>/dev/null || true
        rm -rf "$sandbox"
        note "  control-enforce: the agent finished all $steps steps before the interrupt could land"
        return 1
    fi

    note "  control-enforce: interrupting from a separate process"
    local frame
    frame="$(ONEHARNESS_NO_CONFIG=1 "$bin" interrupt --session "$name" --session-dir "$store" --cwd "$sandbox" --compact 2>&1)" || {
        # A turn that ended between the count check and the interrupt answers
        # `no_active_turn` — retryable, not a regression.
        if printf '%s' "$frame" | grep -q no_active_turn; then
            kill "$run_pid" 2>/dev/null || true
            wait "$run_pid" 2>/dev/null || true
            rm -rf "$sandbox"
            return 1
        fi
        printf '%s\n' "$frame" >&2
        kill "$run_pid" 2>/dev/null || true
        wait "$run_pid" 2>/dev/null || true
        rm -rf "$sandbox"
        fail "$id: the interrupt was refused"
    }
    local ok mechanism
    ok="$(printf '%s' "$frame" | jq -r '.ok')"
    mechanism="$(printf '%s' "$frame" | jq -r '.mechanism // ""')"
    [ "$ok" = "true" ] || {
        printf '%s\n' "$frame" >&2
        kill "$run_pid" 2>/dev/null || true
        wait "$run_pid" 2>/dev/null || true
        rm -rf "$sandbox"
        fail "$id: the interrupt response was not ok"
    }
    if ! _oh_control_mechanism_matches "$mechanism" "$expected_mechanism"; then
        rm -rf "$sandbox"
        fail "$id: the interrupt response carried mechanism '$mechanism', expected '$expected_mechanism'"
    fi
    note "  ok: interrupt served by mechanism '$mechanism'"

    # THE assertion: the work is frozen. Sample right after the interrupt (with
    # a beat for an in-flight tool call to land), then again 15s later.
    sleep 3
    frozen="$(_oh_step_count "$sandbox")"
    # Same trap on the other side of the interrupt: a turn that raced to the
    # ceiling in the seconds it took to send the frame would hold at $steps for
    # any window you cared to watch, and that reading is indistinguishable from a
    # completed turn. Retry rather than bank it.
    if [ "$frozen" -ge "$steps" ]; then
        kill "$run_pid" 2>/dev/null || true
        wait "$run_pid" 2>/dev/null || true
        rm -rf "$sandbox"
        note "  control-enforce: the count reached all $steps steps, so a freeze proves nothing"
        return 1
    fi
    sleep 15
    frozen_after="$(_oh_step_count "$sandbox")"
    if [ "$frozen_after" != "$frozen" ]; then
        kill "$run_pid" 2>/dev/null || true
        wait "$run_pid" 2>/dev/null || true
        rm -rf "$sandbox"
        fail "$id: work did NOT stop — step files went from $frozen to $frozen_after in the 15s after the interrupt, so the '$mechanism' interrupt is not honored"
    fi
    note "  ok: work frozen at $frozen step files for 15s after the interrupt"

    wait "$run_pid" 2>/dev/null || true
    # The run must have ended on its own (not been killed) and recorded the
    # interrupt, and the session must have survived.
    if ! jq -e '.control.interrupts | length >= 1 and (.[0].outcome == "served")' "$report" >/dev/null 2>&1; then
        sed 's/^/    /' "$sandbox/run.err" >&2 || true
        head -c 2000 "$report" >&2 || true
        rm -rf "$sandbox"
        fail "$id: the run report did not record a served interrupt"
    fi
    if ! jq -e '.session.token != null' "$report" >/dev/null 2>&1; then
        rm -rf "$sandbox"
        fail "$id: the session did not survive the interrupt (no token was captured), so the turn was destroyed rather than redirected"
    fi
    if [ -e "$socket" ]; then
        rm -rf "$sandbox"
        fail "$id: the control socket outlived the run ($socket still present)"
    fi
    note "  ok: report records the interrupt, session survived, socket removed"

    rm -rf "$sandbox"
    return 0
}

_oh_control_mechanism_matches() {
    [ -n "$2" ] && [ "$1" = "$2" ]
}

# Live proof that an interrupt's REDIRECTION is delivered: drive a real turn
# doing the wrong thing, interrupt it with `--input` naming a different piece of
# work, and prove both halves — the original work froze, AND the redirected work
# was actually done, by the same dispatch, without the session being redispatched.
#
# The redirected artifact is the assertion that separates this from
# `oh_control_enforce`: an interrupt that stopped the turn and quietly dropped
# the message would pass that phase and fail this one, which is the exact failure
# this feature exists to make impossible. Measured on the filesystem for the same
# reason: several harnesses report a normal `end_turn` after a cancellation, so
# nothing the harness says about its own turns can carry this.
#
# Only call this for a harness `oneharness list` reports a `control` mechanism
# for. Retries an inconclusive attempt (a turn that ended or finished every step
# before the interrupt landed) exactly like `oh_control_enforce`.
#
#   $1 harness id
#
# Its progress lines fall under the `tool_output_is_signal` ignore-block opened
# above `oh_control_enforce`: a CI log needs them to attribute a failure inside a
# multi-minute two-turn exchange to the step that produced it.
oh_control_redirect_enforce() {
    local id="$1"
    local attempt attempts=3
    for attempt in $(seq 1 "$attempts"); do
        if _oh_control_redirect_enforce_once "$id" "$attempt"; then
            note "PASS: $id redirection enforcement"
            return 0
        fi
        note "  redirect-enforce: attempt $attempt was inconclusive (the turn ended, or finished every step, before the interrupt landed); retrying"
    done
    fail "$id: the turn never stayed in flight long enough to redirect across $attempts attempts"
}

# One attempt. Returns 0 on a proven redirection, 1 when the turn ended too early
# to prove anything (retryable). Any real contract violation calls fail().
_oh_control_redirect_enforce_once() {
    local id="$1" attempt="$2"
    local bin sandbox store name socket count frozen frozen_after report marker redirected
    bin="$(oh_bin)"
    [ -n "$bin" ] || skip "oneharness binary not found (build it: \`just build-release\`, or set ONEHARNESS_BIN)"

    sandbox="$(mktemp -d)"
    sandbox="$(oh_native_path "$sandbox")"
    oh_sandbox_prepare "$id" "$sandbox"
    git init -q "$sandbox" 2>/dev/null || true
    store="$sandbox/sessions"
    name="ohred${attempt}${RANDOM}"
    socket="$store/control/$name.sock"
    report="$sandbox/report.json"
    # The redirected work names a file nothing else in this sandbox could create,
    # so its presence can only mean the message reached the agent.
    marker="ohredirect${RANDOM}${RANDOM}"
    redirected="$sandbox/$marker.txt"

    local model_args=()
    [ -n "${OH_MODEL:-}" ] && model_args+=(--model "$OH_MODEL")

    local steps=60 last
    last="$(printf 'step-%03d.txt' "$steps")"
    local prompt="You are a non-interactive test fixture in a scratch directory. Using your shell tool, create $steps files named step-001.txt through $last in the current directory, ONE PER TOOL CALL, sleeping 1 second between each (for example: sleep 1 && touch step-001.txt). Do not use a loop and do not create them in one command — make a separate tool call for every file. Start now and keep going."
    local redirect="Stop creating step-NNN.txt files immediately. Instead, using your shell tool, run exactly this one command and then stop: touch $marker.txt"

    # The turn must actually run shell commands for there to be work to stop and
    # work to redirect to. Confined to a fresh mktemp sandbox, like every other
    # oh_*_enforce phase.
    local grant=(--mode bypass) # llmlint: ignore[least_privilege_grants] see above
    note "  redirect-enforce: starting a controlled run ($id, session $name)"
    ONEHARNESS_NO_CONFIG=1 "$bin" run --harness "$id" --prompt "$prompt" \
        --control --session "$name" --session-dir "$store" --cwd "$sandbox" \
        "${grant[@]}" --timeout "${OH_TIMEOUT:-300}" --compact \
        "${model_args[@]+"${model_args[@]}"}" >"$report" 2>"$sandbox/run.err" &
    local run_pid=$!

    if ! _oh_wait_for 60 test -S "$socket"; then
        kill "$run_pid" 2>/dev/null || true
        wait "$run_pid" 2>/dev/null || true
        sed 's/^/    /' "$sandbox/run.err" >&2 || true
        rm -rf "$sandbox"
        fail "$id: no control socket appeared at $socket (--control did not open one)"
    fi

    if ! _oh_wait_for 180 _oh_steps_at_least "$sandbox" 2; then
        kill "$run_pid" 2>/dev/null || true
        wait "$run_pid" 2>/dev/null || true
        rm -rf "$sandbox"
        note "  redirect-enforce: the agent never produced two steps"
        return 1
    fi
    if ! kill -0 "$run_pid" 2>/dev/null; then
        rm -rf "$sandbox"
        return 1
    fi
    count="$(_oh_step_count "$sandbox")"
    if [ "$count" -ge "$steps" ]; then
        kill "$run_pid" 2>/dev/null || true
        wait "$run_pid" 2>/dev/null || true
        rm -rf "$sandbox"
        note "  redirect-enforce: the agent finished all $steps steps before the interrupt could land"
        return 1
    fi

    note "  redirect-enforce: interrupting WITH a redirection from a separate process"
    local frame
    frame="$(ONEHARNESS_NO_CONFIG=1 "$bin" interrupt --session "$name" --session-dir "$store" --cwd "$sandbox" --input "$redirect" --compact 2>&1)" || {
        if printf '%s' "$frame" | grep -q no_active_turn; then
            kill "$run_pid" 2>/dev/null || true
            wait "$run_pid" 2>/dev/null || true
            rm -rf "$sandbox"
            return 1
        fi
        printf '%s\n' "$frame" >&2
        kill "$run_pid" 2>/dev/null || true
        wait "$run_pid" 2>/dev/null || true
        rm -rf "$sandbox"
        fail "$id: the interrupt carrying a redirection was refused"
    }
    if ! printf '%s' "$frame" | jq -e '.ok == true and .redirected == true' >/dev/null 2>&1; then
        printf '%s\n' "$frame" >&2
        kill "$run_pid" 2>/dev/null || true
        wait "$run_pid" 2>/dev/null || true
        rm -rf "$sandbox"
        fail "$id: the answer did not say the redirection was taken (expected ok+redirected)"
    fi
    note "  ok: the run took the redirection"

    # Half one: the original work stopped. Sampled after a beat for an in-flight
    # tool call, then again — the redirected turn must not be the old one
    # carrying on.
    sleep 5
    frozen="$(_oh_step_count "$sandbox")"
    if [ "$frozen" -ge "$steps" ]; then
        kill "$run_pid" 2>/dev/null || true
        wait "$run_pid" 2>/dev/null || true
        rm -rf "$sandbox"
        note "  redirect-enforce: the count reached all $steps steps, so a freeze proves nothing"
        return 1
    fi

    # Half two: the redirected work was DONE — by this same dispatch, with no
    # second `oneharness run`. This is what an interrupt that dropped the message
    # would fail.
    if ! _oh_wait_for "${OH_TIMEOUT:-300}" test -e "$redirected"; then
        kill "$run_pid" 2>/dev/null || true
        wait "$run_pid" 2>/dev/null || true
        sed 's/^/    /' "$sandbox/run.err" >&2 || true
        rm -rf "$sandbox"
        fail "$id: the redirection never reached the agent ($marker.txt was never created), so \`interrupt --input\` stopped the turn and lost the message"
    fi
    note "  ok: the redirected work was done by the same dispatch"

    frozen_after="$(_oh_step_count "$sandbox")"
    if [ "$frozen_after" != "$frozen" ]; then
        kill "$run_pid" 2>/dev/null || true
        wait "$run_pid" 2>/dev/null || true
        rm -rf "$sandbox"
        fail "$id: the original work did NOT stop — step files went from $frozen to $frozen_after after the interrupt, so the redirection was queued behind a turn that kept running"
    fi
    note "  ok: the original work stayed frozen at $frozen step files"

    wait "$run_pid" 2>/dev/null || true
    if ! jq -e '.control.interrupts | length >= 1 and (.[0].outcome == "served") and (.[0].redirected == true)' "$report" >/dev/null 2>&1; then
        sed 's/^/    /' "$sandbox/run.err" >&2 || true
        head -c 2000 "$report" >&2 || true
        rm -rf "$sandbox"
        fail "$id: the run report did not record a served REDIRECTED interrupt"
    fi
    if ! jq -e '.session.token != null' "$report" >/dev/null 2>&1; then
        rm -rf "$sandbox"
        fail "$id: the session did not survive the redirection (no token was captured)"
    fi
    note "  ok: report records the redirection and the session survived"

    rm -rf "$sandbox"
    return 0
}

# How many observable work artifacts the agent has produced so far. A glob
# rather than `ls | grep` so a name shellcheck worries about can never miscount.
_oh_step_count() {
    local dir="$1" file count=0
    for file in "$dir"/step-*; do
        [ -e "$file" ] && count=$((count + 1))
    done
    printf '%s' "$count"
}

# llmlint: ignore-end[tool_output_is_signal]

# Whether `dir` holds at least `want` work artifacts — the predicate form of
# `_oh_step_count`, so waiting on it needs no shell expression.
_oh_steps_at_least() {
    [ "$(_oh_step_count "$1")" -ge "$2" ]
}

# Poll `command…` until it succeeds or `seconds` elapse. Returns 1 on timeout.
# Control is a race between two processes, so every wait here is on observable
# state rather than a fixed sleep.
#
# The condition is a command and its arguments, never a string this evals: the
# things worth waiting on are named by sandbox paths, and a path is external
# input — under `eval` any shell syntax in one stops being a path and becomes
# part of the expression.
_oh_wait_for() {
    local seconds="$1" waited=0
    shift
    while [ "$waited" -lt "$seconds" ]; do
        if "$@"; then
            return 0
        fi
        sleep 1
        waited=$((waited + 1))
    done
    return 1
}

# Live proof that `run --mock-rules` — the single-flag ephemeral mock — is
# honored end to end by the real harness: the hook is delivered for THIS run
# only (claude: a per-run --settings temp file; the rest: a project-scope
# install snapshotted and restored; codex: its hook-engine opt-in flags
# auto-appended), the ruleset rewrites the marked command (`touch ORIG…`) to a
# different one (`touch MOCK…`), and afterwards the workspace carries no trace
# of the hook. Only call this for a harness `oneharness list` marks with a
# `mock_rewrite` shape. Asserts:
#   * the MOCK file exists — the rewritten input is what actually executed
#     (this doubles as the positive control: a command demonstrably ran), and
#   * the ORIG file does not — the original input was really substituted, and
#   * the spy log recorded the ORIGINAL event with action `rewrite`, and
#   * no file in the workspace still mentions the mock hook — the ephemeral
#     delivery restored/removed everything.
#
#   $1 harness id
oh_mock_enforce() {
    local id="$1"
    local bin sandbox marker origfile mockfile rulesfile spyfile status
    bin="$(oh_bin)"
    [ -n "$bin" ] || skip "oneharness binary not found (build it: \`just build-release\`, or set ONEHARNESS_BIN)"

    sandbox="$(mktemp -d)"
    sandbox="$(oh_native_path "$sandbox")"
    oh_sandbox_prepare "$id" "$sandbox"
    # A real repo: some harnesses only discover project-scoped hooks inside one.
    git init -q "$sandbox" 2>/dev/null || true
    marker="OHMOCKORIG${RANDOM}${RANDOM}"
    origfile="$sandbox/$marker.txt"
    mockfile="$sandbox/ohmock-rewritten-${RANDOM}${RANDOM}.txt"
    rulesfile="$sandbox/mock-rules.json"
    spyfile="$sandbox/mock-spy.jsonl"

    # The ruleset: rewrite the marked command to touch the MOCK file instead.
    # Every rewrite-capable harness's shell tool takes (and exposes) its command
    # in a `command` input field, so one input object serves them all — and the
    # MATCH here uses a per-field `input.command.regex` predicate (the marker is
    # alphanumeric, a valid literal regex), so this phase is also the live proof
    # of regex + input-field matching against real harness events on every
    # rewrite harness. (Stub/deny phases keep `event_contains`, so both matcher
    # styles stay live-covered.)
    cat > "$rulesfile" <<JSON
{"rules":[{"match":{"input":{"command":{"regex":"$marker"}}},"action":{"rewrite":{"input":{"command":"touch $mockfile"},"message":"rewritten by oh_mock_enforce"}}}]}
JSON

    # Up to two attempts: a model occasionally refuses the fixture framing and
    # never invokes its shell tool at all — visible as an EMPTY spy log (the
    # hook never fired), which is agent flakiness, not a verdict-shape failure.
    # Only that case is retried; a fired hook with a wrong outcome fails hard.
    local attempt
    for attempt in 1 2; do
        rm -f "$spyfile"
        note "  mock-enforce[rewrite]: the marked command must run REWRITTEN under bypass (attempt $attempt)"
        oh_run "$id" "You are a non-interactive test fixture in a scratch directory. Execute exactly this shell command, then stop: touch $origfile. Rules: you MUST actually invoke your shell tool with that exact command — never decide on your own that it is not permitted; attempt it. Use only the shell tool, and do NOT create the file by any other means." --cwd "$sandbox" --mock-rules "$rulesfile" --spy-file "$spyfile"
        status="$(oh_field '.results[0].status')"
        if [ "$status" = "skipped" ]; then
            rm -rf "$sandbox"
            skip "$id is not installed (oneharness reported status=skipped); nothing to verify"
        fi
        if [ -s "$spyfile" ]; then
            break
        fi
        note "  note: the spy log is empty — the agent never attempted the tool call (a refusal, not a hook failure)"
    done
    if [ ! -s "$spyfile" ]; then
        oh_dump
        rm -rf "$sandbox"
        fail "$id: the agent never invoked its shell tool in $attempt attempts (spy log empty) — the rewrite path could not be exercised (a prompt-robustness problem, not a verdict-shape one)"
    fi
    if [ ! -e "$mockfile" ]; then
        oh_dump
        head -5 "$spyfile" >&2
        rm -rf "$sandbox"
        fail "$id: the rewrite was NOT honored ($mockfile absent although the hook fired — see the spy records above) — the harness ignored or misparsed the mock_rewrite verdict shape (drift)"
    fi
    if [ -e "$origfile" ]; then
        oh_dump
        rm -rf "$sandbox"
        fail "$id: the ORIGINAL command still ran ($origfile exists) — the rewrite did not substitute the input"
    fi
    note "  ok[rewrite]: the rewritten command ran and the original did not"

    if ! jq -e -s 'map(select(.action == "rewrite")) | length >= 1' "$spyfile" >/dev/null 2>&1; then
        oh_dump
        [ -f "$spyfile" ] && head -5 "$spyfile" >&2
        rm -rf "$sandbox"
        fail "$id: the spy log recorded no rewrite action ($spyfile) — the spy channel is broken even though the rewrite executed"
    fi
    if ! grep -q "$marker" "$spyfile"; then
        rm -rf "$sandbox"
        fail "$id: the spy log lost the ORIGINAL command (marker $marker absent) — it must record pre-rewrite intent"
    fi
    note "  ok[spy]: the spy log preserved the original event"

    # Ephemerality: after the run, no CONFIG file in the workspace may still
    # mention the mock hook (the snapshotted files were restored, created ones
    # removed; claude's delivery never wrote into the workspace at all). The
    # rules/spy files are ours and carry no hook command, so a text hit is
    # residue. -I ignores binary files: a harness's own runtime artifacts (e.g.
    # crush's .crush/crush.db session database) may journal whatever config the
    # run loaded — that is the harness recording history, not a missed restore.
    if grep -rIqF "mock $id --rules" "$sandbox" 2>/dev/null; then
        grep -rIlF "mock $id --rules" "$sandbox" >&2
        rm -rf "$sandbox"
        fail "$id: the ephemeral mock hook left residue in the workspace (files above) — the restore did not run or missed a file"
    fi
    note "  ok[ephemeral]: the workspace carries no trace of the hook"

    # Stub enforcement: the `stub` action declares only the OUTPUT; oneharness
    # generates the printf rewrite itself, so the stub's text becomes the tool's
    # genuine result — the live proof of the declare-the-output mock.
    #
    # The assertion observes the TOOL RESULT, not the model's prose. The stub's
    # contract is "the declared text is what the tool returns"; whether the model
    # then repeats it is model behavior, not oneharness's, and asserting on it is
    # flaky — a capable model reads the coercive "you MUST repeat this verbatim"
    # framing (or a stubbed output it can't reconcile) as a prompt injection and
    # lectures instead of relaying (observed live — opencode/haiku, twice). So we
    # ask (plainly, no coercion) the model to run the command, pass `--events` so
    # the tool transcript lands in the result's raw `stdout`, and assert the
    # marker surfaced there (the tool output) OR in the extracted `text` (the
    # fallback for crush, whose headless output carries no transcript). The
    # marker cannot appear unless the stub rule matched and its printf executed,
    # so a hit proves the stub end to end regardless of what the model says.
    local stubmark stubrules stubspy blob
    stubmark="OHSTUBOUT${RANDOM}${RANDOM}"
    stubrules="$sandbox/stub-rules.json"
    stubspy="$sandbox/stub-spy.jsonl"
    cat > "$stubrules" <<JSON
{"rules":[{"match":{"event_contains":"ohstub-probe"},"action":{"stub":{"output":"$stubmark"}}}]}
JSON
    for attempt in 1 2; do
        rm -f "$stubspy"
        note "  mock-enforce[stub]: the stub's output must become the tool result (attempt $attempt)"
        oh_run "$id" "You are a non-interactive test fixture in a scratch directory. Using your shell tool, run this command to read a status token, then tell me the token it printed: cat ohstub-probe.txt" --cwd "$sandbox" --events --mock-rules "$stubrules" --spy-file "$stubspy"
        [ -s "$stubspy" ] && break
        note "  note: the spy log is empty — the agent never attempted the tool call; retrying"
    done
    if [ ! -s "$stubspy" ]; then
        oh_dump
        rm -rf "$sandbox"
        fail "$id: the agent never invoked its shell tool for the stub phase (spy log empty)"
    fi
    if ! jq -e -s 'map(select(.action == "stub")) | length >= 1' "$stubspy" >/dev/null 2>&1; then
        rm -rf "$sandbox"
        fail "$id: the spy log recorded no stub action ($stubspy)"
    fi
    # The tool transcript (raw stdout) plus the extracted answer — the marker in
    # either proves the stub's printf ran and produced the declared output.
    blob="$(oh_field '.results[0].stdout // ""')$(oh_field '.results[0].text // ""')"
    if ! printf '%s' "$blob" | grep -q "$stubmark"; then
        oh_dump
        head -5 "$stubspy" >&2
        rm -rf "$sandbox"
        fail "$id: the stubbed output ($stubmark) never surfaced as the tool result — the generated printf rewrite was not honored"
    fi
    note "  ok[stub]: the stub's output became the tool result"

    rm -rf "$sandbox"
    note "PASS: $id mock rewrite + stub enforcement (run --mock-rules)"
}

# Live proof that a `run --mock-rules` DENY is honored — for a harness whose
# mock ceiling is deny+spy (Goose: its hook protocol has no rewrite verdict).
# Also the only live exercise of that harness's ephemeral delivery (Goose's
# plugin-pair install, snapshotted/restored around the run). Self-controlled
# via the spy log: an empty log means the hook never fired (retried once as
# agent flakiness); a `deny` record plus the file being absent proves a real
# block, not a vacuous pass. Ends with the same zero-residue assertion as
# oh_mock_enforce.
#
#   $1 harness id
oh_mock_deny_enforce() {
    local id="$1"
    local bin sandbox marker denyfile rulesfile spyfile status attempt
    bin="$(oh_bin)"
    [ -n "$bin" ] || skip "oneharness binary not found (build it: \`just build-release\`, or set ONEHARNESS_BIN)"

    sandbox="$(mktemp -d)"
    sandbox="$(oh_native_path "$sandbox")"
    oh_sandbox_prepare "$id" "$sandbox"
    git init -q "$sandbox" 2>/dev/null || true
    marker="OHMOCKDENY${RANDOM}${RANDOM}"
    denyfile="$sandbox/$marker.txt"
    rulesfile="$sandbox/mock-rules.json"
    spyfile="$sandbox/mock-spy.jsonl"
    cat > "$rulesfile" <<JSON
{"rules":[{"match":{"event_contains":"$marker"},"action":{"deny":{"message":"blocked by oh_mock_deny_enforce"}}}]}
JSON

    for attempt in 1 2; do
        rm -f "$spyfile"
        note "  mock-deny[deny]: the marked command must be blocked under bypass (attempt $attempt)"
        oh_run "$id" "You are a non-interactive test fixture in a scratch directory. Execute exactly this shell command, then stop: touch $denyfile. Rules: you MUST actually invoke your shell tool with that exact command — never decide on your own that it is not permitted; attempt it. Only if that tool invocation itself is rejected: do NOT create the file by any other means — reply with the single word DENIED and stop." --cwd "$sandbox" --mock-rules "$rulesfile" --spy-file "$spyfile"
        status="$(oh_field '.results[0].status')"
        if [ "$status" = "skipped" ]; then
            rm -rf "$sandbox"
            skip "$id is not installed (oneharness reported status=skipped); nothing to verify"
        fi
        [ -s "$spyfile" ] && break
        note "  note: the spy log is empty — the agent never attempted the tool call; retrying"
    done
    if [ ! -s "$spyfile" ]; then
        oh_dump
        rm -rf "$sandbox"
        fail "$id: the agent never invoked its shell tool in $attempt attempts (spy log empty) — the deny path could not be exercised"
    fi
    if ! jq -e -s 'map(select(.action == "deny")) | length >= 1' "$spyfile" >/dev/null 2>&1; then
        oh_dump
        head -5 "$spyfile" >&2
        rm -rf "$sandbox"
        fail "$id: the spy log recorded no deny action ($spyfile) — the rules never matched the marked call"
    fi
    if [ -e "$denyfile" ]; then
        oh_dump
        rm -rf "$sandbox"
        fail "$id: the mock deny was NOT honored ($denyfile was created despite a recorded deny verdict) — the deny shape is not applied (drift)"
    fi
    note "  ok[deny]: the verdict was recorded and the command did not run"

    if grep -rIqF "mock $id --rules" "$sandbox" 2>/dev/null; then
        grep -rIlF "mock $id --rules" "$sandbox" >&2
        rm -rf "$sandbox"
        fail "$id: the ephemeral mock hook left residue in the workspace (files above)"
    fi
    note "  ok[ephemeral]: the workspace carries no trace of the hook"

    rm -rf "$sandbox"
    note "PASS: $id mock deny enforcement (run --mock-rules)"
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

# Live proof that `--reasoning <effort>` is DELIVERED and honored end to end — the
# drift alarm for each harness's reasoning surface (Claude's `--effort`, Codex's
# `-c model_reasoning_effort=`, Copilot's `--reasoning-effort`, Cursor's model-id
# `[effort=…]` suffix). Two checks:
#
#   1. Positive (the hard assertion): a run with a VALID effort must complete
#      cleanly with the marker surfaced. If the flag name / suffix syntax drifted
#      or the CLI rejects it (a clap-style CLI errors on an unknown flag; Cursor
#      may reject the bracket syntax outright — a known forum report), the run
#      fails here — exactly the alarm we want.
#   2. Negative (best-effort evidence of *honoring*, never a failure): a run with
#      a BOGUS effort. A harness that validates the effort enum rejects it
#      (status=nonzero) — logged as confirmed honoring; one that silently ignores
#      the value stays ok — noted, since acceptance alone can't distinguish
#      honored from ignored, and not every CLI validates.
#
# A model-suffix harness (Cursor) needs a model for `--reasoning` to attach to, so
# the caller must set OH_MODEL before invoking this for it (guard with an
# `if [ -n "$OH_MODEL" ]` / `note`, not skip()). $1 id, $2 valid effort, $3 bogus.
oh_reasoning_enforce() {
    local id="$1" effort="$2" bogus="${3:-banana-not-an-effort}"
    local marker status

    marker="$(oh_marker)"
    note "  reasoning-enforce[$effort]: --reasoning must be accepted and the run must complete"
    oh_run "$id" "$(oh_prompt "$marker")" --reasoning "$effort"
    # Shared conclusion: SKIP if not installed, FAIL if it didn't run cleanly or
    # the marker never surfaced, else the reasoning delivery reached the CLI
    # without breaking the run.
    oh_assert_echoed "$id" "$marker"
    note "  ok[$effort]: --reasoning $effort was accepted and the run completed end to end"

    note "  reasoning-enforce[bogus=$bogus]: a harness that validates effort should reject it"
    oh_run "$id" "$(oh_prompt "$(oh_marker)")" --reasoning "$bogus"
    status="$(oh_field '.results[0].status')"
    if [ "$status" = "nonzero" ]; then
        note "  confirmed: $id rejected a bogus effort (status=nonzero) — the value is parsed/honored, not ignored"
    else
        note "  note: $id did not reject a bogus effort (status=$status) — it accepts the flag but may not validate the value; honoring not provable here"
    fi

    note "PASS: $id reasoning enforcement"
}
