#!/usr/bin/env bash
# Hold the workflows the required checks run to `.harness-cli-versions`: every
# `npm install -g` in them names an exact version, that version is the pin, and
# every Goose vendor install passes its pin. An unpinned install there fails a
# passing tree whenever a publisher is mid-release (Copilot 1.0.115's package
# landed before its darwin-arm64 platform package and failed
# `check (macos-latest)`), so a floating version is a finding, not a style.
#
# Usage: check-harness-cli-pins.sh [PINS_FILE [WORKFLOW...]]
# Defaults: .harness-cli-versions and .github/workflows/ci.yml.
set -euo pipefail

cd "$(dirname "$0")/.." || {
  echo "check-harness-cli-pins: cannot enter the repository root above $0; run it as 'bash scripts/check-harness-cli-pins.sh' from a checkout" >&2
  exit 1
}

pins="${1:-.harness-cli-versions}"
if [ "$#" -gt 1 ]; then
  shift
  workflows=("$@")
else
  workflows=(.github/workflows/ci.yml)
fi

fail() {
  echo "check-harness-cli-pins: $1" >&2
  echo "check-harness-cli-pins: ${2:-pin every harness CLI install to $pins, then rerun 'bash scripts/check-harness-cli-pins.sh'}" >&2
  exit 1
}

[ -f "$pins" ] || fail "no pin file at $pins"
for workflow in "${workflows[@]}"; do
  [ -f "$workflow" ] || fail "no workflow at $workflow"
done

# One awk pass: read the pins (exactly one x.y.z per package, each once), then
# every workflow line, with `\` continuations joined so a package on the next
# line is still read. Each finding is printed as `file:line: message`; the pins
# no workflow installs are reported once the workflows are read, since a pin
# nothing reads is one nobody moves.
findings="$(
  awk -v pins_file="$pins" '
    function finding(where, msg) { print where ": " msg; bad = 1 }
    # Every `npm install -g` on the line is read, not only the first: a shell
    # separator ends one install, and the scan carries on to find the next.
    function check_npm(where, text,    n, tok, i, j, t, at, pkg, ver, in_install) {
      # Separators become tokens of their own, glued (`a;npm`) or not.
      gsub(/&&|\|\||;|\|/, " & ", text)
      n = split(text, tok, /[ \t]+/)
      in_install = 0
      for (i = 1; i <= n; i++) {
        t = tok[i]
        if (!in_install) {
          if (t == "npm" && (tok[i + 1] == "install" || tok[i + 1] == "i") &&
              (tok[i + 2] == "-g" || tok[i + 2] == "--global")) {
            in_install = 1
            i += 2
          }
          continue
        }
        if (t ~ /^(&&|\|\||;|\||>|2>|<)/) { in_install = 0; continue }
        if (t == "" || t ~ /^-/) continue
        # A scoped package starts with @, so the version separator is the
        # LAST @ past position one.
        at = 0
        for (j = length(t); j > 1; j--) if (substr(t, j, 1) == "@") { at = j; break }
        pkg = at ? substr(t, 1, at - 1) : t
        ver = at ? substr(t, at + 1) : ""
        if (!(pkg in pin))
          finding(where, "npm installs " t ", which " pins_file " does not pin")
        else {
          installed[pkg] = 1
          if (ver == "")
            finding(where, "npm installs " pkg " unpinned; install " pkg "@" pin[pkg])
          else if (ver != pin[pkg])
            finding(where, "npm installs " pkg "@" ver " but " pins_file " pins " pin[pkg] "; install " pkg "@" pin[pkg])
        }
      }
    }
    # Goose: every installer URL on the line is the pinned vendor URL, compared
    # whole (host, repository and tag), and every GOOSE_VERSION assignment is the
    # pin, compared as a string — so a second assignment cannot override it.
    function check_goose(where, text,    want_url, want_env, n, tok, i, t, urls, envs) {
      if (!("goose" in pin)) {
        finding(where, "installs Goose, which " pins_file " does not pin")
        return
      }
      installed["goose"] = 1
      want_url = "https://github.com/block/goose/releases/download/v" pin["goose"] "/download_cli.sh"
      want_env = "GOOSE_VERSION=" pin["goose"]
      n = split(text, tok, /[ \t]+/)
      for (i = 1; i <= n; i++) {
        t = tok[i]
        gsub(/^["'"'"']+|["'"'"']+$/, "", t)
        if (t ~ /download_cli\.(sh|ps1)$/) {
          urls++
          if (t != want_url)
            finding(where, "fetches the Goose installer " t "; fetch " want_url)
        }
        if (t ~ /^GOOSE_VERSION=/) {
          envs++
          if (t != want_env)
            finding(where, "passes Goose " t "; pass " want_env)
        }
      }
      if (!urls) finding(where, "installs Goose from no URL this check can read; fetch " want_url)
      if (!envs) finding(where, "installs Goose without " want_env "; pass it to the installer")
    }
    FILENAME == pins_file {
      if ($0 ~ /^[ \t]*(#|$)/) next
      if (NF != 2 || $2 !~ /^[0-9]+\.[0-9]+\.[0-9]+$/) {
        finding(pins_file ":" FNR, "must pin one package to exactly one x.y.z version, found: " $0)
        next
      }
      if ($1 in pin) finding(pins_file ":" FNR, "pins " $1 " twice")
      pin[$1] = $2
      order[++npins] = $1
      next
    }
    FNR == 1 { joined = ""; start = 0 }
    {
      line = $0
      if (!start) start = FNR
      if (sub(/\\[ \t]*$/, " ", line)) { joined = joined line; next }
      joined = joined line
      where = FILENAME ":" start
      if (joined ~ /npm[ \t]+(install|i)[ \t]+(-g|--global)/) check_npm(where, joined)
      if (joined ~ /download_cli\.(sh|ps1)/) check_goose(where, joined)
      joined = ""; start = 0
    }
    END {
      for (k = 1; k <= npins; k++)
        if (!(order[k] in installed))
          finding(pins_file, "pins " order[k] " but no checked workflow installs it; remove the pin or the workflow lost its install")
      exit bad
    }
  ' "$pins" "${workflows[@]}"
)" || {
  printf '%s\n' "$findings" >&2
  fail "the harness CLI installs disagree with $pins (findings above)"
}

echo "check-harness-cli-pins: ok"
