#!/usr/bin/env bash
# Prove the workflow drift gate rejects a missing release-tag boundary check, a
# third-party `just` installer, a release that re-runs a check CI already ran,
# and a registry metadata probe standing in for a consumer's install — required
# lines, a required GUARD above a line, and forbidden patterns, so all three of
# the gate's mechanisms are exercised rather than assumed.
set -euo pipefail

cd "$(dirname "$0")/.."

work="$(mktemp -d)"
workflow=.github/workflows/release.yml
ci=.github/workflows/ci.yml
release_plz=.github/workflows/release-plz.yml
cp "$workflow" "$work/release.yml"
cp "$ci" "$work/ci.yml"
cp "$release_plz" "$work/release-plz.yml"
restore() {
  cp "$work/release.yml" "$workflow"
  cp "$work/ci.yml" "$ci"
  cp "$work/release-plz.yml" "$release_plz"
  rm -rf "$work"
}
trap restore EXIT

# The single-quoted program is JavaScript; $tag is fixture text, not shell.
# shellcheck disable=SC2016
node -e '
  const fs = require("node:fs");
  const path = process.argv[1];
  const source = fs.readFileSync(path, "utf8").replaceAll("\r\n", "\n");
  const line = "          if ! [[ \"$tag\" =~ ^v[0-9]+\\.[0-9]+\\.[0-9]+([+-][0-9A-Za-z.-]+)?$ ]]; then\n";
  if (!source.includes(line)) throw new Error("release tag validation fixture is missing");
  fs.writeFileSync(path, source.replace(line, ""));
' "$workflow"

if bash scripts/check-workflows.sh >"$work/stdout" 2>"$work/stderr"; then
  echo 'check-workflows-e2e: missing release-tag validation unexpectedly passed the gate' >&2
  echo '  fix: restore the release-tag boundary check in scripts/check-workflows.sh' >&2
  exit 1
fi
grep -Fq 'release.yml must validate release-event tags before using them in paths' "$work/stderr" || {
  echo 'check-workflows-e2e: drift failure lacked the expected diagnostic' >&2
  echo "  fix: restore the wording 'release.yml must validate release-event tags before using them in paths' in scripts/check-workflows.sh, or update this expectation to the new wording" >&2
  cat "$work/stderr" >&2
  exit 1
}
cp "$work/release.yml" "$workflow"

# The other half: a third-party `just` installer creeping back in. It is a
# forbidden pattern rather than a missing line, so it exercises the gate's other
# mechanism — and it is exactly the drift that took required checks down.
printf '      - uses: extractions/setup-just@v3\n' >>"$ci"
if bash scripts/check-workflows.sh >"$work/stdout" 2>"$work/stderr"; then
  echo 'check-workflows-e2e: a third-party setup-just in ci.yml unexpectedly passed the gate' >&2
  echo "  fix: restore the forbidden-pattern check for 'setup-just@' in scripts/check-workflows.sh" >&2
  exit 1
fi
grep -Fq 'workflows must install just through ./.github/actions/setup-just' "$work/stderr" || {
  echo 'check-workflows-e2e: the setup-just drift lacked the expected diagnostic' >&2
  echo "  fix: restore the wording 'workflows must install just through ./.github/actions/setup-just' in scripts/check-workflows.sh, or update this expectation to the new wording" >&2
  cat "$work/stderr" >&2
  exit 1
}

cp "$work/ci.yml" "$ci"

# The semver gate's two halves, which fail in opposite directions: without the
# analysis run it degrades to a presence probe that passes on a tool that cannot
# build rustdoc, and without the toolchain the analysis cannot run at all. Both
# leave `semver_check = true` looking enforced, which is the whole hazard.
while IFS='|' read -r pattern expected; do
  # The single-quoted program is JavaScript; the argument is fixture text.
  # shellcheck disable=SC2016
  node -e '
    const fs = require("node:fs");
    const [path, needle] = process.argv.slice(1);
    const source = fs.readFileSync(path, "utf8").replaceAll("\r\n", "\n");
    const line = source.split("\n").find((l) => l.includes(needle));
    if (!line) throw new Error(`semver gate fixture is missing: ${needle}`);
    fs.writeFileSync(path, source.split("\n").filter((l) => l !== line).join("\n"));
  ' "$release_plz" "$pattern"

  if bash scripts/check-workflows.sh >"$work/stdout" 2>"$work/stderr"; then
    echo "check-workflows-e2e: release-plz.yml without '$pattern' unexpectedly passed the gate" >&2
    echo "  fix: restore the require_line for '$pattern' in scripts/check-workflows.sh" >&2
    exit 1
  fi
  grep -Fq "$expected" "$work/stderr" || {
    echo "check-workflows-e2e: the semver drift failure lacked the expected diagnostic" >&2
    echo "  fix: restore the wording '$expected' in scripts/check-workflows.sh, or update this expectation to the new wording" >&2
    cat "$work/stderr" >&2
    exit 1
  }
  cp "$work/release-plz.yml" "$release_plz"
