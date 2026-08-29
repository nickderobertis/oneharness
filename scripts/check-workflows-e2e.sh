#!/usr/bin/env bash
# Prove the workflow drift gate rejects a missing release-tag boundary check, a
# third-party `just` installer and a deleted unattended-failure reporter — required
# lines and a forbidden pattern, so every mechanism the gate has is exercised
# rather than assumed.
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

# The unattended-reporting gate's two halves, one per workflow so both files are
# proven rather than one standing in for the other. Neither is sufficient alone:
# a reporter nothing gates on `if: failure()` files an issue for a green release,
# and a failure condition with no reporter under it announces nothing. Deleting
# either from either workflow is the drift that leaves a broken release
# reporting to nobody, which is the state this gate exists to refuse.
while IFS='|' read -r file needle expected; do
  # The single-quoted program is JavaScript; the argument is fixture text.
  # shellcheck disable=SC2016
  node -e '
    const fs = require("node:fs");
    const [path, needle] = process.argv.slice(1);
    const source = fs.readFileSync(path, "utf8").replaceAll("\r\n", "\n");
    const line = source.split("\n").find((l) => l.includes(needle));
    if (!line) {
      throw new Error(
        `${path} has no line containing "${needle}", so this case cannot delete it to prove the gate rejects its absence. ` +
        "Fix: restore that line in the workflow, or update the CASES table at the bottom of scripts/check-workflows-e2e.sh to the wording it now uses."
      );
    }
    fs.writeFileSync(path, source.split("\n").filter((l) => l !== line).join("\n"));
  ' ".github/workflows/$file" "$needle"

  if bash scripts/check-workflows.sh >"$work/stdout" 2>"$work/stderr"; then
    echo "check-workflows-e2e: $file without '$needle' unexpectedly passed the gate" >&2
    echo "  fix: restore the unattended-reporting check for '$needle' in scripts/check-workflows.sh" >&2
    exit 1
  fi
  grep -Fq "$expected" "$work/stderr" || {
    echo "check-workflows-e2e: the unattended-reporting drift lacked the expected diagnostic" >&2
    echo "  fix: restore the wording '$expected' in scripts/check-workflows.sh, or update this expectation to the new wording" >&2
    cat "$work/stderr" >&2
    exit 1
  }
  cp "$work/$file" ".github/workflows/$file"
done <<'CASES'
release.yml|run: bash scripts/report-workflow-failure.sh|release.yml must report its own failure through the reporter
release-plz.yml|if: failure()|release-plz.yml must gate its reporting job on `if: failure()`
CASES

echo 'check-workflows-e2e: ok'
