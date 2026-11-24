import { readFileSync } from "node:fs";

// The harness CLIs ci.yml's required checks install at a pin
// (.harness-cli-versions). scripts/check-harness-cli-pins.sh owns WHICH version
// each must name; the workflow contracts own WHAT is installed. So, for them, a
// pinned install is the same install as the unpinned one in the paid e2e-*
// workflows (which track latest on purpose) and as its own earlier pin: only
// the version is read away, and only for a CLI that file pins, so a dropped,
// renamed or extra install still reads as a different command.
export function parsePins(text, path = ".harness-cli-versions") {
  const pinned = new Set();
  text.split("\n").forEach((raw, index) => {
    const line = raw.trim();
    if (!line || line.startsWith("#")) return;
    const fields = line.split(/\s+/);
    // npm and Goose take an exact x.y.z; Cursor only its build id.
    const shape = fields[0] === "cursor-agent" ? /^\d{4}\.\d{2}\.\d{2}-[0-9a-f]+$/ : /^\d+\.\d+\.\d+$/;
    if (fields.length !== 2 || !shape.test(fields[1]))
      throw Error(`${path}:${index + 1}: must pin one package to exactly one version (x.y.z; cursor-agent YYYY.MM.DD-<hex>), found: ${line}. Next: fix it and rerun scripts/check-harness-cli-pins.sh.`);
    if (pinned.has(fields[0])) throw Error(`${path}:${index + 1}: pins ${fields[0]} twice. Next: keep one line for it.`);
    pinned.add(fields[0]);
  });
  return pinned;
}

export const harnessPins = (path = ".harness-cli-versions") => parsePins(readFileSync(path, "utf8"), path);

const pins = harnessPins();
const version = String.raw`\d+\.\d+\.\d+`;
const npmInstall = /(\bnpm\s+(?:install|i)\s+(?:-g|--global)\b)(.*)$/;
const npmPinned = new RegExp(String.raw`(\S+)@${version}(?=\s|$)`, "g");
// Goose's installer, fetched from a release tag and told the same version, is
// its stable-channel installer at a pin.
const goosePinned = new RegExp(
  String.raw`/releases/download/v${version}/download_cli\.sh(\s*\|\s*)GOOSE_VERSION=${version}\s+`,
);
// Its PowerShell installer, read at a tag and told the same version, is the
// one on main.
const goosePs1Pinned = new RegExp(
  String.raw`\$env:GOOSE_VERSION = '${version}'; irm 'https://raw\.githubusercontent\.com/block/goose/v${version}/download_cli\.ps1'`,
);
// Cursor's installer takes no version, so a pinned Cursor install is one of the
// helpers that pin it (scripts/install-cursor-agent.sh, cursor-installer.ps1).
const build = String.raw`\d{4}\.\d{2}\.\d{2}-[0-9a-f]+`;
const cursorUnixPinned = new RegExp(String.raw`^(\s*)bash scripts/install-cursor-agent\.sh ${build}$`);
const cursorWindowsPinned = new RegExp(String.raw`& \./scripts/cursor-installer\.ps1 -Version ${build}$`);

const unpinnedPackages = (text, pinned) => text.replace(npmPinned, (token, pkg) => (pinned.has(pkg) ? pkg : token));

// `continued` is true for a line that continues an `npm install -g … \`, whose
// packages are the install's too.
export function unpinnedLine(line, pinned = pins, continued = false) {
  let out = continued
    ? unpinnedPackages(line, pinned)
    : line.replace(npmInstall, (_, install, rest) => install + unpinnedPackages(rest, pinned));
  if (pinned.has("goose"))
    out = out
      .replace(goosePinned, "/releases/download/stable/download_cli.sh$1")
      .replace(goosePs1Pinned, "irm 'https://raw.githubusercontent.com/block/goose/main/download_cli.ps1'");
  if (pinned.has("cursor-agent"))
    out = out
      .replace(cursorUnixPinned, "$1curl https://cursor.com/install -fsS | bash")
      .replace(cursorWindowsPinned, "irm 'https://cursor.com/install?win32=true'");
  return out;
}

export function unpinnedRun(run, pinned = pins) {
  let inInstall = false;
  return run
    .split("\n")
    .map((line) => {
      const out = unpinnedLine(line, pinned, inInstall);
      inInstall = /\\\s*$/.test(line) && (inInstall || npmInstall.test(line));
      return out;
    })
    .join("\n");
}
