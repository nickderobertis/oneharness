#!/usr/bin/env bash
# Hold every workflow under .github/workflows/ to `.harness-cli-versions`: every
# `npm install -g` names an exact version and it is the pin, every Goose vendor
# install (bash or PowerShell) fetches and passes its pin, and every Cursor
# install goes through the pinning helpers at its pin — never Cursor's bare
# installer, which takes no version. An unpinned install fails a passing tree
# whenever a publisher is mid-release (Copilot 1.0.115's package landed before
# its darwin-arm64 platform package and failed `check (macos-latest)`), so a
# floating version is a finding, not a style.
#
# Usage: check-harness-cli-pins.sh [PINS_FILE [WORKFLOW...]]
# Defaults: .harness-cli-versions and every .github/workflows/*.yml.
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
  workflows=(.github/workflows/*.yml)
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
    function unquote(t) { gsub(/^["'"'"']+|["'"'"';]+$/, "", t); return t }
    # Goose: every installer URL on the line is the pinned vendor URL, compared
    # whole (host, repository and tag): the release asset for bash, the tagged
    # source file for PowerShell (Goose publishes no .ps1 asset). Every
    # GOOSE_VERSION assignment, bash `GOOSE_VERSION=x` or PowerShell
    # `$env:GOOSE_VERSION = "x"`, is the pin compared as a string, so a dot is no
    # wildcard and a second assignment cannot override the first.
    function check_goose(where, text,    want_sh, want_ps1, want, n, tok, i, t, v, urls, envs) {
      if (!("goose" in pin)) {
        finding(where, "installs Goose, which " pins_file " does not pin")
        return
      }
      installed["goose"] = 1
      want_sh = "https://github.com/block/goose/releases/download/v" pin["goose"] "/download_cli.sh"
      want_ps1 = "https://raw.githubusercontent.com/block/goose/v" pin["goose"] "/download_cli.ps1"
      n = split(text, tok, /[ \t]+/)
      for (i = 1; i <= n; i++) {
        t = unquote(tok[i])
        if (t ~ /download_cli\.(sh|ps1)$/) {
          urls++
          want = t ~ /\.ps1$/ ? want_ps1 : want_sh
          if (t != want) finding(where, "fetches the Goose installer " t "; fetch " want)
        }
        if (t ~ /GOOSE_VERSION/) {
          envs++
          if (t ~ /^GOOSE_VERSION=/) v = substr(t, length("GOOSE_VERSION=") + 1)
          else if (t == "$env:GOOSE_VERSION" && tok[i + 1] == "=") { v = unquote(tok[i + 2]); i += 2 }
          else v = t
          if (v != pin["goose"]) finding(where, "passes Goose GOOSE_VERSION " v "; pass GOOSE_VERSION " pin["goose"])
        }
      }
      if (!urls) finding(where, "installs Goose from no URL this check can read; fetch " want_sh)
      if (!envs) finding(where, "installs Goose without GOOSE_VERSION " pin["goose"] "; pass it to the installer")
    }
    # Cursor: its installer takes no version, so it is installed only through
    # the helpers that pin it, each handed exactly the pin; a bare installer
    # fetch, or a versioned package URL at another build, is a finding.
    function check_cursor(where, text,    n, tok, i, t, v, helpers) {
      if (!("cursor-agent" in pin)) {
        finding(where, "installs Cursor, which " pins_file " does not pin")
        return
      }
      n = split(text, tok, /[ \t]+/)
      for (i = 1; i <= n; i++) {
        t = unquote(tok[i])
        if (t ~ /cursor\.com\/install/)
          finding(where, "runs Cursor'"'"'s installer unpinned (" t "); install through scripts/install-cursor-agent.sh " pin["cursor-agent"] " (Unix) or scripts/cursor-installer.ps1 -Version " pin["cursor-agent"] " (Windows)")
        # A helper counts where it is run (`bash …sh`, `& …ps1`, `./…`), not
        # where it is only named, as in a path list.
        if ((t ~ /install-cursor-agent\.sh$/ && (tok[i - 1] ~ /^(bash|sh)$/ || t ~ /^\.\//)) ||
            (t ~ /cursor-installer\.ps1$/ && (tok[i - 1] ~ /^(&|pwsh|powershell)$/ || t ~ /^\.\//))) {
          helpers++
          installed["cursor-agent"] = 1
          v = unquote(tok[i + 1])
          if (t ~ /\.ps1$/) v = (tok[i + 1] == "-Version") ? unquote(tok[i + 2]) : ""
          if (v != pin["cursor-agent"])
            finding(where, "installs Cursor at \"" v "\"; pass " (t ~ /\.ps1$/ ? "-Version " : "") pin["cursor-agent"])
        }
        if (match(t, /downloads\.cursor\.com\/lab\/[^\/]+\//)) {
          v = substr(t, RSTART + length("downloads.cursor.com/lab/"), RLENGTH - length("downloads.cursor.com/lab/") - 1)
          installed["cursor-agent"] = 1
          if (v != pin["cursor-agent"]) finding(where, "downloads Cursor build " v "; download " pin["cursor-agent"])
        }
      }
    }
    FILENAME == pins_file {
      if ($0 ~ /^[ \t]*(#|$)/) next
      # npm and Goose take an exact x.y.z; Cursor only its build id.
      if (NF != 2 || ($1 == "cursor-agent" ? $2 !~ /^[0-9][0-9][0-9][0-9]\.[0-9][0-9]\.[0-9][0-9]-[0-9a-f]+$/ : $2 !~ /^[0-9]+\.[0-9]+\.[0-9]+$/)) {
        finding(pins_file ":" FNR, "must pin one package to exactly one version (x.y.z; cursor-agent YYYY.MM.DD-<hex>), found: " $0)
        next
      }
      if ($1 in pin) finding(pins_file ":" FNR, "pins " $1 " twice")
      pin[$1] = $2
      order[++npins] = $1
      next
    }
    FNR == 1 { joined = ""; start = 0 }
    # A comment installs nothing, though it may name an installer.
    !start && /^[ \t]*#/ { next }
    {
      line = $0
      if (!start) start = FNR
      if (sub(/\\[ \t]*$/, " ", line)) { joined = joined line; next }
      joined = joined line
      where = FILENAME ":" start
      if (joined ~ /npm[ \t]+(install|i)[ \t]+(-g|--global)/) check_npm(where, joined)
      if (joined ~ /download_cli\.(sh|ps1)|GOOSE_VERSION/) check_goose(where, joined)
      if (joined ~ /cursor\.com\/install|install-cursor-agent\.sh|cursor-installer\.ps1|downloads\.cursor\.com/) check_cursor(where, joined)
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
