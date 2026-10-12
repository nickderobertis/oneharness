import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { parse } from "yaml";
import { evaluate } from "../workflow-graph.mjs";
test("real required-result guard permits only completed authorized routing", () => {
  const job = parse(readFileSync(".github/workflows/ci.yml", "utf8")).jobs
    .check;
  assert.equal(job.if, "always()");
  assert.equal(job.needs, "macos-policy");
  const guard = job.steps.find((s) => s.name === "Require routing success").run;
  for (const result of ["success", "failure", "cancelled", "skipped", ""])
    for (const required of ["true", "false", "", "invalid"]) {
      const run = spawnSync("bash", ["-e", "-c", guard], {
        env: {
          ...process.env,
          POLICY_RESULT: result,
          MACOS_REQUIRED: required,
        },
      });
      assert.equal(
        run.status === 0,
        result === "success" && ["true", "false"].includes(required),
        `${result}/${required}`,
      );
      assert.equal(
        evaluate(job.if, { needs: { "macos-policy": { result } } }),
        true,
      );
    }
  // Actions' shell must preserve a failing proof after successful prerequisite routing.
  const failed = spawnSync("bash", ["-e", "-c", guard + "\nexit 7\n"], {
    env: { ...process.env, POLICY_RESULT: "success", MACOS_REQUIRED: "true" },
  });
  assert.equal(failed.status, 7);
  const omission = job.steps.find((s) => s.name === "Authorized Mac omission");
  for (const required of ["true", "false", ""])
    assert.equal(
      evaluate(omission.if, {
        matrix: { os: "macos-latest" },
        needs: { "macos-policy": { outputs: { macos_required: required } } },
      }),
      required === "false",
    );
  assert.ok(omission.run.includes("omitted"));
});