done <<'CASES'
run: cargo-semver-checks check-release --workspace --baseline-rev HEAD|run the semver analysis itself
RUSTUP_TOOLCHAIN=stable|give cargo-semver-checks the toolchain it needs
CASES

# The release's consumption of CI's verdict, and the consumer-operation waits.
# Each case leaves release.yml in a shape the gate must refuse, because each is a
# shape this workflow has had: a gate re-run on an already-gated commit, and a
# metadata probe read as installability.
expect_gate_refusal() {
  local expected="$1"
  if bash scripts/check-workflows.sh >"$work/stdout" 2>"$work/stderr"; then
    echo "check-workflows-e2e: release.yml without '$expected' unexpectedly passed the gate" >&2
    echo "  fix: restore that requirement in scripts/check-workflows.sh" >&2
    exit 1
  fi
  grep -Fq "$expected" "$work/stderr" || {
    echo "check-workflows-e2e: the release drift failure lacked the expected diagnostic" >&2
    echo "  fix: restore the wording '$expected' in scripts/check-workflows.sh, or update this expectation to the new wording" >&2
    cat "$work/stderr" >&2
    exit 1
  }
  cp "$work/release.yml" "$workflow"
}

# A missing line: the verdict is never read, so every release re-runs the gate.
while IFS='|' read -r pattern expected; do
  # The single-quoted program is JavaScript; the argument is fixture text.
  # shellcheck disable=SC2016
  node -e '
    const fs = require("node:fs");
    const [path, needle] = process.argv.slice(1);
    const source = fs.readFileSync(path, "utf8").replaceAll("\r\n", "\n");
    const line = source.split("\n").find((l) => l.includes(needle));
    if (!line) throw new Error(`release fixture is missing: ${needle}`);
    fs.writeFileSync(path, source.split("\n").filter((l) => l !== line).join("\n"));
  ' "$workflow" "$pattern"
  expect_gate_refusal "$expected"
done <<'CASES'
run: scripts/ci-verdict.sh|read CI's verdict for the tagged commit
run: scripts/verify-published.sh npm-cli|verify the published npm-cli with the consumer's own install
CASES

# A missing GUARD rather than a missing line: `just check` is still there, but
# nothing conditions it, so it runs on every release again.
# The single-quoted program is JavaScript.
# shellcheck disable=SC2016
node -e '
  const fs = require("node:fs");
  const path = process.argv[1];
  const lines = fs.readFileSync(path, "utf8").replaceAll("\r\n", "\n").split("\n");
  const at = lines.findIndex((l) => l.trim() === "run: just check");
  if (at < 1) throw new Error("the conditioned gate fixture is missing");
  lines.splice(at - 1, 1);
  fs.writeFileSync(path, lines.join("\n"));
' "$workflow"
expect_gate_refusal "run the complete repository gate only when CI reached no verdict for the tagged commit"

# A forbidden pattern: a registry's metadata API answering, read as a consumer
# being able to install.
printf '          npm view "oneharness-cli@1.2.3" version\n' >>"$workflow"
expect_gate_refusal "must not wait on a registry's metadata API"

# The other forbidden pattern: a gate `just check` already contains, run again
# here against the same commit.
printf '        run: just sdk-check\n' >>"$workflow"
expect_gate_refusal "must not run an SDK gate"

# A SECOND copy of the gate, this one unconditioned. The guard requirement is
# about every occurrence: one guarded copy says nothing about a sibling that
# runs on every release.
printf '        run: just check\n' >>"$workflow"
expect_gate_refusal "run the complete repository gate only when CI reached no verdict for the tagged commit"

echo 'check-workflows-e2e: ok'
