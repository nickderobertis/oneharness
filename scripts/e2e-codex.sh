#!/usr/bin/env bash
# Live e2e: drive the real OpenAI Codex CLI through oneharness and assert the
# JSON contract. Auth: an existing codex login, else OPENAI_API_KEY. Model:
# $CODEX_E2E_MODEL (default: the CLI's own default).
set -euo pipefail
# shellcheck source=scripts/e2e-lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/e2e-lib.sh"

note "== oneharness live e2e: codex =="
need jq
# Codex authenticates from its own login (CI makes one from OPENAI_API_KEY with
# `codex login --with-api-key`), so an existing login is auth enough.
if [ ! -f "${CODEX_HOME:-$HOME/.codex}/auth.json" ]; then
  need_env "OpenAI auth" OPENAI_API_KEY
fi

# The drift gate for Codex's socket rendezvous, which the shared-server launch
# tightens before Codex starts (stated once as `codex_daemon_dir` and
# `CODEX_DAEMON_DIR_MODE` in crates/oneharness-core/src/domain/app_server.rs):
# every shared codex server must link its --listen path into exactly that
# directory for the euid it runs as, at the mode Codex accepts.
#
# It judges what scripts/e2e-codex-rendezvous.sh, in front of every codex
# launch, recorded as each server bound — before any dispatch could reach the
# server, so before the pool could stop it. A reading of the live server after
# its turn raced its linger: one the pool had stopped, or was stopping, left
# nothing to read.
_oh_codex_rendezvous_dir=""
_oh_codex_rendezvous_cleanup() {
  [ -z "$_oh_codex_rendezvous_dir" ] || rm -rf "$_oh_codex_rendezvous_dir" ||
    note "  could not remove the scratch directory $_oh_codex_rendezvous_dir (rm's error above) — delete it by hand"
}

# Put the rendezvous wrapper first on PATH as `codex`, before
# oh_telemetry_setup resolves the codex its shim runs, so the wrapper runs
# inside the recorder's namespace, where Codex does. Its scratch directory goes
# however the suite ends.
oh_codex_rendezvous_setup() {
  local real dir
  real="$(_oh_unshimmed codex)" || return 0 # no codex: the first turn reports it
  dir="$(mktemp -d "${TMPDIR:-/tmp}/oh-codex-rendezvous.XXXXXX")" ||
    fail "codex rendezvous: could not create a scratch directory — check that the host temp dir is writable"
  _oh_codex_rendezvous_dir="$dir"
  trap _oh_codex_rendezvous_cleanup EXIT
  # llmlint: ignore[changed_behavior_has_e2e] mktemp has just made this directory, private to this user, so writing the evidence file and the wrapper in it fails only on a full or read-only filesystem, which no unprivileged journey can stage between those lines; the directory's own failure is test_the_rendezvous_setup_names_a_temp_dir_it_cannot_use's.
  : >"$dir/rendezvous.jsonl" || fail "codex rendezvous: cannot create $dir/rendezvous.jsonl; check that the host temp dir is writable"
  { printf '#!/usr/bin/env bash\nexec bash %q --real %q --evidence %q -- "$@"\n' \
    "$OH_REPO_ROOT/scripts/e2e-codex-rendezvous.sh" "$real" "$dir/rendezvous.jsonl" >"$dir/codex" &&
    chmod +x "$dir/codex"; } || fail "codex rendezvous: cannot write the wrapper $dir/codex; check that the host temp dir is writable"
  PATH="$dir:$PATH"
  export PATH
}

