#!/usr/bin/env bash
#
# Hermetic test for scripts/prune-mistake-releases.sh — a deletion tool, so its
# safety properties are PROVEN here, not eyeballed. No network, no real GitHub:
# a fake `gh` on PATH acts as a spy. `gh api` runs real `jq` over a release-JSON
# fixture (so the actual tag extraction is exercised); `gh release delete`
# records the tag it was asked to remove instead of deleting anything. We then
# assert exactly which tags the tool would delete under each scenario.
#
# What this pins (the dangerous-to-get-wrong behavior):
#   - dry run (the default) deletes NOTHING;
#   - only tags matching the anchored pattern are ever deleted — near-misses
#     (`v0.2.0-rc.1`, `v0.20.0`, `oneharness-core-v0.2.0`) and the real releases
#     to keep (`v0.1.x`, `v0.3.x`) are never touched;
#   - each delete carries `--cleanup-tag` and `--yes`, against the right repo;
#   - `--pattern` retargets safely; a no-match run is a clean no-op;
#   - a mid-run delete failure is counted, does not abort the rest, and exits
#     non-zero; missing tools / failed auth abort before any deletion.
#
# jq-gated like the repo's other external-tool e2e (the tool itself requires jq,
# and GitHub's Linux/macOS runners ship it): a jq-less host skips with a notice.
# Runs inside `scripts/smoke.sh` (so `just check`/CI cover it) and standalone.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
SCRIPT="${here}/prune-mistake-releases.sh"

if ! command -v jq >/dev/null 2>&1; then
  echo "prune-releases test: jq not found; skipped (install jq to run it)" >&2
  exit 0
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
BIN="${work}/bin"
mkdir -p "$BIN"
GH_SPY="${work}/gh-spy.log"        # every gh invocation, full argv
GH_DELETED="${work}/deleted.log"   # one line per tag `gh release delete` got
FIXTURE="${work}/releases.json"
REPO_UT="acme/widget"

# A realistic Releases-API page. The near-misses are the point: an anchored
# `^v0\.2\.[0-9]+$` must reject the -rc suffix, the `v0.20.x` line (0.2 is not a
# prefix of 0.20), and any `…-v0.2.0` that merely contains the string.
cat >"$FIXTURE" <<'JSON'
[
  {"tag_name":"v0.1.0","draft":false,"prerelease":false},
  {"tag_name":"v0.1.1","draft":false,"prerelease":false},
  {"tag_name":"v0.2.0","draft":false,"prerelease":false},
  {"tag_name":"v0.2.1","draft":false,"prerelease":false},
  {"tag_name":"v0.2.5","draft":false,"prerelease":false},
  {"tag_name":"v0.2.10","draft":false,"prerelease":false},
  {"tag_name":"v0.2.100","draft":false,"prerelease":false},
  {"tag_name":"v0.2.0-rc.1","draft":false,"prerelease":true},
  {"tag_name":"v0.20.0","draft":false,"prerelease":false},
  {"tag_name":"oneharness-core-v0.2.0","draft":false,"prerelease":false},
  {"tag_name":"oneharness-core-v0.3.0","draft":false,"prerelease":false},
  {"tag_name":"v0.3.0","draft":false,"prerelease":false},
  {"tag_name":"v0.3.12","draft":false,"prerelease":false}
]
JSON

