#!/usr/bin/env bash
# Provider-process adapter used only by the hermetic live-variants script test.
set -euo pipefail

marker=""
for arg in "$@"; do
    case "$arg" in
        *"Reply exactly "*) marker="${arg##*Reply exactly }" ;;
    esac
done

if [ -n "${OH_E2E_DOUBLE_INVALID_REPORT:-}" ]; then
    report="$(find "$TMPDIR" -name claude-code-apikey.json -type f -print -quit)"
    [ -n "$report" ] || {
        echo "e2e-variants-provider-double: could not locate the open report" >&2
        exit 2
    }
    rm -f "$report"
    exit 7
fi

if [ -n "${OH_E2E_DOUBLE_VALID_FAILURE:-}" ]; then
    export MOCK_STDERR="provider rejected request"
    export MOCK_EXIT=7
    exec "$OH_E2E_PROVIDER_DOUBLE" "$@"
fi

case "$(basename "$0")" in
    claude)
        if [ "${ANTHROPIC_API_KEY:-}" = "deliberately-invalid" ]; then
            export MOCK_STDERR="authentication_error"
            export MOCK_EXIT=1
        else
            export MOCK_STDOUT="{\"result\":\"$marker\",\"usage\":{\"cache_creation\":{\"ephemeral_5m_input_tokens\":1}}}"
        fi
        ;;
    codex)
        export MOCK_STDOUT="{\"type\":\"item.completed\",\"item\":{\"id\":\"m1\",\"type\":\"agent_message\",\"text\":\"$marker\"}}"
        ;;
    *)
        echo "e2e-variants-provider-double: unknown provider executable" >&2
        exit 2
        ;;
esac

exec "$OH_E2E_PROVIDER_DOUBLE" "$@"
