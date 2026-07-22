import { describe, expect, test } from "bun:test";
import { execFileSync } from "node:child_process";
import { mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import type { z } from "zod";
import {
	type ActionEvent,
	ActionEventSchema,
	BatchReportSchema,
	type DetectReport,
	DetectReportSchema,
	FallbackReportSchema,
	type HarnessInfo,
	HarnessInfoSchema,
	type HistoryLine,
	HistoryLineSchema,
	type HistoryList,
	type HistoryListOptions,
	HistoryListOptionsSchema,
	HistoryListSchema,
	type HistoryLookup,
	HistoryLookupSchema,
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
	type ListReport,
	ListReportSchema,
	OneHarness,
	type RunOptions,
	RunOptionsSchema,
	type RunReport,
	RunReportSchema,
	type RunResult,
	RunResultSchema,
	type RunStreamEnvelope,
	RunStreamEnvelopeSchema,
	SessionReportSchema,
	type Usage,
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

function sdk(): OneHarness {
	return new OneHarness({
		executable: binary,
		env: { ONEHARNESS_NO_CONFIG: "1" },
	});
}

describe("OneHarness", () => {
	test("generated validators match the shared SDK acceptance matrix", () => {
		const schemas = {
			run_options: RunOptionsSchema,
			history_lookup: HistoryLookupSchema,
			history_list_options: HistoryListOptionsSchema,
			history_watch_options: HistoryWatchOptionsSchema,
			history_line: HistoryLineSchema,
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
			harnesses: ["codex"] as const,
			models: ["provider/model"] as const,
		} satisfies RunOptions;
		expect(RunOptionsSchema.parse(readonlyOptions).harnesses).toEqual([
			"codex",
		]);
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
		expect(unmeasured?.schema_version).toBe("1.0");
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
		for (const [name, prompt, graph] of [
			["cursor-record", "cursor record", "release"],
			["filtered-record", "filtered record", "other"],
			["resumed-record", "resumed record", "release"],
		] as const) {
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
});
