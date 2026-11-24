import { readFileSync } from "node:fs";

// The harness CLIs ci.yml's required checks install at a pin
// (.harness-cli-versions). scripts/check-harness-cli-pins.sh owns WHICH version
// each must name; the workflow contracts own WHAT is installed. So, for them, a
// pinned install is the same install as the unpinned one in the paid e2e-*
// workflows (which track latest on purpose) and as its own earlier pin: only
// the version is read away, and only for a CLI that file pins, so a dropped,
// renamed or extra install still reads as a different command.
export function harnessPins(path = ".harness-cli-versions") {
  return new Set(
    readFileSync(path, "utf8")
      .split("\n")
      .map((line) => line.trim())
      .filter((line) => line && !line.startsWith("#"))
      .map((line) => line.split(/\s+/)[0]),
  );
}

const pins = harnessPins();
const version = String.raw`\d+\.\d+\.\d+`;
const npmInstall = /(\bnpm\s+(?:install|i)\s+(?:-g|--global)\b)(.*)$/;
const npmPinned = new RegExp(String.raw`(\S+)@${version}(?=\s|$)`, "g");
// Goose's installer, fetched from a release tag and told the same version, is
// its stable-channel installer at a pin.
const goosePinned = new RegExp(
  String.raw`/releases/download/v${version}/download_cli\.sh(\s*\|\s*)GOOSE_VERSION=${version}\s+`,
);

export function unpinnedLine(line, pinned = pins) {
  let out = line.replace(npmInstall, (_, install, rest) =>
    install + rest.replace(npmPinned, (token, pkg) => (pinned.has(pkg) ? pkg : token)),
  );
  if (pinned.has("goose")) out = out.replace(goosePinned, "/releases/download/stable/download_cli.sh$1");
  return out;
}

export const unpinnedRun = (run, pinned = pins) =>
  run
    .split("\n")
    .map((line) => unpinnedLine(line, pinned))
    .join("\n");
