// The literal environment each check-matrix platform's gate step runs with
// (workflow, job and step env, expressions dropped), read from ci.yml itself.
import { readFileSync } from "node:fs";
import { parse } from "yaml";
import { evaluate } from "./workflow-graph.mjs";

const mapping = (value) => value !== null && typeof value === "object" && !Array.isArray(value);
const scalar = (value) => ["string", "number", "boolean"].includes(typeof value);

function validEnv(env, where) {
  if (env === undefined) return {};
  if (!mapping(env) || Object.values(env).some((value) => !scalar(value)))
    throw Error(`Invalid ${where} env: restore a mapping of scalar values`);
  return env;
}

export function checkJob(path = ".github/workflows/ci.yml") {
  const workflow = parse(readFileSync(path, "utf8"));
  const check = workflow?.jobs?.check;
  if (
    !mapping(workflow) || !mapping(check) ||
    !Array.isArray(check.strategy?.matrix?.os) ||
    check.strategy.matrix.os.some((os) => typeof os !== "string") ||
    !Array.isArray(check.steps) ||
    check.steps.some((step) => !mapping(step) ||
      ["run", "if"].some((key) => step[key] !== undefined && typeof step[key] !== "string"))
  )
    throw Error(`Invalid ${path}: restore the check job's string os matrix and string command/condition steps`);
  validEnv(workflow.env, "workflow");
  validEnv(check.env, "check job");
  for (const step of check.steps) validEnv(step.env, `check step ${JSON.stringify(step.name ?? step.run ?? step.uses)}`);
  return { workflow, check };
}

export function gateSteps(check) {
  return check.steps.filter((s) => /^just check( all)?$/.test(s.run ?? ""));
}

export function gateEnv({ workflow, check }, os, tier) {
  const context = {
    matrix: { os },
    needs: { "macos-policy": { outputs: { macos_required: "true" }, result: "success" } },
    steps: { tier: { outputs: { tier, base: "origin/main" } } },
    github: { event_name: "pull_request" },
  };
  const step = gateSteps(check).find((s) => evaluate(s.if, context));
  if (!step) throw Error(`${os}/${tier}: no gate step runs; restore the check job's just check steps`);
  const literal = (env, where) =>
    Object.fromEntries(
      Object.entries(validEnv(env, where))
        .filter(([, v]) => !String(v).includes("${{"))
        .map(([k, v]) => [k, String(v)]),
    );
  return { ...literal(workflow.env, "workflow"), ...literal(check.env, "check job"), ...literal(step.env, "gate step") };
}
