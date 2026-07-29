#!/usr/bin/env bash
# Hermetic boundary coverage for check-codex-usage-schema.sh. No real codex is
# invoked and no network is touched: each case puts a scripted `codex` on PATH
# and drives the real gate script end to end.
#
# The gate itself is what stands between an upstream rename and `oneharness
# usage` silently reporting zeros, so its own branches need proving. In
# particular the skip branch: it exits 0, which is indistinguishable from a pass
# in CI output, so a bug that made *every* run skip would otherwise disable the
# gate without anyone noticing.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
gate="$root/scripts/check-codex-usage-schema.sh"
snapshot="$root/tests/fixtures/codex-rate-limits.schema.json"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

fail() {
    echo "check-codex-usage-schema-test: $1" >&2
    echo "  rerun with 'bash -x scripts/check-codex-usage-schema-test.sh' to inspect the failing case" >&2
    exit 1
}

# A `codex` whose `app-server generate-json-schema --out DIR` behaves as
# `$mode` dictates. Written fresh per case so PATH holds exactly one.
write_fake_codex() {
    local mode="$1" bin="$tmp/bin"
    rm -rf "$bin"
    mkdir -p "$bin"
    cat >"$bin/codex" <<FAKE
#!/usr/bin/env bash
set -euo pipefail
if [ "\${1:-}" = "--version" ]; then
    echo "codex-cli 0.145.0-fake"
    exit 0
fi
# Expect: app-server generate-json-schema --out <dir>
out=""
while [ \$# -gt 0 ]; do
    case "\$1" in
        --out) out="\$2"; shift 2 ;;
        *) shift ;;
    esac
done
case "$mode" in
    match)
        mkdir -p "\$out/v2"
        cp "$snapshot" "\$out/v2/GetAccountRateLimitsResponse.json"
        ;;
    drift)
        mkdir -p "\$out/v2"
        sed 's/usedPercent/pctUsed/g' "$snapshot" \\
            >"\$out/v2/GetAccountRateLimitsResponse.json"
        ;;
    missing-output)
        # Succeeds, but emits nothing where the rate-limits response used to be.
        mkdir -p "\$out/v2"
        echo '{}' >"\$out/v2/SomethingElse.json"
        ;;
    generator-failure)
        echo "codex: app-server: unrecognized subcommand 'generate-json-schema'" >&2
        exit 2
        ;;
esac
FAKE
    chmod +x "$bin/codex"
    printf '%s' "$bin"
}

# Run the gate with `$1` as the only PATH entry ahead of a minimal system PATH.
run_gate() {
    local extra_path="$1" out="$2"
    set +e
    PATH="$extra_path:/usr/bin:/bin" "$gate" >"$out" 2>&1
    local status=$?
    set -e
    printf '%s' "$status"
}

assert_contains() {
    local file="$1" pattern="$2" description="$3"
    grep -qF "$pattern" "$file" || {
        echo "--- captured output ---" >&2
        cat "$file" >&2
        fail "$description"
    }
}

# 1. No codex installed: skip cleanly, and say so — a silent exit 0 would be
#    indistinguishable from a real pass.
empty="$tmp/empty-bin"
mkdir -p "$empty"
status="$(run_gate "$empty" "$tmp/skip.log")"
[ "$status" = "0" ] || fail "a missing codex must skip (exit 0), got exit $status"
assert_contains "$tmp/skip.log" "skipped" \
    "the skip must announce itself rather than passing silently"
assert_contains "$tmp/skip.log" "codex is not installed" \
    "the skip must name why it skipped"

# 2. The installed schema matches the snapshot: the ordinary green path.
status="$(run_gate "$(write_fake_codex match)" "$tmp/ok.log")"
[ "$status" = "0" ] || {
    cat "$tmp/ok.log" >&2
    fail "a matching schema must pass, got exit $status"
}
assert_contains "$tmp/ok.log" "check-codex-usage-schema: ok" \
    "a pass must report a pass, not a skip"
grep -qF "skipped" "$tmp/ok.log" &&
    fail "a real run must not report itself as skipped"

# 3. The generator itself fails: a loud failure naming the next action.
status="$(run_gate "$(write_fake_codex generator-failure)" "$tmp/genfail.log")"
[ "$status" = "1" ] || fail "a failing generator must fail the gate, got exit $status"
assert_contains "$tmp/genfail.log" "unrecognized subcommand" \
    "the generator's own error must be preserved"
assert_contains "$tmp/genfail.log" "update this script to the new command" \
    "a generator failure must name a concrete next action"

# 4. The generator succeeds but no longer emits the rate-limits response.
status="$(run_gate "$(write_fake_codex missing-output)" "$tmp/missing.log")"
[ "$status" = "1" ] || fail "a missing generated file must fail the gate, got exit $status"
assert_contains "$tmp/missing.log" "no longer emits v2/GetAccountRateLimitsResponse.json" \
    "a moved response must be reported as moved, not as a diff"
assert_contains "$tmp/missing.log" "parse_codex_rate_limits" \
    "the failure must point at the parser that depends on the shape"

# 5. The contract drifted: fail, and show the diff plus how to refresh.
status="$(run_gate "$(write_fake_codex drift)" "$tmp/drift.log")"
[ "$status" = "1" ] || fail "a drifted schema must fail the gate, got exit $status"
assert_contains "$tmp/drift.log" "pctUsed" \
    "the diff must show what actually changed"
assert_contains "$tmp/drift.log" "the codex rate-limits contract drifted" \
    "drift must be named as drift"
assert_contains "$tmp/drift.log" "would otherwise become a silent zero" \
    "the failure must say why the drift matters"

# 6. The diff must survive the way `just lint-workflows` invokes the gate —
#    stdout suppressed. A drift report whose diff went to /dev/null tells a
#    reader the schema moved without saying what moved.
drift_path="$(write_fake_codex drift)"
set +e
PATH="$drift_path:/usr/bin:/bin" "$gate" >/dev/null 2>"$tmp/drift-stderr.log"
status=$?
set -e
[ "$status" = "1" ] || fail "drift must still fail with stdout suppressed, got exit $status"
assert_contains "$tmp/drift-stderr.log" "pctUsed" \
    "the diff must reach stderr, so 'just lint-workflows' still shows what changed"

echo "check-codex-usage-schema-test: ok"
