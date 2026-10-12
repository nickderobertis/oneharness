import { assertPayload } from "../../tools/release-tooling/payload-contract/contract.mjs";
// Slow release-edge proof: consume a real native bundle, double only Actions
// artifact transport and the forge CLI, and execute the workflow's copy/upload
// commands. This proves transport; it does not execute a Darwin binary on Linux.
import {
  readFileSync,
  readdirSync,
  mkdtempSync,
  cpSync,
  mkdirSync,
  writeFileSync,
  chmodSync,
  rmSync,
} from "node:fs";
import { join, resolve } from "node:path";
import { tmpdir } from "node:os";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { parseArgs } from "node:util";
import { parse } from "yaml";
function render(value, context) {
  return value.replace(/\$\{\{([\s\S]*?)\}\}/g, (_, expression) => {
    const key = expression.trim();
    if (key === "matrix.target") return context.matrix.target;
    if (key === "github.repository") return context.github.repository;
    throw Error(`Unknown transfer expression ${key}. Next: reconcile the integration renderer with release.yml.`);
  });
}
const { values } = parseArgs({ options: { root: { type: "string" } } });
const root = resolve(values.root);
const target = readdirSync(root).find((t) =>
  t.includes(
    process.platform === "darwin" ? "apple-darwin" : "unknown-linux-gnu",
  ),
);
if (!target)
  throw Error("Build a native release bundle before the transfer journey");
const source = join(root, target);
const record = assertPayload(
  JSON.parse(readFileSync(join(source, "payload.json"), "utf8")),
);
const workflow = parse(readFileSync(".github/workflows/release.yml", "utf8"));
const work = mkdtempSync(
  join(
    process.env.ONEPIPELINE_NODE_SCRATCH_DIR ?? tmpdir(),
    "release-transfer-",
  ),
);
const hash = (p) => createHash("sha256").update(readFileSync(p)).digest("hex");
try {
  // llmlint: ignore[e2e_not_mocked] Only GitHub Actions artifact transport is doubled; packaging and the workflow's staging commands run for real, without publishing a release.
  cpSync(source, join(work, "darwin-release", target), { recursive: true });
  for (const dir of ["bin", "dist", "npm-artifacts"])
    mkdirSync(join(work, dir));
  const forge = join(work, "bin", "gh");
  // llmlint: ignore[e2e_not_mocked] Only forge upload transport is doubled; this proof runs real packaging commands and may not publish a release.
  writeFileSync(
    forge,
    '#!/usr/bin/env bash\nset -euo pipefail\n[ "$1" = release ] && [ "$2" = upload ] || exit 1\nprintf "%s\\n" "$@" > "$PROOF_FORGE_ARGS"\n',
  );
  chmodSync(forge, 0o755);
  const env = {
    ...process.env,
    PATH: join(work, "bin") + ":" + process.env.PATH,
    GITHUB_REF_NAME: record.tag,
    GITHUB_WORKSPACE: work,
    PROOF_FORGE_ARGS: join(work, "forge-argv"),
  };
  const context = {
    matrix: { target },
    github: { repository: "nickderobertis/oneharness" },
  };
  for (const [job, name] of [
    ["upload", "Stage and publish the Mac archive"],
    ["publish-pypi", "Stage Darwin packages"],
    ["publish-npm", "Stage Darwin packages"],
  ]) {
    const step = workflow.jobs[job].steps.find((s) => s.name === name);
    if (!step?.run) throw Error(`Missing real transfer step ${job}/${name}`);
    // llmlint: ignore[tests_mirror_real_usage] Execute the actual workflow command after substituting only known expressions; hosted Actions/forge transport is doubled because this proof may not publish a release.
    execFileSync("bash", ["-e", "-c", render(step.run, context)], {
      cwd: work,
      env,
      stdio: "inherit",
    });
  }
  const archive = `oneharness-${record.tag}-${target}.tar.gz`,
    checksum = `oneharness-${record.tag}-${target}.sha256`;
  for (const file of [archive, checksum])
    if (hash(join(source, file)) !== hash(join(work, file)))
      throw Error("Archive transfer changed bytes");
  if (
    hash(join(source, "wheels", record.wheel)) !==
    hash(join(work, "dist", record.wheel))
  )
    throw Error("Wheel transfer changed bytes");
  for (const file of readdirSync(join(source, "npm")))
    if (
      hash(join(source, "npm", file)) !==
      hash(join(work, "npm-artifacts", file))
    )
      throw Error("npm transfer changed bytes");
  const args = readFileSync(join(work, "forge-argv"), "utf8")
    .trim()
    .split("\n");
  if (
    args.join("\n") !==
    ["release", "upload", record.tag, archive, checksum, "--clobber"].join("\n")
  )
    throw Error(
      `Unexpected forge transport arguments: ${JSON.stringify(args)}`,
    );
  console.log(
    `release-transfer: real ${target} archive/wheel/npm bytes reached publication staging; forge transport doubled`,
  );
} finally {
  rmSync(work, { recursive: true, force: true });
}
