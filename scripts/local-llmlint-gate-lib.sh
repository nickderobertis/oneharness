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
