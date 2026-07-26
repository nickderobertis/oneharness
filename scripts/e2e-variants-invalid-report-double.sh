#!/usr/bin/env bash
# Narrow process-boundary fixture for the live workflow's malformed-report path.
set -euo pipefail

printf '{not-json\n'
echo "e2e-variants-invalid-report-double: injected malformed report" >&2
exit 7
