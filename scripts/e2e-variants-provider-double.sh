#!/usr/bin/env bash
# Provider-process adapter used only by the hermetic live-variants script test.
# llmlint: ignore-file[e2e_not_mocked] This adapter is the narrow paid-provider substitute for scripts/e2e-variants-test.sh: the test still executes the real CLI and live workflow, while deterministic failure injection cannot be obtained safely or repeatably from real billed providers. The separate scripts/e2e-variants.sh live job covers the real provider boundary.
set -euo pipefail

provider_double="${OH_E2E_PROVIDER_DOUBLE:-}"
if [ -z "$provider_double" ] || [ ! -f "$provider_double" ] || [ ! -x "$provider_double" ]; then
    echo "e2e-variants-provider-double: OH_E2E_PROVIDER_DOUBLE must name the executable mock-harness fixture; build it with 'cargo build --features mock-harness --bin oneharness-mock-harness' and retry" >&2
    exit 2
fi

marker=""
for arg in "$@"; do
    case "$arg" in
        *"Reply exactly "*) marker="${arg##*Reply exactly }" ;;
    esac
done

if [ -n "${OH_E2E_DOUBLE_INVALID_REPORT:-}" ]; then
    report="$(find "$TMPDIR" -name claude-code-apikey.json -type f -print -quit)"
    [ -n "$report" ] || {
        echo "e2e-variants-provider-double: could not locate the open report; run through scripts/e2e-variants-test.sh so TMPDIR owns the report and retry" >&2
        exit 2
    }
    rm -f "$report"
    exit 7
fi

if [ -n "${OH_E2E_DOUBLE_VALID_FAILURE:-}" ]; then
    export MOCK_STDERR="provider rejected request"
    export MOCK_EXIT=7
    exec "$provider_double" "$@"
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
        echo "e2e-variants-provider-double: unknown provider executable; install it as the test's claude or codex adapter and retry" >&2
        exit 2
        ;;
esac

exec "$provider_double" "$@"
