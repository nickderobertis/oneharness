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
// Keep the contract generator out of the workspace's shared target directory.
// `sdk-check` builds the same crates with the mock-harness feature first, and
// historically the core and CLI schema examples also shared an output filename.
// A dedicated target makes the drift gate independent of either prior artifact.
const generatorTarget = resolve(root, "target/sdk-schema-generator");
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
		{
			cwd: root,
			encoding: "utf8",
			env: { ...process.env, CARGO_TARGET_DIR: generatorTarget },
		},
	),
	(_key, value) =>
		typeof value === "string" ? normalizeNewlines(value) : value,
);
/**
 * Every contract module this package publishes, in one table.
 *
 * A root added to `sdk_schema::bundle` becomes a generated module by adding a
 * row here — the alternative, a hand-written `compile()` block per contract, is
 * what let ten verbs' option and report types go unwritten while the bundle
 * already carried them.
 *
 * `output` marks a contract oneharness *emits*: those are widened by
 * `typescriptSchema` so an additive field a newer CLI sends is `unknown` rather
 * than a type error, which is the same forward-compatibility the Zod side
 * preserves. An input contract is fully specified by definition, so it compiles
 * as written and stays strict.
 */
const CONTRACT_MODULES = Object.freeze([
	{ module: "contracts", key: "run_report", type: "RunReport", output: true },
	{
		module: "run-stream-envelope",
		key: "run_stream_envelope",
		type: "RunStreamEnvelope",
		output: true,
	},
	{ module: "options", key: "run_options", type: "RunOptions", arrays: true },
	{ module: "history-lookup", key: "history_lookup", type: "HistoryLookup" },
	{
		module: "history-list-options",
		key: "history_list_options",
		type: "HistoryListOptions",
	},
	{
		module: "history-watch-options",
		key: "history_watch_options",
		type: "HistoryWatchOptions",
	},
	{ module: "history", key: "history_record", type: "HistoryRecord", output: true },
	{ module: "history-line", key: "history_line", type: "HistoryLine", output: true },
	{
		module: "history-stream-envelope",
		key: "history_stream_envelope",
		type: "HistoryStreamEnvelope",
		output: true,
	},
	// The two array roots carry no title of their own, so the compiler would
	// name them after their element type; `title` is what keeps the exported
	// name the one the client imports.
	{
		module: "history-records",
		key: "history_records",
		type: "HistoryRecords",
		output: true,
		title: true,
	},
	{
		module: "history-list",
		key: "history_list",
		type: "HistoryList",
		output: true,
		title: true,
	},
	{
		module: "registry",
		key: "list_report",
		type: "ListReport",
		output: true,
		banner: "oneharness",
	},
	{
		module: "detection",
		key: "detect_report",
		type: "DetectReport",
		output: true,
		banner: "oneharness",
	},
	{ module: "detect-options", key: "detect_options", type: "DetectOptions" },
	{ module: "config-options", key: "config_options", type: "ConfigOptions" },
	{ module: "config-report", key: "config_report", type: "ConfigReport", output: true },
	{ module: "sync-options", key: "sync_options", type: "SyncOptions" },
	{ module: "sync-report", key: "sync_report", type: "SyncReport", output: true },
	{ module: "init-options", key: "init_options", type: "InitOptions" },
	{ module: "usage-options", key: "usage_options", type: "UsageOptions" },
	{ module: "usage-report", key: "usage_report", type: "UsageReport", output: true },
	{ module: "gate-options", key: "gate_options", type: "GateOptions" },
	{ module: "mock-options", key: "mock_options", type: "MockOptions" },
	{
		module: "interrupt-options",
		key: "interrupt_options",
		type: "InterruptOptions",
	},
	{
		// The Rust type is `ControlResponse` — the frame the control socket
		// speaks, which `interrupt` happens to print. `title` renames it after
		// the capability's contract so the exported name matches the root the
		// Zod module imports, as it does for every other row.
		module: "interrupt-response",
		key: "interrupt_response",
		type: "InterruptResponse",
		output: true,
		title: true,
	},
	{
		module: "history-clear-options",
		key: "history_clear_options",
		type: "HistoryClearOptions",
	},
	{
		module: "history-clear-report",
		key: "history_clear_report",
		type: "HistoryClearReport",
		output: true,
	},
	{
		module: "history-migrate-options",
		key: "history_migrate_options",
		type: "HistoryMigrateOptions",
	},
	{
		module: "history-migrate-report",
		key: "history_migrate_report",
		type: "HistoryMigrateReport",
		output: true,
	},
]);

/** @param {(typeof CONTRACT_MODULES)[number]} contract */
async function compileContract(contract) {
	const source = bundle[contract.key];
	if (source === undefined) {
		throw new Error(
			`sdk_schema::bundle emits no root \`${contract.key}\`; add it there or drop the row from CONTRACT_MODULES in scripts/generate.mjs, then rerun just sdk-generate`,
		);
	}
	const prepared = contract.output ? typescriptSchema(source) : source;
	const schema = contract.title
		? { ...prepared, title: contract.type }
		: prepared;
	const declarations = await compile(schema, contract.type, {
		bannerComment: `/* Generated from ${contract.banner ?? "oneharness-core"}. Do not edit. */`,
		additionalProperties: true,
		style: { endOfLine: "lf" },
	});
	return exactOptionalProperties(
		contract.arrays
			? readonlyArrayProperties(declarations, source)
			: declarations,
	);
}

/** @type {Record<string, Buffer>} */
const contractFiles = {};
for (const contract of CONTRACT_MODULES) {
	contractFiles[`${contract.module}.ts`] = generatedBytes(
		await compileContract(contract),
	);
}
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
	...contractFiles,
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
