import { assertRevision } from "./git-input.mjs";
import { execFileSync } from "node:child_process";
import { parse } from "yaml";
import { readPlatforms } from "../tools/release-tooling/payload-contract/io.mjs";
import { workflows, resolveJobs, validateWorkflow } from "./workflow-graph.mjs";
const base = assertRevision(process.argv[2] ?? "origin/main", "baseline");
const final = workflows();
const before = Object.fromEntries(
  Object.keys(final).map((file) => [
    file,
    validateWorkflow(parse(
      execFileSync("git", ["show", `${base}:.github/workflows/${file}`], {
        encoding: "utf8",
      }),
    ),file),
  ]),
);
function counts(file, all) {
  const workflow = all[file];
  const on = workflow.on;
  return Object.keys(on).flatMap((event) => {
    const cases = event === "workflow_dispatch" ? [false, true] : [false];
    return cases.map((manual) => {
      const scenario = {
        event,
        branch: "main",
        action:
          event === "release"
            ? "published"
            : event === "pull_request"
              ? "synchronize"
              : "",
        manual,
        macosRequired: event === "pull_request",
        releaseOnly: false,
        fork: false,
        publish: "both",
      };
      return [
        event +
          (event === "workflow_dispatch"
            ? manual
              ? " opt-in"
              : " default"
            : ""),
        resolveJobs(file, scenario, all).filter((j) => j.macos).length,
      ];
    });
  });
}
console.log(
  "Contract-level scheduling inventory, derived from baseline " +
    base +
    " and the finished workflow files. Hosted PR evidence is listed separately; release routes are modeled, not published.\n",
);
console.log(
  "| Workflow | Trigger / Mac jobs before | Trigger / Mac jobs after |\n|---|---|---|",
);
for (const file of Object.keys(final)) {
  const old = counts(file, before),
    current = counts(file, final);
  if (![...old, ...current].some(([, count]) => count)) continue;
  console.log(
    `| ${file} | ${old.map(([event, count]) => `${event}: ${count}`).join("; ")} | ${current.map(([event, count]) => `${event}: ${count}`).join("; ")} |`,
  );
}
console.log(
  "\nOrdinary PR CI requests Mac; automation version/changelog-only PRs retain the same protected contexts through a labeled Linux omission carrier. Forks receive deterministic Mac coverage without live credentials. Main/base/tag pushes, schedules and downstream events request no Mac.\n",
);
const platforms = readPlatforms();
console.log("| Retained target | Archive | PyPI | npm |\n|---|---|---|---|");
for (const p of platforms)
  console.log(
    `| ${p.target} | ${p.archive} + sha256 + Sigstore bundle | platform wheel | @oneharness/cli-${p.npm} |`,
  );
console.log(
  "\nBoth Darwin targets build in one Apple Silicon job. Each target is compiled once through maturin; the stripped wheel payload supplies its archive and npm package. Native and Rosetta install/run proofs precede Linux publication. Existing Windows ARM build/install lanes and SDK publishing remain.\n",
);

console.log(
  "Budget-site ignore directives were omitted per the planner's final ruling because this repository does not configure budgets_track_product_owner_outcomes. Plain comments retain the approved reasons; llmlint.yml and the two budget bounds remain unchanged.",
);
