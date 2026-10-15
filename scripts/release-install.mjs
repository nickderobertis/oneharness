import {
  archiveMembers,
  assertPayload,
  assertPackageDirectory,
  assertPackResponse,
} from "./release-payload.mjs";
// Prove staged archive, wheel and npm installs on the executing Node architecture.
import { execFileSync, spawnSync } from "node:child_process";
import {
  mkdtempSync,
  rmSync,
  writeFileSync,
  readFileSync,
  readdirSync,
  mkdirSync,
} from "node:fs";
import { join, resolve } from "node:path";
import { tmpdir } from "node:os";
import { createHash } from "node:crypto";
import { parseArgs } from "node:util";
let values = {};
try { ({ values } = parseArgs({ options: { root: { type: "string" } } }));
 } catch (error) { throw Error(`${error.message}. Next: node scripts/release-install.mjs --root <directory>.`, {cause:error}); }
function main() {
if (!values.root?.trim()) throw Error("Missing --root. Next: pass the staged release bundle directory.");
const target = `${process.arch === "arm64" ? "aarch64" : "x86_64"}-${process.platform === "darwin" ? "apple-darwin" : "unknown-linux-gnu"}`;
const source = resolve(values.root, target);
const record = assertPayload(
  JSON.parse(readFileSync(join(source, "payload.json"), "utf8")),
);
if (record.target !== target)
  throw Error(
    `Staged target ${record.target} differs from executing ${target}. Next: select a matching bundle or rebuild with just release-bundle ${target}.`,
  );
const work = mkdtempSync(
  join(
    process.env.ONEPIPELINE_NODE_SCRATCH_DIR ?? tmpdir(),
    "release-install-",
  ),
);
const env = { ...process.env, ONEHARNESS_NO_CONFIG: "1" };
const run = (cmd, args, opts = {}) => {
  try {
    return execFileSync(cmd, args, { env, stdio: "inherit", ...opts });
  } catch (error) {
    throw Error(
      `${cmd} failed with status ${error.status ?? "unavailable"}. Next: fix its error above and rerun node scripts/release-install.mjs --root ${values.root}.`,
      { cause: error },
    );
  }
};
const prove = (bin) => {
  const actual = run(bin, ["--version"], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "inherit"],
  });
  if (actual.trim() !== `oneharness ${record.version}`)
    throw Error(
      `Unexpected CLI version ${JSON.stringify(actual)}, expected oneharness ${record.version}. Next: rebuild all forms from the same checkout.`,
    );
  const command = JSON.parse(
    run(
      bin,
      [
        "run",
        "--format",
        "json",
        "--harness",
        "codex",
        "--print-command",
        "--prompt",
        "hello",
      ],
      { encoding: "utf8", stdio: ["ignore", "pipe", "inherit"] },
    ),
  );
  if (
    command === null || typeof command !== "object" || Array.isArray(command) ||
    !Array.isArray(command.results) ||
    command.results.length !== 1 ||
    command.results[0]?.status !== "planned" ||
    !Array.isArray(command.results[0]?.command) ||
    !command.results[0].command.includes("hello")
  )
    throw Error(
      `Missing planned command contract: ${JSON.stringify(command)}. Next: run this staged CLI with --format json and inspect its report.`,
    );
  const invalid = spawnSync(
    bin,
    ["run", "--harness", "missing-harness", "--prompt", "hello"],
    { env, encoding: "utf8" },
  );
  if (invalid.error) throw Error(`Cannot start usage proof: ${invalid.error.message}. Next: rebuild the staged executable and inspect its permissions.`,{cause:invalid.error});
  if (invalid.status !== 2)
    throw Error(
      `Invalid harness exited ${invalid.status}, expected 2; stderr: ${invalid.stderr}. Next: inspect this staged CLI usage error.`,
    );
};
const hash = (path) =>
  createHash("sha256").update(readFileSync(path)).digest("hex");
