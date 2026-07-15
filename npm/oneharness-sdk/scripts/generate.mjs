import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { compile } from "json-schema-to-typescript";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const out = resolve(root, "npm/oneharness-sdk/src/generated");
mkdirSync(out, { recursive: true });
const bundle = JSON.parse(
	execFileSync(
		"cargo",
		[
			"run",
			"-q",
			"-p",
			"oneharness",
			"--features",
			"sdk-schema",
			"--example",
			"generate_sdk_schema",
		],
		{ cwd: root, encoding: "utf8" },
	),
);
const run = await compile(bundle.run_report, "RunReport", {
	bannerComment: "/* Generated from oneharness-core. Do not edit. */",
	additionalProperties: false,
});
const history = await compile(bundle.history_record, "HistoryRecord", {
	bannerComment: "/* Generated from oneharness-core. Do not edit. */",
	additionalProperties: false,
});
const registry = await compile(bundle.list_report, "ListReport", {
	bannerComment: "/* Generated from oneharness. Do not edit. */",
	additionalProperties: false,
});
const detection = await compile(bundle.detect_report, "DetectReport", {
	bannerComment: "/* Generated from oneharness. Do not edit. */",
	additionalProperties: false,
});
const files = {
	"schemas.json": `${JSON.stringify(bundle, null, 2)}\n`,
	"contracts.ts": run,
	"history.ts": history,
	"registry.ts": registry,
	"detection.ts": detection,
};
let stale = false;
for (const [name, content] of Object.entries(files)) {
	const path = resolve(out, name);
	if (process.argv.includes("--check")) {
		if (readFileSync(path, "utf8") !== content) stale = true;
	} else writeFileSync(path, content);
}
if (stale) {
	console.error("generated SDK contracts are stale; run just sdk-generate");
	process.exit(1);
}
