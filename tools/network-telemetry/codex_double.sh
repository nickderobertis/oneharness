#!/usr/bin/env bash
# A scripted `codex` for test_codex_rendezvous.py, standing in for Codex
# behind scripts/e2e-codex-rendezvous.sh. It writes its argv (one per line) to
# FAKE_ARGV and its pid to FAKE_PID, then acts as FAKE says:
#   args               exit 0 having bound nothing
#   exit:N             exit N before binding
#   link               link the --listen path to FAKE_SOCKDIR/s, made private
#   dangling           link it to FAKE_SOCKDIR/s, creating nothing there
#   rel                link it relatively to ../rel/s beside its directory
#   direct             bind it directly (a plain file stands for the socket)
#   lock-after         link, then once FAKE_PUBLISHED is a link make the
#                      --listen directory read-only and exit 3
# and, having bound, serves until SIGTERM, which it exits 0 on.
set -euo pipefail

printf '%s\n' "$@" >"$FAKE_ARGV"
echo "$$" >"$FAKE_PID"
listen="" previous=""
for arg in "$@"; do
  [ "$previous" != --listen ] || listen="${arg#unix://}"
  previous="$arg"
done
case "${FAKE:?FAKE must say how this double behaves}" in
  args) exit 0 ;;
  exit:*) exit "${FAKE#exit:}" ;;
esac
dir="$(dirname "$listen")"
trap 'exit 0' TERM
case "$FAKE" in
  link | lock-after)
    [ -d "$FAKE_SOCKDIR" ] || mkdir -m 700 "$FAKE_SOCKDIR"
    : >"$FAKE_SOCKDIR/s"
    ln -s "$FAKE_SOCKDIR/s" "$listen"
    ;;
  dangling) ln -s "$FAKE_SOCKDIR/s" "$listen" ;;
  rel)
    [ -d "$dir/../rel" ] || mkdir -m 700 "$dir/../rel"
    : >"$dir/../rel/s"
    ln -s ../rel/s "$listen"
    ;;
  direct) : >"$listen" ;;
  *)
    echo "codex_double: unknown FAKE=$FAKE" >&2
    exit 2
    ;;
esac
if [ "$FAKE" = lock-after ]; then
  until [ -L "$FAKE_PUBLISHED" ]; do sleep 0.02; done
  chmod 555 "$dir"
  exit 3
fi
while :; do sleep 0.05; done
