// Run e2e-support:test's real usage-enforce check, the way that target runs it,
// under each check platform's gate env with claude-code off PATH. The CI check
// job once leaked the live suites' OH_E2E_NO_SKIP into its gate, and this check
// then failed on every platform; it must pass under the env ci.yml gives now.
import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { join, delimiter } from "node:path";
import { spawnSync } from "node:child_process";
import { checkJob, gateEnv } from "../gate-env.mjs";

const ci = checkJob();
// The host PATH minus every directory holding a claude-code CLI.
const path = (process.env.PATH ?? "")
  .split(delimiter)
  .filter((dir) => dir && !existsSync(join(dir, "claude")) && !existsSync(join(dir, "claude.exe")))
  .join(delimiter);
assert.notEqual(spawnSync("bash", ["-c", "command -v claude"], { env: { PATH: path } }).status, 0);
const ambient = Object.fromEntries(Object.entries(process.env).filter(([k]) => !k.startsWith("OH_E2E_")));
// Platforms whose gate env coincides run once: the check takes tens of seconds.
const distinct = new Map();
for (const os of ci.check.strategy.matrix.os) {
  const env = gateEnv(ci, os, "affected");
  const key = JSON.stringify(env);
  distinct.set(key, { env, platforms: [...(distinct.get(key)?.platforms ?? []), os] });
}
for (const { env, platforms } of distinct.values()) {
  const run = spawnSync("bash", ["scripts/with-portable-sed.sh", "scripts/check-usage-enforce.sh"], {
    encoding: "utf8",
    env: { ...ambient, PATH: path, ...env },
  });
  assert.equal(run.status, 0, `${platforms.join(", ")}: e2e-support's usage-enforce check failed under the gate env:\n${run.stdout}${run.stderr}`);
}
console.log(`gate-env-journey: e2e-support's usage-enforce check passes under the gate env of ${ci.check.strategy.matrix.os.join(", ")} without claude-code`);
