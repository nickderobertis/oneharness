// Drive e2e-lib.sh's real absent-harness branch under each check platform's
// gate env, with claude-code off PATH: it must skip, and the old job-level
// OH_E2E_NO_SKIP demand must turn it into the CI failure it once caused.
import assert from "node:assert/strict";
import { existsSync, mkdtempSync, rmSync, writeFileSync, chmodSync } from "node:fs";
import { join, delimiter } from "node:path";
import { tmpdir } from "node:os";
import { spawnSync } from "node:child_process";
import { checkJob, gateEnv } from "../gate-env.mjs";

if (spawnSync("bash", ["-c", "command -v jq"]).status !== 0) {
  console.log("gate-env-journey: skipped (jq is not installed; oh_usage_enforce parses its report with it)");
  process.exit(0);
}
const ci = checkJob();
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
  const probe = (env) =>
    spawnSync("bash", ["-c", "set -euo pipefail; source scripts/e2e-lib.sh; oh_usage_enforce claude-code"], {
      encoding: "utf8",
      env: { ...ambient, PATH: path, ONEHARNESS_BIN: stub, FAKE_STDOUT: absent, ...env },
    });
  assert.notEqual(spawnSync("bash", ["-c", "command -v claude"], { env: { PATH: path } }).status, 0);
  for (const os of ci.check.strategy.matrix.os) {
    const result = probe(gateEnv(ci, os, "affected"));
    assert.equal(result.status, 0, `${os}: ${result.stdout}${result.stderr}`);
    assert.match(result.stderr, /SKIP:.*not installed/, `${os}: absent harness skips`);
  }
  const refused = probe({ ...gateEnv(ci, "ubuntu-latest", "affected"), OH_E2E_NO_SKIP: "1" });
  assert.equal(refused.status, 1);
  assert.match(refused.stderr, /skip disallowed \(OH_E2E_NO_SKIP set\).*not installed/);
  console.log("gate-env-journey: every check platform's gate skips an absent harness; a no-skip demand refuses it");
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
