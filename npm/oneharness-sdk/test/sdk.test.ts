import { describe, expect, test } from "bun:test";
import { execFileSync } from "node:child_process";
import { mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import type { ZodType, z } from "zod";
import {
	type ActionEvent,
	ActionEventSchema,
	BatchReportSchema,
	CAPABILITIES,
	ConfigOptionsSchema,
	DetectOptionsSchema,
	type DetectReport,
	DetectReportSchema,
	FallbackReportSchema,
	GateOptionsSchema,
	type HarnessInfo,
	HarnessInfoSchema,
	HistoryClearOptionsSchema,
	type HistoryLine,
	HistoryLineSchema,
	type HistoryList,
	type HistoryListOptions,
	HistoryListOptionsSchema,
	HistoryListSchema,
	type HistoryLookup,
	HistoryLookupSchema,
	HistoryMigrateOptionsSchema,
	HistoryNotFoundError,
	type HistoryRecord,
	HistoryRecordSchema,
	type HistoryRecords,
	HistoryRecordsSchema,
	type HistorySessionSummary,
	HistorySessionSummarySchema,
	type HistoryStreamEnvelope,
	HistoryStreamEnvelopeSchema,
	type HistoryWatchOptions,
	HistoryWatchOptionsSchema,
	InitOptionsSchema,
	InterruptOptionsSchema,
	type ListReport,
	ListReportSchema,
	MockOptionsSchema,
	OneHarness,
	OneHarnessProcessError,
	type RunOptions,
	RunOptionsSchema,
	type RunReport,
	RunReportSchema,
	type RunResult,
	RunResultSchema,
	type RunStreamEnvelope,
	RunStreamEnvelopeSchema,
	SessionReportSchema,
	SyncOptionsSchema,
	type Usage,
	UsageOptionsSchema,
	UsageSchema,
} from "../src/index.js";

const here = dirname(fileURLToPath(import.meta.url));
const binary = resolve(here, "../../../target/debug/oneharness");
const mock = resolve(here, "../../../target/debug/oneharness-mock-harness");
const invalidCli = resolve(here, "invalid-cli-fixture.mjs");
const contractMatrix = JSON.parse(
	await readFile(
		resolve(here, "../../../tests/fixtures/sdk-contract-matrix.json"),
		"utf8",
	),
) as {
	cases: Array<{
		name: string;
		root:
			| "run_options"
			| "history_lookup"
			| "history_list_options"
			| "history_watch_options"
			| "history_line"
			| "history_record"
			| "history_stream_envelope";
		accepted: boolean;
		value: unknown;
	}>;
};
// Deliberately absent: a client pointed here can validate but never spawn, so a
// boundary check that runs before the subprocess is the only way to see the
// validation error rather than this path's spawn failure.
const unspawnable = resolve(here, "missing-oneharness-fixture");
const historyTrace = [
	'{"type":"turn.started"}',
	'{"type":"item.completed","item":{"id":"m1","type":"agent_message","text":"history"}}',
	'{"type":"turn.completed"}',
].join("\n");

type Equal<Left, Right> =
	(<Value>() => Value extends Left ? 1 : 2) extends <
		Value,
	>() => Value extends Right ? 1 : 2
		? true
		: false;

const inferredSchemasMatchGeneratedTypes: [
	Equal<z.infer<typeof RunOptionsSchema>, RunOptions>,
	Equal<z.infer<typeof HistoryListOptionsSchema>, HistoryListOptions>,
	Equal<z.infer<typeof HistoryLineSchema>, HistoryLine>,
	Equal<z.infer<typeof HistoryLookupSchema>, HistoryLookup>,
	Equal<z.infer<typeof HistoryWatchOptionsSchema>, HistoryWatchOptions>,
	Equal<z.infer<typeof RunReportSchema>, RunReport>,
	Equal<z.infer<typeof RunResultSchema>, RunResult>,
	Equal<z.infer<typeof ActionEventSchema>, ActionEvent>,
	Equal<z.infer<typeof UsageSchema>, Usage>,
	Equal<z.infer<typeof HistoryRecordSchema>, HistoryRecord>,
	Equal<z.infer<typeof HistoryStreamEnvelopeSchema>, HistoryStreamEnvelope>,
	Equal<z.infer<typeof HistoryRecordsSchema>, HistoryRecords>,
	Equal<z.infer<typeof HistoryListSchema>, HistoryList>,
	Equal<z.infer<typeof HistorySessionSummarySchema>, HistorySessionSummary>,
	Equal<z.infer<typeof ListReportSchema>, ListReport>,
	Equal<z.infer<typeof HarnessInfoSchema>, HarnessInfo>,
	Equal<z.infer<typeof DetectReportSchema>, DetectReport>,
	Equal<z.infer<typeof RunStreamEnvelopeSchema>, RunStreamEnvelope>,
] = [
	true,
	true,
	true,
	true,
	true,
	true,
	true,
	true,
	true,
	true,
	true,
	true,
	true,
	true,
	true,
	true,
	true,
	true,
];

// A host that runs its agents through oneharness carries `ONEHARNESS_*`
// overrides, and they outrank a project file — a leaked `ONEHARNESS_HARNESSES`
// silently reselects the harnesses a config or sync assertion is about. The
// Rust suite strips them in `run_with_config` and `smoke.sh` in `oh_hermetic`;
// this is the same rule for the tests that spawn from `process.env`.
for (const name of Object.keys(process.env)) {
	if (name.startsWith("ONEHARNESS_")) delete process.env[name];
}

type JsonSchema = Record<string, unknown>;
const bundle = JSON.parse(
	await readFile(resolve(here, "../src/generated/schemas.json"), "utf8"),
) as Record<string, JsonSchema>;

// A string satisfying each pattern the bundle constrains one with. The default
// is deliberately alphanumeric: it starts with `[A-Za-z0-9]`, carries no
// control character, and uses no character a label key forbids, so it passes
// every `not`-shaped rule at once.
const PATTERNED: Array<[RegExp, string]> = [
	[/T\(\[01\]/u, "2026-08-11T00:00:00.000Z"],
	[/\{8\}-/u, "0198f0d0-7b31-7000-8000-000000000001"],
];

/**
 * A value populating every field of one generated schema.
 *
 * Populating every field is the point: a generated validator is a per-field
 * check, so a field no instance ever carries is a check no test ever runs. At
 * every union it takes the first arm, which keeps the conditionals these
 * schemas encode as parallel alternatives in agreement with each other.
 */
function populate(
	node: JsonSchema | undefined,
	defs: Record<string, JsonSchema>,
): unknown {
	if (!node) return null;
	const scope = {
		...defs,
		...((node.$defs as Record<string, JsonSchema>) ?? {}),
	};
	const deref = (child: JsonSchema | undefined) =>
		typeof child?.$ref === "string"
			? scope[child.$ref.split("/").pop() as string]
			: child;
	const firstArm = (child: JsonSchema | undefined) => {
		const arms = (child?.oneOf ?? child?.anyOf) as JsonSchema[] | undefined;
		return arms ? arms[0] : child;
	};
	if (typeof node.$ref === "string") return populate(deref(node), scope);
	if ("const" in node) return node.const;
	if (Array.isArray(node.enum)) return node.enum[0];
	// Flatten as SCHEMAS, never as the values the parts would each build, when a
	// node's fields are spread across siblings rather than stated in one place:
	// `allOf` parts, and a `properties` that sits BESIDE a union rather than
	// inside it. `UsageWindow` declares `id` and `usage` itself and refines the
	// rest through a `oneOf`, so reading only the union would drop both required
	// fields; and merging built objects rather than schemas would let a later
	// part's value overwrite an earlier part's variant tag.
	if (
		Array.isArray(node.allOf) ||
		(node.properties && firstArm(node) !== node)
	) {
		const properties: Record<string, JsonSchema> = {};
		// A variant that FORBIDS a field spells it `false`, and these variants are
		// exclusive: a record measured at the stdout pipe carries
		// `observed_tool_ms` and must not carry `model_ms`. Merging every part's
		// fields without honouring that would build a document combining two
		// variants, which the contract rightly refuses.
		const forbidden = new Set<string>();
		const collect = (part: JsonSchema | undefined): void => {
			const here = deref(part);
			if (!here) return;
			for (const [name, child] of Object.entries(
				(here.properties as Record<string, unknown>) ?? {},
			)) {
				if (child === false) {
					forbidden.add(name);
					delete properties[name];
					continue;
				}
				if (!forbidden.has(name)) properties[name] = child as JsonSchema;
			}
			// A node's own `oneOf` and `allOf` are siblings, not alternatives:
			// `history_record` carries both, and reading only the `allOf` would
			// build a document missing every field the union half declares.
			const arm = firstArm(here);
			if (arm !== here) collect(arm);
			for (const nested of (here.allOf as JsonSchema[]) ?? []) collect(nested);
		};
		collect(node);
		return populate({ type: "object", properties }, scope);
	}
	const arms = (node.oneOf ?? node.anyOf) as JsonSchema[] | undefined;
	if (arms) return populate(arms[0], scope);
	const declared = node.type;
	const kind = Array.isArray(declared)
		? ((declared.find((item) => item !== "null") ?? "null") as string)
		: (declared as string | undefined);
	switch (kind) {
		case "object": {
			const value: Record<string, unknown> = {};
			for (const [name, child] of Object.entries(
				(node.properties as Record<string, unknown>) ?? {},
			)) {
				// `false` is how a variant FORBIDS a field — a served interrupt
				// frame must carry no `error`. Emitting one would build a
				// document the contract refuses on purpose.
				if (child === false) continue;
				value[name] = populate(child as JsonSchema, scope);
			}
			const extra = node.additionalProperties;
			if (extra && typeof extra === "object")
				value.a = populate(extra as JsonSchema, scope);
			return value;
		}
		case "array":
			return [populate(node.items as JsonSchema, scope)];
		case "boolean":
			return true;
		case "integer":
		case "number":
			// A window's length is `>= 1`, not `>= 0`: a bound stated in the
			// schema has to be met or the instance is not one.
			return typeof node.minimum === "number" ? node.minimum : 0;
		case "null":
			return null;
		default: {
			const pattern = String(node.pattern ?? "");
			const matched = PATTERNED.find(([probe]) => probe.test(pattern));
			return matched ? matched[1] : "a";
		}
	}
}

function sdk(): OneHarness {
	return new OneHarness({
		executable: binary,
		env: { ONEHARNESS_NO_CONFIG: "1" },
	});
}

/**
 * A client that DOES read configuration files, for the two verbs whose whole
 * subject is the layering. Hermetic by the ambient strip above rather than by
 * `ONEHARNESS_NO_CONFIG`, which would switch off the thing under test.
 */
function layered(): OneHarness {
	return new OneHarness({ executable: binary });
}

describe("OneHarness", () => {
	test("generated validators match the shared SDK acceptance matrix", () => {
		const schemas = {
			run_options: RunOptionsSchema,
			history_lookup: HistoryLookupSchema,
			history_list_options: HistoryListOptionsSchema,
			history_watch_options: HistoryWatchOptionsSchema,
			history_line: HistoryLineSchema,
			history_record: HistoryRecordSchema,
			history_stream_envelope: HistoryStreamEnvelopeSchema,
		};
		expect(contractMatrix.cases.length).toBeGreaterThan(0);
		for (const fixture of contractMatrix.cases) {
			expect(
				schemas[fixture.root].safeParse(fixture.value).success,
				fixture.name,
			).toBe(fixture.accepted);
		}
	});

	test("generated schema inference matches every generated public type", () => {
		expect(inferredSchemasMatchGeneratedTypes.every(Boolean)).toBe(true);
		const readonlyOptions = {
			prompt: "typed caller",
			harnesses: ["codex:apikey"],
			models: ["provider/model"],
		} satisfies RunOptions;
		expect(RunOptionsSchema.parse(readonlyOptions).harnesses).toEqual([
			"codex:apikey",
		]);
		const identity: Pick<RunResult, "variant" | "harness_id"> = {
			variant: "apikey",
			harness_id: "codex:apikey",
		};
		expect(identity.variant).toBe("apikey");
		expect(
			BatchReportSchema.parse({
				forked: false,
				prompt_count: 1,
				strategy: "speed",
			}),
		).toEqual({ forked: false, prompt_count: 1, strategy: "speed" });
		expect(
			FallbackReportSchema.parse({
				fell_through: [{ harness: "codex", reason: "auth" }],
				ran: null,
			}),
		).toEqual({
			fell_through: [{ harness: "codex", reason: "auth" }],
			ran: null,
		});
		expect(
			SessionReportSchema.parse({
				name: "named",
				phase: "create",
				token: null,
				store_file: null,
			}),
		).toEqual({
			name: "named",
			phase: "create",
			token: null,
			store_file: null,
		});
	});

	test("every generated validator accepts a fully populated instance of its own schema", async () => {
		// The Zod modules are generated from the JSON Schema bundle, and both are
		// checked in — so the two can disagree, and nothing else here would
		// notice. This walks every root: it synthesizes a value populating every
		// field from the schema, then parses it with the validator generated
		// from that same schema. A field-level check the generator got wrong
		// fails here rather than at a consumer.
		//
		// It is also what exercises the per-field validators at all. They are one
		// function apiece, and a field no instance ever carries is a function no
		// test ever calls, which is what the package's coverage floor is really
		// asking about.
		const exported = (await import("../src/index.js")) as unknown as Record<
			string,
			ZodType<unknown> | undefined
		>;
		const roots = Object.keys(bundle).filter((name) => name !== "capabilities");
		expect(roots.length).toBeGreaterThan(20);
		let checked = 0;
		const rejections: string[] = [];
		for (const root of roots) {
			const name = `${root
				.split("_")
				.map((part) => part[0]?.toUpperCase() + part.slice(1))
				.join("")}Schema`;
			const schema = exported[name];
			// Not every root is published as a validator; the ones that are must
			// agree with the schema they were generated from.
			if (!schema) continue;
			const value = populate(bundle[root], {});
			if (!schema.safeParse(value).success)
				rejections.push(`${name}: ${JSON.stringify(value)}`);
			checked += 1;
		}
		// Every root is walked before anything is asserted, so one disagreement
		// does not hide the rest — and so the parse of every other root still
		// happens, which is what exercises their validators.
		expect(rejections.slice(0, 3).join("\n")).toBe("");
		expect(checked).toBeGreaterThan(20);
	});

	test("crosses the Node to CLI boundary and preserves absent usage", async () => {
		const client = sdk();
		const report = await client.run({
			prompt: "sdk boundary",
			harnesses: ["claude-code"],
			mode: "bypass",
			env: { MOCK_STDOUT: '{"result":"hello from sdk"}' },
			bins: { "claude-code": mock },
		});
		expect(report.results[0]?.text).toBe("hello from sdk");
		expect(report.results[0]?.usage.input_tokens).toBeNull();
		expect(RunReportSchema.safeParse(report).success).toBe(true);
		expect(RunResultSchema.safeParse(report.results[0]).success).toBe(true);
		expect(UsageSchema.safeParse(report.results[0]?.usage).success).toBe(true);
		const incompleteReport: Partial<RunReport> = { ...report };
		delete incompleteReport.history_file;
		expect(RunReportSchema.safeParse(incompleteReport).success).toBe(false);
		expect(
			RunStreamEnvelopeSchema.parse({
				type: "result",
				report,
				future_output_field: true,
			}),
		).toMatchObject({ type: "result", future_output_field: true });

		const traced = await client.run({
			prompt: "sdk trace",
			harnesses: ["claude-code"],
			mode: "bypass",
			events: true,
			env: {
				MOCK_STDOUT: [
					'{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"echo hi"}}]}}',
					'{"type":"result","result":"done","usage":{"input_tokens":0,"output_tokens":1,"cache_read_input_tokens":0,"cache_creation_input_tokens":2}}',
				].join("\n"),
			},
			bins: { "claude-code": mock },
		});
		expect(traced.results[0]?.usage.input_tokens).toBe(0);
		expect(traced.results[0]?.usage.cache_read_tokens).toBe(0);
		expect(traced.results[0]?.events?.[0]?.name).toBe("Bash");
		expect(traced.results[0]?.events?.[0]?.input).toEqual({
			command: "echo hi",
		});
		expect(traced.results[0]?.events?.[0]?.kind).toBe("tool_call");
		expect(traced.results[0]?.events?.[0]?.index).toBe(0);
		expect(traced.results[0]?.events?.[0]?.output).toBeNull();
		expect(
			RunStreamEnvelopeSchema.safeParse({
				type: "event",
				event: traced.results[0]?.events?.[0],
			}).success,
		).toBe(true);
		expect(
			RunStreamEnvelopeSchema.safeParse({ type: "future_variant" }).success,
		).toBe(false);
		expect(traced.results[0]?.structured).toBeNull();
		expect(
			ActionEventSchema.safeParse(traced.results[0]?.events?.[0]).success,
		).toBe(true);
	});

	test("runMock uses the responder shipped in the main CLI", async () => {
		const report = await sdk().runMock(
			"claude-code",
			{ prompt: "deterministic", mode: "bypass" },
			{ stdout: '{"result":"node mock"}', exitCode: 0, latencyMs: 1 },
		);
		expect(report.results[0]?.text).toBe("node mock");
	});

	test("streams every validated envelope across the real CLI boundary", async () => {
		const envelopes: RunStreamEnvelope[] = [];
		for await (const envelope of sdk().runStream({
			prompt: "stream from sdk",
			harnesses: ["opencode"],
			mode: "bypass",
			env: {
				MOCK_STDOUT: [
					'{"type":"tool_use","part":{"type":"tool","tool":"bash","state":{"status":"completed","input":{"command":"echo hi"},"output":"hi"}}}',
					'{"type":"text","part":{"type":"text","text":"stream finished"}}',
				].join("\n"),
			},
			bins: { opencode: mock },
		})) {
			envelopes.push(envelope);
		}

		expect(envelopes.map(({ type }) => type)).toEqual(["event", "result"]);
		expect(envelopes[0]).toMatchObject({
			type: "event",
			event: { name: "bash", input: { command: "echo hi" } },
		});
		expect(envelopes[1]).toMatchObject({
			type: "result",
			report: { results: [{ text: "stream finished" }] },
		});
		for (const envelope of envelopes)
			expect(RunStreamEnvelopeSchema.safeParse(envelope).success).toBe(true);
	});

	test("terminates a streaming subprocess when the iterator closes early", async () => {
		const directory = await mkdtemp(
			resolve(tmpdir(), "oneharness-sdk-cancel-"),
		);
		const log = resolve(directory, "mock.log");
		const stream = sdk().runStream({
			prompt: "stop after the first action",
			harnesses: ["opencode"],
			mode: "bypass",
			env: {
				MOCK_LOG_FILE: log,
				MOCK_STREAM_DELAY_MS: "500",
				MOCK_STDOUT: [
					'{"type":"tool_use","part":{"type":"tool","tool":"first","state":{"input":{}}}}',
					'{"type":"tool_use","part":{"type":"tool","tool":"second","state":{"input":{}}}}',
					'{"type":"tool_use","part":{"type":"tool","tool":"third","state":{"input":{}}}}',
				].join("\n"),
			},
			bins: { opencode: mock },
		});

		expect(await stream.next()).toMatchObject({
			done: false,
			value: { type: "event", event: { name: "first" } },
		});
		await stream.return(undefined);
		await Bun.sleep(700);
		expect(await readFile(log, "utf8")).not.toContain("COMPLETE");
	});

	test("preserves unknown output fields but rejects unknown run options", async () => {
		const report = await sdk().run({
			prompt: "future contract",
			harnesses: ["claude-code"],
			mode: "bypass",
			bins: { "claude-code": mock },
		});
		const result = report.results[0];
		if (!result) throw new Error("real CLI fixture had no result");
		const future = {
			...report,
			future_report: { enabled: true },
			results: [
				{
					...result,
					future_result: "kept",
					usage: { ...result.usage, future_usage: 7 },
				},
			],
		};
		const parsed = RunReportSchema.parse(future) as RunReport & {
			future_report: { enabled: boolean };
			results: Array<
				RunResult & {
					future_result: string;
					usage: Usage & { future_usage: number };
				}
			>;
		};
		expect(parsed.future_report.enabled).toBe(true);
		expect(parsed.results[0]?.future_result).toBe("kept");
		expect(parsed.results[0]?.usage.future_usage).toBe(7);

		const misspelled = {
			prompt: "typo",
			harneses: ["codex"],
		} as unknown as RunOptions;
		expect(RunOptionsSchema.safeParse(misspelled).success).toBe(false);
		await expect(sdk().run(misspelled)).rejects.toThrow(
			"invalid oneharness run options",
		);
	});

	test("preserves additive fields returned by a newer real CLI subprocess", async () => {
		const client = new OneHarness({
			executable: process.execPath,
			executableArgs: [invalidCli],
			env: {
				ONEHARNESS_NO_CONFIG: "1",
				SDK_FIXTURE_MODE: "additive",
				SDK_REAL_CLI: binary,
			},
		});
		const report = (await client.run({
			prompt: "future CLI",
			harnesses: ["claude-code"],
			mode: "bypass",
			bins: { "claude-code": mock },
		})) as RunReport & {
			future_output_field: { preserved: boolean };
			results: Array<RunResult & { future_result_field: number }>;
		};
		expect(report.future_output_field.preserved).toBe(true);
		expect(report.results[0]?.future_result_field).toBe(7);
		const listed = (await client.list()) as Array<
			HarnessInfo & { future_harness_field: number }
		>;
		expect(listed[0]?.future_harness_field).toBe(7);
	});

	test("rejects unusable public options before spawning the CLI", async () => {
		// This client can never spawn, so an option that reached the CLI would
		// surface that spawn failure instead. Only a check that runs before the
		// subprocess can produce the validation error each case asserts.
		const client = new OneHarness({ executable: unspawnable });
		const runRejects = async (options: unknown) => {
			await expect(client.run(options as RunOptions)).rejects.toThrow(
				"invalid oneharness run options",
			);
		};
		const listRejects = async (options: unknown) => {
			await expect(
				client.historyList(options as HistoryListOptions),
			).rejects.toThrow("invalid oneharness history list options");
		};
		const historyRejects = async (lookup: unknown) => {
			await expect(client.history(lookup as HistoryLookup)).rejects.toThrow(
				"invalid oneharness history options",
			);
		};

		// Non-object: nothing to read an option off of.
		for (const nonObject of [null, "run", 42, true, [], () => "run"]) {
			await runRejects(nonObject);
			await listRejects(nonObject);
			await historyRejects(nonObject);
		}

		// Empty: run needs a prompt, while every history list option is optional.
		await runRejects({});
		expect(HistoryListOptionsSchema.safeParse({}).success).toBe(true);

		// Misspelled: a near-miss key this SDK version cannot forward.
		await runRejects({ prompt: "typo", harneses: ["codex"] });
		await listRejects({ allProject: true });
		await historyRejects({ sesion: "node-session" });
		await historyRejects({ session: "node-session", lastest: true });

		// Malformed: a known key carrying the wrong type.
		await runRejects({ prompt: 42 });
		await runRejects({ prompt: "wrong shape", harnesses: "codex" });
		await listRejects({ project: 42 });
		await listRejects({ allProjects: "yes" });
		await historyRejects({ session: 42 });
		await historyRejects({ last: "yes" });
		await historyRejects({ last: true, historyDir: 42 });
	});

	test("rejects a history lookup that selects no session", async () => {
		// Same unspawnable client: every rejection below happens before a process.
		const client = new OneHarness({ executable: unspawnable });
		const selectorRejects = async (lookup: unknown) => {
			expect(HistoryLookupSchema.safeParse(lookup).success).toBe(false);
			await expect(client.history(lookup as HistoryLookup)).rejects.toThrow(
				"invalid oneharness history options",
			);
		};

		// The union has no variant for a lookup that neither names a session nor
		// asks for the last one, so validation rejects these — an SDK-side selector
		// rule no longer has to. An empty session and an explicit `last: false`
		// select nothing, so they are not spellings of a selector.
		for (const selectorless of [
			{},
			{ historyDir: "/tmp/oneharness-history" },
			{ session: "" },
			{ last: false },
			{ session: "", last: false },
		]) {
			await selectorRejects(selectorless);
		}

		// Every valid selector clears validation, so this client fails on the spawn
		// it reached rather than on validation — the proof the rejections above are
		// the boundary talking, not a missing binary.
		const selectors = [
			{ session: "node-session" },
			{ last: true },
			{ session: "node-session", last: false },
			{ session: "node-session", last: true },
		] satisfies HistoryLookup[];
		for (const selector of selectors) {
			expect(HistoryLookupSchema.safeParse(selector).success).toBe(true);
			await expect(client.history(selector)).rejects.toThrow(
				"missing-oneharness-fixture",
			);
		}
	});

	test("lists and detects the open harness registry", async () => {
		const client = sdk();
		const listed = await client.list();
		const claude = listed.find(({ id }) => id === "claude-code");
		expect(claude?.supports_resume).toBe(true);
		expect(claude?.supports_fork).toBe(true);
		expect(claude?.supports_reasoning).toBe(true);
		expect(claude?.modes).toContainEqual({ mode: "bypass", headless: "clean" });
		const detected = await client.detect(["claude-code"]);
		expect(detected).toHaveLength(1);
		expect(detected[0]?.id).toBe("claude-code");

		const rawList = JSON.parse(
			execFileSync(binary, ["list", "--compact"], {
				encoding: "utf8",
				env: { ...process.env, ONEHARNESS_NO_CONFIG: "1" },
			}),
		);
		expect(ListReportSchema.safeParse(rawList).success).toBe(true);
		expect(HarnessInfoSchema.safeParse(rawList.harnesses[0]).success).toBe(
			true,
		);
		const rawDetect = JSON.parse(
			execFileSync(
				binary,
				[
					"detect",
					"--compact",
					"--harness",
					"claude-code",
					"--bin",
					`claude-code=${mock}`,
				],
				{
					encoding: "utf8",
					env: { ...process.env, ONEHARNESS_NO_CONFIG: "1" },
				},
			),
		);
		expect(DetectReportSchema.safeParse(rawDetect).success).toBe(true);
	}, 30_000);

	test("looks up standardized history created across the CLI boundary", async () => {
		const historyDir = await mkdtemp(resolve(tmpdir(), "oneharness-sdk-"));
		const client = sdk();
		await client.run({
			prompt: "history sdk",
			harnesses: ["codex"],
			mode: "bypass",
			history: true,
			historyName: "node-session",
			historyDir,
			env: { MOCK_STDOUT: historyTrace },
			bins: { codex: mock },
		});
		const records = await client.history({
			session: "node-session",
			historyDir,
		});
		expect(records[0]?.prompt).toBe("history sdk");
		expect(records[0]?.name).toBe("node-session");
		expect(records[0]?.status).toBe("ok");
		const historyId = records[0]?.history_id;
		if (!historyId) throw new Error("history record had no exact id");
		const exact = await client.history({ session: historyId, historyDir });
		expect(exact).toHaveLength(1);
		expect(exact[0]?.history_id).toBe(historyId);

		// Only one session exists, so every way to select reaches the same records
		// across the real CLI boundary.
		for (const lookup of [
			{ last: true, historyDir },
			{ session: "node-session", last: false, historyDir },
			{ session: "node-session", last: true, historyDir },
		] satisfies HistoryLookup[]) {
			expect((await client.history(lookup))[0]?.name).toBe("node-session");
		}
		expect(HistoryRecordsSchema.safeParse(records).success).toBe(true);
		expect(HistoryRecordSchema.safeParse(records[0]).success).toBe(true);
		await client.run({
			prompt: "history without timing",
			harnesses: ["claude-code"],
			history: true,
			historyName: "node-session-unmeasured",
			historyDir,
			env: { MOCK_STDOUT: '{"type":"result","result":"done"}' },
			bins: { "claude-code": mock },
		});
		const [unmeasured] = await client.history({
			session: "node-session-unmeasured",
			historyDir,
		});
		expect(unmeasured?.schema_version).toBe("1.1");
		expect(unmeasured).not.toHaveProperty("model_ms");
		expect(HistoryRecordSchema.safeParse(unmeasured).success).toBe(true);
		expect(
			HistoryRecordSchema.safeParse({
				...unmeasured,
				events: [
					{
						kind: "tool_call",
						name: "shell",
						input: {},
						output: null,
						index: 0,
						tool_call_id: "unmeasured-call",
					},
				],
			}).success,
		).toBe(true);
		expect(
			HistoryRecordSchema.safeParse({
				...unmeasured,
				events: [
					{
						kind: "tool_call",
						name: "shell",
						input: {},
						output: null,
						index: 0,
						tool_call_id: "partial-call",
						started_at: "2026-07-19T00:00:00Z",
						finished_at: null,
						duration_ms: null,
						status: null,
					},
				],
			}).success,
		).toBe(false);
		const baseTool = {
			kind: "tool_call",
			name: "shell",
			input: {},
			output: "ok",
			index: 0,
			tool_call_id: "call-1",
			started_at: "2026-07-19T00:00:00Z",
			finished_at: "2026-07-19T00:00:00Z",
			duration_ms: 1,
		};
		for (const status of ["completed", "failed", "timeout", "interrupted"]) {
			expect(
				HistoryRecordSchema.safeParse({
					...records[0],
					events: [{ ...baseTool, status }],
				}).success,
				status,
			).toBe(true);
		}
		expect(
			HistoryRecordSchema.safeParse({
				...records[0],
				events: [
					{
						...baseTool,
						kind: "tool_result",
						status: null,
						tool_call_id: null,
						started_at: null,
						finished_at: null,
						duration_ms: null,
					},
				],
			}).success,
		).toBe(true);
		expect(
			HistoryStreamEnvelopeSchema.parse({
				type: "record",
				record: records[0],
				future_output_field: true,
			}),
		).toMatchObject({ type: "record", future_output_field: true });
		const incompleteRecord: Partial<HistoryRecord> = { ...records[0] };
		delete incompleteRecord.text;
		expect(HistoryRecordSchema.safeParse(incompleteRecord).success).toBe(false);
		const sessions = await client.historyList({
			historyDir,
			allProjects: true,
		});
		expect(sessions.map(({ name }) => name)).toContain("node-session");
		expect(HistoryListSchema.safeParse(sessions).success).toBe(true);
		expect(HistorySessionSummarySchema.safeParse(sessions[0]).success).toBe(
			true,
		);
		expect(
			HistoryListOptionsSchema.safeParse({ allProjects: true, historyDir })
				.success,
		).toBe(true);
	});

	test("watches history after a cursor without duplicates and filters labels", async () => {
		const historyDir = await mkdtemp(
			resolve(tmpdir(), "oneharness-sdk-watch-"),
		);
		const client = sdk();
		const records: HistoryRecord[] = [];
		const fixtures: Array<[name: string, prompt: string, graph: string]> = [
			["cursor-record", "cursor record", "release"],
			["filtered-record", "filtered record", "other"],
			["resumed-record", "resumed record", "release"],
		];
		for (const [name, prompt, graph] of fixtures) {
			await client.run({
				prompt,
				harnesses: ["codex"],
				mode: "bypass",
				history: true,
				historyName: name,
				historyDir,
				historyLabels: { graph, task: "sdk" },
				env: { MOCK_STDOUT: historyTrace },
				bins: { codex: mock },
			});
			records.push(...(await client.history({ session: name, historyDir })));
		}
		const cursor = records[0]?.history_id;
		if (!cursor) throw new Error("cursor fixture had no history id");

		const watch = client.historyWatch({
			after: cursor,
			allProjects: true,
			historyDir,
			labels: { graph: "release", task: "sdk" },
		});
		expect(
			HistoryWatchOptionsSchema.safeParse({
				allProjects: true,
				historyDir,
				labels: { graph: "release", task: "sdk" },
			}).success,
		).toBe(true);
		const first = await watch.next();
		expect(first).toMatchObject({
			done: false,
			value: {
				type: "record",
				record: {
					prompt: "resumed record",
					labels: { graph: "release", task: "sdk" },
				},
			},
		});
		expect(first.value?.record.history_id).toBe(records[2]?.history_id);
		expect(first.value?.record.history_id).not.toBe(cursor);
		expect(HistoryStreamEnvelopeSchema.safeParse(first.value).success).toBe(
			true,
		);
		await watch.return(undefined);
	});

	test("applies CLI history labels over environment and project labels", async () => {
		const directory = await mkdtemp(
			resolve(tmpdir(), "oneharness-sdk-label-precedence-"),
		);
		const project = resolve(directory, "project");
		const userConfig = resolve(directory, "user.toml");
		await mkdir(project);
		await writeFile(
			resolve(project, "oneharness.toml"),
			'history_labels = { graph = "project", project = "kept" }\n',
		);
		await writeFile(userConfig, "");
		const historyDir = resolve(directory, "history");
		const client = new OneHarness({
			executable: binary,
			env: {
				ONEHARNESS_CONFIG: userConfig,
				ONEHARNESS_HISTORY_LABELS: "graph=environment,env=kept",
				ONEHARNESS_NO_CONFIG: "0",
			},
		});
		await client.run({
			prompt: "label precedence",
			cwd: project,
			harnesses: ["codex"],
			mode: "bypass",
			history: true,
			historyName: "label-precedence",
			historyDir,
			historyLabels: { graph: "cli", cli: "kept" },
			env: { MOCK_STDOUT: historyTrace },
			bins: { codex: mock },
		});
		const records = await client.history({
			session: "label-precedence",
			allProjects: true,
			historyDir,
		});
		expect(records[0]?.labels).toEqual({
			cli: "kept",
			env: "kept",
			graph: "cli",
			project: "kept",
		});
	});

	test("gives last priority over a named session across the CLI boundary", async () => {
		const historyDir = await mkdtemp(resolve(tmpdir(), "oneharness-sdk-last-"));
		const client = sdk();
		const older = await client.run({
			prompt: "the older session",
			harnesses: ["codex"],
			mode: "bypass",
			history: true,
			historyName: "older-session",
			historyDir,
			env: { MOCK_STDOUT: historyTrace },
			bins: { codex: mock },
		});

		// A session's start time is its first record's timestamp, at whole-second
		// precision — so two real runs could tie and make "last" ambiguous. Deriving
		// the newer session from this run's own recorded file keeps every other
		// field real while pinning the one thing this test turns on.
		const olderFile = older.history_file;
		if (!olderFile) throw new Error("run --history recorded no history file");
		const [line] = (await readFile(olderFile, "utf8")).trim().split("\n");
		if (!line) throw new Error(`history file ${olderFile} recorded no run`);
		await writeFile(
			resolve(dirname(olderFile), "newer-session-id.jsonl"),
			`${JSON.stringify({
				...JSON.parse(line),
				session: "newer-session-id",
				name: "newer-session",
				prompt: "the newer session",
				timestamp: "2099-01-01T00:00:00Z",
			})}\n`,
		);

		// `last: true` selects the most recent session even though the lookup also
		// carries an older name: `last` has priority, and the name rides along
		// unselected. Dropping it to `false` is what asks for the name instead.
		// This is the pair the manifest annotates `prefer`: the union defines
		// `{session, last: true}` as "the most recent", so it is one request with
		// one meaning, and refusing it the way a contradictory pair is refused
		// would break the very lookup the suppression was written for.
		expect((await client.history({ last: true, historyDir }))[0]?.name).toBe(
			"newer-session",
		);
		expect(
			(
				await client.history({
					session: "older-session",
					last: true,
					historyDir,
				})
			)[0]?.name,
		).toBe("newer-session");
		expect(
			(
				await client.history({
					session: "older-session",
					last: false,
					historyDir,
				})
			)[0]?.name,
		).toBe("older-session");

		// The name beside `last: true` never selects, so it is unconstrained: an
		// empty one is meaningless rather than invalid, and still gets the last
		// session.
		expect(
			(await client.history({ session: "", last: true, historyDir }))[0]?.name,
		).toBe("newer-session");

		// `last` beside a named session is an ordinary boolean, not a literal, so a
		// caller holding a widened `boolean` type-checks without a cast — this line
		// failing to compile is the regression this guards.
		const wantsLast: boolean = false;
		expect(
			(
				await client.history({
					session: "older-session",
					last: wantsLast,
					historyDir,
				})
			)[0]?.name,
		).toBe("older-session");
	});

	test("continues a native session with the new user message", async () => {
		const argvFile = resolve(
			await mkdtemp(resolve(tmpdir(), "oneharness-sdk-resume-")),
			"argv",
		);
		const client = sdk();
		const first = await client.run({
			prompt: "first user message",
			harnesses: ["claude-code"],
			mode: "bypass",
			env: {
				MOCK_STDOUT: '{"result":"first","session_id":"sdk-session-1"}',
			},
			bins: { "claude-code": mock },
		});
		expect(first.results[0]?.session_id).toBe("sdk-session-1");

		const continued = await client.run({
			prompt: "second user message",
			harnesses: ["claude-code"],
			resume: first.results[0]?.session_id ?? "",
			mode: "bypass",
			env: {
				MOCK_ARGV_FILE: argvFile,
				MOCK_STDOUT: '{"result":"continued"}',
			},
			bins: { "claude-code": mock },
		});
		expect(continued.resume).toBe("sdk-session-1");
		expect(continued.prompt).toBe("second user message");
		const argv = (await readFile(argvFile, "utf8")).split("\n");
		expect(argv).toContain("sdk-session-1");
		expect(argv).toContain("second user message");
	});

	test("surfaces missing history and unsupported continuation selections", async () => {
		const historyDir = await mkdtemp(
			resolve(tmpdir(), "oneharness-sdk-missing-"),
		);
		const client = sdk();
		await expect(
			client.history({ session: "does-not-exist", historyDir }),
		).rejects.toBeInstanceOf(HistoryNotFoundError);
		await expect(
			client
				.historyWatch({
					after: "00000000-0000-7000-8000-000000000000",
					allProjects: true,
					historyDir,
				})
				.next(),
		).rejects.toBeInstanceOf(HistoryNotFoundError);
		await expect(
			client.run({
				prompt: "cannot continue two providers",
				harnesses: ["claude-code", "codex"],
				resume: "sdk-session-1",
			}),
		).rejects.toThrow("--resume needs exactly one harness");
	});

	test("rejects invalid stream inputs and envelopes at their boundaries", async () => {
		const client = new OneHarness({ executable: unspawnable });
		expect(() =>
			client.runStream({ prompt: "typo", harneses: ["codex"] } as never),
		).toThrow("invalid oneharness run options");
		expect(() => client.historyWatch({ allProject: true } as never)).toThrow(
			"invalid oneharness history watch options",
		);

		const invalidRun = new OneHarness({
			executable: process.execPath,
			executableArgs: [invalidCli],
			env: { SDK_FIXTURE_MODE: "run-stream" },
		});
		await expect(
			Array.fromAsync(invalidRun.runStream({ prompt: "malformed" })),
		).rejects.toThrow("invalid oneharness run stream contract");

		const invalidHistory = new OneHarness({
			executable: process.execPath,
			executableArgs: [invalidCli],
			env: { SDK_FIXTURE_MODE: "history-watch" },
		});
		await expect(
			invalidHistory.historyWatch({ allProjects: true }).next(),
		).rejects.toThrow("invalid oneharness history watch contract");
	});

	test("classifies provider failures and tolerates malformed provider output", async () => {
		const client = sdk();
		const failed = await client.run({
			prompt: "provider failure",
			harnesses: ["claude-code"],
			mode: "bypass",
			env: {
				MOCK_EXIT: "1",
				MOCK_STDERR: "rate limit exceeded",
				MOCK_STDOUT: "",
			},
			bins: { "claude-code": mock },
		});
		expect(failed.results[0]?.status).toBe("nonzero");
		expect(failed.results[0]?.failure_kind).toBe("rate_limit");
		expect(failed.results[0]?.failure_kind_source).toBe("stderr");

		const malformed = await client.run({
			prompt: "malformed provider response",
			harnesses: ["claude-code"],
			mode: "bypass",
			env: { MOCK_STDOUT: "{not-json" },
			bins: { "claude-code": mock },
		});
		expect(malformed.results[0]?.status).toBe("ok");
		expect(malformed.results[0]?.text).toBeNull();
		expect(malformed.results[0]?.stdout).toBe("{not-json");
	});

	test("forwards optional reasoning without confusing thinking with actions", async () => {
		const argvFile = resolve(
			await mkdtemp(resolve(tmpdir(), "oneharness-sdk-reasoning-")),
			"argv",
		);
		const report = await sdk().run({
			prompt: "think then act",
			harnesses: ["claude-code"],
			mode: "bypass",
			reasoning: "high",
			events: true,
			env: {
				MOCK_ARGV_FILE: argvFile,
				MOCK_STDOUT: [
					'{"type":"reasoning","text":"private thought"}',
					'{"type":"result","result":"done"}',
				].join("\n"),
			},
			bins: { "claude-code": mock },
		});
		expect(report.results[0]?.events).toBeNull();
		const argv = (await readFile(argvFile, "utf8")).split("\n");
		expect(argv).toContain("--effort");
		expect(argv).toContain("high");
	});

	test("suppresses a bound option only when its suppressor renders an argument", async () => {
		// An `unless` encodes a clap conflict, and clap conflicts on a flag being
		// present — so an option carrying nothing must suppress nothing. Truthiness
		// is a different test in this language: `harnesses: []` is truthy while
		// sending no `--harness`, and reading it as truth dropped the `--all` that
		// was the call's only selection, which the CLI answers by refusing to run.
		const client = sdk();
		const registry = (await client.list()).map((harness) => harness.id).sort();
		const everything = await client.run({
			prompt: "empty suppressor keeps --all",
			all: true,
			harnesses: [],
			mode: "bypass",
			printCommand: true,
		});
		expect(everything.dry_run).toBe(true);
		expect(everything.results.map((result) => result.harness).sort()).toEqual(
			registry,
		);

		// A `value` binding renders whatever it carries, `""` included: `--system ""`
		// reaches the argv, so it suppresses the `--system-file` clap refuses beside
		// it. Truthiness would call that empty string absent and send both.
		const emptySystem = await client.run({
			prompt: "empty system still suppresses --system-file",
			harnesses: ["codex"],
			system: "",
			systemFile: resolve(tmpdir(), "oneharness-sdk-never-read-system.txt"),
			mode: "bypass",
			printCommand: true,
		});
		expect(emptySystem.results[0]?.command).toEqual([
			"codex",
			"exec",
			"--dangerously-bypass-approvals-and-sandbox",
			"--json",
			"empty system still suppresses --system-file",
		]);
	});

	test("refuses a contradiction rather than editing it out of the argv", async () => {
		// `{all: true, harnesses: ["codex"]}` is a caller asking for every harness
		// and for one harness at once. Suppression alone answers it by running
		// `codex` — a paid turn on an identity nobody chose, reported as a success,
		// with the disagreement visible only in the results. The manifest annotates
		// the pair `refuse`, so the call ends before anything is spawned.
		const client = sdk();
		await expect(
			client.run({
				prompt: "never spawned",
				all: true,
				harnesses: ["codex"],
				mode: "bypass",
				printCommand: true,
			}),
		).rejects.toThrow(
			/invalid oneharness run options: `all` and `harnesses` are mutually exclusive/,
		);

		// The same pair the empty-`system` suppression above turns on, read from
		// its other side: two non-empty system sources are two answers to "what
		// instructs this turn", and suppression picks the file silently. Only the
		// stated choice refuses — `system: ""` still suppresses, unchanged.
		await expect(
			client.run({
				prompt: "never spawned",
				harnesses: ["codex"],
				system: "be terse",
				systemFile: resolve(tmpdir(), "oneharness-sdk-never-read-system.txt"),
				mode: "bypass",
				printCommand: true,
			}),
		).rejects.toThrow(
			/invalid oneharness run options: `systemFile` and `system` are mutually exclusive/,
		);

		// Every refuse pair, not just the selectors: two answers to "which config
		// layers", "is this run recorded", "which project's store".
		await expect(
			client.run({
				prompt: "never spawned",
				config: "oneharness.toml",
				noConfig: true,
			}),
		).rejects.toThrow(/`config` and `noConfig` are mutually exclusive/);
		await expect(
			client.run({ prompt: "never spawned", history: true, noHistory: true }),
		).rejects.toThrow(/`history` and `noHistory` are mutually exclusive/);
		await expect(
			client.historyList({ project: "somewhere", allProjects: true }),
		).rejects.toThrow(
			/invalid oneharness history list options: `project` and `allProjects` are mutually exclusive/,
		);

		// Not only the verbs that return a report: `detect` decides which binaries
		// are probed, and the same pair means the same contradiction there.
		await expect(
			client.detect({ all: true, harnesses: ["codex"] }),
		).rejects.toThrow(
			/invalid oneharness detect options: `all` and `harnesses` are mutually exclusive/,
		);

		// `runStream` returns a lazy stream but refuses eagerly: it builds its argv
		// in the method body rather than an `async function*`, so a contradiction
		// throws where the call is written, not on the first `next()`. A caller who
		// only ever `await`s the iteration would otherwise see the refusal arrive
		// somewhere it cannot be caught alongside the call.
		expect(() =>
			client.runStream({
				prompt: "never spawned",
				all: true,
				harnesses: ["codex"],
			}),
		).toThrow(
			/invalid oneharness run options: `all` and `harnesses` are mutually exclusive/,
		);

		// The switch half is a value like any other: `false` renders nothing, so
		// there is no second answer and the call proceeds on the one it has.
		const one = await client.run({
			prompt: "an unset switch contradicts nothing",
			all: false,
			harnesses: ["codex"],
			mode: "bypass",
			printCommand: true,
		});
		expect(one.results.map((result) => result.harness)).toEqual(["codex"]);
	});

	test("rejects an empty prompt before spawning", async () => {
		await expect(new OneHarness().run({ prompt: "" })).rejects.toThrow(
			"invalid oneharness run options",
		);
	});

	test("surfaces an executable spawn failure", async () => {
		const missing = resolve(
			await mkdtemp(resolve(tmpdir(), "oneharness-sdk-no-bin-")),
			"missing-oneharness",
		);
		await expect(
			new OneHarness({ executable: missing }).list(),
		).rejects.toThrow("missing-oneharness");
	});

	test("rejects malformed run, history, list, and detect data from an external CLI", async () => {
		const runClient = new OneHarness({
			executable: process.execPath,
			executableArgs: [invalidCli],
			env: { SDK_FIXTURE_MODE: "run" },
		});
		await expect(runClient.run({ prompt: "malformed" })).rejects.toThrow(
			"invalid oneharness run contract",
		);

		const historyClient = new OneHarness({
			executable: process.execPath,
			executableArgs: [invalidCli],
			env: { SDK_FIXTURE_MODE: "history" },
		});
		await expect(historyClient.history({ last: true })).rejects.toThrow(
			"invalid history contract",
		);

		const historyListClient = new OneHarness({
			executable: process.execPath,
			executableArgs: [invalidCli],
			env: { SDK_FIXTURE_MODE: "history-list" },
		});
		// Both spellings of "no options" clear validation and reach the CLI, so the
		// only contract they can fail is the response one.
		await expect(historyListClient.historyList()).rejects.toThrow(
			"invalid history list contract",
		);
		await expect(historyListClient.historyList({})).rejects.toThrow(
			"invalid history list contract",
		);

		const listClient = new OneHarness({
			executable: process.execPath,
			executableArgs: [invalidCli],
			env: { SDK_FIXTURE_MODE: "list" },
		});
		await expect(listClient.list()).rejects.toThrow(
			"invalid oneharness list contract",
		);

		const detectClient = new OneHarness({
			executable: process.execPath,
			executableArgs: [invalidCli],
			env: { SDK_FIXTURE_MODE: "detect" },
		});
		await expect(detectClient.detect()).rejects.toThrow(
			"invalid oneharness detect contract",
		);
	});
	test("reads the layered configuration and its provenance", async () => {
		const project = await mkdtemp(resolve(tmpdir(), "oneharness-sdk-config-"));
		await writeFile(
			resolve(project, "oneharness.toml"),
			'harnesses = ["codex"]\nmode = "bypass"\n',
		);
		// `noConfig` is deliberately absent: this asserts the project layer is
		// read and attributed, which is the whole of what the verb reports.
		const report = await layered().config({ cwd: project });
		expect(report.harnesses.value).toEqual(["codex"]);
		expect(report.harnesses.source).toContain("oneharness.toml");
		expect(report.mode.value).toBe("bypass");
	}, 30_000);

	test("reports what a sync would change without writing it", async () => {
		const project = await mkdtemp(resolve(tmpdir(), "oneharness-sdk-sync-"));
		await writeFile(
			resolve(project, "oneharness.toml"),
			'allowed_tools = ["Bash(echo:*)"]\n',
		);
		// `--check` exits non-zero precisely because a file *would* change, and
		// the method has to surface that as the report rather than as a throw.
		const planned = await layered().sync({
			cwd: project,
			harnesses: ["claude-code"],
			check: true,
		});
		const claude = planned.results.find(
			({ harness }) => harness === "claude-code",
		);
		// `created`, because the file does not exist yet and `--check` reports
		// the status the write would reach — while writing nothing, which the
		// second sync reaching `created` too is what proves.
		expect(claude?.status).toBe("created");

		const written = await layered().sync({
			cwd: project,
			harnesses: ["claude-code"],
		});
		expect(
			written.results.find(({ harness }) => harness === "claude-code")?.status,
		).toBe("created");
		// Idempotent, so a second sync of the same policy changes nothing.
		const again = await layered().sync({
			cwd: project,
			harnesses: ["claude-code"],
		});
		expect(
			again.results.find(({ harness }) => harness === "claude-code")?.status,
		).toBe("unchanged");
	}, 30_000);

	test("scaffolds a starter config and refuses to clobber it", async () => {
		const project = await mkdtemp(resolve(tmpdir(), "oneharness-sdk-init-"));
		const path = resolve(project, "oneharness.toml");
		expect(await sdk().init({ path })).toBe(path);
		expect(await readFile(path, "utf8")).toContain("run_mode");

		// The failure path: an existing file is a refusal, not a silent
		// overwrite, and `force` is the only way past it.
		await expect(sdk().init({ path })).rejects.toThrow(OneHarnessProcessError);
		expect(await sdk().init({ path, force: true })).toBe(path);
	}, 30_000);

	test("reports subscription headroom for a harness that cannot be probed", async () => {
		// `oneharness-mock-harness` is a real executable that answers no usage
		// protocol, so this exercises the whole probe path and asserts the
		// honest tier rather than a fabricated 0%.
		const report = await sdk().usage({
			harnesses: ["claude-code"],
			bins: { "claude-code": mock },
			timeoutSeconds: 20,
		});
		expect(report.schema_version).toBe("0.1");
		const claude = report.identities.find(
			({ harness }) => harness === "claude-code",
		);
		expect(claude).toBeDefined();
		expect(["known", "unknown", "unavailable"]).toContain(
			String(claude?.availability.state),
		);
	}, 40_000);

	test("answers a pre-tool hook event with the harness's own verdict", async () => {
		const client = sdk();
		const blocked = await client.gate({
			harness: "claude-code",
			denyIfContains: "BLOCKED",
			reason: "policy",
			event: JSON.stringify({
				tool_name: "Bash",
				tool_input: { command: "echo BLOCKED" },
			}),
		});
		expect(blocked).not.toBeNull();
		expect(JSON.parse(blocked ?? "")).toMatchObject({
			hookSpecificOutput: {
				permissionDecision: "deny",
				permissionDecisionReason: "policy",
			},
		});

		// An allowed call is said with silence, so `null` is the answer rather
		// than a missing one.
		const allowed = await client.gate({
			harness: "claude-code",
			denyIfContains: "BLOCKED",
			event: JSON.stringify({
				tool_name: "Bash",
				tool_input: { command: "echo fine" },
			}),
		});
		expect(allowed).toBeNull();
	}, 30_000);

	test("applies a mock ruleset to a hook event and records the original", async () => {
		const dir = await mkdtemp(resolve(tmpdir(), "oneharness-sdk-mock-"));
		const rules = resolve(dir, "rules.json");
		const spyFile = resolve(dir, "spy.jsonl");
		await writeFile(
			rules,
			JSON.stringify({
				rules: [
					{
						match: { tool: "Bash", input: { command: { contains: "secret" } } },
						action: { deny: { message: "no secrets" } },
					},
				],
			}),
		);
		const verdict = await sdk().mock({
			harness: "claude-code",
			rules,
			spyFile,
			event: JSON.stringify({
				tool_name: "Bash",
				tool_input: { command: "cat secret" },
			}),
		});
		expect(JSON.parse(verdict ?? "")).toMatchObject({
			hookSpecificOutput: { permissionDecision: "deny" },
		});
		// The spy log keeps the call as it was observed, which is what a
		// behavioral suite asserts against.
		expect(await readFile(spyFile, "utf8")).toContain("cat secret");
	}, 30_000);

	test("answers an interrupt for a session no run is serving", async () => {
		const sessionDir = await mkdtemp(resolve(tmpdir(), "oneharness-sdk-int-"));
		// A refusal is the answer, not a throw: the CLI exits non-zero and the
		// frame says which of the refusal reasons applies, which is what a
		// supervisor branches on.
		const response = await sdk().interrupt({
			session: "no-such-session",
			sessionDir,
		});
		expect(response.ok).toBe(false);
		if (response.ok === false) expect(response.reason).toBe("not_running");
	}, 30_000);

	test("clears and migrates a history store through typed methods", async () => {
		const historyDir = await mkdtemp(resolve(tmpdir(), "oneharness-sdk-hist-"));
		const client = sdk();
		await client.run({
			prompt: "history clear sdk",
			harnesses: ["codex"],
			mode: "bypass",
			history: true,
			historyDir,
			bins: { codex: mock },
			env: { MOCK_STDOUT: historyTrace },
		});
		expect(await client.historyList({ historyDir })).toHaveLength(1);

		expect(await client.historyMigrate({ historyDir })).toMatchObject({
			files_processed: expect.any(Number),
		});

		// A dry run reports what it *would* remove and deletes nothing, which
		// the following list still seeing the session is what proves.
		const dry = await client.historyClear({ historyDir });
		expect(dry.dry_run).toBe(true);
		if (dry.dry_run === true) expect(dry.would_remove).toBe(1);
		expect(await client.historyList({ historyDir })).toHaveLength(1);

		const cleared = await client.historyClear({ historyDir, yes: true });
		expect(cleared.dry_run).toBe(false);
		if (cleared.dry_run === false) expect(cleared.removed).toBe(1);
		expect(await client.historyList({ historyDir })).toHaveLength(0);
	}, 60_000);

	test("every option contract accepts a fully populated value", () => {
		// One populated value per contract, so each generated validator is walked
		// over every field it declares rather than only the handful a happy-path
		// call happens to set. This is the check that would have caught
		// `--variant`/`--config`/`--no-config` being bound by the manifest and
		// absent from `HistoryListOptions`: a populated object naming them would
		// have been rejected as an unknown field.
		const populated: Array<[string, ZodType<unknown>, unknown]> = [
			[
				"DetectOptions",
				DetectOptionsSchema,
				{
					harnesses: ["codex"],
					all: false,
					exclude: ["goose"],
					bins: { codex: "/bin/codex" },
					config: "/tmp/oneharness.toml",
					noConfig: false,
					requireAvailable: true,
				},
			],
			[
				"ConfigOptions",
				ConfigOptionsSchema,
				{ cwd: "/tmp", config: "/tmp/oneharness.toml", noConfig: false },
			],
			[
				"SyncOptions",
				SyncOptionsSchema,
				{
					cwd: "/tmp",
					harnesses: ["claude-code"],
					check: true,
					global: false,
					config: "/tmp/oneharness.toml",
					noConfig: false,
				},
			],
			[
				"InitOptions",
				InitOptionsSchema,
				{ path: "/tmp/oneharness.toml", force: true },
			],
			[
				"UsageOptions",
				UsageOptionsSchema,
				{
					harnesses: ["codex"],
					all: false,
					exclude: ["goose"],
					bins: { codex: "/bin/codex" },
					cwd: "/tmp",
					timeoutSeconds: 30,
					config: "/tmp/oneharness.toml",
					noConfig: false,
				},
			],
			[
				"GateOptions",
				GateOptionsSchema,
				{
					harness: "claude-code",
					event: "{}",
					denyIfContains: "rm -rf",
					reason: "policy",
				},
			],
			[
				"MockOptions",
				MockOptionsSchema,
				{
					harness: "claude-code",
					event: "{}",
					rules: "/tmp/rules.json",
					spyFile: "/tmp/spy.jsonl",
				},
			],
			[
				"InterruptOptions",
				InterruptOptionsSchema,
				{
					session: "work",
					input: "do this instead",
					sessionDir: "/tmp/sessions",
					cwd: "/tmp",
				},
			],
			[
				"HistoryClearOptions",
				HistoryClearOptionsSchema,
				{
					project: "oneharness",
					allProjects: false,
					yes: true,
					historyDir: "/tmp/history",
					config: "/tmp/oneharness.toml",
					noConfig: false,
				},
			],
			[
				"HistoryMigrateOptions",
				HistoryMigrateOptionsSchema,
				{
					historyDir: "/tmp/history",
					config: "/tmp/oneharness.toml",
					noConfig: false,
				},
			],
			[
				"HistoryListOptions",
				HistoryListOptionsSchema,
				{
					project: "oneharness",
					allProjects: false,
					historyDir: "/tmp/history",
					variant: "claude-code:work",
					config: "/tmp/oneharness.toml",
					noConfig: false,
				},
			],
			[
				"HistoryWatchOptions",
				HistoryWatchOptionsSchema,
				{
					after: "0198f0d0-7b31-7000-8000-000000000001",
					labels: { run: "sdk" },
					project: "oneharness",
					allProjects: false,
					historyDir: "/tmp/history",
					events: true,
					variant: "claude-code:work",
					config: "/tmp/oneharness.toml",
					noConfig: false,
				},
			],
			[
				"HistoryLookup",
				HistoryLookupSchema,
				{
					session: "work",
					last: false,
					all: true,
					project: "oneharness",
					allProjects: false,
					historyDir: "/tmp/history",
					config: "/tmp/oneharness.toml",
					noConfig: false,
				},
			],
		];
		for (const [name, schema, value] of populated) {
			const parsed = schema.safeParse(value);
			expect(
				parsed.success,
				`${name}: ${parsed.success ? "" : JSON.stringify(parsed.error?.issues)}`,
			).toBe(true);
			// Every bound option must reach argv, so each key is one the
			// capability's builder can render.
			expect(Object.keys(value as object).length).toBeGreaterThan(0);
		}
	});

	test("every capability the manifest declares has a method on the client", () => {
		// The client-side half of the coverage gate. `scripts/sdk-coverage.mjs`
		// enforces this across both languages in `just check`; asserting it here
		// too means a missing method fails the package's own suite rather than
		// only a repo-level script.
		const client = sdk() as unknown as Record<string, unknown>;
		for (const method of Object.keys(CAPABILITIES)) {
			expect(typeof client[method], `${method} is missing`).toBe("function");
		}
	});
});
