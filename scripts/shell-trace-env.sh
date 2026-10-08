# shellcheck shell=bash
# The BASH_ENV every bash a kcov-measured shell test step starts reads first
# (scripts/shell-test.sh puts it there, in place of kcov's own helper): turn on
# the line trace kcov collects, in kcov's own `kcov@<file>@<line>@` format.
#
# Two differences from kcov's helper, each a way its tracing changed what the
# test observed. It traces only when kcov's trace descriptor is still open
# here, pointing BASH_XTRACEFD at it itself (unexported, so no child inherits
# a descriptor it may not have): a bash started through a process that closed
# it (Node, Rust and just spawn with stdio alone) would otherwise write its
# trace to stderr, which the tests read. And its PS4 survives `set -u` from
# the command line (`bash -eu -c`, as just runs a recipe), where a `bash -c`
# has no BASH_SOURCE to expand. Every expansion here must hold under -e and -u
# for that reason.
# llmlint: ignore[robust_shell] This file is a BASH_ENV hook sourced into every bash a measured test step starts, before that bash's own script: `set -euo pipefail` here would impose those options on every script and `bash -c` under test, changing the very behavior the step measures. Each script sets its own options; this hook keeps to expansions that hold under any of them.
if [[ "${KCOV_BASH_XTRACEFD:-}" =~ ^[0-9]+$ ]] && { : >&"$KCOV_BASH_XTRACEFD"; } 2>/dev/null; then
  BASH_XTRACEFD="$KCOV_BASH_XTRACEFD"
  PS4='kcov@${BASH_SOURCE:-}@${LINENO}@'
  set -x
fi
