// The literal environment each check-matrix platform's gate step runs with
// (workflow, job and step env, expressions dropped), read from ci.yml itself.
import { readFileSync } from "node:fs";
import { parse } from "yaml";
import { evaluate } from "./workflow-graph.mjs";

export function checkJob(path = ".github/workflows/ci.yml") {
  const workflow = parse(readFileSync(path, "utf8"));
  const check = workflow?.jobs?.check;
  if (!Array.isArray(check?.steps) || !Array.isArray(check?.strategy?.matrix?.os))
    throw Error(`Invalid ${path}: restore the check job's os matrix and steps`);
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
  const literal = (env) =>
    Object.fromEntries(
      Object.entries(env ?? {})
        .filter(([, v]) => !String(v).includes("${{"))
        .map(([k, v]) => [k, String(v)]),
    );
  return { ...literal(workflow.env), ...literal(check.env), ...literal(step.env) };
}
