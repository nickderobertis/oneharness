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
    if (fields.length !== 2 || !/^\d+\.\d+\.\d+$/.test(fields[1]))
      throw Error(`${path}:${index + 1}: must pin one package to exactly one x.y.z version, found: ${line}. Next: fix it and rerun scripts/check-harness-cli-pins.sh.`);
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
