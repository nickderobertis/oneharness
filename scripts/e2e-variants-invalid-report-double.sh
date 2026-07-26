#!/usr/bin/env bash
# Narrow process-boundary fixture for the live workflow's malformed-report path.
set -euo pipefail

printf '{not-json\n'
echo "e2e-variants-invalid-report-double: injected malformed report; rerun scripts/e2e-variants-test.sh without this fixture to inspect the real CLI boundary" >&2
exit 7
