#!/usr/bin/env bash
# Hermetic boundary coverage for e2e-variants.sh; no provider process is run.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/bin"

for tool in claude codex; do
    printf '#!/usr/bin/env bash\nexit 0\n' >"$tmp/bin/$tool"
    chmod +x "$tmp/bin/$tool"
done

cat >"$tmp/bin/oneharness" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [ -n "${STUB_INVALID_REPORT:-}" ]; then
    printf 'provider rejected sk-ant-secret-value\n' >&2
    exit 7
fi
harness=""
marker=""
fallback=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        --harness)
            harness="$2"
            shift 2
            ;;
        --prompt)
            marker="${2#Reply exactly }"
            shift 2
            ;;
        --run-mode)
            fallback="$2"
            shift 2
            ;;
        *)
            shift
            ;;
    esac
done
if [ "$fallback" = "fallback" ]; then
    jq -n --arg marker "$marker" --arg harness "$harness" '{
        results: [
            {status: "nonzero", exit_code: 1, failure_kind: "auth", stdout: "", harness_id: "claude-code:invalid"},
            {status: "ok", exit_code: 0, failure_kind: null, stdout: $marker, harness_id: $harness}
        ]
    }'
else
    stdout="$(jq -nc --arg marker "$marker" '{
        result: $marker,
        usage: {cache_creation: {ephemeral_5m_input_tokens: 1}}
    }')"
    jq -n --arg stdout "$stdout" --arg harness "$harness" '{
        results: [{status: "ok", exit_code: 0, failure_kind: null, stdout: $stdout, harness_id: $harness}]
    }'
fi
EOF
chmod +x "$tmp/bin/oneharness"

common_env=(
    PATH="$tmp/bin:$PATH"
    ONEHARNESS_BIN="$tmp/bin/oneharness"
    ANTHROPIC_API_KEY="test-anthropic-material"
    OPENAI_API_KEY="test-openai-material"
    OH_E2E_VARIANTS_API_ONLY=1
)

evidence="$tmp/evidence.log"
env "${common_env[@]}" OH_E2E_EVIDENCE_FILE="$evidence" \
    bash "$root/scripts/e2e-variants.sh" >"$tmp/success.out" 2>"$tmp/success.err"
grep -q 'IDENTITY claude-code:apikey' "$evidence"
grep -q 'ASSERT fallback: first_failure_kind=auth' "$evidence"
grep -q 'live variants: ok' "$tmp/success.err"

if env "${common_env[@]}" OH_E2E_EVIDENCE_FILE="$tmp/bin" \
    bash "$root/scripts/e2e-variants.sh" >"$tmp/path.out" 2>"$tmp/path.err"; then
    echo "e2e-variants-test: directory evidence target unexpectedly succeeded" >&2
    exit 1
fi
grep -q 'remove the non-file target or choose a new file path' "$tmp/path.err"

if env "${common_env[@]}" STUB_INVALID_REPORT=1 \
    bash "$root/scripts/e2e-variants.sh" >"$tmp/failure.out" 2>"$tmp/failure.err"; then
    echo "e2e-variants-test: invalid provider report unexpectedly succeeded" >&2
    exit 1
fi
grep -q 'exited 7 before producing a valid report' "$tmp/failure.err"
grep -q '<redacted>' "$tmp/failure.err"
if grep -q 'sk-ant-secret-value' "$tmp/failure.err"; then
    echo "e2e-variants-test: credential-shaped stderr was not redacted" >&2
    exit 1
fi

echo "e2e-variants-test: ok"
