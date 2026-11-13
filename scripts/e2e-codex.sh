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

# The effective uid the shared server's Codex runs as, in its own user
# namespace: on Linux read from the `codex` process carrying the pool record's
# server marker, through that process's uid_map (the network recorder runs the
# launch in pasta's namespace, where this user is root); elsewhere this user's.
#   $1 the record's server marker
_oh_codex_server_euid() {
  local marker="$1" env pid exe outer
  if [ "$(uname -s)" != Linux ]; then
    id -u
    return
  fi
  for env in /proc/[0-9]*/environ; do
    pid="${env#/proc/}" pid="${pid%/environ}"
    exe="$(readlink "/proc/$pid/exe" 2>/dev/null)" || continue
    [ "${exe##*/}" = codex ] || continue
    tr '\0' '\n' 2>/dev/null <"$env" | grep -qxF "ONEHARNESS_APP_SERVER_MARKER=$marker" || continue
    outer="$(awk '/^Uid:/ { print $3 }' "/proc/$pid/status" 2>/dev/null)" && [ -n "$outer" ] || continue
    awk -v u="$outer" '$2 <= u && u < $2 + $3 { print $1 + u - $2; found = 1; exit } END { exit !found }' \
      "/proc/$pid/uid_map" 2>/dev/null && return
  done
  return 1
}

# The drift gate for Codex's socket rendezvous, which the shared-server launch
# tightens before Codex starts (stated once as `codex_daemon_dir` and
# `CODEX_DAEMON_DIR_MODE` in crates/oneharness-core/src/domain/app_server.rs):
# a pooled codex server must leave its --listen path linking into exactly that
# directory for the euid it runs as, at the mode Codex accepts. Read from the
# server's own process, so it holds under the recorder's namespace as well as
# outside it.
#
# The reading is taken while the controlled turn runs, under the pool entry's
# lock (`.lock`, the flock every start and stop of its server takes): a server
# seen running and bound under that lock cannot be stopped until it is
# released, however short its linger. Read after the turn instead, it raced
# the linger — a server the pool had already stopped, or was stopping, left
# nothing to check.

# Check pool entry $1's server against the rendezvous, $2 the canonical /tmp,
# holding the entry's lock: 0 once checked, 1 when it names no running, bound
# server (it has stopped, or not bound yet). A drift fails the suite.
_oh_codex_rendezvous_entry() {
  local record="$1/server.json" link="$1/sock/app-server.sock" tmp="$2" fields pid marker euid target bound want mode
  # The rendezvous as the Rust source states it, held there by core's
  # `the_live_rendezvous_check_expects_the_rendezvous_the_launch_prepares`.
  local rendezvous_prefix=codex-daemon- rendezvous_mode=700
  [ -f "$record" ] && [ -L "$link" ] || return 1
  fields="$(jq -r '"\(.pid // "") \(.marker // "")"' "$record")" ||
    fail "codex rendezvous: cannot read the pool record $record (jq's error above); a damaged record stops the pool too, so remove that entry's directory and rerun"
  read -r pid marker <<<"$fields"
  case "${pid:-}" in '' | 0 | *[!0-9]*)
    fail "codex rendezvous: the pool record $record names pid '${pid:-}', not a process id; remove that entry's directory and rerun"
    ;;
  esac
  # A killed server can leave its record and link behind; only a running one
  # says where Codex binds now.
  ps -p "$pid" >/dev/null 2>&1 || return 1
  [ -n "$marker" ] ||
    fail "codex rendezvous: the pool record $record names no server marker, which every shared codex server is launched with; remove that entry's directory and rerun"
  target="$(readlink "$link")" ||
    fail "codex rendezvous: cannot read the socket link $link (readlink's error above); check the entry's permissions with \`ls -la $(dirname "$link")\`"
  euid="$(_oh_codex_server_euid "$marker")" ||
    fail "codex rendezvous: no running codex process carries the server marker in $record, though the pool, whose lock this check holds, has not stopped it; check \`ps -ef | grep app-server\` and that entry's server.log for a server that died on its own"
  want="$tmp/$rendezvous_prefix$euid"
  bound="$(cd "$(dirname "$target")" && pwd -P)" ||
    fail "codex rendezvous: the socket link $link names $target, whose directory cannot be entered (cd's error above); a server that is still running holds its socket there, so check that directory's owner and mode with \`ls -ld $(dirname "$target")\`"
  [ "$bound" = "$want" ] ||
    fail "codex rendezvous: the shared server runs as euid $euid and bound its socket under $bound, not $want; Codex has moved its rendezvous, so update codex_daemon_dir in crates/oneharness-core/src/domain/app_server.rs, the one source the launch and the mock both read, to it"
  # Probe the stat dialect rather than the OS: GNU's (Linux, Git Bash) takes
  # -c, BSD's (macOS) takes -f.
  if stat -c %a / >/dev/null 2>&1; then
    mode="$(stat -c %a "$bound")"
  else
    mode="$(stat -f %Lp "$bound")"
  fi || fail "codex rendezvous: cannot read the mode of $bound (stat's error above); run \`ls -ld $bound\` to see it"
  [ "$mode" = "$rendezvous_mode" ] ||
    fail "codex rendezvous: $bound is mode $mode, not the 0$rendezvous_mode Codex demands of it; read Codex's refusal for what it now requires"
}

