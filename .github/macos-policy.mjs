import { validVersion } from "../tools/release-tooling/payload-contract/contract.mjs";
import { assertRevision } from "./git-input.mjs";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { parseArgs } from "node:util";
import { pathToFileURL } from "node:url";

const RELEASE_BRANCH_PREFIX = "release-plz-";
const repository = "nickderobertis/oneharness";
const manifests = new Set(["Cargo.toml", "crates/oneharness-core/Cargo.toml"]);
function git(args, cwd) {
  return execFileSync("git", args, {
    cwd,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
    maxBuffer: 16 * 1024 * 1024,
  });
}
function parseToml(text) {
  return JSON.parse(
    execFileSync(
      "python3",
      [
        "-c",
        "import json,sys,tomllib; print(json.dumps(tomllib.loads(sys.stdin.read())))",
      ],
      { input: text, encoding: "utf8" },
    ),
  );
}
function normalize(path, text) {
  if (manifests.has(path)) {
    const data = parseToml(text);
    if (!validVersion(data.package?.version))
      throw Error(`Invalid package version: ${path}`);
    delete data.package.version;
    if (path === "Cargo.toml") {
      const dep = data.dependencies?.["oneharness-core"];
      if (dep?.version) {
        if (!validVersion(dep.version)) throw Error("Invalid engine version");
        delete dep.version;
      }
    }
    return data;
  }
  if (path === "Cargo.lock") {
    const data = parseToml(text);
    for (const pkg of data.package ?? []) {
      if (["oneharness", "oneharness-core"].includes(pkg.name) && !pkg.source) {
        if (!validVersion(pkg.version)) throw Error("Invalid lockfile version");
        delete pkg.version;
      }
      if (pkg.dependencies)
        pkg.dependencies = pkg.dependencies.map((d) =>
          d.replace(/^(oneharness(?:-core)?) \d+\.\d+\.\d+$/, "$1"),
        );
    }
    return data;
  }
  throw Error(`Not release metadata: ${path}`);
}
function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === "object")
    return Object.fromEntries(
      Object.keys(value)
        .sort()
        .map((key) => [key, canonical(value[key])]),
    );
  return value;
}
export function classify(event, base, head, cwd = process.cwd()) {
  const ordinary = (reason) => ({
    isReleaseOnly: false,
    macosRequired: true,
    reason,
  });
  if (event?.event_name !== undefined && typeof event.event_name !== "string")
    return ordinary("invalid event type");
  if (event?.event_name !== undefined && event.event_name !== "pull_request" && !["push","release","workflow_run","repository_dispatch","schedule","workflow_dispatch"].includes(event.event_name)) return ordinary(`unrecognised event name ${JSON.stringify(event.event_name)}`);
  if (event?.event_name && event.event_name !== "pull_request")
    return {
      isReleaseOnly: false,
      macosRequired: false,
      reason: "non-PR event",
    };
  const pr = event?.pull_request;
  if (!pr) return ordinary("unknown PR input");
  if (
    event.repository?.full_name !== repository ||
    pr.head?.repo?.full_name !== repository ||
    typeof pr.head?.ref !== "string" ||
    !pr.head.ref.startsWith(RELEASE_BRANCH_PREFIX)
  )
    return ordinary("not repository release automation");
  try {
    assertRevision(base, "base");
    assertRevision(head, "head");
    const paths = git(
      ["diff", "--name-only", "--no-renames", "-z", base, head, "--"],
      cwd,
    )
      .split("\0")
      .filter(Boolean);
    if (!paths.length) return ordinary("empty release diff");
    for (const path of paths) {
      const before = git(["show", `${base}:${path}`], cwd);
      const after = git(["show", `${head}:${path}`], cwd);
      if (
        path === "CHANGELOG.md" ||
        path === "crates/oneharness-core/CHANGELOG.md"
      ) {
        if (!/^## /m.test(after)) return ordinary("unparsed changelog");
        continue;
      }
      if (
        JSON.stringify(canonical(normalize(path, before))) !==
        JSON.stringify(canonical(normalize(path, after)))
      )
        return ordinary(`non-version content: ${path}`);
    }
    return {
      isReleaseOnly: true,
      macosRequired: false,
      reason: "automation version/changelog-only diff",
    };
  } catch (error) {
    return ordinary(`unreadable release diff: ${error.message}`);
  }
}
if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  const { values, positionals } = parseArgs({
    options: {
      event: { type: "string" },
      base: { type: "string" },
      head: { type: "string" },
    },
    allowPositionals: true,
  });
  if (positionals.join(" ") !== "classify")
    throw Error(
      "Usage: macos-policy.mjs classify --event JSON --base REF --head REF",
    );
  let event;
  try {
    event = JSON.parse(readFileSync(values.event, "utf8"));
  } catch {
    event = {};
  }
  console.log(JSON.stringify(classify(event, values.base, values.head)));
}
