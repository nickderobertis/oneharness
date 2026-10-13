import { test } from "node:test";
import assert from "node:assert/strict";
import { checkJob, gateEnv, gateSteps } from "../gate-env.mjs";

// The check job's deterministic gate ran with the live suites' OH_E2E_NO_SKIP
// once, and e2e-support:test's hermetic check-usage-enforce.sh then refused its
// own "absent harness skips" case on every platform. These pin that the gate
// steps each platform runs see none of the live-only OH_E2E_* demands;
// .github/integration/gate-env-journey.mjs drives the real helper under them.
const ci = checkJob();

test("every platform's gate step runs without the live suites' no-skip demand", () => {
  assert.equal(gateSteps(ci.check).length, 2, "affected and full-sweep gate steps");
  for (const os of ci.check.strategy.matrix.os) {
    for (const tier of ["affected", "all"]) {
      for (const key of Object.keys(gateEnv(ci, os, tier)))
        assert.ok(!key.startsWith("OH_E2E_"), `${os}/${tier}: gate inherits ${key}`);
    }
  }
});

test("a job- or workflow-level no-skip demand reaches every platform's gate", () => {
  for (const level of ["workflow", "check"]) {
    const doubled = structuredClone(ci);
    doubled[level].env = { ...(doubled[level].env ?? {}), OH_E2E_NO_SKIP: 1 };
    for (const os of ci.check.strategy.matrix.os)
      assert.equal(gateEnv(doubled, os, "affected").OH_E2E_NO_SKIP, "1", `${level}/${os}`);
  }
});
