import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, existsSync, mkdtempSync, rmSync, writeFileSync, chmodSync } from "node:fs";
import { tmpdir } from "node:os";
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

test("absent-harness usage probe skips under each platform's gate env without claude-code", (t) => {
  if (spawnSync("bash", ["-c", "command -v jq"]).status !== 0) return t.skip("jq absent: oh_usage_enforce parses with it");
  const scratch = mkdtempSync(join(process.env.ONEPIPELINE_NODE_SCRATCH_DIR ?? tmpdir(), "gate-env-"));
  try {
    // The one boundary doubled is `oneharness usage`'s report, as
    // scripts/check-usage-enforce.sh does; e2e-lib.sh's real helper reads it.
    const stub = join(scratch, "oneharness");
    writeFileSync(stub, '#!/usr/bin/env bash\nprintf \'%s\\n\' "$FAKE_STDOUT"\n');
    chmodSync(stub, 0o755);
    const absent = JSON.stringify({
      schema_version: "0.1",
      identities: [{
        harness: "claude-code",
        availability: { state: "unknown", reason: { kind: "binary_missing", bin: "claude" } },
        reset_credits: { state: "reported", available_count: 0, credits: [] },
      }],
    });
    // The host PATH minus every directory holding a claude-code CLI.
    const path = (process.env.PATH ?? "")
      .split(delimiter)
      .filter((dir) => dir && !existsSync(join(dir, "claude")) && !existsSync(join(dir, "claude.exe")))
      .join(delimiter);
    const ambient = Object.fromEntries(Object.entries(process.env).filter(([k]) => !k.startsWith("OH_E2E_")));
    const probe = (ciEnv) =>
      spawnSync("bash", ["-c", 'set -euo pipefail; source scripts/e2e-lib.sh; oh_usage_enforce claude-code'], {
        encoding: "utf8",
        env: { ...ambient, PATH: path, ONEHARNESS_BIN: stub, FAKE_STDOUT: absent, ...ciEnv },
      });
    assert.notEqual(spawnSync("bash", ["-c", "command -v claude"], { env: { PATH: path } }).status, 0);
    for (const os of check.strategy.matrix.os) {
      const result = probe(gateEnv(os, "affected"));
      assert.equal(result.status, 0, `${os}: ${result.stdout}${result.stderr}`);
      assert.match(result.stderr, /SKIP:.*not installed/, `${os}: absent harness skips`);
    }
    // The CI failure this guards against, induced: the old job-level demand.
    const refused = probe({ ...gateEnv("ubuntu-latest", "affected"), OH_E2E_NO_SKIP: "1" });
    assert.equal(refused.status, 1);
    assert.match(refused.stderr, /skip disallowed \(OH_E2E_NO_SKIP set\).*not installed/);
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
});
