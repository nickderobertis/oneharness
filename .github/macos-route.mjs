import { readFileSync, appendFileSync } from "node:fs";
import { classify } from "./macos-policy.mjs";
if (!process.env.GITHUB_OUTPUT?.trim()) throw Error("Missing GITHUB_OUTPUT. Next: supply the Actions output file path.");
let event;
try {
  event = JSON.parse(readFileSync(process.env.GITHUB_EVENT_PATH, "utf8"));
} catch {
  event = {};
}
if (!event || typeof event !== "object" || Array.isArray(event)) event = {};
event.event_name = process.env.EVENT_NAME ?? "";
const result = classify(event, process.env.BASE_SHA, process.env.HEAD_SHA);
appendFileSync(
  process.env.GITHUB_OUTPUT,
  `macos_required=${result.macosRequired}\nis_release_only=${result.isReleaseOnly}\n`,
);
console.log(result.reason);
