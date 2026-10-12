#!/usr/bin/env bash
# Fetch pasta (passt) without root: `apt-get download` needs no privilege and
# `dpkg -x` unpacks into a directory we own. Prints the pasta path.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tools="${NETMEASURE_TOOLS:-$here/.tools}"
pasta="$tools/root/usr/bin/pasta"
if [ ! -x "$pasta" ]; then
  mkdir -p "$tools/debs"
  (cd "$tools/debs" && apt-get download passt >/dev/null)
  for deb in "$tools"/debs/*.deb; do dpkg -x "$deb" "$tools/root"; done
fi
printf '%s\n' "$pasta"
