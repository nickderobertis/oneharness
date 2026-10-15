// Run e2e-support:test's real usage-enforce check, the way that target runs it,
// under the env ci.yml gives its check job's gate steps, with claude-code off
// PATH. That job once leaked the live suites' OH_E2E_NO_SKIP into its gate and
// this check then failed on every platform. Which gate step each platform runs
// is .github/tests/gate-env.test.mjs's to prove; this owns the run.
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { join, delimiter } from "node:path";
import { spawnSync } from "node:child_process";
import { parse } from "yaml";

const mapping = (value) => value !== null && typeof value === "object" && !Array.isArray(value);
const workflow = parse(readFileSync(".github/workflows/ci.yml", "utf8"));
const check = workflow?.jobs?.check;
if (!mapping(workflow) || !mapping(check) || !Array.isArray(check.steps) || check.steps.some((step) => !mapping(step)))
  throw Error("Invalid .github/workflows/ci.yml: restore the check job's steps");
const literal = (env, where) => {
  if (env === undefined) return {};
  if (!mapping(env) || Object.values(env).some((v) => !["string", "number", "boolean"].includes(typeof v)))
    throw Error(`Invalid ci.yml ${where} env: restore a mapping of scalar values`);
  return Object.fromEntries(Object.entries(env).filter(([, v]) => !String(v).includes("${{")).map(([k, v]) => [k, String(v)]));
};
const gates = check.steps.filter((step) => /^just check( all)?$/.test(step.run ?? ""));
assert.equal(gates.length, 2, "ci.yml's check job must keep its affected and full-sweep gate steps");
const path = (process.env.PATH ?? "")
  .split(delimiter)
  .filter((dir) => dir && !existsSync(join(dir, "claude")) && !existsSync(join(dir, "claude.exe")))
  .join(delimiter);
assert.notEqual(spawnSync("bash", ["-c", "command -v claude"], { env: { PATH: path } }).status, 0);
const ambient = Object.fromEntries(Object.entries(process.env).filter(([k]) => !k.startsWith("OH_E2E_")));
// Gate steps whose env coincides run once: the check takes tens of seconds.
const distinct = new Map();
for (const step of gates) {
  const env = { ...literal(workflow.env, "workflow"), ...literal(check.env, "check job"), ...literal(step.env, step.run) };
  distinct.set(JSON.stringify(env), { env, steps: [...(distinct.get(JSON.stringify(env))?.steps ?? []), step.run] });
}
for (const { env, steps } of distinct.values()) {
  const run = spawnSync("bash", ["scripts/with-portable-sed.sh", "scripts/check-usage-enforce.sh"], {
    encoding: "utf8",
    env: { ...ambient, PATH: path, ...env },
  });
  assert.equal(run.status, 0, `${steps.join(", ")}: e2e-support's usage-enforce check failed under the gate env:\n${run.stdout}${run.stderr}`);
}
console.log("gate-env-journey: e2e-support's usage-enforce check passes under ci.yml's gate env without claude-code");
