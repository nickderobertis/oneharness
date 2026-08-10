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

# The first line of every recorded verdict, so a reader can tell a record it
# understands from a half-written one or one a later format wrote.
LLMLINT_VERDICT_FORMAT='oneharness-llmlint-verdict 1'

# The judge's identity: the installed llmlint plus its effective merged config,
# resolved from the checkout the judge runs against rather than the caller's cwd —
# that is what makes a plugin pin or a rule edit invalidate even when no file in
# the tree moved.
#
# LLMLINT_ONEHARNESS_BIN is cleared for the read. llmlint renders that override
# into `llmlint config`, but it names the executable that *dispatches* the model
# call, not what the judge asks: with no override the field is null however PATH
# resolves oneharness, so the caller's value gave each environment its own key for
# one unchanged judge. That is what kept the lifecycle publication re-rolling — it
# points llmlint at a sandbox wrapper, so its pre-push gate could never replay the
# green the working tree's own gate had just recorded. Clearing it here does not
# touch the judge run below, which still dispatches through the caller's wrapper.
llmlint_judge_fingerprint() {
  local root=$1
  (
    cd "$root" || exit 1
    unset LLMLINT_ONEHARNESS_BIN
    llmlint --version && llmlint config
  )
}

# The identity of one judge verdict: the workspace content, the resolved base
# commit, and the judge itself. Content is digested through a scratch index so
# the caller's staged state is never touched, and it covers untracked-unignored
# files because the judge reads them too.
llmlint_verdict_key() {
  local root=$1 base_commit=$2 index tree fingerprint
  index=$(mktemp) || return 1
  tree=$(
    export GIT_INDEX_FILE=$index
    git read-tree HEAD && git add -A && git write-tree
  ) || {
    rm -f "$index"
    return 1
  }
  rm -f "$index"
  fingerprint=$(llmlint_judge_fingerprint "$root") || return 1
  # Digested by git, which this function already depends on and every platform
  # running the gate therefore has — the tree half above is a git hash too.
  printf '%s\0%s\0%s\0' "$tree" "$base_commit" "$fingerprint" |
    git hash-object --stdin
}

# When this key's green was recorded, if the store holds a record that belongs to
# it. A verdict file is external input the caller trusts enough to skip the judge
# entirely, so the whole record is validated before it counts: the format it was
# written with, the key it was recorded under, the base commit it was judged
# against, and a well-formed timestamp — four lines, no more. Anything else (a
# half-written file, a leftover from another key or base, a later format) is not
# an error but a miss, so the caller judges again; failing toward the judge is the
# only safe direction.
llmlint_recorded_verdict() {
  local key=$1 base_commit=$2 dir entry format recorded_key recorded_base stamp
  dir=$(llmlint_cache_dir) || return 1
  entry="$dir/$key.verdict"
  [[ -f $entry ]] || return 1
  # The trailing read must FAIL: a fifth line means this is not the record this
  # reader understands, whoever wrote it.
  {
    IFS= read -r format &&
      IFS= read -r recorded_key &&
      IFS= read -r recorded_base &&
      IFS= read -r stamp &&
      ! IFS= read -r _
  } <"$entry" || return 1
  [[ $format == "$LLMLINT_VERDICT_FORMAT" ]] || return 1
  [[ $recorded_key == "key $key" ]] || return 1
  [[ $recorded_base == "base $base_commit" ]] || return 1
  stamp=${stamp#recorded }
  # Spelled as a glob rather than a regex: bash 3.2 is still the system shell on
  # macOS, where this gate runs.
  [[ $stamp == [0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T[0-9][0-9]:[0-9][0-9]:[0-9][0-9]Z ]] || return 1
  printf '%s\n' "$stamp"
}

llmlint_record_verdict() {
  local key=$1 base_commit=$2 dir entry temp stamp
  dir=$(llmlint_cache_dir) || return 1
  mkdir -p "$dir" || return 1
  stamp=$(date -u +%Y-%m-%dT%H:%M:%SZ) || return 1
  entry="$dir/$key.verdict"
  # Written beside the entry and moved onto it, so a reader never sees a record
  # this writer was interrupted halfway through.
  temp=$(mktemp "$entry.XXXXXX") || return 1
  if ! printf '%s\nkey %s\nbase %s\nrecorded %s\n' \
    "$LLMLINT_VERDICT_FORMAT" "$key" "$base_commit" "$stamp" >"$temp" ||
    ! mv -f "$temp" "$entry"; then
    rm -f "$temp"
    return 1
  fi
  # A key no tree has matched in a month cannot match one again; drop it rather
  # than growing the cache without bound. The glob also reaps a scratch file an
  # interrupted write left behind.
  find "$dir" -name '*.verdict*' -mtime +30 -delete 2>/dev/null || true
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