# Fake gh: spy + jq-backed listing + record-only delete. Honors GH_AUTH_FAIL and
# GH_FAIL_TAG so the failure paths are testable.
cat >"${BIN}/gh" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
{ printf '%s ' gh "$@"; echo; } >>"$GH_SPY"
case "${1:-}" in
  auth)
    [ "${GH_AUTH_FAIL:-0}" = "1" ] && { echo "not logged in" >&2; exit 1; }
    exit 0 ;;
  api)
    expr='.'
    args=("$@")
    for ((i=0; i<${#args[@]}; i++)); do
      [ "${args[i]}" = "--jq" ] && expr="${args[i+1]:-.}"
    done
    jq -r "$expr" <"$GH_RELEASES_JSON" ;;
  release)   # gh release delete <tag> --repo R --cleanup-tag --yes
    tag="${3:-}"
    if [ -n "${GH_FAIL_TAG:-}" ] && [ "$tag" = "$GH_FAIL_TAG" ]; then
      echo "simulated delete failure for $tag" >&2; exit 1
    fi
    printf '%s\n' "$tag" >>"$GH_DELETED" ;;
  *) echo "unexpected gh call: $*" >&2; exit 99 ;;
esac
SH
chmod +x "${BIN}/gh"

fail() { echo "FAIL: $1" >&2; [ $# -gt 1 ] && printf '%s\n' "$2" >&2; exit 1; }
reset() { : >"$GH_SPY"; rm -f "$GH_DELETED"; }

# Run the tool under test with the fake gh first on PATH. RUNPATH lets one test
# point at an isolated PATH to prove the missing-tool guard. The interpreter is
# resolved to an ABSOLUTE path once, so a test can hand the child a PATH with no
# `bash` (or nothing at all) without breaking how we launch it — the child's
# PATH controls only what the *script* can find, not what starts it.
BASH_BIN="${BASH:-$(command -v bash)}"   # the interpreter already running us
RUNPATH="${BIN}:${PATH}"
run() {
  local rp="${RUNPATH}"
  set +e
  OUT=$(PATH="$rp" GH_SPY="$GH_SPY" GH_DELETED="$GH_DELETED" \
        GH_RELEASES_JSON="$FIXTURE" REPO="$REPO_UT" \
        GH_AUTH_FAIL="${GH_AUTH_FAIL:-0}" GH_FAIL_TAG="${GH_FAIL_TAG:-}" \
        "$BASH_BIN" "$SCRIPT" "$@" 2>&1)
  RC=$?
  set -e
}
deleted_sorted() { [ -f "$GH_DELETED" ] || return 0; sort "$GH_DELETED" | tr '\n' ' '; }

EXPECT_02="v0.2.0 v0.2.1 v0.2.10 v0.2.100 v0.2.5 "   # sorted, the 5 real 0.2.x

# --- T1: dry run deletes nothing, reports the right count -------------------
reset; run
[ "$RC" -eq 0 ] || fail "T1 dry-run exit=$RC" "$OUT"
[ -f "$GH_DELETED" ] && fail "T1 dry run DELETED something" "$(cat "$GH_DELETED")"
grep -q "Matched 5 release" <<<"$OUT" || fail "T1 wrong match count" "$OUT"
grep -q "dry run" <<<"$OUT" || fail "T1 not labeled dry run" "$OUT"

# --- T2: --execute deletes exactly the 5 v0.2.x, with the right flags/repo ---
reset; run --execute
[ "$RC" -eq 0 ] || fail "T2 exit=$RC" "$OUT"
got=$(deleted_sorted)
[ "$got" = "$EXPECT_02" ] || fail "T2 deleted set wrong: [$got] != [$EXPECT_02]" "$OUT"
# every delete carried the tag-cleanup + non-interactive flags, against acme/widget
dels=$(grep -c 'gh release delete' "$GH_SPY" || true)
[ "$dels" -eq 5 ] || fail "T2 expected 5 delete calls, got $dels" "$(cat "$GH_SPY")"
grep 'gh release delete' "$GH_SPY" | grep -qv -- '--cleanup-tag' && fail "T2 a delete missed --cleanup-tag" "$(cat "$GH_SPY")"
grep 'gh release delete' "$GH_SPY" | grep -qv -- '--yes' && fail "T2 a delete missed --yes" "$(cat "$GH_SPY")"
grep 'gh release delete' "$GH_SPY" | grep -qv -- "--repo $REPO_UT" && fail "T2 a delete hit the wrong repo" "$(cat "$GH_SPY")"

# --- T2b: the listing call asks for pagination + 100/page on the right repo --
grep -q -- '--paginate' "$GH_SPY" || fail "T2b listing did not paginate" "$(cat "$GH_SPY")"
grep -q "per_page=100" "$GH_SPY" || fail "T2b listing lacked per_page=100" "$(cat "$GH_SPY")"
grep -q "/repos/${REPO_UT}/releases" "$GH_SPY" || fail "T2b listing hit wrong path" "$(cat "$GH_SPY")"

# --- T3: --pattern retargets; only v0.1.x removed ---------------------------
reset; run --pattern '^v0\.1\.[0-9]+$' --execute
[ "$RC" -eq 0 ] || fail "T3 exit=$RC" "$OUT"
got=$(deleted_sorted)
[ "$got" = "v0.1.0 v0.1.1 " ] || fail "T3 deleted set wrong: [$got]" "$OUT"

# --- T4: a mid-run delete failure is counted, non-fatal, and exits non-zero --
reset; GH_FAIL_TAG="v0.2.10" run --execute
[ "$RC" -ne 0 ] || fail "T4 should exit non-zero when a delete fails" "$OUT"
got=$(deleted_sorted)                      # the other four still went
[ "$got" = "v0.2.0 v0.2.1 v0.2.100 v0.2.5 " ] || fail "T4 wrong survivors: [$got]" "$OUT"
grep -q "1 failed" <<<"$OUT" || fail "T4 summary missing failure count" "$OUT"

# --- T5: missing gh aborts before doing anything ----------------------------
# Hand the child an isolated, empty PATH so `command -v gh` finds nothing — this
# must hold even though the host HAS gh (CI runners and dev machines all ship it;
# needing it is the whole point of the tool). The guard aborts before touching
# jq/grep, so an empty PATH is enough; the interpreter is launched by absolute
# path (see run), so an empty child PATH can't stop the script from starting.
EMPTY_PATH="${work}/empty-path"
mkdir -p "$EMPTY_PATH"
reset; RUNPATH="${EMPTY_PATH}" run --execute
[ "$RC" -ne 0 ] || fail "T5 should fail when gh is absent" "$OUT"
grep -qi "gh.*required\|required.*gh" <<<"$OUT" || fail "T5 no useful gh-missing message" "$OUT"
[ -f "$GH_DELETED" ] && fail "T5 deleted despite missing gh" "$(cat "$GH_DELETED")"
RUNPATH="${BIN}:${PATH}"

# --- T6: failed auth aborts before any deletion -----------------------------
reset; GH_AUTH_FAIL=1 run --execute
[ "$RC" -ne 0 ] || fail "T6 should fail on bad auth" "$OUT"
[ -f "$GH_DELETED" ] && fail "T6 deleted despite failed auth" "$(cat "$GH_DELETED")"

# --- T7: a pattern that matches nothing is a clean no-op --------------------
reset; run --pattern '^v9\.[0-9]+\.[0-9]+$' --execute
[ "$RC" -eq 0 ] || fail "T7 no-match exit=$RC" "$OUT"
[ -f "$GH_DELETED" ] && fail "T7 deleted on a no-match pattern" "$(cat "$GH_DELETED")"
grep -q "nothing to do" <<<"$OUT" || fail "T7 missing no-op message" "$OUT"

echo "prune-releases test: ok (7 scenarios, hermetic)"
