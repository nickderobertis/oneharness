import { expect, test } from "bun:test";
import { execFileSync, spawnSync } from "node:child_process";
import {
	copyFileSync,
	existsSync,
	mkdirSync,
	mkdtempSync,
	readFileSync,
	rmSync,
	symlinkSync,
	writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { generatedFileMatches } from "../scripts/generated-file.mjs";
import {
	exactOptionalProperties,
	typescriptSchema,
} from "../scripts/typescript-generator.mjs";
import {
	generateZodModule,
	SDK_SCHEMA_ALIASES,
	SDK_SCHEMA_ROOTS,
} from "../scripts/zod-generator.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "../../..");
const sdkDirectory = "npm/oneharness-sdk";
const generatedDirectory = `${sdkDirectory}/src/generated`;
const lfSdkFile = /\.(?:m?js|ts|json)$/u;

test("SDK inputs stay LF under Windows checkout semantics", () => {
	const checkout = mkdtempSync(resolve(tmpdir(), "oneharness-sdk-checkout-"));
	const checkoutInputs = execFileSync("git", ["ls-files", "--", sdkDirectory], {
		cwd: root,
		encoding: "utf8",
	})
		.trimEnd()
		.split("\n")
		.filter((path) => lfSdkFile.test(path));
	const generated = checkoutInputs.filter((path) =>
		path.startsWith(`${generatedDirectory}/`),
	);
	const authored = checkoutInputs.filter(
		(path) => !path.startsWith(`${generatedDirectory}/`),
	);

	try {
		execFileSync(
			"git",
			[
				"-c",
				"core.autocrlf=true",
				"-c",
				"core.eol=crlf",
				"checkout-index",
				`--prefix=${checkout.replaceAll("\\", "/")}/`,
				"--",
				...checkoutInputs,
			],
			{ cwd: root },
		);

		expect(generated.length).toBeGreaterThan(0);
		expect(authored.length).toBeGreaterThan(0);
		for (const path of checkoutInputs) {
			const content = readFileSync(resolve(checkout, path));
			expect(content.includes(Buffer.from("\r\n"))).toBe(false);
			expect(content.at(-1)).toBe("\n".charCodeAt(0));
		}
	} finally {
		rmSync(checkout, { recursive: true, force: true });
	}
});

test("a missing generated contract is reported as stale", () => {
	const missing = resolve(
		tmpdir(),
		`oneharness-missing-${crypto.randomUUID()}`,
	);
	expect(generatedFileMatches(missing, Buffer.from("expected"))).toBe(false);
});

test("generated optional properties remain exact-optional compatible", () => {
	expect(
		exactOptionalProperties(
			"export interface Options {\n  name?: string;\n  env?: {\n    [key: string]: string;\n  };\n}\n",
		),
	).toBe(
		"export interface Options {\n  name?: string | undefined;\n  env?: {\n    [key: string]: string;\n  } | undefined;\n}\n",
	);
	expect(() =>
		exactOptionalProperties("type Broken = {\n  value?: {\n"),
	).toThrow("generated optional property has no terminator");
	expect(() =>
		exactOptionalProperties("type Broken = {\n  value?: {\n"),
	).toThrow(
		"extend scripts/typescript-generator.mjs for this declaration shape, then rerun just sdk-generate",
	);
});

test("TypeScript generation preserves unconstrained JSON values", () => {
	const unconstrained = { description: "any JSON value" };
	const transformed = typescriptSchema({
		type: "object",
		properties: { value: unconstrained },
		$defs: { Defined: unconstrained },
		oneOf: [unconstrained],
		anyOf: [unconstrained],
		allOf: [unconstrained],
		items: unconstrained,
		additionalProperties: unconstrained,
	});
	expect(transformed.properties.value.tsType).toBe("unknown");
	expect(transformed.$defs.Defined.tsType).toBe("unknown");
	expect(transformed.oneOf[0].tsType).toBe("unknown");
	expect(transformed.anyOf[0].tsType).toBe("unknown");
	expect(transformed.allOf[0].tsType).toBe("unknown");
	expect(transformed.items.tsType).toBe("unknown");
	expect(transformed.additionalProperties.tsType).toBe("unknown");
	expect(typescriptSchema(true)).toBe(true);
	expect(typescriptSchema([unconstrained])).toEqual([unconstrained]);
});

test("Zod generation is deterministic and encodes deliberate unknown-key behavior", () => {
	const bundle = {
		input: {
			title: "Input",
			type: "object",
			properties: { prompt: { type: "string", minLength: 1 } },
			required: ["prompt"],
			additionalProperties: false,
		},
		output: {
			title: "Output",
			type: "object",
			properties: {
				value: { type: "integer", minimum: 0 },
				requiredUnknown: true,
				optionalUnknown: true,
			},
			required: ["value", "requiredUnknown"],
		},
	};
	const roots = [
		{ key: "input", type: "Input", module: "input" },
		{ key: "output", type: "Output", module: "output" },
	];
	const first = generateZodModule(bundle, roots);
	expect(generateZodModule(bundle, roots)).toBe(first);
	expect(first).toContain("InputSchema: z.ZodType<Input> = z.strictObject");
	expect(first).toContain("OutputSchema: z.ZodType<Output> = z.looseObject");
	expect(first).toContain(
		'"value": z.int().gte(0).refine((value) => value !== undefined, { message: "Required" })',
	);
	expect(first).toContain(
		'"requiredUnknown": z.unknown().refine((value) => value !== undefined, { message: "Required" })',
	);
	expect(first).toContain('"optionalUnknown": z.unknown().optional()');
});

test("focused Zod generator covers the complete checked-in Rust schema bundle", () => {
	const bundle = JSON.parse(
		readFileSync(resolve(root, generatedDirectory, "schemas.json"), "utf8"),
	);
	const generated = generateZodModule(
		bundle,
		SDK_SCHEMA_ROOTS,
		SDK_SCHEMA_ALIASES,
	);
	for (const name of [
		"RunOptions",
		"HistoryLookup",
		"HistoryLookupBySession",
		"HistoryLookupByLast",
		"HistoryListOptions",
		"HistoryWatchOptions",
		"RunReport",
		"RunStreamEnvelope",
		"RunResult",
		"ActionEvent",
		"Usage",
		"HistoryRecord",
		"HistoryStreamEnvelope",
		"HistoryRecords",
		"HistoryList",
		"HistorySessionSummary",
		"ListReport",
		"DetectReport",
	]) {
		expect(generated).toContain(`export const ${name}Schema`);
	}
	expect(generated).not.toContain("as unknown as z.ZodType");
	expect(generated).toContain(
		"export const RunReportSchema: z.ZodType<RunReport>",
	);
	expect(generated).toContain(
		"export const RunStreamEnvelopeSchema: z.ZodType<RunStreamEnvelope>",
	);
	expect(generated).toContain(
		"export const HistoryStreamEnvelopeSchema: z.ZodType<HistoryStreamEnvelope>",
	);
	expect(generated).toContain("z.lazy(() => RunResultSchema)");
	expect(generated).toContain("z.record(z.string(), z.string())");
	const options = readFileSync(
		resolve(root, generatedDirectory, "options.ts"),
		"utf8",
	);
	expect(options).toContain("cwd?: string | undefined;");
	const contracts = readFileSync(
		resolve(root, generatedDirectory, "contracts.ts"),
		"utf8",
	);
	expect(contracts).toContain("input: unknown;");
	expect(contracts).toContain("[k: string]: unknown;");
});

test("Zod generation rejects schema keywords it cannot enforce", () => {
	expect(() =>
		generateZodModule(
			{
				instant: {
					title: "Instant",
					type: "string",
					format: "date-time",
				},
			},
			[{ key: "instant", type: "Instant", module: "instant" }],
		),
	).toThrow("unsupported JSON Schema string format date-time");
});

test("Zod generation validates every supported schema boundary", () => {
	const render = (schema: object) =>
		generateZodModule({ boundary: schema }, [
			{ key: "boundary", type: "Boundary", module: "boundary" },
		]);
	const reject = (schema: object, message: string) =>
		expect(() => render(schema)).toThrow(message);

	reject(
		{ type: "string", minItems: 1 },
		"unsupported JSON Schema keyword boundary.minItems",
	);
	reject(
		{ type: "string", minItems: 1 },
		"extend scripts/zod-generator.mjs to enforce it, then rerun just sdk-generate",
	);
	// A string's length is measured in code points, not the UTF-16 code units
	// Zod's `.max()` counts, so the bound is generated as a spread instead.
	expect(render({ type: "string", minLength: 1, maxLength: 4 })).toContain(
		"z.string().min(1).refine((value) => [...value].length <= 4",
	);
	expect(
		render({ type: "string", not: { pattern: "[\\u0000-\\u001f]" } }),
	).toContain(
		'.refine((value) => !new RegExp("[\\\\u0000-\\\\u001f]", "u").test(value)',
	);
	for (const unsupported of [
		{ pattern: "a", minLength: 1 },
		{ minLength: 1 },
		true,
	]) {
		reject(
			{ type: "string", not: unsupported },
			"unsupported JSON Schema `not` at boundary.not",
		);
	}
	// A keyword no member of a nullable union claims would otherwise be filtered
	// away silently rather than enforced.
	reject(
		{ type: ["string", "null"], minItems: 1 },
		"unsupported JSON Schema keyword boundary.minItems",
	);
	expect(render({ type: ["string", "null"], not: { pattern: "x" } })).toContain(
		".test(value)",
	);
	reject({ $ref: "other.json" }, "unsupported non-local JSON Schema reference");
	reject(
		{ $ref: "#/$defs/not/a/name" },
		"unsupported JSON Schema definition name",
	);
	reject(
		{ type: "number", format: "decimal128" },
		"unsupported JSON Schema number format decimal128",
	);
	expect(
		render({
			type: "number",
			minimum: 1,
			maximum: 10,
			exclusiveMinimum: 0,
			exclusiveMaximum: 11,
			multipleOf: 0.5,
		}),
	).toContain("z.number().gte(1).lte(10).gt(0).lt(11).multipleOf(0.5)");
	reject({ type: "array" }, "array schema needs one items schema");
	reject(
		{ type: "array", items: [{ type: "string" }] },
		"array schema needs one items schema",
	);
	reject(
		{ type: "array", items: { type: "string" }, uniqueItems: true },
		"unsupported JSON Schema keyword boundary.uniqueItems",
	);
	reject(
		{ type: "object", minProperties: 1 },
		"unsupported JSON Schema object size constraint",
	);
	reject(
		{ type: "object", properties: {}, required: ["missing"] },
		"required property boundary.missing has no schema",
	);
	reject(
		{ type: "object", properties: { broken: undefined } },
		"property boundary.broken has no schema",
	);
	expect(
		render({
			type: "object",
			properties: { known: { type: "string" } },
			additionalProperties: { type: "boolean" },
		}),
	).toContain(".catchall(z.boolean())");
	reject(
		{ type: "object", properties: { broken: null } },
		"invalid JSON Schema node",
	);
	expect(render({ enum: ["one", "two"] })).toContain(
		'z.union([z.literal("one"), z.literal("two")])',
	);
	expect(
		render({ allOf: [{ type: "string" }, { enum: ["value"] }] }),
	).toContain('z.intersection(z.string(), z.literal("value"))');
	expect(render({ allOf: [] })).toContain("z.unknown()");
	reject({ type: "date" }, "unsupported JSON Schema type date");
	expect(() =>
		generateZodModule({}, [
			{ key: "missing", type: "Missing", module: "missing" },
		]),
	).toThrow("Rust schema bundle is missing missing");
	expect(() =>
		generateZodModule(
			{
				first: { type: "object", $defs: { Shared: { type: "string" } } },
				second: { type: "object", $defs: { Shared: { type: "number" } } },
			},
			[
				{ key: "first", type: "First", module: "first" },
				{ key: "second", type: "Second", module: "second" },
			],
		),
	).toThrow("conflicting Rust schemas named Shared");
	expect(() =>
		generateZodModule({ invalid: { type: "object", $defs: { Flag: true } } }, [
			{ key: "invalid", type: "Invalid", module: "invalid" },
		]),
	).toThrow("Rust schema definition invalid.Flag is not an object");
});

test("generator check reports a missing generated contract as stale", () => {
	const checkout = mkdtempSync(resolve(tmpdir(), "oneharness-sdk-generate-"));

	try {
		execFileSync(
			"git",
			["checkout-index", `--prefix=${checkout.replaceAll("\\", "/")}/`, "-a"],
			{ cwd: root },
		);
		symlinkSync(
			resolve(root, sdkDirectory, "node_modules"),
			resolve(checkout, sdkDirectory, "node_modules"),
			process.platform === "win32" ? "junction" : "dir",
		);

		const missing = resolve(checkout, generatedDirectory, "zod.ts");
		copyFileSync(resolve(root, generatedDirectory, "zod.ts"), missing);
		rmSync(missing);
		const result = spawnSync(
			"node",
			[`${sdkDirectory}/scripts/generate.mjs`, "--check"],
			{
				cwd: checkout,
				encoding: "utf8",
				env: { ...process.env, CARGO_TARGET_DIR: resolve(root, "target") },
			},
		);

		expect(result.status).toBe(1);
		expect(result.stderr.trim()).toBe(
			"generated SDK contracts are stale; run just sdk-generate",
		);
		expect(existsSync(missing)).toBe(false);
	} finally {
		rmSync(checkout, { recursive: true, force: true });
	}
}, 15_000);

test("SDK packing reports a missing Cargo version without a stack trace", () => {
	const checkout = mkdtempSync(resolve(tmpdir(), "oneharness-sdk-pack-"));
	mkdirSync(resolve(checkout, "scripts"));
	copyFileSync(
		resolve(root, "scripts/sdk-pack.mjs"),
		resolve(checkout, "scripts/sdk-pack.mjs"),
	);
	writeFileSync(
		resolve(checkout, "Cargo.toml"),
		'[package]\nname = "broken"\n',
	);

	try {
		execFileSync(process.execPath, ["scripts/sdk-pack.mjs"], {
			cwd: checkout,
			stdio: "pipe",
		});
		throw new Error("sdk-pack unexpectedly accepted a missing version");
	} catch (error) {
		if (!(error instanceof Error) || !("stderr" in error)) throw error;
		const stderr = String(error.stderr);
		expect(stderr).toContain("Cargo.toml has no [package] version");
		expect(stderr).toContain("restore the root manifest");
		expect(stderr).not.toContain("at file:");
	} finally {
		rmSync(checkout, { recursive: true, force: true });
	}
});
