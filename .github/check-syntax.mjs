import { readdirSync } from "node:fs";
import { execFileSync } from "node:child_process";
for (const path of [
  ...readdirSync(".github")
    .filter((f) => f.endsWith(".mjs"))
    .map((f) => `.github/${f}`),
  ...readdirSync(".github/tests")
    .filter((f) => f.endsWith(".mjs"))
    .map((f) => `.github/tests/${f}`),
])
  execFileSync(process.execPath, ["--check", path], { stdio: "inherit" });
