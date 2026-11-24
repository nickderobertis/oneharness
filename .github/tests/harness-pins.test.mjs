import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { harnessPins, unpinnedRun } from "../harness-pins.mjs";

const pinned = new Set(["@github/copilot", "opencode-ai", "goose"]);
const same = (a, b) => assert.equal(unpinnedRun(a, pinned), unpinnedRun(b, pinned));
const differs = (a, b) => assert.notEqual(unpinnedRun(a, pinned), unpinnedRun(b, pinned));

test("a pinned harness CLI install reads as its unpinned source", () => {
  same("npm install -g @github/copilot@1.0.114\ncopilot --version", "npm install -g @github/copilot\ncopilot --version");
  same("npm install -g @github/copilot@1.0.114 opencode-ai@1.19.6", "npm install -g @github/copilot opencode-ai");
  same("npm install -g @github/copilot@1.0.114", "npm install -g @github/copilot@1.0.115");
  same(
    "curl -fsSL https://github.com/block/goose/releases/download/v1.60.2/download_cli.sh | GOOSE_VERSION=1.60.2 CONFIGURE=false bash",
    "curl -fsSL https://github.com/block/goose/releases/download/stable/download_cli.sh | CONFIGURE=false bash",
  );
});

test("a swapped, dropped, extra or unknown install still reads as a different command", () => {
  differs("npm install -g @github/copilot-fork@1.0.114", "npm install -g @github/copilot");
  differs("npm install -g @github/copilot@1.0.114", "npm install -g @github/copilot opencode-ai");
  differs("npm install -g @github/copilot@1.0.114 extra", "npm install -g @github/copilot");
  differs("true", "npm install -g @github/copilot");
  // Only a CLI the pin file names loses its version.
  differs("npm install -g some-tool@1.2.3", "npm install -g some-tool");
  differs("npm install -g @github/copilot@latest", "npm install -g @github/copilot");
  differs(
    "curl -fsSL https://github.com/block/goose/releases/download/v1.60.2/download_cli.sh | CONFIGURE=false bash",
    "curl -fsSL https://github.com/block/goose/releases/download/stable/download_cli.sh | CONFIGURE=false bash",
  );
});

test("the repo's pin file is what the contracts read", () => {
  const pins = harnessPins();
  for (const line of readFileSync(".harness-cli-versions", "utf8").split("\n"))
    if (line.trim() && !line.trim().startsWith("#")) assert.ok(pins.has(line.trim().split(/\s+/)[0]));
  assert.ok(pins.has("@github/copilot"));
});