oh_codex_rendezvous_check() {
  local evidence="$_oh_codex_rendezvous_dir/rendezvous.jsonl" tmp rows listen euid linked target bound want mode
  # The rendezvous as the Rust source states it, held there by core's
  # `the_live_rendezvous_check_expects_the_rendezvous_the_launch_prepares`.
  local rendezvous_prefix=codex-daemon- rendezvous_mode=700
  tmp="$(cd /tmp && pwd -P)" || fail "codex rendezvous: cannot resolve /tmp; check that it exists"
  rows="$(jq -r '[.listen, .euid, .linked, .target, .bound, .mode] | @tsv' "$evidence")" ||
    fail "codex rendezvous: cannot read the rendezvous evidence $evidence (jq's error above); each line is one scripts/e2e-codex-rendezvous.sh wrote, so a damaged one is that wrapper's to fix"
  [ -n "$rows" ] ||
    fail "codex rendezvous: no shared codex server started through scripts/e2e-codex-rendezvous.sh during the controlled turn, so nothing could be checked; a server started before this suite may have served it, so let it linger out (60 s) and rerun"
  while IFS=$'\t' read -r listen euid linked target bound mode; do
    [ "$linked" = true ] ||
      fail "codex rendezvous: the shared server bound $target itself rather than linking that --listen path into its rendezvous directory, so where Codex binds cannot be checked; list $(dirname "$listen") to see how Codex now binds a long path"
    want="$tmp/$rendezvous_prefix$euid"
    [ "$bound" = "$want" ] ||
      fail "codex rendezvous: the shared server runs as euid $euid and bound its socket under $bound, not $want; Codex has moved its rendezvous, so update codex_daemon_dir in crates/oneharness-core/src/domain/app_server.rs, the one source the launch and the mock both read, to it"
    [ "$mode" = "$rendezvous_mode" ] ||
      fail "codex rendezvous: $bound is mode $mode, not the 0$rendezvous_mode Codex demands of it; read Codex's refusal for what it now requires"
  done <<<"$rows"
}

export OH_MODEL="${CODEX_E2E_MODEL:-}"
# Every codex launch below is measured into target/e2e-telemetry/network/codex.jsonl,
# which `just live-codex` then holds to live/pr-matrix/harness/codex/budgets.yaml.
[ "${OS:-}" = "Windows_NT" ] || oh_codex_rendezvous_setup
oh_telemetry_setup codex
marker="$(oh_marker)"
oh_run codex "$(oh_prompt "$marker")"
oh_assert_echoed codex "$marker"

# The network budget's launch kinds, one phase each, first: the exec turn
# above, a controlled turn over the stdio app-server, and the usage probe.
if [ "${OS:-}" = "Windows_NT" ]; then
  note "» controlled turn: not run on Windows (the unix legs carry it)"
else
  # llmlint: ignore[tool_output_is_signal] Every phase in this script announces itself on one line before it runs, and that header is what attributes a later failure (or a hang) to a phase in the CI log.
  note "» controlled turn: a --control turn must round-trip over codex's stdio app-server"
  oh_controlled_turn codex
  oh_codex_rendezvous_check
fi

# Usage enforcement: the app-server answers `account/rateLimits/read`
# asynchronously and shuts down on stdin EOF, so the probe holds stdin open until
# its answer lands. Whatever that answer is — headroom on a ChatGPT login, an
# auth error under this suite's API key — it must be an answer.
# llmlint: ignore[tool_output_is_signal] Every phase in this script announces itself on one line before it runs, and that header is what attributes a later failure (or a hang) to a phase in the CI log; dropping it here alone would make this one phase the unlabelled one.
note "» usage: the zero-turn probe must get an answer out of the real app-server"
oh_usage_enforce codex

# Synced permission rules, honored by Codex started DIRECTLY (not through
# `oneharness run`): `sync` writes .codex/rules/oneharness.rules into a trusted
# scratch project, and a synced deny must refuse a command that ran before it,
# and a synced allow must run a command that was refused before it.
# llmlint: ignore[tool_output_is_signal] Every phase in this script announces itself on one line before it runs, and that header is what attributes a later failure (or a hang) to a phase in the CI log; dropping it here alone would make this one phase the unlabelled one.
note "» synced rules: a directly started codex must honor .codex/rules/oneharness.rules"
oh_codex_rules_enforce
# Model-free: the synced rules match the argv codex checks on every platform —
# on Windows the words it lowers a `pwsh -Command` script into.
# llmlint: ignore[tool_output_is_signal] Every phase in this script announces itself on one line before it runs, and that header is what attributes a later failure to a phase in the CI log.
note "» synced rules: codex's own execpolicy must match them against the POSIX and Windows argv"
oh_codex_rules_match

# Long-session settings (domain::lean's translation table): one session takes
# every tool restriction codex has (reporting `tool_set` as partial), sees an
# attached image, reports per-call usage, compacts below
# --auto-compact-tokens, compacts on `oneharness compact` (one stdio
# app-server launch), and still recalls its first turn afterwards.
oh_lean_session_enforce codex

# Large prompt + system (issue #1115): oneharness pipes a >128 KiB prompt to
# `codex exec -` (stdin sentinel), with the system prepended into that stream —
# so it never trips the argv ceiling. The marker must still round-trip.
note "» long prompt: a >128 KiB prompt+system must round-trip off the argv"
oh_long_prompt_enforce codex

