import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { compile } from "json-schema-to-typescript";
import { format } from "prettier";
import { generatedFileMatches } from "./generated-file.mjs";
import {
	exactOptionalProperties,
	typescriptSchema,
} from "./typescript-generator.mjs";
import {
	generateZodModule,
	SDK_SCHEMA_ALIASES,
	SDK_SCHEMA_ROOTS,
} from "./zod-generator.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const out = resolve(root, "npm/oneharness-sdk/src/generated");
mkdirSync(out, { recursive: true });

const normalizeNewlines = (value) => value.replace(/\r\n?/gu, "\n");
const generatedBytes = (value) =>
	Buffer.from(`${normalizeNewlines(value).replace(/\n*$/u, "")}\n`, "utf8");
const readonlyArrayProperties = (declarations, schema) => {
	let output = declarations;
	for (const [name, property] of Object.entries(schema.properties ?? {})) {
		if (property?.type !== "array") continue;
		const escaped = name.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");
		const pattern = new RegExp(`^(\\s*${escaped}\\?: )([^;]+\\[\\]);$`, "mu");
		if (!pattern.test(output)) {
			throw new Error(
				`generated RunOptions array ${name} was not found in TypeScript output; update the Rust schema or extend scripts/generate.mjs for the new declaration shape, then rerun just sdk-generate`,
			);
		}
		output = output.replace(pattern, "$1readonly $2;");
	}
	return output;
};
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
const run = exactOptionalProperties(
	await compile(typescriptSchema(bundle.run_report), "RunReport", {
		bannerComment: "/* Generated from oneharness-core. Do not edit. */",
		additionalProperties: true,
		style: { endOfLine: "lf" },
	}),
);
const runStreamEnvelope = exactOptionalProperties(
	await compile(
		typescriptSchema(bundle.run_stream_envelope),
		"RunStreamEnvelope",
		{
			bannerComment: "/* Generated from oneharness-core. Do not edit. */",
			additionalProperties: true,
			style: { endOfLine: "lf" },
		},
	),
);
const options = exactOptionalProperties(
	readonlyArrayProperties(
		await compile(bundle.run_options, "RunOptions", {
			bannerComment: "/* Generated from oneharness-core. Do not edit. */",
			additionalProperties: true,
			style: { endOfLine: "lf" },
		}),
		bundle.run_options,
	),
);
const historyLookup = exactOptionalProperties(
	await compile(bundle.history_lookup, "HistoryLookup", {
		bannerComment: "/* Generated from oneharness-core. Do not edit. */",
		additionalProperties: true,
		style: { endOfLine: "lf" },
	}),
);
const historyListOptions = exactOptionalProperties(
	await compile(bundle.history_list_options, "HistoryListOptions", {
		bannerComment: "/* Generated from oneharness-core. Do not edit. */",
		additionalProperties: true,
		style: { endOfLine: "lf" },
	}),
);
const historyWatchOptions = exactOptionalProperties(
	await compile(bundle.history_watch_options, "HistoryWatchOptions", {
		bannerComment: "/* Generated from oneharness-core. Do not edit. */",
		additionalProperties: true,
		style: { endOfLine: "lf" },
	}),
);
const history = exactOptionalProperties(
	await compile(typescriptSchema(bundle.history_record), "HistoryRecord", {
		bannerComment: "/* Generated from oneharness-core. Do not edit. */",
		additionalProperties: true,
		style: { endOfLine: "lf" },
	}),
);
const historyStreamEnvelope = exactOptionalProperties(
	await compile(
		typescriptSchema(bundle.history_stream_envelope),
		"HistoryStreamEnvelope",
		{
			bannerComment: "/* Generated from oneharness-core. Do not edit. */",
			additionalProperties: true,
			style: { endOfLine: "lf" },
		},
	),
);
const historyRecords = exactOptionalProperties(
	await compile(
		{ ...typescriptSchema(bundle.history_records), title: "HistoryRecords" },
		"HistoryRecords",
		{
			bannerComment: "/* Generated from oneharness-core. Do not edit. */",
			additionalProperties: true,
			style: { endOfLine: "lf" },
		},
	),
);
const historyList = exactOptionalProperties(
	await compile(
		{ ...typescriptSchema(bundle.history_list), title: "HistoryList" },
		"HistoryList",
		{
			bannerComment: "/* Generated from oneharness-core. Do not edit. */",
			additionalProperties: true,
			style: { endOfLine: "lf" },
		},
	),
);
const registry = exactOptionalProperties(
	await compile(typescriptSchema(bundle.list_report), "ListReport", {
		bannerComment: "/* Generated from oneharness. Do not edit. */",
		additionalProperties: true,
		style: { endOfLine: "lf" },
	}),
);
const detection = exactOptionalProperties(
	await compile(typescriptSchema(bundle.detect_report), "DetectReport", {
		bannerComment: "/* Generated from oneharness. Do not edit. */",
		additionalProperties: true,
		style: { endOfLine: "lf" },
	}),
);
const zod = await format(
	generateZodModule(bundle, SDK_SCHEMA_ROOTS, SDK_SCHEMA_ALIASES),
	{
		parser: "typescript",
		endOfLine: "lf",
		printWidth: 120,
		tabWidth: 2,
		useTabs: false,
	},
);
const files = {
	"schemas.json": generatedBytes(JSON.stringify(bundle, null, 2)),
	"contracts.ts": generatedBytes(run),
	"run-stream-envelope.ts": generatedBytes(runStreamEnvelope),
	"options.ts": generatedBytes(options),
	"history-lookup.ts": generatedBytes(historyLookup),
	"history-list-options.ts": generatedBytes(historyListOptions),
	"history-watch-options.ts": generatedBytes(historyWatchOptions),
	"history.ts": generatedBytes(history),
	"history-stream-envelope.ts": generatedBytes(historyStreamEnvelope),
	"history-records.ts": generatedBytes(historyRecords),
	"history-list.ts": generatedBytes(historyList),
	"registry.ts": generatedBytes(registry),
	"detection.ts": generatedBytes(detection),
	"zod.ts": generatedBytes(zod),
};
let stale = false;
for (const [name, content] of Object.entries(files)) {
	const path = resolve(out, name);
	if (process.argv.includes("--check")) {
		if (!generatedFileMatches(path, content)) stale = true;
	} else writeFileSync(path, content);
}
if (stale) {
	console.error("generated SDK contracts are stale; run just sdk-generate");
	process.exit(1);
}
