import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { harnessPins, parsePins, unpinnedRun } from "../harness-pins.mjs";

const pinned = new Set(["@github/copilot", "opencode-ai", "goose", "cursor-agent"]);
const same = (a, b) => assert.equal(unpinnedRun(a, pinned), unpinnedRun(b, pinned));
const differs = (a, b) => assert.notEqual(unpinnedRun(a, pinned), unpinnedRun(b, pinned));

test("a pinned harness CLI install reads as its unpinned source", () => {
  same("npm install -g @github/copilot@1.0.114\ncopilot --version", "npm install -g @github/copilot\ncopilot --version");
  same("npm install -g @github/copilot@1.0.114 opencode-ai@1.19.6", "npm install -g @github/copilot opencode-ai");
  same("npm install -g @github/copilot@1.0.114", "npm install -g @github/copilot@1.0.115");
  // A package on an install's continuation line belongs to the install.
  same("npm install -g @github/copilot@1.0.114 \\\n  opencode-ai@1.19.6", "npm install -g @github/copilot \\\n  opencode-ai");
  differs("echo done \\\n  opencode-ai@1.19.6", "echo done \\\n  opencode-ai");
  same(
    "curl -fsSL https://github.com/block/goose/releases/download/v1.60.2/download_cli.sh | GOOSE_VERSION=1.60.2 CONFIGURE=false bash",
    "curl -fsSL https://github.com/block/goose/releases/download/stable/download_cli.sh | CONFIGURE=false bash",
  );
});

test("a pinned Goose PowerShell or Cursor install reads as its vendor installer", () => {
  same(
    "$env:GOOSE_VERSION = '1.60.2'; irm 'https://raw.githubusercontent.com/block/goose/v1.60.2/download_cli.ps1' | iex",
    "irm 'https://raw.githubusercontent.com/block/goose/main/download_cli.ps1' | iex",
  );
  same("  bash scripts/install-cursor-agent.sh 2026.11.22-e9042b2", "  curl https://cursor.com/install -fsS | bash");
  same("$script = & ./scripts/cursor-installer.ps1 -Version 2026.11.22-e9042b2", "$script = irm 'https://cursor.com/install?win32=true'");
  // A helper handed a non-build, or run some other way, is not the installer.
  differs("  bash scripts/install-cursor-agent.sh latest", "  curl https://cursor.com/install -fsS | bash");
  differs("  bash scripts/install-cursor-agent.sh 2026.11.22-e9042b2 && rm -rf ~", "  curl https://cursor.com/install -fsS | bash");
  differs(
    "irm 'https://raw.githubusercontent.com/block/goose/v1.60.2/download_cli.ps1' | iex",
    "irm 'https://raw.githubusercontent.com/block/goose/main/download_cli.ps1' | iex",
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

test("a malformed pin file is refused, never read as a partial allowlist", () => {
  assert.deepEqual([...parsePins("# c\n\n@github/copilot 1.0.114\ngoose 1.60.2\n")], ["@github/copilot", "goose"]);
  for (const bad of ["@github/copilot", "@github/copilot latest", "@github/copilot 1.0", "@github/copilot 1.0.114 1.0.115"])
    assert.throws(() => parsePins(`goose 1.60.2\n${bad}\n`), /:2: must pin one package to exactly one version/);
  assert.throws(() => parsePins("goose 1.60.2\ngoose 1.60.2\n"), /:2: pins goose twice/);
  assert.deepEqual([...parsePins("cursor-agent 2026.11.22-e9042b2\n")], ["cursor-agent"]);
  // The build-id boundary every Cursor validator shares: 7 to 40 hex digits.
  for (const ok of ["2026.11.22-abcdef1", "2026.11.22-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"]) assert.deepEqual([...parsePins(`cursor-agent ${ok}\n`)], ["cursor-agent"]);
  for (const bad of ["cursor-agent 1.2.3", "cursor-agent latest", "goose 2026.11.22-e9042b2", "cursor-agent 2026.11.22-abcdef", "cursor-agent 2026.11.22-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"])
    assert.throws(() => parsePins(`${bad}\n`), /must pin one package to exactly one version/);
});
