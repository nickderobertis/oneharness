import { describe, expect, test } from "bun:test";
import { execFileSync } from "node:child_process";
import { mkdtemp, readFile } from "node:fs/promises";
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
	type HistoryList,
	type HistoryListOptions,
	HistoryListOptionsSchema,
	HistoryListSchema,
	type HistoryRecord,
	HistoryRecordSchema,
	type HistoryRecords,
	HistoryRecordsSchema,
	type HistorySessionSummary,
	HistorySessionSummarySchema,
	type ListReport,
	ListReportSchema,
	OneHarness,
	type RunOptions,
	RunOptionsSchema,
	type RunReport,
	RunReportSchema,
	type RunResult,
	RunResultSchema,
	SessionReportSchema,
	type Usage,
	UsageSchema,
} from "../src/index.js";

const here = dirname(fileURLToPath(import.meta.url));
const binary = resolve(here, "../../../target/debug/oneharness");
const mock = resolve(here, "../../../target/debug/oneharness-mock-harness");
const invalidCli = resolve(here, "invalid-cli-fixture.mjs");
// Deliberately absent: a client pointed here can validate but never spawn, so a
// boundary check that runs before the subprocess is the only way to see the
// validation error rather than this path's spawn failure.
const unspawnable = resolve(here, "missing-oneharness-fixture");

type Equal<Left, Right> =
	(<Value>() => Value extends Left ? 1 : 2) extends <
		Value,
	>() => Value extends Right ? 1 : 2
		? true
		: false;

const inferredSchemasMatchGeneratedTypes: [
	Equal<z.infer<typeof RunOptionsSchema>, RunOptions>,
	Equal<z.infer<typeof HistoryListOptionsSchema>, HistoryListOptions>,
	Equal<z.infer<typeof RunReportSchema>, RunReport>,
	Equal<z.infer<typeof RunResultSchema>, RunResult>,
	Equal<z.infer<typeof ActionEventSchema>, ActionEvent>,
	Equal<z.infer<typeof UsageSchema>, Usage>,
	Equal<z.infer<typeof HistoryRecordSchema>, HistoryRecord>,
	Equal<z.infer<typeof HistoryRecordsSchema>, HistoryRecords>,
	Equal<z.infer<typeof HistoryListSchema>, HistoryList>,
	Equal<z.infer<typeof HistorySessionSummarySchema>, HistorySessionSummary>,
	Equal<z.infer<typeof ListReportSchema>, ListReport>,
	Equal<z.infer<typeof HarnessInfoSchema>, HarnessInfo>,
	Equal<z.infer<typeof DetectReportSchema>, DetectReport>,
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
];

function sdk(): OneHarness {
	return new OneHarness({
		executable: binary,
		env: { ONEHARNESS_NO_CONFIG: "1" },
	});
}

describe("OneHarness", () => {
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
		expect(traced.results[0]?.structured).toBeNull();
		expect(
			ActionEventSchema.safeParse(traced.results[0]?.events?.[0]).success,
		).toBe(true);
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

		// Non-object: nothing to read an option off of.
		for (const nonObject of [null, "run", 42, true, [], () => "run"]) {
			await runRejects(nonObject);
			await listRejects(nonObject);
		}

		// Empty: run needs a prompt, while every history list option is optional.
		await runRejects({});
		expect(HistoryListOptionsSchema.safeParse({}).success).toBe(true);

		// Misspelled: a near-miss key this SDK version cannot forward.
		await runRejects({ prompt: "typo", harneses: ["codex"] });
		await listRejects({ allProject: true });

		// Malformed: a known key carrying the wrong type.
		await runRejects({ prompt: 42 });
		await runRejects({ prompt: "wrong shape", harnesses: "codex" });
		await listRejects({ project: 42 });
		await listRejects({ allProjects: "yes" });
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
	});

	test("looks up standardized history created across the CLI boundary", async () => {
		const historyDir = await mkdtemp(resolve(tmpdir(), "oneharness-sdk-"));
		const client = sdk();
		await client.run({
			prompt: "history sdk",
			harnesses: ["claude-code"],
			mode: "bypass",
			history: true,
			historyName: "node-session",
			historyDir,
			bins: { "claude-code": mock },
		});
		const records = await client.history({
			session: "node-session",
			historyDir,
		});
		expect(records[0]?.prompt).toBe("history sdk");
		expect(records[0]?.name).toBe("node-session");
		expect(records[0]?.status).toBe("ok");
		expect(HistoryRecordsSchema.safeParse(records).success).toBe(true);
		expect(HistoryRecordSchema.safeParse(records[0]).success).toBe(true);
		const incompleteRecord: Partial<HistoryRecord> = { ...records[0] };
		delete incompleteRecord.text;
		expect(HistoryRecordSchema.safeParse(incompleteRecord).success).toBe(false);
		const sessions = await client.historyList({
			historyDir,
			allProjects: true,
		});
		expect(sessions[0]?.name).toBe("node-session");
		expect(HistoryListSchema.safeParse(sessions).success).toBe(true);
		expect(HistorySessionSummarySchema.safeParse(sessions[0]).success).toBe(
			true,
		);
		expect(
			HistoryListOptionsSchema.safeParse({ allProjects: true, historyDir })
				.success,
		).toBe(true);
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
		).rejects.toThrow("oneharness exited 1");
		await expect(
			client.run({
				prompt: "cannot continue two providers",
				harnesses: ["claude-code", "codex"],
				resume: "sdk-session-1",
			}),
		).rejects.toThrow("--resume needs exactly one harness");
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

	test("requires an explicit history selector", async () => {
		await expect(sdk().history()).rejects.toThrow(
			"history requires session or last",
		);
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
