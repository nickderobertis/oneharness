import { spawn } from "node:child_process";
import { createRequire } from "node:module";
import { createInterface } from "node:readline";
import type { ZodType } from "zod";
import type { RunReport } from "./generated/contracts.js";
import type { DetectInfo } from "./generated/detection.js";
import type { HistoryRecord } from "./generated/history.js";
import type { HistorySessionSummary } from "./generated/history-list.js";
import type { HistoryListOptions } from "./generated/history-list-options.js";
import type { HistoryLookup } from "./generated/history-lookup.js";
import type { HistoryStreamEnvelope } from "./generated/history-stream-envelope.js";
import type { HistoryWatchOptions } from "./generated/history-watch-options.js";
import type { RunOptions } from "./generated/options.js";
import type { HarnessInfo } from "./generated/registry.js";
import type { RunStreamEnvelope } from "./generated/run-stream-envelope.js";
import {
	DetectReportSchema,
	HistoryListOptionsSchema,
	HistoryListSchema,
	HistoryLookupSchema,
	HistoryRecordsSchema,
	HistoryStreamEnvelopeSchema,
	HistoryWatchOptionsSchema,
	ListReportSchema,
	RunOptionsSchema,
	RunReportSchema,
	RunStreamEnvelopeSchema,
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
export type { HistoryWatchOptions } from "./generated/history-watch-options.js";
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

/** Script for the shipped deterministic provider substitute. */
export type MockHarnessScript = {
	stdout?: string;
	stderr?: string;
	exitCode?: number;
	latencyMs?: number;
};

/** A non-zero exit from the oneharness subprocess. */
export class OneHarnessProcessError extends Error {
	constructor(
		message: string,
		readonly exitCode: number | null,
		readonly stderr: string,
	) {
		super(message);
		this.name = "OneHarnessProcessError";
	}
}

/** A history lookup or watch cursor that the CLI could not resolve. */
export class HistoryNotFoundError extends OneHarnessProcessError {
	constructor(message: string, exitCode: number | null, stderr: string) {
		super(message, exitCode, stderr);
		this.name = "HistoryNotFoundError";
	}
}

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
				return reject(processError(code, stderr));
			try {
				resolve(JSON.parse(stdout));
			} catch (error) {
				if (code !== 0) return reject(processError(code, stderr));
				reject(new Error(`oneharness returned invalid JSON: ${String(error)}`));
			}
		});
	});
}

function processError(
	code: number | null,
	stderr: string,
): OneHarnessProcessError {
	return new OneHarnessProcessError(
		`oneharness exited ${code}: ${stderr.trim()}`,
		code,
		stderr,
	);
}

function historyNotFound(error: unknown): error is OneHarnessProcessError {
	return (
		error instanceof OneHarnessProcessError &&
		(error.exitCode === 1 || error.stderr.includes("was not found"))
	);
}

function asHistoryNotFound(
	error: OneHarnessProcessError,
): HistoryNotFoundError {
	return new HistoryNotFoundError(error.message, error.exitCode, error.stderr);
}

async function* invokeStreamWith<T>(
	options: OneHarnessOptions,
	args: readonly string[],
	schema: ZodType<T>,
	label: string,
	cwd?: string,
	historyStream = false,
): AsyncGenerator<T> {
	const bin = executable(options);
	const child = spawn(bin.command, [...bin.prefix, ...args], {
		cwd,
		env: { ...process.env, ...options.env },
		windowsHide: true,
	});
	let stderr = "";
	let spawnError: Error | undefined;
	let closed = false;
	child.stderr
		.setEncoding("utf8")
		.on("data", (chunk: string) => (stderr += chunk));
	const completion = new Promise<number | null>((resolve) => {
		child.once("error", (error) => {
			spawnError = error;
		});
		child.once("close", (code) => {
			closed = true;
			resolve(code);
		});
	});
	const lines = createInterface({
		input: child.stdout,
		crlfDelay: Number.POSITIVE_INFINITY,
	});
	try {
		for await (const line of lines) {
			if (line.trim() === "") continue;
			let value: unknown;
			try {
				value = JSON.parse(line);
			} catch (error) {
				throw new Error(`${label}: invalid JSON: ${String(error)}`);
			}
			yield parseContract(schema, value, label);
		}
		const code = await completion;
		if (spawnError) throw spawnError;
		if (code !== 0) {
			const error = processError(code, stderr);
			if (historyStream && historyNotFound(error))
				throw asHistoryNotFound(error);
			throw error;
		}
	} finally {
		lines.close();
		child.stdout.destroy();
		if (!closed) {
			child.kill();
			await completion;
		}
	}
}

