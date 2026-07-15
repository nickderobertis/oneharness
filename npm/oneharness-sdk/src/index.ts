import { spawn } from "node:child_process";
import { createRequire } from "node:module";
import Ajv2020Module from "ajv/dist/2020.js";
import type { RunReport } from "./generated/contracts.js";
import type { DetectInfo, DetectReport } from "./generated/detection.js";
import type { HistoryRecord } from "./generated/history.js";
import type { HarnessInfo, ListReport } from "./generated/registry.js";
import schemas from "./generated/schemas.json" with { type: "json" };

export type {
	ActionEvent,
	FailureKind,
	RunReport,
	RunResult,
	Usage,
} from "./generated/contracts.js";
export type { DetectInfo, DetectReport } from "./generated/detection.js";
export type { HistoryRecord } from "./generated/history.js";
export type Detection = DetectInfo;
export type {
	HarnessInfo,
	ListReport,
	ModeInfo,
} from "./generated/registry.js";

export type PermissionMode =
	| "read-only"
	| "plan"
	| "default"
	| "edit"
	| "auto"
	| "bypass";
export type RunOptions = {
	prompt: string;
	harnesses?: readonly string[];
	models?: readonly string[];
	system?: string;
	reasoning?: string;
	resume?: string;
	session?: string;
	fork?: boolean;
	mode?: PermissionMode;
	cwd?: string;
	timeoutSeconds?: number;
	events?: boolean;
	history?: boolean;
	historyName?: string;
	historyDir?: string;
	env?: Readonly<Record<string, string>>;
	bins?: Readonly<Record<string, string>>;
};

export type HistoryLookup = {
	session?: string;
	last?: boolean;
	project?: string;
	allProjects?: boolean;
	historyDir?: string;
};
export type OneHarnessOptions = {
	executable?: string;
	executableArgs?: readonly string[];
	env?: Readonly<Record<string, string>>;
};

const Ajv2020 = Ajv2020Module.default;
const ajv = new Ajv2020({ strict: true });
for (const format of ["int32", "uint", "uint32", "uint64", "uint128"]) {
	ajv.addFormat(format, { type: "number", validate: Number.isSafeInteger });
}
ajv.addFormat("double", { type: "number", validate: Number.isFinite });
const validateRun = ajv.compile(schemas.run_report);
const validateHistory = ajv.compile(schemas.history_record);
const validateList = ajv.compile(schemas.list_report);
const validateDetect = ajv.compile(schemas.detect_report);

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
		if (!options.prompt) throw new TypeError("prompt must not be empty");
		const args = ["run", "--prompt", options.prompt, "--compact"];
		pushMany(args, "--harness", options.harnesses);
		pushMany(args, "--model", options.models);
		if (options.system !== undefined) args.push("--system", options.system);
		if (options.reasoning !== undefined)
			args.push("--reasoning", options.reasoning);
		if (options.resume !== undefined) args.push("--resume", options.resume);
		if (options.session !== undefined) args.push("--session", options.session);
		if (options.fork) args.push("--fork");
		if (options.mode) args.push("--mode", options.mode);
		if (options.timeoutSeconds !== undefined)
			args.push("--timeout", String(options.timeoutSeconds));
		if (options.events) args.push("--events");
		if (options.history) args.push("--history");
		if (options.historyName !== undefined)
			args.push("--history-name", options.historyName);
		if (options.historyDir !== undefined)
			args.push("--history-dir", options.historyDir);
		for (const [key, value] of Object.entries(options.env ?? {}))
			args.push("--env", `${key}=${value}`);
		for (const [key, value] of Object.entries(options.bins ?? {}))
			args.push("--bin", `${key}=${value}`);
		const value = await invokeWith(this.options, args, options.cwd, true);
		if (!validateRun(value))
			throw new Error(
				`invalid oneharness run contract: ${ajv.errorsText(validateRun.errors)}`,
			);
		return value as unknown as RunReport;
	}

	async list(): Promise<HarnessInfo[]> {
		const value = await invokeWith(this.options, ["list", "--compact"]);
		if (!validateList(value))
			throw new Error(
				`invalid oneharness list contract: ${ajv.errorsText(validateList.errors)}`,
			);
		return (value as unknown as ListReport).harnesses;
	}

	async detect(harnesses: readonly string[] = []): Promise<Detection[]> {
		const args = ["detect", "--compact"];
		pushMany(args, "--harness", harnesses);
		const value = await invokeWith(this.options, args);
		if (!validateDetect(value))
			throw new Error(
				`invalid oneharness detect contract: ${ajv.errorsText(validateDetect.errors)}`,
			);
		return (value as unknown as DetectReport).detected;
	}

	async history(lookup: HistoryLookup = {}): Promise<HistoryRecord[]> {
		const args = ["history", "show", "--compact"];
		if (lookup.last) args.push("--last");
		else if (lookup.session) args.push(lookup.session);
		else throw new TypeError("history requires session or last");
		if (lookup.project) args.push("--project", lookup.project);
		if (lookup.allProjects) args.push("--all-projects");
		if (lookup.historyDir) args.push("--history-dir", lookup.historyDir);
		const value = await invokeWith(this.options, args);
		if (
			!Array.isArray(value) ||
			value.some((record) => !validateHistory(record))
		)
			throw new Error(
				`invalid history contract: ${ajv.errorsText(validateHistory.errors)}`,
			);
		return value as HistoryRecord[];
	}
}
