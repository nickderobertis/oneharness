// Maturin compiles once; its stripped wheel payload supplies archive and npm bytes.
import { execFileSync } from "node:child_process";
import {
  mkdirSync,
  readdirSync,
  copyFileSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import { resolve, join } from "node:path";
import { createHash } from "node:crypto";
import { parseArgs } from "node:util";
import {
  archiveMembers,
  validVersion,
  readPlatforms,
  makePayload,
  assertPackageDirectory,
} from "./release-payload.mjs";
const { values } = parseArgs({
  options: {
    target: { type: "string" },
    out: { type: "string" },
    tag: { type: "string" },
  },
});
function main() {
const target = values.target;
const platforms = readPlatforms();
const facts = platforms.find(
  (p) => p.target === target && !p.target.includes("windows"),
);
if (!facts)
  throw Error(
    `Unsupported release target ${JSON.stringify(target)}. Accepted native bundle targets: ${platforms
      .filter((p) => !p.target.includes("windows"))
      .map((p) => p.target)
      .join(", ")}. Next: just release-bundle <target> <output-directory>.`,
  );
const manifest = readFileSync("Cargo.toml", "utf8");
const section = manifest.split(/^\[package\]\s*\r?\n/m)[1]?.split(/^\[/m)[0];
const version = section?.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
if (!validVersion(version))
  throw Error(
    "Cargo.toml has no valid package version. Next: restore its [package] version before building release artifacts.",
  );
const tag = values.tag ?? `v${version}`;
if (tag !== `v${version}`)
  throw Error(
    `Release tag differs from Cargo version: got ${tag}, expected v${version}. Next: use the matching tag and checkout.`,
  );
const out = resolve(values.out ?? "dist/release", target);
mkdirSync(join(out, "wheels"), { recursive: true });
const run = (cmd, args, opts = {}) => {
  try {
    return execFileSync(cmd, args, { stdio: "inherit", ...opts });
  } catch (error) {
    throw Error(
      `${cmd} failed with status ${error.status ?? "unavailable"}. Next: fix the subprocess error above, then rerun just release-bundle ${target} ${values.out ?? "dist/release"}.`,
      { cause: error },
    );
  }
};
run(
  "uv",
  [
    "tool",
    "run",
    "--from",
    "maturin==1.14.1",
    "maturin",
    "build",
    "--release",
    "--locked",
    "--target",
    target,
    "--out",
    join(out, "wheels"),
  ],
  {
    env: {
      ...process.env,
      RUSTFLAGS: `${process.env.RUSTFLAGS ?? ""} -D warnings`.trim(),
    },
  },
);
const wheels = readdirSync(join(out, "wheels")).filter((p) =>
  p.endsWith(".whl"),
);
if (wheels.length !== 1)
  throw Error(
    `Expected exactly one target wheel in ${join(out, "wheels")}, found ${JSON.stringify(wheels)}. Next: clear this target staging directory and rerun just release-bundle ${target}.`,
  );
const wheel = join(out, "wheels", wheels[0]);
const stage = join(out, "archive");
mkdirSync(stage, { recursive: true });
const binary = join(stage, "oneharness");
run("python3", [
  "-c",
  'import sys,zipfile,os; z=zipfile.ZipFile(sys.argv[1]); entries=[n for n in z.namelist() if n == "oneharness_cli-"+sys.argv[3]+".data/scripts/oneharness"]; assert len(entries)==1,entries; open(sys.argv[2],"wb").write(z.read(entries[0])); os.chmod(sys.argv[2],0o755)',
  wheel,
  binary,
  version,
]);
for (const file of archiveMembers.filter((file) => file !== "oneharness"))
  copyFileSync(file, join(stage, file));
const archive = join(out, `oneharness-${tag}-${target}.${facts.archive}`);
run("tar", [
  "-czf",
  archive,
  "-C",
  stage,
  ...archiveMembers,
]);
const returned = run(
  "node",
  [
    "scripts/npm-build.mjs",
    "platform",
    "--target",
    target,
    "--binary",
    binary,
    "--out",
    join(out, "npm-stage"),
  ],
  { stdio: ["ignore", "pipe", "inherit"], encoding: "utf8" },
);
const npm = assertPackageDirectory(
  returned,
  join(out, "npm-stage", `cli-${facts.npm}`),
  `@oneharness/cli-${facts.npm}`,
  version,
);
mkdirSync(join(out, "npm"), { recursive: true });
run("npm", ["pack", "--pack-destination", join(out, "npm")], { cwd: npm });
const hash = (p) => createHash("sha256").update(readFileSync(p)).digest("hex");
writeFileSync(
  join(out, `oneharness-${tag}-${target}.sha256`),
  `${hash(archive)}  ${archive.split("/").at(-1)}\n`,
);
writeFileSync(
  join(out, "payload.json"),
  JSON.stringify(
    makePayload({
      target,
      tag,
      version,
      binarySha256: hash(binary),
      wheel: wheels[0],
    }),
    null,
    2,
  ) + "\n",
);
console.log(out);

}
try { main(); } catch (error) {
  throw Error(`Release builder failed: ${error.message}. Next: repair the named input or staging path, then rerun just release-bundle ${values.target ?? "<target>"}.`, {cause:error});
}