function pushMany(
	args: string[],
	flag: string,
	values?: readonly string[],
): void {
	for (const value of values ?? []) args.push(flag, value);
}

function runArguments(input: RunOptions, stream: boolean): string[] {
	const args = ["run", "--prompt", input.prompt, "--compact"];
	pushMany(args, "--harness", input.harnesses);
	pushMany(args, "--model", input.models);
	if (input.system !== undefined) args.push("--system", input.system);
	if (input.reasoning !== undefined) args.push("--reasoning", input.reasoning);
	if (input.resume !== undefined) args.push("--resume", input.resume);
	if (input.session !== undefined) args.push("--session", input.session);
	if (input.fork) args.push("--fork");
	if (input.mode) args.push("--mode", input.mode);
	if (input.timeoutSeconds !== undefined)
		args.push("--timeout", String(input.timeoutSeconds));
	if (input.events) args.push("--events");
	if (stream) args.push("--stream");
	if (input.history) args.push("--history");
	if (input.historyName !== undefined)
		args.push("--history-name", input.historyName);
	if (input.historyDir !== undefined)
		args.push("--history-dir", input.historyDir);
	for (const [key, value] of Object.entries(input.historyLabels ?? {}))
		args.push("--history-label", `${key}=${value}`);
	for (const [key, value] of Object.entries(input.env ?? {}))
		args.push("--env", `${key}=${value}`);
	for (const [key, value] of Object.entries(input.bins ?? {}))
		args.push("--bin", `${key}=${value}`);
	return args;
}

export class OneHarness {
	constructor(private readonly options: OneHarnessOptions = {}) {}

	async run(options: RunOptions): Promise<RunReport> {
		const input = parseContract(
			RunOptionsSchema,
			options,
			"invalid oneharness run options",
		);
		const args = runArguments(input, false);
		const value = await invokeWith(this.options, args, input.cwd, true);
		return parseContract(
			RunReportSchema,
			value,
			"invalid oneharness run contract",
		);
	}

	async runMock(
		harness: string,
		options: RunOptions,
		script: MockHarnessScript = {},
	): Promise<RunReport> {
		if (harness.length === 0) throw new Error("mock harness must not be empty");
		const input = parseContract(
			RunOptionsSchema,
			options,
			"invalid oneharness run options",
		);
		const env: Record<string, string> = { ...(input.env ?? {}) };
		if (script.stdout !== undefined) env.MOCK_STDOUT = script.stdout;
		if (script.stderr !== undefined) env.MOCK_STDERR = script.stderr;
		if (script.exitCode !== undefined) env.MOCK_EXIT = String(script.exitCode);
		if (script.latencyMs !== undefined)
			env.MOCK_SLEEP_MS = String(script.latencyMs);
		const args = runArguments({ ...input, harnesses: [harness], env }, false);
		args.push("--mock-harness", harness);
		const value = await invokeWith(this.options, args, input.cwd, true);
		return parseContract(
			RunReportSchema,
			value,
			"invalid oneharness run contract",
		);
	}

	runStream(options: RunOptions): AsyncGenerator<RunStreamEnvelope> {
		const input = parseContract(
			RunOptionsSchema,
			options,
			"invalid oneharness run options",
		);
		return invokeStreamWith(
			this.options,
			runArguments(input, true),
			RunStreamEnvelopeSchema,
			"invalid oneharness run stream contract",
			input.cwd,
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
		let value: unknown;
		try {
			value = await invokeWith(this.options, args);
		} catch (error) {
			if (historyNotFound(error)) throw asHistoryNotFound(error);
			throw error;
		}
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

	historyWatch(
		options: HistoryWatchOptions = {},
	): AsyncGenerator<HistoryStreamEnvelope> {
		const input = parseContract(
			HistoryWatchOptionsSchema,
			options,
			"invalid oneharness history watch options",
		);
		const args = ["history", "watch", "--format", "jsonl"];
		if (input.after) args.push("--after", input.after);
		for (const [key, value] of Object.entries(input.labels ?? {}))
			args.push("--label", `${key}=${value}`);
		if (input.project) args.push("--project", input.project);
		if (input.allProjects) args.push("--all-projects");
		if (input.historyDir) args.push("--history-dir", input.historyDir);
		return invokeStreamWith(
			this.options,
			args,
			HistoryStreamEnvelopeSchema,
			"invalid oneharness history watch contract",
			undefined,
			true,
		);
	}
}
