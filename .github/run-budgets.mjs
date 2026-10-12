// The CLI alone compares approved limits; keep its successful report concise.
import { readFileSync } from "node:fs";
import { parse } from "yaml";
import { spawnSync } from "node:child_process";
const run = spawnSync(
  "uv",
  [
    "tool",
    "run",
    "--from",
    "onebudgetspec-cli==0.1.4",
    "onebudgetspec",
    "check",
    ".github/budgets.yaml",
    "--json",
  ],
  { encoding: "utf8", maxBuffer: 16 * 1024 * 1024 },
);
if (![0, 1].includes(run.status)) {
  process.stderr.write(run.stderr);
  throw Error(
    `onebudgetspec did not return a check report (exit ${run.status}). Next: inspect its error and rerun github-workflows:budgets.`,
  );
}
const report = JSON.parse(run.stdout);
if (
  report === null || typeof report !== "object" || Array.isArray(report) ||
  report.schema_version !== 1 ||
  !Array.isArray(report.results) ||
  !report.results.length
)
  throw Error(
    "Invalid onebudgetspec check report. Next: verify the pinned CLI installation.",
  );
const document = parse(readFileSync(".github/budgets.yaml", "utf8"));
const declarations = document?.budgets;
if (!Array.isArray(declarations) || !declarations.length || declarations.some((b) => b === null || typeof b !== "object" || typeof b.id !== "string" || typeof b.unit !== "string" || b.direction !== "max" || typeof b.threshold !== "number" || !Number.isFinite(b.threshold)))
 throw Error("Invalid budget declarations. Next: repair budgets.yaml to the pinned CLI schema.");
if (report.results.some((r) => r === null || typeof r !== "object" || Array.isArray(r)))
 throw Error("Invalid budget result entries. Next: inspect the pinned CLI report.");
if (report.results.length !== declarations.length || new Set(report.results.map((r) => r.id)).size !== declarations.length)
  throw Error("Budget report identities differ from declarations. Next: inspect the pinned CLI JSON report.");
for (const result of report.results) {
  const declared = declarations.find((b) => b.id === result?.id);
  if (!declared || result.unit !== declared.unit || result.threshold !== declared.threshold || result.direction !== declared.direction || !["within", "exceeded", "error"].includes(result.verdict) || (result.verdict !== "error" && (typeof result.actual !== "number" || !Number.isFinite(result.actual))))
    throw Error(`Invalid budget result ${JSON.stringify(result?.id)}. Next: inspect the pinned CLI report against budgets.yaml.`);
  if (run.status === 0 && result.verdict !== "within") throw Error("Successful CLI exit contradicts its verdict. Next: inspect the pinned CLI report.");
  console.log(
    `${result.id}: ${result.verdict}; ${result.actual} ${result.unit}, limit ${result.threshold}`,
  );
  if (result.verdict !== "within")
    process.stderr.write(
      (result.error ?? result.detail ?? "Inspect the budget check report") +
        "\n",
    );
}
process.exitCode = run.status;
