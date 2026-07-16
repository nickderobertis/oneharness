import { spawn } from "node:child_process";
import { createRequire } from "node:module";
import type { ZodType } from "zod";
import type { RunReport } from "./generated/contracts.js";
import type { DetectInfo } from "./generated/detection.js";
import type { HistoryRecord } from "./generated/history.js";
import type { HistorySessionSummary } from "./generated/history-list.js";
import type { HistoryListOptions } from "./generated/history-list-options.js";
import type { HistoryLookup } from "./generated/history-lookup.js";
import type { RunOptions } from "./generated/options.js";
import type { HarnessInfo } from "./generated/registry.js";
import {
	DetectReportSchema,
	HistoryListOptionsSchema,
	HistoryListSchema,
	HistoryLookupSchema,
	HistoryRecordsSchema,
	ListReportSchema,
	RunOptionsSchema,
	RunReportSchema,
} from "./generated/zod.js";

export type {
	ActionEvent,
	BatchReport,
	FailureKind,
	FallbackReport,
	FallThrough,
	OutputFormat,
	RunReport,
	RunResult,
	SessionReport,
	Status,
	Usage,
} from "./generated/contracts.js";
export type { DetectInfo, DetectReport } from "./generated/detection.js";
export type { HistoryRecord } from "./generated/history.js";
export type {
	HistoryList,
	HistorySessionSummary,
} from "./generated/history-list.js";
export type { HistoryListOptions } from "./generated/history-list-options.js";
export type {
	HistoryLookup,
	HistoryLookupByLast,
	HistoryLookupBySession,
} from "./generated/history-lookup.js";
export type { HistoryRecords } from "./generated/history-records.js";
export type { HistoryStreamEnvelope } from "./generated/history-stream-envelope.js";
export type { PermissionMode, RunOptions } from "./generated/options.js";
export type { RunStreamEnvelope } from "./generated/run-stream-envelope.js";
export type Detection = DetectInfo;
export type {
	HarnessInfo,
	ListReport,
	ModeInfo,
} from "./generated/registry.js";
export * from "./generated/zod.js";

export type OneHarnessOptions = {
	executable?: string;
	executableArgs?: readonly string[];
	env?: Readonly<Record<string, string>>;
};

function parseContract<T>(
	schema: ZodType<T>,
	value: unknown,
	label: string,
): T {
	const parsed = schema.safeParse(value);
	if (parsed.success) return parsed.data;
	const details = parsed.error.issues
		.map((issue) => `${issue.path.join(".") || "<root>"}: ${issue.message}`)
		.join("; ");
	throw new Error(`${label}: ${details}`);
}

function executable(options: OneHarnessOptions): {
	command: string;
	prefix: string[];
} {
	if (options.executable)
		return {
			command: options.executable,
			prefix: [...(options.executableArgs ?? [])],
		};
	if (process.env.ONEHARNESS_BIN)
		return { command: process.env.ONEHARNESS_BIN, prefix: [] };
	const require = createRequire(import.meta.url);
	return {
		command: process.execPath,
		prefix: [require.resolve("oneharness-cli/bin/oneharness.js")],
	};
}

async function invokeWith(
	options: OneHarnessOptions,
	args: readonly string[],
	cwd?: string,
	acceptJsonOnNonzero = false,
): Promise<unknown> {
	const bin = executable(options);
	return await new Promise((resolve, reject) => {
		const child = spawn(bin.command, [...bin.prefix, ...args], {
			cwd,
			env: { ...process.env, ...options.env },
			windowsHide: true,
		});
		let stdout = "";
		let stderr = "";
		child.stdout
			.setEncoding("utf8")
			.on("data", (chunk: string) => (stdout += chunk));
		child.stderr
			.setEncoding("utf8")
			.on("data", (chunk: string) => (stderr += chunk));
		child.on("error", reject);
		child.on("close", (code) => {
			if (code !== 0 && !acceptJsonOnNonzero)
				return reject(new Error(`oneharness exited ${code}: ${stderr.trim()}`));
			try {
				resolve(JSON.parse(stdout));
			} catch (error) {
				if (code !== 0)
					return reject(
						new Error(`oneharness exited ${code}: ${stderr.trim()}`),
					);
				reject(new Error(`oneharness returned invalid JSON: ${String(error)}`));
			}
		});
	});
}

