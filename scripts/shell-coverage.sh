#!/usr/bin/env bash
#
# The shell line-coverage floor, enforced once over every project's shell test
# steps.
#
#   scripts/shell-coverage.sh
#
# Each shell test step (scripts/shell-test.sh, in some project's `test`) leaves
# its kcov report in target/coverage/shell/<project>/<run id>/. This merges all
# of them with `kcov --merge` and fails when the merged line rate is below the
# floor (SHELL_COVERAGE_MIN, default 54; kcov has no fail-under flag of its
# own). The steps are read from the project definitions themselves, so a new
# step cannot be left out by a list, and every one must have left its report:
# a missing one is a run that never happened, and leaving it out could only
# move the number.
#
# The denominator is every shell script under scripts/ and .githooks/, not only
# the ones a step ran: a kcov pass over a no-op, parsing both directories from
# this tree, joins the merge with each script at zero, so an untested script
# lowers the rate rather than vanishing from it.
#
# Linux only, with its reason: kcov publishes no binaries, so the pin is a
# source build made on Linux (scripts/shell-tools.sh), and the floor is a
# property of the suite measured there. macOS and Windows run the same tests
# uninstrumented and skip this.
#
# Quiet on success: one line. Writes a per-script table, lowest first, to
# target/coverage/shell-coverage.txt and the merged report to
# target/coverage/shell/merged/, the target's declared outputs.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)" || {
  echo "shell-coverage: could not resolve the repository root from ${BASH_SOURCE[0]}; run it from a checkout as 'bash scripts/shell-coverage.sh'" >&2
  exit 2
}
cd "$root" || {
  echo "shell-coverage: could not enter the repository root $root; check that it exists and is readable, then re-run" >&2
  exit 2
}

# The floor, below the 95% default for the reasons tools/shell-coverage/AGENTS.md
# records with the measurement it rests on. Raised as tests land; never lowered.
floor="${SHELL_COVERAGE_MIN:-54}"

if [ "$(uname -s)" != Linux ]; then
  echo "shell-coverage: skipped on $(uname -s) (kcov is built and the floor measured on Linux only — see scripts/shell-coverage.sh)"
  exit 0
fi
if ! [[ "$floor" =~ ^[0-9]+(\.[0-9]+)?$ ]] || ! awk -v f="$floor" 'BEGIN { exit !(f <= 100) }'; then
  echo "shell-coverage: SHELL_COVERAGE_MIN '$floor' is not a percentage from 0 to 100; unset it for the default or set a number such as 95" >&2
  exit 2
fi

fail() {
  echo "shell-coverage: $1" >&2
  shift
  for line in "$@"; do echo "  $line" >&2; done
  exit 1
}

# Every shell test step, one per line: the defining project's name, a tab, and
# the step's arguments after `bash scripts/shell-test.sh <project>`. A step
# naming another project, or a target not declaring that project's report
# directory as an output, would leave a report the cache never restores, so
# both are refused here.
# The single-quoted program is JavaScript.
# shellcheck disable=SC2016
steps="$(git ls-files --cached --others --exclude-standard '*project.json' | node -e '
  const fs = require("fs");
  let input = "";
  process.stdin.on("data", (d) => { input += d; }).on("end", () => {
    const problems = [];
    for (const file of input.split("\n").filter((f) => f && fs.existsSync(f))) {
      const project = JSON.parse(fs.readFileSync(file, "utf8"));
      for (const [name, target] of Object.entries(project.targets ?? {})) {
        const options = target.options ?? {};
        for (const command of [...(options.commands ?? []), options.command ?? ""]) {
          const step = typeof command === "string" ? command : command.command ?? "";
          const match = step.match(/^bash scripts\/shell-test\.sh (\S+) (.+)$/u);
          if (!match) continue;
          const output = `{workspaceRoot}/target/coverage/shell/${match[1]}`;
          if (!/^[a-z0-9-]+$/u.test(match[1])) {
            problems.push(`${file}: ${project.name}:${name} runs a step as project "${match[1]}", which is not an Nx project name`);
          } else if (match[1] !== project.name) {
            problems.push(`${file}: ${project.name}:${name} runs a step as project ${match[1]}; name ${project.name} in it`);
          } else if (!(target.outputs ?? []).includes(output)) {
            problems.push(`${file}: ${project.name}:${name} runs shell-test.sh but does not declare ${output} as an output`);
          } else {
            console.log(`${match[1]}\t${match[2]}`);
          }
        }
      }
    }
    if (problems.length > 0) {
      for (const problem of problems) console.error(problem);
      process.exit(1);
    }
  });
')" || fail "the project definitions declare shell test steps whose reports this cannot read (above)" \
  "fix: each as it says, then re-run 'bash scripts/shell-coverage.sh'"
[ -n "$steps" ] || fail "read no shell test steps from the project definitions" \
  "fix: run each shell test as 'bash scripts/shell-test.sh <project> scripts/<name>.sh [args...]', or update the reader here"

work="$(mktemp -d)" || fail "could not create a scratch directory (above)" "fix: check that ${TMPDIR:-/tmp} is writable and has room, then re-run"
trap 'rm -rf "$work" || echo "shell-coverage: could not remove its scratch $work (above); remove it by hand" >&2' EXIT
kcov="$(bash scripts/shell-tools.sh path kcov)"

