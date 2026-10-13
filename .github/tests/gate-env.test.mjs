import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, existsSync } from "node:fs";
import { join, delimiter } from "node:path";
import { spawnSync } from "node:child_process";
import { parse } from "yaml";
import { evaluate } from "../workflow-graph.mjs";

// The check job's deterministic gate ran with the live suites' OH_E2E_NO_SKIP
// once, and e2e-support:test's hermetic check-usage-enforce.sh then refused its
// own "absent harness skips" case on every platform. These pin that the gate
// steps each platform runs see none of the live-only OH_E2E_* demands, and that
// the hermetic check passes under exactly the environment the workflow gives.
const workflow = parse(readFileSync(".github/workflows/ci.yml", "utf8"));
const check = workflow.jobs.check;
const gateSteps = check.steps.filter((s) => /^just check( all)?$/.test(s.run ?? ""));

function gateEnv(os, tier) {
  const context = {
    matrix: { os },
    needs: { "macos-policy": { outputs: { macos_required: "true" }, result: "success" } },
    steps: { tier: { outputs: { tier, base: "origin/main" } } },
    github: { event_name: "pull_request" },
  };
  const step = gateSteps.find((s) => evaluate(s.if, context));
  assert.ok(step, `${os}/${tier}: a gate step must run`);
  const literal = (env) =>
    Object.fromEntries(
      Object.entries(env ?? {}).filter(([, v]) => !String(v).includes("${{")).map(([k, v]) => [k, String(v)]),
    );
  return { ...literal(workflow.env), ...literal(check.env), ...literal(step.env) };
}

test("every platform's gate step runs without the live suites' no-skip demand", () => {
  assert.equal(gateSteps.length, 2, "affected and full-sweep gate steps");
  for (const os of check.strategy.matrix.os) {
    for (const tier of ["affected", "all"]) {
      const env = gateEnv(os, tier);
      for (const key of Object.keys(env))
        assert.ok(!key.startsWith("OH_E2E_"), `${os}/${tier}: gate inherits ${key}`);
    }
  }
});

test("a job-level no-skip demand is caught for every platform", () => {
  const saved = check.env;
  check.env = { ...(saved ?? {}), OH_E2E_NO_SKIP: 1 };
  try {
    for (const os of check.strategy.matrix.os)
      assert.equal(gateEnv(os, "affected").OH_E2E_NO_SKIP, "1");
  } finally {
    check.env = saved;
  }
});

test("hermetic usage-enforce check passes under each platform's gate env without claude-code", () => {
  // The host PATH minus every directory holding a claude-code CLI.
  const bin = (process.env.PATH ?? "")
    .split(delimiter)
    .filter((dir) => dir && !existsSync(join(dir, "claude")) && !existsSync(join(dir, "claude.exe")))
    .join(delimiter);
  // Every platform's gate env, run once per distinct value (they coincide
  // today, and the script takes tens of seconds).
  const envs = new Map();
  for (const os of check.strategy.matrix.os) {
    const ciEnv = gateEnv(os, "affected");
    const key = JSON.stringify(ciEnv);
    envs.set(key, [[...(envs.get(key)?.[0] ?? []), os], ciEnv]);
  }
  assert.equal(spawnSync("bash", ["-c", "command -v claude"], { env: { PATH: bin } }).status, 1);
  const run = (ciEnv, extra) =>
    spawnSync("bash", ["scripts/with-portable-sed.sh", "scripts/check-usage-enforce.sh"], {
      encoding: "utf8",
      env: { ...process.env, OH_E2E_NO_SKIP: "", PATH: bin, ...ciEnv, ...extra },
    });
  for (const [platforms, ciEnv] of envs.values()) {
    const result = run(ciEnv, {});
    assert.equal(result.status, 0, `${platforms}: ${result.stdout}${result.stderr}`);
  }
  // The failure this guards against, induced: the old job-level demand.
  const refused = run({}, { OH_E2E_NO_SKIP: "1" });
  assert.notEqual(refused.status, 0, "no-skip must refuse the absent-harness case");
  assert.match(refused.stderr + refused.stdout, /OH_E2E_NO_SKIP must turn the absence into a failure|skip disallowed/);
});
