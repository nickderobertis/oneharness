#!/usr/bin/env bash
# Install Cursor's agent CLI at an exact version.
#
# Cursor's installer (https://cursor.com/install) takes no version: it is
# regenerated per release with that release's version written into its download
# URL and install paths. So the pin is applied to the installer itself — its
# own embedded version is read from its download URL, every occurrence is
# replaced with the pinned one, and the result is checked to download exactly
# the pinned package before it runs. The vendor's install logic (paths,
# symlinks, PATH hints) is kept; only the version it fetches is fixed.
#
# Usage: install-cursor-agent.sh VERSION
#   VERSION  the cursor-agent build, e.g. 2026.11.22-e9042b2 (.harness-cli-versions)
#   CURSOR_INSTALLER_URL  overrides the installer source (tests use a file:// fixture)
#   OH_CURSOR_PRINT_ONLY=1  prints the pinned installer instead of running it
set -euo pipefail

fail() {
  echo "install-cursor-agent: $1" >&2
  echo "install-cursor-agent: ${2:-rerun 'bash scripts/install-cursor-agent.sh VERSION' with the version .harness-cli-versions pins}" >&2
  exit 1
}

[ "$#" -eq 1 ] || fail "expected exactly one argument, the version; got $#"
pin="$1"
# A version reaches a URL and filesystem paths, so it is held to Cursor's
# build-id shape and nothing else.
[[ "$pin" =~ ^[0-9]{4}\.[0-9]{2}\.[0-9]{2}-[0-9a-f]{7,40}$ ]] ||
  fail "'$pin' is not a Cursor build id (YYYY.MM.DD-<hex>)" "pass the cursor-agent version .harness-cli-versions pins"

url="${CURSOR_INSTALLER_URL:-https://cursor.com/install}"
installer="$(curl -fsS "$url")" ||
  fail "could not fetch Cursor's installer from $url" "check the network and that $url still serves the installer, then rerun"

pattern='https://downloads\.cursor\.com/lab/([0-9]{4}\.[0-9]{2}\.[0-9]{2}-[0-9a-f]{7,40})/'
[[ "$installer" =~ $pattern ]] ||
  fail "Cursor's installer at $url no longer downloads from https://downloads.cursor.com/lab/<version>/" \
    "read the current installer and update this script's pinning to its new shape"
embedded="${BASH_REMATCH[1]}"
pinned="${installer//"$embedded"/"$pin"}"

# Every download the pinned installer could make must be the pin's: a second
# build anywhere in it would install something other than what was asked for.
rest="$pinned"
while [[ "$rest" =~ https://downloads\.cursor\.com/lab/([^/[:space:]\"\']*)/ ]]; do
  [ "${BASH_REMATCH[1]}" = "$pin" ] ||
    fail "Cursor's installer at $url also downloads build ${BASH_REMATCH[1]}, which pinning $pin leaves in place" \
      "read the current installer; pin it only once it downloads a single build"
  rest="${rest#*"${BASH_REMATCH[0]}"}"
done

if [ "${OH_CURSOR_PRINT_ONLY:-0}" = 1 ]; then
  printf '%s\n' "$pinned"
  exit 0
fi
status=0
bash -o pipefail -c "$pinned" || status=$?
[ "$status" -eq 0 ] || fail "Cursor's installer failed installing $pin with exit $status (its output, if any, is above)" \
  "check that https://downloads.cursor.com/lab/$pin/ still serves this platform's package"
