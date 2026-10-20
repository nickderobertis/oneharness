#!/usr/bin/env bash
# Drift gate for the codex app-server contracts oneharness parses: the
# `account/rateLimits/read` payload `oneharness usage` reads, and the
# `thread/tokenUsage/updated` notification a controlled turn's usage is read off.
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

# Hold one checked-in snapshot to the schema codex generates for it:
#   $1 the generated file under the schema dir   $2 the snapshot (repo-relative)
#   $3 what it is, in prose                       $4 the parser that reads it
#   $5 what a silent drift would turn into
check_snapshot() {
  local name="$1" fixture="$2" what="$3" parser="$4" consequence="$5"
  local generated="$out/$name"
  if [[ ! -f "$generated" ]]; then
    echo "check-codex-usage-schema: codex $(codex --version) no longer emits $name." >&2
    echo "  The $what moved or was renamed; find its new path under the generated schema dir," >&2
    echo "  update this script and $fixture, and re-check the parser in" >&2
    echo "  $parser." >&2
    exit 1
  fi
  # The diff goes to stderr with the rest of the failure: `just lint-workflows`
  # suppresses stdout, and a drift report without its diff is not a report.
  if ! diff -u "$root/$fixture" "$generated" >&2; then
    cat >&2 <<EOF
check-codex-usage-schema: the codex $what contract drifted (codex $(codex --version)).

  The diff above is against $fixture.
  Read it before refreshing: a removed or renamed field that
  $parser reads
  would otherwise become $consequence.

  Once the parser handles the new shape, refresh the snapshot with:
    codex app-server generate-json-schema --out /tmp/codex-schema
    cp /tmp/codex-schema/$name \\
       $fixture
EOF
    exit 1
  fi
}

check_snapshot v2/GetAccountRateLimitsResponse.json tests/fixtures/codex-rate-limits.schema.json \
  "rate-limits" "parse_codex_rate_limits (crates/oneharness-core/src/domain/usage.rs)" \
  "a silent zero in 'oneharness usage'"
check_snapshot v2/ThreadTokenUsageUpdatedNotification.json tests/fixtures/codex-thread-token-usage.schema.json \
  "thread token-usage notification" \
  "Dialogue::absorb_codex_token_usage (crates/oneharness-core/src/domain/dialogue.rs)" \
  "a controlled codex turn that reports no usage"

echo "check-codex-usage-schema: ok"
