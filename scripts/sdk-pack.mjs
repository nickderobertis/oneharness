#!/usr/bin/env node
import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const cargo = readFileSync(resolve(root, "Cargo.toml"), "utf8");
const version = cargo.match(/\[package\][\s\S]*?^version\s*=\s*"([^"]+)"/m)?.[1];
if (!version) {
  console.error(
    "sdk-pack: Cargo.toml has no [package] version; restore the root manifest before packing the SDK",
  );
  process.exit(1);
}
const source = resolve(root, "npm/oneharness-sdk");
const output = resolve(root, "npm/dist/sdk");
rmSync(output, { recursive: true, force: true });
mkdirSync(output, { recursive: true });
cpSync(resolve(source, "dist"), resolve(output, "dist"), { recursive: true });
cpSync(resolve(source, "README.md"), resolve(output, "README.md"));
const manifest = JSON.parse(readFileSync(resolve(source, "package.json"), "utf8"));
manifest.version = version;
manifest.dependencies["oneharness-cli"] = version;
delete manifest.scripts;
delete manifest.devDependencies;
writeFileSync(resolve(output, "package.json"), `${JSON.stringify(manifest, null, 2)}\n`);
process.stdout.write(`${output}\n`);