const same = (path) => {
  if (hash(path) !== record.binarySha256)
    throw Error(
      `Payload reuse failed: ${path}; got ${hash(path)}, expected ${record.binarySha256}. Next: rebuild archive, wheel and npm from one release bundle.`,
    );
};
try {
  const archive = `oneharness-${record.tag}-${target}.tar.gz`;
  const archives = readdirSync(source).filter((p) => p.endsWith(".tar.gz"));
  if (archives.length !== 1 || archives[0] !== archive)
    throw Error(
      `Expected ${archive}, found ${JSON.stringify(archives)}. Next: rebuild a clean target bundle.`,
    );
  const extracted = join(work, "archive");
  mkdirSync(extracted);
  run("python3", ["-c", `import tarfile,sys,json
with tarfile.open(sys.argv[1]) as archive:
 members=archive.getmembers()
 expected=set(json.loads(sys.argv[2]))
 if len(members)!=len(expected) or {m.name for m in members}!=expected or any(not m.isfile() for m in members):
  raise ValueError("Unsafe archive members or links; rebuild a clean release bundle")
`, join(source, archive), JSON.stringify(archiveMembers)]);
  run("tar", ["-xzf", join(source, archive), "-C", extracted]);
  const description = run("file", ["-b", join(extracted, "oneharness")], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "inherit"],
  });
  const cpu = process.platform === "darwin"
    ? description.match(/^Mach-O (?:64-bit )?(?:executable )?(x86_64|arm64)\b/)?.[1]
    : description.match(/^ELF \d+-bit [^,]+,\s*([^,]+)/)?.[1];
  const architecturePattern =
    process.arch === "arm64" ? /arm64|aarch64/ : /x86[_-]64/;
  const wheelArchitecture =
    process.arch === "arm64"
      ? process.platform === "darwin"
        ? "arm64"
        : "aarch64"
      : "x86_64";
  if (
    !cpu || !architecturePattern.test(cpu) ||
    !record.wheel.endsWith(`_${wheelArchitecture}.whl`)
  )
    throw Error(
      `Artifact architecture mismatch for ${target}: ${description}, wheel ${record.wheel}. Next: rebuild the requested Rust target and its wheel.`,
    );
  same(join(extracted, "oneharness"));
  prove(join(extracted, "oneharness"));
  const venv = join(work, "venv");
  run("uv", ["venv", venv]);
  run("uv", [
    "pip",
    "install",
    "--python",
    join(venv, "bin/python"),
    "--no-index",
    ...(process.platform === "darwin" ? ["--python-platform", target] : []),
    join(source, "wheels", record.wheel),
  ]);
  same(join(venv, "bin/oneharness"));
  prove(join(venv, "bin/oneharness"));
  const tgz = `oneharness-cli-${process.platform}-${process.arch}-${record.version}.tgz`;
  const tarballs = readdirSync(join(source, "npm")).filter((p) =>
    p.endsWith(".tgz"),
  );
  if (tarballs.length !== 1 || tarballs[0] !== tgz)
    throw Error(
      `Expected ${tgz}, found ${JSON.stringify(tarballs)}. Next: rebuild the matching platform npm carrier.`,
    );
  const launcherOutput = run(
    "node",
    ["scripts/npm-build.mjs", "launcher", "--out", join(work, "stage")],
    { encoding: "utf8", stdio: ["ignore", "pipe", "inherit"] },
  ).trim();
  const launcher = assertPackageDirectory(
    launcherOutput,
    join(work, "stage", "oneharness"),
    "oneharness-cli",
    record.version,
  );
  const packed = assertPackResponse(
    JSON.parse(
      run("npm", ["pack", "--json", "--pack-destination", work], {
        cwd: launcher,
        encoding: "utf8",
        stdio: ["ignore", "pipe", "inherit"],
      }),
    ),
    work,
  );
  const install = join(work, "npm");
  mkdirSync(install);
  run("npm", [
    "install",
    "--prefix",
    install,
    "--omit=optional",
    "--ignore-scripts",
    "--no-audit",
    "--no-fund",
    join(source, "npm", tgz),
    join(work, packed),
  ]);
  same(
    join(
      install,
      "node_modules",
      "@oneharness",
      `cli-${process.platform}-${process.arch}`,
      "bin/oneharness",
    ),
  );
  prove(join(install, "node_modules/.bin/oneharness"));
  writeFileSync(
    join(source, "native-install.json"),
    JSON.stringify(
      {
        target,
        version: record.version,
        platform: process.platform,
        architecture: process.arch,
        binaryDescription: description.trim(),
        payloads: {
          archive: hash(join(extracted, "oneharness")),
          wheel: hash(join(venv, "bin/oneharness")),
          npm: hash(
            join(
              install,
              "node_modules",
              "@oneharness",
              `cli-${process.platform}-${process.arch}`,
              "bin/oneharness",
            ),
          ),
        },
      },
      null,
      2,
    ) + "\n",
  );
  console.log(
    `release-install: ${target} archive/wheel/npm reuse and CLI journeys passed`,
  );
} finally {
  rmSync(work, { recursive: true, force: true });
}

}
try {main();} catch(error) {
 throw Error(`Release installation proof failed: ${error.message}. Next: repair the named artifact or staging path, then rerun node scripts/release-install.mjs --root ${values.root ?? "<directory>"}.`,{cause:error});
}