reports=()
missing=()
while IFS=$'\t' read -r project args; do
  # A step's arguments are plain words, as the project definition spells them.
  read -r -a argv <<<"$args"
  id="$(bash scripts/shell-test.sh --id "${argv[@]}")"
  dir="target/coverage/shell/$project/$id"
  if [ -n "$(find "$dir" -name coverage.json -print -quit 2>/dev/null)" ]; then
    reports+=("$dir")
  else
    missing+=("$project: $args (expected $dir)")
  fi
done <<<"$steps"
[ "${#missing[@]}" -eq 0 ] || fail "${#missing[@]} shell test step(s) left no kcov report, so their runs never happened here:" \
  "${missing[@]}" \
  "fix: run this through Nx ('bash scripts/nx run shell-coverage:coverage'), which runs every test first"

printf '#!/usr/bin/env bash\ntrue\n' >"$work/noop.sh" ||
  fail "could not write the baseline's no-op script in $work (above)" "fix: check that ${TMPDIR:-/tmp} is writable and has room, then re-run"
include="$root/scripts,$root/.githooks"
"$kcov" "--bash-parse-files-in-dir=$include" "--include-path=$include" "$work/baseline" "$work/noop.sh" >/dev/null ||
  fail "kcov could not parse the scripts under scripts/ and .githooks/ for the baseline (above)" \
    "fix: run 'bash -n' on the script kcov names to find its syntax error; if none, reinstall kcov with 'just bootstrap'"
merged="target/coverage/shell/merged"
rm -rf "$merged" || fail "could not clear the previous merged report $merged (above)" "fix: check that target/coverage is writable, then re-run"
"$kcov" --merge "$merged" "$work/baseline" "${reports[@]}" >/dev/null ||
  fail "kcov could not merge the ${#reports[@]} shell test reports (above)" \
    "fix: delete target/coverage/shell and rerun through Nx with --skip-nx-cache, which writes every report afresh"

summary="target/coverage/shell-coverage.txt"
# The single-quoted program is JavaScript.
# shellcheck disable=SC2016
read -r rate covered total < <(node -e '
  const fs = require("fs");
  const [report, summary, root] = process.argv.slice(1);
  const merged = JSON.parse(fs.readFileSync(report, "utf8"));
  if (!Array.isArray(merged?.files) || merged.files.some((f) => typeof f?.file !== "string")) {
    console.error("the merged report has no list of scripts, each with a file name");
    process.exit(1);
  }
  const outside = merged.files.filter((f) => !f.file.startsWith(`${root}/scripts/`) && !f.file.startsWith(`${root}/.githooks/`));
  if (outside.length > 0) {
    console.error(`the merged report counts files outside scripts/ and .githooks/: ${outside.map((f) => f.file).join(", ")}`);
    process.exit(1);
  }
  const count = (value) => (/^[0-9]+$/u.test(String(value)) ? Number(value) : NaN);
  const rows = merged.files
    .map((f) => ({ file: f.file.slice(root.length + 1), covered: count(f.covered_lines), total: count(f.total_lines) }))
    .sort((a, b) => a.covered / (a.total || 1) - b.covered / (b.total || 1) || a.file.localeCompare(b.file));
  const bad = rows.filter((r) => !Number.isSafeInteger(r.covered) || !Number.isSafeInteger(r.total) || r.covered > r.total);
  if (rows.length === 0 || bad.length > 0) {
    console.error(`the merged report lists ${rows.length} scripts, with unreadable line counts for: ${bad.map((r) => r.file).join(", ") || "(none)"}`);
    process.exit(1);
  }
  const lines = rows.map((r) => `${(100 * r.covered / (r.total || 1)).toFixed(2).padStart(7)}%  ${String(r.covered).padStart(5)}/${String(r.total).padEnd(5)}  ${r.file}`);
  const covered = rows.reduce((n, r) => n + r.covered, 0);
  const total = rows.reduce((n, r) => n + r.total, 0);
  const rate = (100 * covered / (total || 1)).toFixed(2);
  fs.writeFileSync(summary, `shell line coverage: ${rate}% (${covered}/${total} lines, ${rows.length} scripts)\n\n${lines.join("\n")}\n`);
  console.log(`${rate} ${covered} ${total}`);
' "$merged/kcov-merged/coverage.json" "$summary" "$root") ||
  fail "could not read line counts from the merged report $merged/kcov-merged/coverage.json (above)" \
    "fix: delete target/coverage/shell and rerun through Nx with --skip-nx-cache; if it recurs, reinstall kcov with 'just bootstrap'"

if awk -v r="$rate" -v f="$floor" 'BEGIN { exit !(r < f) }'; then
  echo "shell-coverage: ${rate}% of shell lines covered ($covered/$total), below the ${floor}% floor" >&2
  echo "  the least-covered scripts ($summary has every one):" >&2
  sed -n '3,12p' "$summary" | sed 's/^/  /' >&2
  echo "  fix: test the untested lines (a shell test step in the project that runs the script); never lower the floor to pass" >&2
  exit 1
fi
echo "shell-coverage: ok (${rate}% of $total shell lines across ${#reports[@]} test steps, floor ${floor}%)"
