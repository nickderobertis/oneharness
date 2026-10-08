#!/usr/bin/env bash
#
# Hold every byte-compared contract to LF under a Windows checkout.
#
# Two gates compare a checked-in file against what its generator writes, byte
# for byte: `check-parity-audit.sh` for `docs/sdk-parity.md`, and the Python
# SDK's `generate.py --check` for its `_generated` contracts. Both generators
# write LF. Git rewrites a text file to CRLF on a Windows checkout unless
# `.gitattributes` pins it, so an unpinned contract is a gate that can only fail
# there — which is how `check (windows-latest)` went red on every pull request
# while Linux and macOS stayed green.
#
# So ask the question everywhere instead: check the tracked blobs out under
# Windows conversion rules, here, and fail on a carriage return. The Node SDK's
# half of the same property is a test in its own suite, over its own files.
#
# Quiet on success, one line. On failure it names each unpinned file.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

# The contracts to check. Overridable by argument so the gate can be pointed at
# a blob that carries carriage returns on purpose — which is how
# `check-lf-contracts-test.sh` proves it still goes red, without having to unpin
# a real contract to do it.
#
# `--shell` checks the other LF contract instead: every shell script bash runs
# or shellcheck reads (`scripts:lint`), whether `*.sh` or extensionless with a
# shell shebang like `scripts/nx` and `.githooks/pre-push`. A CRLF checkout of
# one is a carriage return on every line — shellcheck's SC1017, and bash's
# `$'\r': command not found`.
rerun="bash scripts/check-lf-contracts.sh"
if [ "${1:-}" = --shell ]; then
  rerun+=" --shell"
  contracts=()
  # Each step stands alone so `set -e` sees it fail: a listing git could not
  # produce, or a script it could not read, stops the gate rather than leaving
  # a shorter list that passes.
  if ! tracked="$(git ls-files -- scripts .githooks)"; then
    echo "check-lf-contracts: git could not list the tracked shell scripts (above)." >&2
    echo "  fix: run this from a git checkout of the repository, then rerun '$rerun'." >&2
    exit 1
  fi
  while IFS= read -r path; do
    case "$path" in
      '') ;;
      *.sh) contracts+=("$path") ;;
      *)
        if ! first="$(head -n 1 -- "$path")"; then
          echo "check-lf-contracts: could not read the tracked file $path (above)." >&2
          echo "  fix: restore it ('git checkout -- $path') or remove it from the index, then rerun '$rerun'." >&2
          exit 1
        fi
        if [[ "$first" =~ ^\#!.*[/\ ](ba)?sh([[:space:]]|$) ]]; then contracts+=("$path"); fi
        ;;
    esac
  done <<<"$tracked"
elif [ "$#" -gt 0 ]; then
  contracts=("$@")
else
  contracts=(docs/sdk-parity.md)
  while IFS= read -r path; do
    contracts+=("$path")
  done < <(git ls-files -- python/oneharness-sdk | grep -E '\.(py|json)$')
fi

# Check out inside the ignored build directory with a RELATIVE prefix: git is a
# Windows executable under Git Bash and does not read that shell's `/tmp`.
work="target/lf-contracts"
rm -rf "$work"
mkdir -p "$work"
trap 'rm -rf "$work"' EXIT

# `set -e` alone would exit here on git's raw diagnostic, which names the path
# but not what to do with it.
if ! git -c core.autocrlf=true -c core.eol=crlf checkout-index \
  --prefix="$work/" -- "${contracts[@]}"; then
  echo "check-lf-contracts: git could not check out the contracts named above." >&2
  echo "  fix: this gate reads tracked blobs, so every path it checks must be in the" >&2
  echo "       index — 'git add' the file, or drop it from the list in this script." >&2
  exit 1
fi

# Read the bytes with Node, not grep: under Git Bash grep reports a file full of
# carriage returns as having none, which is a gate that passes everywhere and
# checks nothing on the one platform whose checkout it describes. A path git did
# not write throws here, so it stays loud rather than reading as clean.
crlf="$(node -e '
  const { readFileSync } = require("node:fs");
  const [work, ...paths] = process.argv.slice(1);
  for (const path of paths) {
    if (readFileSync(work + "/" + path).includes(0x0d)) console.log(path);
  }
' "$work" "${contracts[@]}")"

if [ -n "$crlf" ]; then
  echo "check-lf-contracts: these must stay LF but check out with CRLF on Windows:" >&2
  printf '%s\n' "$crlf" | sed 's/^/  /' >&2
  echo "  fix: pin each one in .gitattributes with 'text eol=lf', beside the entries" >&2
  echo "       already there, then rerun '$rerun'." >&2
  exit 1
fi

echo "check-lf-contracts: ${#contracts[@]} contract(s) stay LF under a Windows checkout"
