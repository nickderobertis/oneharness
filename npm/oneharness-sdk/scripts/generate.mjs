import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { compile } from "json-schema-to-typescript";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const out = resolve(root, "npm/oneharness-sdk/src/generated");
mkdirSync(out, { recursive: true });

const normalizeNewlines = (value) => value.replace(/\r\n?/gu, "\n");
const generatedBytes = (value) =>
	Buffer.from(`${normalizeNewlines(value).replace(/\n*$/u, "")}\n`, "utf8");
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
	(_key, value) =>
		typeof value === "string" ? normalizeNewlines(value) : value,
);
const run = await compile(bundle.run_report, "RunReport", {
	bannerComment: "/* Generated from oneharness-core. Do not edit. */",
	additionalProperties: false,
	style: { endOfLine: "lf" },
});
const history = await compile(bundle.history_record, "HistoryRecord", {
	bannerComment: "/* Generated from oneharness-core. Do not edit. */",
	additionalProperties: false,
	style: { endOfLine: "lf" },
});
const registry = await compile(bundle.list_report, "ListReport", {
	bannerComment: "/* Generated from oneharness. Do not edit. */",
	additionalProperties: false,
	style: { endOfLine: "lf" },
});
const detection = await compile(bundle.detect_report, "DetectReport", {
	bannerComment: "/* Generated from oneharness. Do not edit. */",
	additionalProperties: false,
	style: { endOfLine: "lf" },
});
const files = {
	"schemas.json": generatedBytes(JSON.stringify(bundle, null, 2)),
	"contracts.ts": generatedBytes(run),
	"history.ts": generatedBytes(history),
	"registry.ts": generatedBytes(registry),
	"detection.ts": generatedBytes(detection),
};
let stale = false;
for (const [name, content] of Object.entries(files)) {
	const path = resolve(out, name);
	if (process.argv.includes("--check")) {
		if (!readFileSync(path).equals(content)) stale = true;
	} else writeFileSync(path, content);
}
if (stale) {
	console.error("generated SDK contracts are stale; run just sdk-generate");
	process.exit(1);
}
