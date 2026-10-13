import { assertRevision } from "./git-input.mjs";
import { readFileSync, readdirSync, appendFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { parse } from "yaml";
import { matches, validateWorkflow } from "./workflow-graph.mjs";
if (!process.env.GITHUB_OUTPUT?.trim()) throw Error("Missing GITHUB_OUTPUT. Next: supply the Actions output file path.");
const paths = execFileSync(
  "git",
  [
    "diff",
    "--name-only",
    "-z",
    assertRevision(process.env.BASE_SHA, "BASE_SHA"),
    assertRevision(process.env.HEAD_SHA, "HEAD_SHA"),
    "--",
  ],
  { encoding: "utf8" },
)
  .split("\0")
  .filter(Boolean);

for (const file of readdirSync(".github/workflows").filter((f) =>
  /^e2e-.*\.yml$/.test(f),
)) {
  const workflow = validateWorkflow(parse(readFileSync(`.github/workflows/${file}`, "utf8")), file);
  const patterns = workflow.on.pull_request?.paths ?? [];
  const name = file.slice(4, -4);
  // Turn control's former schedule proof is consolidated on every trusted PR.
  const selected =
    name === "control" || paths.some((p) => matches(p, patterns));
  appendFileSync(process.env.GITHUB_OUTPUT, `${name}=${selected}\n`);
}
