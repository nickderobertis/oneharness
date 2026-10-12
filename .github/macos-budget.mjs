import { assertTelemetry } from "./telemetry-schema.mjs";
import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { report } from "@onebudgetspec/sdk";
import { digest } from "./workflow-graph.mjs";
const { values } = parseArgs({
  options: { metric: { type: "string" }, telemetry: { type: "string" } },
});
const path = resolve(values.telemetry ?? "");
process.chdir(resolve(dirname(fileURLToPath(import.meta.url)), ".."));
const telemetry = assertTelemetry(JSON.parse(readFileSync(path, "utf8")));
if (telemetry.sourceDigest !== digest())
  throw Error("Stale Mac telemetry: run github-workflows:test on this tree");
const key = {
  "jobs-per-run": "jobsPerRun",
  "runs-per-change": "runsPerChange",
}[values.metric];
if (!key || !Number.isFinite(telemetry[key]))
  throw Error("Missing metric in Mac telemetry");
const detail = JSON.stringify({
  sourceDigest: telemetry.sourceDigest,
  scenarioCount: telemetry.scenarios.length,
  scenarios: telemetry.scenarios.map((s) => ({
    event: s.event,
    action: s.action,
    diff: s.diff,
    fork: s.fork,
    publish: s.publish,
    manual: s.manual,
    ...(s.automation === undefined ? {} : { automation: s.automation }),
    runs: s.runs
      .filter((r) => r.jobs.some((j) => j.macos))
      .map((r) => ({
        workflow: r.workflow,
        jobs: r.jobs.filter((j) => j.macos),
      })),
  })),
  lifecycle: telemetry.lifecycle.map((l) => ({
    ...l,
    runs: l.runs.map((r) => ({ ...r, jobs: r.jobs.filter((j) => j.macos) })),
  })),
});
report(telemetry[key], detail);
console.log(`${values.metric}: ${telemetry[key]}`);
