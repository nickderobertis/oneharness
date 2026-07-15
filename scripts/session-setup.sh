#!/usr/bin/env bash
# Idempotent, non-blocking setup for fresh interactive coding sessions.
# llmlint: ignore-file[tool_output_is_signal, boundary_inputs_validated] startup provisioning must log-and-continue, and installs the pinned tool through uv.
set -uo pipefail

readonly JUST_MIN="1.51.0"
readonly BIN_DIR="$HOME/.local/bin"
readonly ORIG_PATH="$PATH"

log() { printf 'session-setup: %s\n' "$*" >&2; }

if [[ -n "${CI:-}" ]]; then
    exit 0
fi

export PATH="${BIN_DIR}:${PATH}"
if ! command -v just >/dev/null 2>&1; then
    if command -v uv >/dev/null 2>&1; then
        uv tool install --upgrade "rust-just>=${JUST_MIN}" >&2 \
            || log "rust-just install failed (continuing)"
    else
        log "uv not found; cannot install just"
    fi
fi

if [[ -n "${CLAUDE_ENV_FILE:-}" && ":${ORIG_PATH}:" != *":${BIN_DIR}:"* ]]; then
    printf 'export PATH=%q\n' "${BIN_DIR}:${PATH}" >>"$CLAUDE_ENV_FILE"
fi

setup_llmlint="$(dirname "$0")/setup-llmlint.sh"
if [[ -x "$setup_llmlint" ]]; then
    "$setup_llmlint" || log "setup-llmlint.sh reported an issue (continuing)"
fi

exit 0
