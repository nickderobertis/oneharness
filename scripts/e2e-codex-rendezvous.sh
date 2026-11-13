#!/usr/bin/env bash
# The launch wrapper scripts/e2e-codex.sh puts in front of codex, so its drift
# gate on Codex's socket rendezvous judges evidence that outlives the server.
#
#   e2e-codex-rendezvous.sh --real CODEX --evidence FILE -- ARGS...
#
# Every launch but a shared app-server's (`--listen unix://PATH`) is CODEX,
# exec'd. A shared server is started on a private path in PATH's directory, of
# PATH's length, so Codex binds it as it would PATH. Once Codex has, one JSON
# line goes to FILE: where its link points, the directory that resolves to and
# its mode, and the euid Codex runs as — this process's, since Codex is its
# child in the same namespace (the network recorder's, when it measures).
# Only then is PATH linked to the same socket. The pool's dispatch dials PATH,
# so no turn can start, and no lease end and let the pool stop the server,
# before the evidence is written.
#
# Exit status: Codex's own, except 125, which is this wrapper failing (its
# usage, or recording or publishing the evidence), said on stderr — for a
# shared server, the pool entry's server.log.
set -euo pipefail

child=""
die() {
  printf 'e2e-codex-rendezvous: %s\n' "$*" >&2
  [ -z "$child" ] || kill -TERM "$child" 2>/dev/null || true
  exit 125
}

real="" evidence=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --real | --evidence)
      [ "$#" -ge 2 ] || die "$1 needs a value; scripts/e2e-codex.sh writes this wrapper's codex with --real, --evidence and --"
      case "$1" in
        --real) real="$2" ;;
        --evidence) evidence="$2" ;;
      esac
      shift 2
      ;;
    --)
      shift
      break
      ;;
    *) die "unknown option '$1'; scripts/e2e-codex.sh writes this wrapper's codex with --real, --evidence and --" ;;
  esac
done
[ -n "$real" ] && [ -n "$evidence" ] ||
  die "--real and --evidence are both required; rerun scripts/e2e-codex.sh, which writes them"

args=("$@")
at=-1
for i in "${!args[@]}"; do
  if [ "${args[$i]}" = --listen ] && [ "$((i + 1))" -lt "${#args[@]}" ]; then
    at=$((i + 1))
  fi
done
[ "$at" -ge 0 ] || exec "$real" "$@"
case "${args[$at]}" in
  unix://*) ;;
  *) exec "$real" "$@" ;;
esac
path="${args[$at]#unix://}"
dir="$(dirname "$path")"
base="$(basename "$path")"
# Only an absolute path in an existing directory is the pool's; anything else
# is Codex's to refuse, so it gets the launch as given.
case "$path" in /*[!/]) ;; *) exec "$real" "$@" ;; esac
[ -d "$dir" ] && [ ! -d "$path" ] || exec "$real" "$@"
private="$dir/.${base#?}"
[ "$private" != "$path" ] || private="$dir/,${base#?}"
rm -f "$private" || die "cannot clear the stale private path $private (rm's error above); check the permissions of $dir"
args[at]="unix://$private"

"$real" "${args[@]}" &
child=$!
# A stop reaches Codex itself too (every process carrying the server marker);
# passing it on just keeps this wrapper from outliving its child.
trap 'kill -TERM "$child" 2>/dev/null || true' TERM INT HUP

# Nothing can reach the server until PATH exists, so waiting here races only
# Codex itself.
until [ -e "$private" ] || [ -L "$private" ]; do
  if ! kill -0 "$child" 2>/dev/null; then
    rc=0
    wait "$child" || rc=$?
    exit "$rc"
  fi
  sleep 0.02
done

euid="$(id -u)" || die "cannot read the euid this server runs as (id's error above); check that \`id -u\` runs in the recorder's namespace (scripts/network-recorder-preflight.sh)"
linked=false target="$private" bound="" mode="" perms="" cell=0 digit=0 special=0
if [ -L "$private" ]; then
  linked=true
  target="$(readlink "$private")" ||
    die "cannot read the link Codex left at $private (readlink's error above); check its permissions with \`ls -la $dir\`"
  # A relative target is relative to the link's directory, PATH's too.
  bound="$(cd "$dir" && cd "$(dirname "$target")" && pwd -P)" ||
    die "cannot enter $(dirname "$target"), the directory Codex bound its socket in (cd's error above); check its owner and mode with \`ls -ld $(dirname "$target")\`"
  # The octal mode from `ls -ld`'s permission string, which GNU and BSD print
  # alike, where stat's flags differ, spelled as GNU `stat -c %a` spells it:
  # setuid, setgid and sticky (s/S, s/S, t/T in the execute cells) lead as a
  # fourth digit when any is set, so 1700, 2700 or 4700 is never read as 700.
  perms="$(ls -ld "$bound")" || die "cannot read the mode of $bound (ls's error above); check its owner with \`ls -ld $bound\`"
  special=0
  for cell in 1 4 7; do
    digit=0
    [ "${perms:cell:1}" != r ] || digit=$((digit + 4))
    [ "${perms:cell+1:1}" != w ] || digit=$((digit + 2))
    case "${perms:cell+2:1}" in x | s | t) digit=$((digit + 1)) ;; esac
    case "$cell:${perms:cell+2:1}" in
      1:[sS]) special=$((special + 4)) ;;
      4:[sS]) special=$((special + 2)) ;;
      7:[tT]) special=$((special + 1)) ;;
    esac
    mode="$mode$digit"
  done
  [ "$special" -eq 0 ] || mode="$special$mode"
fi
line="$(jq -nc --arg listen "$path" --arg euid "$euid" --argjson linked "$linked" --arg target "$target" \
  --arg bound "$bound" --arg mode "$mode" \
  '{listen: $listen, euid: $euid, linked: $linked, target: $target, bound: $bound, mode: $mode}')" ||
  die "cannot encode the rendezvous evidence (jq's error above); check that \`jq --version\` runs"
printf '%s\n' "$line" >>"$evidence" || die "cannot append the rendezvous evidence to $evidence; check that its directory is writable"

{ ln -s "$target" "$path.$$.partial" && mv -f "$path.$$.partial" "$path"; } ||
  die "cannot link $path to the socket at $target (the error above); check the permissions of $dir"

rc=0
while :; do
  rc=0
  wait "$child" || rc=$?
  kill -0 "$child" 2>/dev/null || break
done
rm -f "$private" || printf 'e2e-codex-rendezvous: could not remove %s; delete it by hand\n' "$private" >&2
exit "$rc"
