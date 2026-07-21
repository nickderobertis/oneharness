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
