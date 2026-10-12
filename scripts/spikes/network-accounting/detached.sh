#!/usr/bin/env bash
# The launch for measurement 6: start `codex app-server --listen unix://SOCK`
# detached — a new session via `setsid -f`, whose forking parent exits at once,
# so the server is reparented away from this script — and return immediately.
# netmeasure.py keeps measuring until that orphan exits (its PID-namespace init
# is the reaper the orphan lands on). Extra args are Codex flags.
set -euo pipefail
sock="$1"; shift
setsid -f codex "$@" app-server --listen "unix://$sock" </dev/null >/dev/null 2>&1
