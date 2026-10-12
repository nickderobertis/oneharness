import { basename, resolve, relative, isAbsolute } from "node:path";
export const PAYLOAD_VERSION = 1;
export const SUPPORTED_TARGETS = Object.freeze(["x86_64", "aarch64"].flatMap(cpu => ["apple-darwin", "unknown-linux-gnu", "pc-windows-msvc"].map(system => `${cpu}-${system}`)));
export const validTarget = value => typeof value === "string" && SUPPORTED_TARGETS.includes(value);
export const validVersion = (value) =>
  typeof value === "string" &&
  /^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(value);
export function assertPayload(value) {
  if (
    !value ||
    typeof value !== "object" ||
    Array.isArray(value) ||
    value.schema_version !== PAYLOAD_VERSION ||
    !validTarget(value.target) ||
    !validVersion(value.version) ||
    value.tag !== `v${value.version}` ||
    typeof value.binarySha256 !== "string" ||
    !/^[a-f0-9]{64}$/.test(value.binarySha256) ||
    typeof value.wheel !== "string" ||
    basename(value.wheel) !== value.wheel ||
    !/^oneharness_cli-[A-Za-z0-9_.+-]+\.whl$/.test(value.wheel)
  ) {
    throw Error(
      "Invalid release payload metadata. Next: rebuild the target with just release-bundle and consume its payload.json unchanged.",
    );
  }
  return value;
}
export function makePayload(fields) {
  return assertPayload({ schema_version: PAYLOAD_VERSION, ...fields });
}
export function assertPackResponse(value, directory) {
  if (
    !Array.isArray(value) ||
    value.length !== 1 ||
    !value[0] ||
    typeof value[0].filename !== "string" ||
    basename(value[0].filename) !== value[0].filename ||
    !value[0].filename.endsWith(".tgz")
  )
    throw Error(
      "npm pack returned no unique local tarball. Next: inspect npm output and rebuild the staged launcher.",
    );
  const path = resolve(directory, value[0].filename),
    rel = relative(directory, path);
  if (isAbsolute(rel) || rel.startsWith(".."))
    throw Error(
      "npm pack returned a path outside its staging directory. Next: rebuild the staged launcher.",
    );
  return value[0].filename;
}

export const archiveMembers = Object.freeze(["oneharness", "README.md", "LICENSE", "CHANGELOG.md"]);
