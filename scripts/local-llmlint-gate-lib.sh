#!/usr/bin/env bash

llmlint_primary_harness() {
  local config=$1 primary_harness
  primary_harness=$(awk '
    /^[[:space:]]*harnesses[[:space:]]*=/ {
      value = $0
      sub(/^[^=]*=[[:space:]]*\[/, "", value)
      if (match(value, /"[^"]+"/)) {
        print substr(value, RSTART + 1, RLENGTH - 2)
        exit
      }
    }
  ' "$config")
  if [[ ! $primary_harness =~ ^[[:alnum:]][[:alnum:]_.-]*$ ]]; then
    echo "llmlint: oneharness.toml must declare a valid first harness in 'harnesses'" >&2
    return 2
  fi
  printf '%s\n' "$primary_harness"
}

# Where recorded green verdicts live. The root comes from the environment, so it
# is validated here: only an absolute, single-line path is accepted, since a
# relative one would scatter verdict files through whatever directory the gate
# happened to run in and an embedded newline would truncate the path the caller
# reads back. A rejected or home-less environment simply cannot cache, which the
# caller reports and judges afresh through rather than failing.
llmlint_cache_dir() {
  local base=${XDG_CACHE_HOME:-}
  [[ -n $base ]] || base=${HOME:+$HOME/.cache}
  [[ $base == /* ]] || return 1
  case $base in
  *$'\n'*) return 1 ;;
  esac
  printf '%s/oneharness/llmlint-gate\n' "$base"
}

llmlint_digest() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum | cut -d' ' -f1
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 | cut -d' ' -f1
  else
    return 1
  fi
}

# The identity of one judge verdict: the workspace content, the resolved base
# commit, and the judge itself. Content is digested through a scratch index so
# the caller's staged state is never touched, and it covers untracked-unignored
# files because the judge reads them too. The judge fingerprint is the installed
# llmlint plus its effective merged config, resolved from the checkout the judge
# runs against rather than the caller's cwd — that is what makes a plugin pin or
# a rule edit invalidate even when no file in the tree moved.
llmlint_verdict_key() {
  local root=$1 base_commit=$2 index tree version config
  index=$(mktemp) || return 1
  tree=$(
    export GIT_INDEX_FILE=$index
    git read-tree HEAD && git add -A && git write-tree
  ) || {
    rm -f "$index"
    return 1
  }
  rm -f "$index"
  version=$(cd "$root" && llmlint --version) || return 1
  config=$(cd "$root" && llmlint config) || return 1
  printf '%s\0%s\0%s\0%s\0' "$tree" "$base_commit" "$version" "$config" | llmlint_digest
}

# Print the recorded verdict's path, or fail when this key has none.
llmlint_replay_verdict() {
  local dir entry
  dir=$(llmlint_cache_dir) || return 1
  entry="$dir/$1.verdict"
  [[ -s $entry ]] || return 1
  printf '%s\n' "$entry"
}

llmlint_record_verdict() {
  local key=$1 base_commit=$2 dir
  dir=$(llmlint_cache_dir) || return 1
  mkdir -p "$dir" || return 1
  printf 'recorded %s against base %s\n' \
    "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$base_commit" >"$dir/$key.verdict" || return 1
  # A key no tree has matched in a month cannot match one again; drop it rather
  # than growing the cache without bound.
  find "$dir" -name '*.verdict' -mtime +30 -delete 2>/dev/null || true
}

llmlint_judge_available() {
  local config=$1 oneharness_bin probe_output probe_stderr status
  oneharness_bin=$(command -v oneharness) || {
    echo "llmlint: judge skipped locally (oneharness unavailable)" >&2
    return 75
  }
  [[ $oneharness_bin == /* && -x $oneharness_bin ]] || {
    echo "llmlint: resolved oneharness path is not an absolute executable: $oneharness_bin; fix PATH or run 'just setup-llmlint'" >&2
    return 2
  }
  command -v jq >/dev/null 2>&1 || {
    echo "llmlint: jq is required to validate the oneharness availability probe; install jq and retry" >&2
    return 2
  }
  probe_stderr=$(mktemp)
  if probe_output=$("$oneharness_bin" run --config "$config" --compact \
    --prompt "Reply with exactly: available" 2>"$probe_stderr"); then
    status=0
  else
    status=$?
  fi

  if ! printf '%s\n' "$probe_output" | jq -e \
    '(.fallback | type == "object") and (.fallback.ran == null or (.fallback.ran | type == "string"))' \
    >/dev/null 2>&1; then
    cat "$probe_stderr" >&2
    rm -f "$probe_stderr"
    echo "llmlint: oneharness availability probe returned an invalid report; run 'oneharness run --config oneharness.toml --prompt test' to diagnose" >&2
    [[ $status -ne 0 ]] && return "$status"
    return 2
  fi

  if [[ $(printf '%s\n' "$probe_output" | jq -r '.fallback.ran == null') == true ]]; then
    rm -f "$probe_stderr"
    echo "llmlint: judge skipped locally (no configured harness is available and authenticated)" >&2
    return 75
  fi

  cat "$probe_stderr" >&2
  rm -f "$probe_stderr"
  [[ $status -eq 0 ]] && return 0
  echo "llmlint: oneharness availability probe failed; check harness authentication and run 'oneharness run --config oneharness.toml --prompt test'" >&2
  return "$status"
}
