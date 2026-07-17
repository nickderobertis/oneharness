#!/usr/bin/env node
import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const cargo = readFileSync(resolve(root, "Cargo.toml"), "utf8");
const version = cargo.match(/\[package\][\s\S]*?^version\s*=\s*"([^"]+)"/m)?.[1];
if (!version) {
  console.error(
    "python-sdk-pack: Cargo.toml has no [package] version; restore the root manifest before packing the SDK",
  );
  process.exit(1);
}

const source = resolve(root, "python/oneharness-sdk");
const output = resolve(root, "python/dist/sdk");
rmSync(output, { recursive: true, force: true });
mkdirSync(output, { recursive: true });
cpSync(resolve(source, "src"), resolve(output, "src"), { recursive: true });
cpSync(resolve(source, "README.md"), resolve(output, "README.md"));
cpSync(resolve(root, "LICENSE"), resolve(output, "LICENSE"));

let project = readFileSync(resolve(source, "pyproject.toml"), "utf8");
const projectVersion = 'version = "0.0.0.dev0"';
const cliDependency = '"oneharness-cli==0.0.0.dev0"';
if (!project.includes(projectVersion) || !project.includes(cliDependency)) {
  console.error(
    "python-sdk-pack: development version placeholders changed; update the packer intentionally",
  );
  process.exit(1);
}
project = project
  .replace(projectVersion, `version = "${version}"`)
  .replace(cliDependency, `"oneharness-cli==${version}"`);
writeFileSync(resolve(output, "pyproject.toml"), project);

const versionFile = resolve(output, "src/oneharness_sdk/_version.py");
let versionSource = readFileSync(versionFile, "utf8");
const versionAssignment = '__version__ = "0.0.0.dev0"';
if (!versionSource.includes(versionAssignment)) {
  console.error(
    "python-sdk-pack: Python version placeholder changed; update the packer intentionally",
  );
  process.exit(1);
}
versionSource = versionSource.replace(versionAssignment, `__version__ = "${version}"`);
writeFileSync(versionFile, versionSource);

process.stdout.write(`${output}\n`);