# Run "$@" holding pool entry $1's lock, the advisory flock the pool's
# EntryLock takes (fs2's lock_exclusive). The lock belongs to fd 9's open file,
# so it outlives perl, which only takes it, and goes when fd 9 is closed.
_oh_pool_entry_locked() {
  local entry="$1" status=0
  shift
  exec 9>>"$entry/.lock" || fail "codex rendezvous: cannot open the pool entry's lock $entry/.lock (the error above); check that entry's permissions with \`ls -la $entry\`"
  perl -MFcntl=:flock -e 'open(my $lock, ">>&=", 9) or die "cannot reopen fd 9: $!\n"; flock($lock, LOCK_EX) or die "cannot lock it: $!\n"' ||
    fail "codex rendezvous: cannot take the pool entry's lock $entry/.lock (perl's error above); check that \`perl -MFcntl=:flock -e 1\` runs and that the state directory's filesystem supports flock"
  "$@" || status=$?
  exec 9>&-
  return "$status"
}

# Whether process $1 is running: a zombie is not. The suite's own parent reaps
# it only once every holder of its output, this watch included, has let go.
_oh_alive() {
  local stat
  stat="$(ps -o stat= -p "$1" 2>/dev/null)" || return 1
  case "$stat" in *Z*) return 1 ;; esac
}

# Watch the pool through the controlled turn until one running, bound server
# has been checked. Once $1 exists the turn is over: one last look, and a fail
# when nothing was there to check. A suite that died ($2, its pid, gone) ends
# the watch: it holds the suite's output open, which its parent waits on.
oh_codex_rendezvous_check() {
  local over="$1" main="$2" state="${XDG_STATE_HOME:-$HOME/.local/state}" tmp entry ended checked=0
  tmp="$(cd /tmp && pwd -P)" || fail "codex rendezvous: cannot resolve /tmp; check that it exists"
  while :; do
    ended=0
    [ ! -e "$over" ] || ended=1
    for entry in "$state"/oneharness/servers/codex-app-server-*; do
      # Unlocked first, so the watch never queues behind a start for nothing.
      [ -f "$entry/server.json" ] && [ -L "$entry/sock/app-server.sock" ] || continue
      if _oh_pool_entry_locked "$entry" _oh_codex_rendezvous_entry "$entry" "$tmp"; then
        checked=$((checked + 1))
      fi
    done
    [ "$checked" -eq 0 ] || return 0
    [ "$ended" -eq 0 ] ||
      fail "codex rendezvous: no pooled codex server under $state/oneharness/servers was running with its --listen path linked to a socket while the controlled turn ran, so nothing could be checked; if the turn passed, list the entry's sock/ directory to see how Codex now binds that path"
    _oh_alive "$main" || return 0
    sleep 0.05
  done
}

_oh_codex_rendezvous_scratch=""
_oh_codex_rendezvous_cleanup() {
  [ -z "$_oh_codex_rendezvous_scratch" ] || rm -rf "$_oh_codex_rendezvous_scratch" ||
    note "  could not remove the scratch directory $_oh_codex_rendezvous_scratch (rm's error above) — delete it by hand"
}

# The scratch directory goes however the suite ends: a failed turn exits it
# from inside oh_controlled_turn, and the watch, in a subshell, cannot know
# whether the suite will still need it.
oh_codex_controlled_turn() {
  local flags witness status=0
  need perl
  flags="$(mktemp -d "${TMPDIR:-/tmp}/oh-codex-rendezvous.XXXXXX")" || fail "codex rendezvous: could not create a scratch directory — check that the host temp dir is writable"
  _oh_codex_rendezvous_scratch="$flags"
  trap _oh_codex_rendezvous_cleanup EXIT
  oh_codex_rendezvous_check "$flags/turn-over" "$$" &
  witness=$!
  oh_controlled_turn codex
  : >"$flags/turn-over" ||
    fail "codex rendezvous: cannot mark the controlled turn over in $flags (the error above); check that the host temp dir is writable"
  wait "$witness" || status=$?
  trap - EXIT
  _oh_codex_rendezvous_cleanup
  _oh_codex_rendezvous_scratch=""
  [ "$status" -eq 0 ] || exit "$status"
}

export OH_MODEL="${CODEX_E2E_MODEL:-}"
# Every codex launch below is measured into target/e2e-telemetry/network/codex.jsonl,
# which `just live-codex` then holds to live/pr-matrix/harness/codex/budgets.yaml.
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
  oh_codex_controlled_turn
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