# Approval-mode enforcement: `read-only` is Codex's OS-enforced read-only
# sandbox, and `plan` is that same sandbox plus a prepended plan instruction —
# each must block a write that `--mode bypass` allows.
note "» read-only / plan enforcement: each must block a write"
oh_mode_enforce codex read-only
oh_mode_enforce codex plan

# Resumed turns keep the mode's sandbox (issue #1372): `codex exec resume` has
# no `--sandbox`, so a continued turn carries it as `-c sandbox_mode=`. Two
# `--session` turns per mode; turn two must run, recall turn one, and write
# (auto) or be blocked (read-only / plan) exactly as a fresh turn would.
# llmlint: ignore[tool_output_is_signal] Every phase in this script announces itself on one line before it runs, and that header is what attributes a later failure (or a hang) to a phase in the CI log; dropping it here alone would make this one phase the unlabelled one.
note "» resume under a sandbox mode: a continued turn must run and keep the sandbox"
oh_resume_mode_enforce codex auto
oh_resume_mode_enforce codex read-only
oh_resume_mode_enforce codex plan
# A resumed turn whose model stream stalls once is recovered by codex inside
# the resume phases' bound: real codex and its built-in provider against a local
# endpoint that stalls one response, so no model call is billed.
# llmlint: ignore[tool_output_is_signal] Every phase in this script announces itself on one line before it runs, and that header is what attributes a later failure (or a hang) to a phase in the CI log; dropping it here alone would make this one phase the unlabelled one.
note "» resume stall: codex must recover a stalled model stream inside the resume bound"
oh_resume_stall_enforce codex
# Model-free: every spelling of a config override oneharness restates the mode
# after is one codex applies, last one winning.
# llmlint: ignore[tool_output_is_signal] Every phase in this script announces itself on one line before it runs, and that header is what attributes a later failure (or a hang) to a phase in the CI log; dropping it here alone would make this one phase the unlabelled one.
note "» config overrides: codex must apply every -c spelling as one last-wins list"
oh_codex_config_override_order
# Model-free: oneharness starts codex with `-c features.plugins=false`, which
# a caller's own `-c features.plugins=true` or `--enable plugins` must override.
# llmlint: ignore[tool_output_is_signal] Every phase in this script announces itself on one line before it runs, and that header is what attributes a later failure (or a hang) to a phase in the CI log; dropping it here alone would make this one phase the unlabelled one.
note "» plugins flags: a caller's opt-in must override oneharness's plugins-off default"
oh_codex_feature_flag_order
# A caller's own `-c sandbox_mode=` must not outrank the mode on a resumed turn
# any more than it does on a fresh one, where `--sandbox` beats it.
oh_resume_mode_enforce codex auto -- -c sandbox_mode=read-only

# Mock enforcement: codex's hooks engine loads project .codex/hooks.json under
# `exec` and honors the claude-nested `updatedInput` rewrite — but only when
# the invocation opts in with `-c features.hooks=true` plus the per-run hook
# trust bypass (probe-verified 2026-07-06; the `projects.<dir>.trust_level`
# config route loads no hooks). `run --mock-rules` appends those flags itself
# and restores the created .codex/hooks.json afterwards.
note "» mock enforcement: run --mock-rules must rewrite a marked command's input"
oh_mock_enforce codex

# Normalized tool events: Codex's default text has no transcript, so `--events`
# upgrades it to `exec --json`, whose `command_execution` items normalize to a
# `tool_call` via `json:codex-items` — the live drift alarm for the codex
# recognizer (sourced from a real `codex exec --json` transcript).
note "» events: a tool-using turn must surface normalized tool_call events (--events)"
oh_events_assert codex "json:codex-items" --events

# Streaming: the same events must arrive incrementally under --stream (with
# --events selecting exec --json), then a terminal result line.
note "» stream: events must arrive incrementally, then a terminal result line"
oh_stream_assert codex --events

# Reasoning enforcement: `--reasoning high` maps to Codex's
# `-c model_reasoning_effort=high` and must round-trip; a bogus effort should be
# rejected (honoring evidence).
note "» reasoning: --reasoning must be accepted (and a bogus effort rejected)"
oh_reasoning_enforce codex high

# The controlled turn's shared app-server is measured over its whole life, so
# its line lands once its linger has passed: wait for it before the budgets.
oh_wait_pooled_servers
