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

# Each failure is the test's scratch directory, which the test owns.
refuse() {
  echo "codex_double: $*; check the test's scratch directory is writable, then rerun test_codex_rendezvous.py" >&2
  exit 2
}

printf '%s\n' "$@" >"$FAKE_ARGV" || refuse "cannot write its argv to FAKE_ARGV=$FAKE_ARGV"
echo "$$" >"$FAKE_PID" || refuse "cannot write its pid to FAKE_PID=$FAKE_PID"
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
# Codex's socket at $1, made with $2 its directory, and the --listen path
# linked to it as $3 says.
socket() {
  { [ -d "$2" ] || mkdir -m 700 "$2"; } || refuse "cannot create the socket directory $2"
  : >"$1" || refuse "cannot create the socket stand-in $1"
  ln -s "$3" "$listen" || refuse "cannot link $listen to $3"
}
case "$FAKE" in
  link | lock-after) socket "$FAKE_SOCKDIR/s" "$FAKE_SOCKDIR" "$FAKE_SOCKDIR/s" ;;
  dangling) ln -s "$FAKE_SOCKDIR/s" "$listen" || refuse "cannot link $listen to $FAKE_SOCKDIR/s" ;;
  rel) socket "$dir/../rel/s" "$dir/../rel" ../rel/s ;;
  direct) : >"$listen" || refuse "cannot bind $listen" ;;
  *) refuse "unknown FAKE=$FAKE; it is one of the behaviours listed at the top of this file" ;;
esac
if [ "$FAKE" = lock-after ]; then
  until [ -L "$FAKE_PUBLISHED" ]; do sleep 0.02; done
  chmod 555 "$dir" || refuse "cannot make $dir read-only"
  exit 3
fi
while :; do sleep 0.05; done
