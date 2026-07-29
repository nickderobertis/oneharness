#!/usr/bin/env bash
# Drift gate for codex's `account/rateLimits/read` contract — the payload
# `oneharness usage` parses.
#
# The codex app-server is marked experimental, so the wire shape can move. Unlike
# Claude Code, codex *generates* its own schema from the installed binary, which
# makes the check exact and cheap: regenerate at the installed version and diff
# against the checked-in snapshot. The failure mode this prevents is the command
# silently reporting zeros after an upstream rename.
#
# Skips cleanly when codex is not installed — the snapshot's field-level
# assertions still run hermetically in the Rust suite
# (`codex_schema_snapshot_still_declares_every_field_the_parser_reads`), so a
# codex-less clone is not left unguarded, only un-refreshed.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
snapshot="$root/tests/fixtures/codex-rate-limits.schema.json"

if ! command -v codex >/dev/null 2>&1; then
    echo "check-codex-usage-schema: skipped (codex is not installed; the snapshot's field assertions still run in 'just test')"
    exit 0
fi

out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT

if ! codex app-server generate-json-schema --out "$out" >/dev/null 2>"$out/err"; then
    echo "check-codex-usage-schema: 'codex app-server generate-json-schema' failed on codex $(codex --version):" >&2
    cat "$out/err" >&2
    echo "  Run that command yourself to see it in full. If the subcommand was removed or renamed," >&2
    echo "  the app-server contract is no longer generatable: update this script to the new command," >&2
    echo "  or drop it and rely on the hermetic field assertions in crates/oneharness-core/tests/usage.rs." >&2
    echo "  If codex is simply broken here (a partial install, an unwritable temp dir), reinstall it —" >&2
    echo "  this gate needs a working 'codex app-server'." >&2
    exit 1
fi

generated="$out/v2/GetAccountRateLimitsResponse.json"
if [[ ! -f "$generated" ]]; then
    echo "check-codex-usage-schema: codex $(codex --version) no longer emits v2/GetAccountRateLimitsResponse.json." >&2
    echo "  The rate-limits response moved or was renamed; find its new path under the generated schema dir," >&2
    echo "  update this script and tests/fixtures/codex-rate-limits.schema.json, and re-check the parser in" >&2
    echo "  crates/oneharness-core/src/domain/usage.rs (parse_codex_rate_limits)." >&2
    exit 1
fi

# The diff goes to stderr with the rest of the failure: `just lint-workflows`
# suppresses stdout, and a drift report without its diff is not a report.
if ! diff -u "$snapshot" "$generated" >&2; then
    cat >&2 <<EOF
check-codex-usage-schema: the codex rate-limits contract drifted (codex $(codex --version)).

  The diff above is against tests/fixtures/codex-rate-limits.schema.json.
  Read it before refreshing: a removed or renamed field that
  parse_codex_rate_limits reads (crates/oneharness-core/src/domain/usage.rs)
  would otherwise become a silent zero in 'oneharness usage'.

  Once the parser handles the new shape, refresh the snapshot with:
    codex app-server generate-json-schema --out /tmp/codex-schema
    cp /tmp/codex-schema/v2/GetAccountRateLimitsResponse.json \\
       tests/fixtures/codex-rate-limits.schema.json
EOF
    exit 1
fi

echo "check-codex-usage-schema: ok"
