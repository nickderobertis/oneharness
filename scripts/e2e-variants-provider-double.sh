#!/usr/bin/env bash
# Provider-process adapter used only by the hermetic live-variants script test.
# llmlint: ignore-file[e2e_not_mocked] This adapter is the narrow paid-provider substitute for scripts/e2e-variants-test.sh: the test still executes the real CLI and live workflow, while deterministic failure injection cannot be obtained safely or repeatably from real billed providers. The separate scripts/e2e-variants.sh live job covers the real provider boundary.
set -euo pipefail

provider_double="${OH_E2E_PROVIDER_DOUBLE:-}"
if [ -z "$provider_double" ] || [ ! -f "$provider_double" ] || [ ! -x "$provider_double" ]; then
    echo "e2e-variants-provider-double: OH_E2E_PROVIDER_DOUBLE must name the executable mock-harness fixture; build the checked fixture with 'just check' and retry" >&2
    exit 2
fi

marker=""
for arg in "$@"; do
    case "$arg" in
        *"Reply exactly "*) marker="${arg##*Reply exactly }" ;;
    esac
done

if [ -n "${OH_E2E_DOUBLE_INVALID_REPORT:-}" ]; then
    if [ -z "${TMPDIR:-}" ] || [ ! -d "$TMPDIR" ] || [ ! -r "$TMPDIR" ]; then
        echo "e2e-variants-provider-double: TMPDIR must name the readable test temp directory; run through scripts/e2e-variants-test.sh and retry" >&2
        exit 2
    fi
    report="$(find "$TMPDIR" -name claude-code-apikey.json -type f -print -quit)"
    [ -n "$report" ] || {
        echo "e2e-variants-provider-double: could not locate the open report; run through scripts/e2e-variants-test.sh so TMPDIR owns the report and retry" >&2
        exit 2
    }
    rm -f "$report"
    exit 7
fi

if [ -n "${OH_E2E_DOUBLE_VALID_FAILURE:-}" ]; then
    export MOCK_STDERR="provider rejected request; verify the injected failure fixture and retry"
    export MOCK_EXIT=7
    exec "$provider_double" "$@"
fi

case "$(basename "$0")" in
    claude)
        if [ "${ANTHROPIC_API_KEY:-}" = "deliberately-invalid" ]; then
            export MOCK_STDERR="authentication_error: verify the deliberately invalid fallback fixture and retry"
            export MOCK_EXIT=1
        else
            MOCK_STDOUT="$(jq -nc --arg marker "$marker" \
                '{result: $marker, usage: {cache_creation: {ephemeral_5m_input_tokens: 1}}}')"
            export MOCK_STDOUT
        fi
        ;;
    codex)
        MOCK_STDOUT="$(jq -nc --arg marker "$marker" \
            '{type: "item.completed", item: {id: "m1", type: "agent_message", text: $marker}}')"
        export MOCK_STDOUT
        ;;
    *)
        echo "e2e-variants-provider-double: unknown provider executable; install it as the test's claude or codex adapter and retry" >&2
        exit 2
        ;;
esac

exec "$provider_double" "$@"
