import {validTarget} from "./contract.mjs";
import { readFileSync, realpathSync } from "node:fs";
import { resolve, relative, isAbsolute, basename } from "node:path";
import { execFileSync } from "node:child_process";
export function readPlatforms() {
  let value;
  try { value = JSON.parse(
    execFileSync(
      "python3",
      [
        "-c",
        'import tomllib,json; print(json.dumps(tomllib.load(open("release-platforms.toml","rb"))["platform"]))',
      ],
      { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] },
    ),
  ); } catch (error) { throw Error(`Cannot parse release-platforms.toml: ${error.stderr?.toString() ?? error.message}. Next: repair its TOML platform declarations and rerun the release builder.`, { cause: error }); }
  if (
    !Array.isArray(value) ||
    !value.length ||
    value.some(
      (p) =>
        p === null || typeof p !== "object" || Array.isArray(p) ||
        !validTarget(p.target) ||
        typeof p.npm !== "string" ||
        !/^(linux|darwin|win32)-(x64|arm64)$/.test(p.npm) ||
        !["tar.gz", "zip"].includes(p.archive),
    )
  )
    throw Error(
      "Invalid release-platforms.toml. Next: restore its target/npm/archive declarations.",
    );
  const seen = new Set();
  for (const p of value) {
    const [cpu, platform] = p.target.split(/-(.*)/s);
    const expectedNpm = `${platform === "apple-darwin" ? "darwin" : platform === "pc-windows-msvc" ? "win32" : "linux"}-${cpu === "aarch64" ? "arm64" : "x64"}`;
    const expectedArchive = platform === "pc-windows-msvc" ? "zip" : "tar.gz";
    if (seen.has(p.target) || p.npm !== expectedNpm || p.archive !== expectedArchive)
      throw Error(`Inconsistent or duplicate release-platforms.toml platform ${p.target}. Next: align its npm/archive declaration with its Rust target.`);
    seen.add(p.target);
  }
  return value;
}
export function assertPackageDirectory(returned, expected, name, version) {
  if (typeof returned !== "string")
    throw Error(
      `Package builder output was ${returned === null ? "null" : typeof returned}, expected a staging directory path. Next: rerun scripts/npm-build.mjs into the release staging directory and capture its stdout as text.`,
    );
  const candidate = returned.trim();
  try {
  if (
    candidate !== resolve(expected) ||
    realpathSync(candidate) !== realpathSync(expected)
  )
    throw Error(
      `Package builder returned ${JSON.stringify(candidate)}, expected ${resolve(expected)}. Next: rerun scripts/npm-build.mjs into the release staging directory.`,
    );
  const manifest = JSON.parse(
    readFileSync(resolve(candidate, "package.json"), "utf8"),
  );
  if (manifest === null || typeof manifest !== "object" || Array.isArray(manifest))
    throw Error(`Invalid package manifest in ${candidate}. Next: rebuild its staging directory.`);
  if (manifest.name !== name || manifest.version !== version)
    throw Error(
      `Package ${candidate} identifies ${manifest.name}@${manifest.version}, expected ${name}@${version}. Next: rebuild its release staging directory.`,
    );
  return candidate;
  } catch (error) { throw Error(`${error.message}. Next: rebuild the package staging directory with scripts/npm-build.mjs.`, { cause: error }); }
}