function pushMany(
	args: string[],
	flag: string,
	values?: readonly string[],
): void {
	for (const value of values ?? []) args.push(flag, value);
}

export class OneHarness {
	constructor(private readonly options: OneHarnessOptions = {}) {}

	async run(options: RunOptions): Promise<RunReport> {
		const input = parseContract(
			RunOptionsSchema,
			options,
			"invalid oneharness run options",
		);
		const args = ["run", "--prompt", input.prompt, "--compact"];
		pushMany(args, "--harness", input.harnesses);
		pushMany(args, "--model", input.models);
		if (input.system !== undefined) args.push("--system", input.system);
		if (input.reasoning !== undefined)
			args.push("--reasoning", input.reasoning);
		if (input.resume !== undefined) args.push("--resume", input.resume);
		if (input.session !== undefined) args.push("--session", input.session);
		if (input.fork) args.push("--fork");
		if (input.mode) args.push("--mode", input.mode);
		if (input.timeoutSeconds !== undefined)
			args.push("--timeout", String(input.timeoutSeconds));
		if (input.events) args.push("--events");
		if (input.history) args.push("--history");
		if (input.historyName !== undefined)
			args.push("--history-name", input.historyName);
		if (input.historyDir !== undefined)
			args.push("--history-dir", input.historyDir);
		for (const [key, value] of Object.entries(input.env ?? {}))
			args.push("--env", `${key}=${value}`);
		for (const [key, value] of Object.entries(input.bins ?? {}))
			args.push("--bin", `${key}=${value}`);
		const value = await invokeWith(this.options, args, input.cwd, true);
		return parseContract(
			RunReportSchema,
			value,
			"invalid oneharness run contract",
		);
	}

	async list(): Promise<HarnessInfo[]> {
		const value = await invokeWith(this.options, ["list", "--compact"]);
		return parseContract(
			ListReportSchema,
			value,
			"invalid oneharness list contract",
		).harnesses;
	}

	async detect(harnesses: readonly string[] = []): Promise<Detection[]> {
		const args = ["detect", "--compact"];
		pushMany(args, "--harness", harnesses);
		const value = await invokeWith(this.options, args);
		return parseContract(
			DetectReportSchema,
			value,
			"invalid oneharness detect contract",
		).detected;
	}

	async history(lookup: HistoryLookup): Promise<HistoryRecord[]> {
		const input = parseContract(
			HistoryLookupSchema,
			lookup,
			"invalid oneharness history options",
		);
		const args = ["history", "show", "--compact"];
		// A lookup that selects no session is not a HistoryLookup, so only these
		// two cases remain. The variants overlap on `{session, last: true}`, and
		// `last: true` keeps its long-standing priority over a name — which is why
		// the union tries the last-session variant first, in Rust and in the
		// generated Zod alike. Ruling that case out here leaves the variant whose
		// session the type guarantees is present.
		if (input.last === true) args.push("--last");
		else args.push(input.session);
		if (input.project) args.push("--project", input.project);
		if (input.allProjects) args.push("--all-projects");
		if (input.historyDir) args.push("--history-dir", input.historyDir);
		const value = await invokeWith(this.options, args);
		return parseContract(
			HistoryRecordsSchema,
			value,
			"invalid history contract",
		);
	}

	async historyList(
		options: HistoryListOptions = {},
	): Promise<HistorySessionSummary[]> {
		const input = parseContract(
			HistoryListOptionsSchema,
			options,
			"invalid oneharness history list options",
		);
		const args = ["history", "list", "--compact"];
		if (input.project) args.push("--project", input.project);
		if (input.allProjects) args.push("--all-projects");
		if (input.historyDir) args.push("--history-dir", input.historyDir);
		const value = await invokeWith(this.options, args);
		return parseContract(
			HistoryListSchema,
			value,
			"invalid history list contract",
		);
	}
}
