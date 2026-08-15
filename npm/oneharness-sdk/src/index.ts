import { spawn } from "node:child_process";
import { createRequire } from "node:module";
import { createInterface } from "node:readline";
import type { ZodType } from "zod";
import {
	CAPABILITIES,
	type Capability,
	type CapabilityMethod,
	type OptionBinding,
} from "./generated/capabilities.js";
import type { ConfigOptions } from "./generated/config-options.js";
import type { ConfigReport } from "./generated/config-report.js";
import type { RunReport } from "./generated/contracts.js";
import type { DetectOptions } from "./generated/detect-options.js";
import type { DetectInfo } from "./generated/detection.js";
import type { GateOptions } from "./generated/gate-options.js";
import type { HistoryRecord } from "./generated/history.js";
import type { HistoryClearOptions } from "./generated/history-clear-options.js";
import type { HistoryClearReport } from "./generated/history-clear-report.js";
import type { HistorySessionSummary } from "./generated/history-list.js";
import type { HistoryListOptions } from "./generated/history-list-options.js";
import type { HistoryLookup } from "./generated/history-lookup.js";
import type { HistoryMigrateOptions } from "./generated/history-migrate-options.js";
import type { HistoryMigrateReport } from "./generated/history-migrate-report.js";
import type { HistoryStreamEnvelope } from "./generated/history-stream-envelope.js";
import type { HistoryWatchOptions } from "./generated/history-watch-options.js";
import type { InitOptions } from "./generated/init-options.js";
import type { InterruptOptions } from "./generated/interrupt-options.js";
import type { InterruptResponse } from "./generated/interrupt-response.js";
import type { MockOptions } from "./generated/mock-options.js";
import type { RunOptions } from "./generated/options.js";
import type { HarnessInfo } from "./generated/registry.js";
import type { RunStreamEnvelope } from "./generated/run-stream-envelope.js";
import type { SyncOptions } from "./generated/sync-options.js";
import type { SyncReport } from "./generated/sync-report.js";
import type { UsageOptions } from "./generated/usage-options.js";
import type { UsageReport } from "./generated/usage-report.js";
import {
	ConfigOptionsSchema,
	ConfigReportSchema,
	DetectOptionsSchema,
	DetectReportSchema,
	GateOptionsSchema,
	HistoryClearOptionsSchema,
	HistoryClearReportSchema,
	HistoryListOptionsSchema,
	HistoryListSchema,
	HistoryLookupSchema,
	HistoryMigrateOptionsSchema,
	HistoryMigrateReportSchema,
	HistoryRecordsSchema,
	HistoryStreamEnvelopeSchema,
	HistoryWatchOptionsSchema,
	InitOptionsSchema,
	InterruptOptionsSchema,
	InterruptResponseSchema,
	ListReportSchema,
	MockOptionsSchema,
	RunOptionsSchema,
	RunReportSchema,
	RunStreamEnvelopeSchema,
	SyncOptionsSchema,
	SyncReportSchema,
	UsageOptionsSchema,
	UsageReportSchema,
} from "./generated/zod.js";

export type {
	Capability,
	CapabilityMethod,
	FlagKind,
	OptionBinding,
	StdoutShape,
	UnlessResolution,
} from "./generated/capabilities.js";
export { CAPABILITIES } from "./generated/capabilities.js";
export type { ConfigOptions } from "./generated/config-options.js";
export type { ConfigReport } from "./generated/config-report.js";
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
export type { DetectOptions } from "./generated/detect-options.js";
export type { DetectInfo, DetectReport } from "./generated/detection.js";
export type { GateOptions } from "./generated/gate-options.js";
export type { HistoryRecord } from "./generated/history.js";
export type { HistoryClearOptions } from "./generated/history-clear-options.js";
export type {
	HistoryClearDryRun,
	HistoryClearRemoved,
	HistoryClearReport,
} from "./generated/history-clear-report.js";
export type { HistoryLine } from "./generated/history-line.js";
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
export type { HistoryMigrateOptions } from "./generated/history-migrate-options.js";
export type { HistoryMigrateReport } from "./generated/history-migrate-report.js";
export type { HistoryRecords } from "./generated/history-records.js";
export type { HistoryStreamEnvelope } from "./generated/history-stream-envelope.js";
export type { HistoryWatchOptions } from "./generated/history-watch-options.js";
export type { InitOptions } from "./generated/init-options.js";
export type { InterruptOptions } from "./generated/interrupt-options.js";
export type { InterruptResponse } from "./generated/interrupt-response.js";
export type { MockOptions } from "./generated/mock-options.js";
export type { PermissionMode, RunOptions } from "./generated/options.js";
export type { RunStreamEnvelope } from "./generated/run-stream-envelope.js";
export type { SyncOptions } from "./generated/sync-options.js";
export type { SyncReport } from "./generated/sync-report.js";
export type { UsageOptions } from "./generated/usage-options.js";
export type { UsageReport } from "./generated/usage-report.js";
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

/**
 * Invoke a capability whose stdout is text rather than a JSON contract, and
 * return it verbatim.
 *
 * `init` prints a confirmation line; `gate` and `mock` answer with a harness's
 * own native verdict, or with nothing when the call is allowed. None of the
 * three has a document to validate, so parsing one would be inventing a shape
 * the CLI does not promise.
 *
 * `stdin` carries the hook event for the two gate verbs, and closing it is what
 * tells them the event is complete.
 */
async function invokeText(
	options: OneHarnessOptions,
	args: readonly string[],
	stdin?: string,
): Promise<string> {
	const bin = executable(options);
	return await new Promise((resolve, reject) => {
		const child = spawn(bin.command, [...bin.prefix, ...args], {
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
			if (code !== 0) return reject(processError(code, stderr));
			resolve(stdout);
		});
		child.stdin.on("error", reject);
		child.stdin.end(stdin ?? "");
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

/**
 * Does this binding put anything on the argv for `value`?
 *
 * This is the question `unless` asks, and asking it any other way is a bug: the
 * conflict a suppression encodes is clap's, and clap conflicts on a flag being
 * *present*. JavaScript truthiness is not that test — `{all: true, harnesses:
 * []}`, the shape a caller assembling options programmatically produces, sends
 * no `--harness` at all, yet an empty array is truthy, so reading it as truth
 * dropped `--all` and left the call with no selection to make.
 */
function rendersArgument(binding: OptionBinding, value: unknown): boolean {
	if (value === undefined || value === null) return false;
	switch (binding.kind) {
		case "repeated":
		case "trailing":
			return (value as readonly unknown[]).length > 0;
		case "key-value":
			return Object.keys(value as object).length > 0;
		case "switch":
			return Boolean(value);
		default:
			// A `positional` or `value` binding renders whatever it carries, an
			// empty string included — which is a present flag to clap.
			return true;
	}
}

/**
 * Does this value assert something, or merely occupy the key?
 *
 * Suppression asks whether a binding renders; refusing asks something narrower,
 * because a contradiction takes two positive answers. An empty value is not one.
 * `{system: ""}` does render `--system ""` — which is why it still suppresses the
 * `--system-file` clap refuses beside it — but an empty system prompt asks for
 * nothing, so a file beside it is a defaulted key next to a real choice rather
 * than a caller asking for two different things.
 */
function statesAChoice(binding: OptionBinding, value: unknown): boolean {
	if (!rendersArgument(binding, value)) return false;
	if (binding.kind === "value" || binding.kind === "positional")
		return String(value) !== "";
	return true;
}

/**
 * Render one capability's argv from its declared bindings.
 *
 * The client lists no flags of its own: `CAPABILITIES` says which option
 * renders which flag and how, so a flag added to the CLI reaches every method
 * as soon as the manifest binds it, and a method cannot quietly omit one. The
 * Rust-side gates hold that manifest to clap and to the option contracts, which
 * is what makes this table trustworthy enough to build calls from.
 */
function capabilityArguments(
	method: CapabilityMethod,
	options: object,
): string[] {
	// The options contracts are closed object types with no index signature, so
	// reading a declared binding's key needs this one widening. It is safe
	// because `every_bound_option_is_a_field_of_its_options_contract` proves in
	// Rust that each key named here is a field of the very contract Zod just
	// validated the value against.
	const input = options as Readonly<Record<string, unknown>>;
	const capability: Capability = CAPABILITIES[method];
	const args: string[] = [...capability.argv, ...capability.always];
	const positional: string[] = [];
	const trailing: string[] = [];
	const bound = new Map(
		capability.bindings.map((binding) => [binding.option, binding]),
	);
	for (const binding of capability.bindings) {
		// `a_suppressing_option_is_one_the_same_capability_binds` proves in Rust
		// that an `unless` always resolves here, so a missing entry is unreachable
		// rather than a case to render through.
		const suppressor =
			binding.unless === null ? null : bound.get(binding.unless);
		if (suppressor && rendersArgument(suppressor, input[suppressor.option])) {
			// What both halves asserting means is the manifest's question, not this
			// client's: `prefer` is a request with one meaning (`{session, last:
			// true}` is "the most recent"), `refuse` is a caller who asked for two
			// different things. Editing the second one out picks an answer silently,
			// which is how a turn gets billed to a harness nobody selected. An
			// absent annotation reads as `refuse` — the recoverable half.
			if (
				(binding.unless_resolution ?? "refuse") === "refuse" &&
				statesAChoice(binding, input[binding.option]) &&
				statesAChoice(suppressor, input[suppressor.option])
			)
				throw new Error(
					`invalid oneharness ${capability.argv.join(" ")} options: \`${binding.option}\` and \`${suppressor.option}\` are mutually exclusive, and this call sets both to values the CLI would send. Pass only the one you mean — resolving it here would run the call as something you did not ask for.`,
				);
			continue;
		}
		const value = input[binding.option];
		if (value === undefined || value === null) continue;
		switch (binding.kind) {
			case "positional":
				positional.push(String(value));
				break;
			case "value":
				args.push(binding.flag, String(value));
				break;
			case "repeated":
				pushMany(args, binding.flag, (value as unknown[]).map(String));
				break;
			case "switch":
				if (value) args.push(binding.flag);
				break;
			case "key-value":
				for (const [key, item] of Object.entries(
					value as Record<string, unknown>,
				))
					args.push(binding.flag, `${key}=${String(item)}`);
				break;
			case "trailing":
				trailing.push(...(value as unknown[]).map(String));
				break;
		}
	}
	args.push(...positional);
	// After a `--` separator, so a passthrough argument that looks like a flag
	// reaches the harness instead of being read by oneharness.
	if (trailing.length > 0) args.push("--", ...trailing);
	return args;
}

export class OneHarness {
	constructor(private readonly options: OneHarnessOptions = {}) {}

	/**
	 * Validate options, render the capability's argv, invoke, validate stdout.
	 *
	 * Every JSON-returning method is this call with two schemas, so none of them
	 * can drift from the manifest in how it builds a command or in what it
	 * promises back.
	 */
	private async call<Options, Report>(
		method: CapabilityMethod,
		options: Options,
		optionsSchema: ZodType<Options>,
		reportSchema: ZodType<Report>,
		{
			acceptJsonOnNonzero = false,
			history = false,
			// The two labels a caller sees on a rejection. They default to the
			// verb, and are given explicitly where a method's wording predates
			// this shared path — those strings are what a consumer's error
			// handling already reads, so they are contract rather than prose.
			optionsLabel = `invalid oneharness ${CAPABILITIES[method].argv.join(" ")} options`,
			contractLabel = `invalid oneharness ${CAPABILITIES[method].argv.join(" ")} contract`,
		}: {
			acceptJsonOnNonzero?: boolean;
			history?: boolean;
			optionsLabel?: string;
			contractLabel?: string;
		} = {},
	): Promise<Report> {
		const input = parseContract(optionsSchema, options, optionsLabel);
		const args = capabilityArguments(method, input as object);
		const cwd = (input as { cwd?: string }).cwd;
		let value: unknown;
		try {
			value = await invokeWith(this.options, args, cwd, acceptJsonOnNonzero);
		} catch (error) {
			if (history && historyNotFound(error)) throw asHistoryNotFound(error);
			throw error;
		}
		return parseContract(reportSchema, value, contractLabel);
	}

	async run(options: RunOptions): Promise<RunReport> {
		// `acceptJsonOnNonzero`, because a harness that fails is data in the
		// report rather than a failure of the call: the CLI still prints a whole
		// contract and exits non-zero to say some result was not `ok`.
		return await this.call("run", options, RunOptionsSchema, RunReportSchema, {
			acceptJsonOnNonzero: true,
			optionsLabel: "invalid oneharness run options",
			contractLabel: "invalid oneharness run contract",
		});
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
		return await this.call(
			"run",
			{
				...input,
				harnesses: [harness],
				mockHarnesses: [harness],
				env,
			},
			RunOptionsSchema,
			RunReportSchema,
			{
				acceptJsonOnNonzero: true,
				optionsLabel: "invalid oneharness run options",
				contractLabel: "invalid oneharness run contract",
			},
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
			capabilityArguments("runStream", input),
			RunStreamEnvelopeSchema,
			"invalid oneharness run stream contract",
			input.cwd,
		);
	}

	async list(): Promise<HarnessInfo[]> {
		const value = await invokeWith(
			this.options,
			capabilityArguments("list", {}),
		);
		return parseContract(
			ListReportSchema,
			value,
			"invalid oneharness list contract",
		).harnesses;
	}

	/**
	 * Probe harness binaries.
	 *
	 * The array overload is the long-standing shape and still means "these
	 * harnesses"; the options object is what reaches the rest of the verb's
	 * flags (`--all`, `--exclude`, `--bin`, the config layer).
	 */
	async detect(
		harnessesOrOptions: readonly string[] | DetectOptions = [],
	): Promise<Detection[]> {
		const options: DetectOptions = Array.isArray(harnessesOrOptions)
			? { harnesses: [...harnessesOrOptions] }
			: (harnessesOrOptions as DetectOptions);
		const report = await this.call(
			"detect",
			options,
			DetectOptionsSchema,
			DetectReportSchema,
			{
				optionsLabel: "invalid oneharness detect options",
				contractLabel: "invalid oneharness detect contract",
			},
		);
		return report.detected;
	}

	async config(options: ConfigOptions = {}): Promise<ConfigReport> {
		return await this.call(
			"config",
			options,
			ConfigOptionsSchema,
			ConfigReportSchema,
		);
	}

	async sync(options: SyncOptions = {}): Promise<SyncReport> {
		// `--check` exits non-zero when a file would change, which is the answer
		// rather than a failure, and the report says which files those are.
		return await this.call(
			"sync",
			options,
			SyncOptionsSchema,
			SyncReportSchema,
			{ acceptJsonOnNonzero: true },
		);
	}

	async usage(options: UsageOptions = {}): Promise<UsageReport> {
		return await this.call(
			"usage",
			options,
			UsageOptionsSchema,
			UsageReportSchema,
		);
	}

	/**
	 * Write a starter `oneharness.toml` and return where it landed.
	 *
	 * The one verb whose stdout is a human confirmation line rather than a JSON
	 * contract — the deliverable is the file — so the return is the resolved
	 * path the caller asked for, not a parsed document.
	 */
	async init(options: InitOptions = {}): Promise<string> {
		const input = parseContract(
			InitOptionsSchema,
			options,
			"invalid oneharness init options",
		);
		await invokeText(this.options, capabilityArguments("init", input));
		return input.path ?? "oneharness.toml";
	}

	/**
	 * Run the pre-tool gate over one harness hook event, as that harness's own
	 * runtime would.
	 *
	 * Returns the harness's native deny verdict, or `null` when the call is
	 * allowed through — which the CLI says by printing nothing at all, so an
	 * empty answer is the allow rather than a missing one.
	 */
	async gate(options: GateOptions): Promise<string | null> {
		const input = parseContract(
			GateOptionsSchema,
			options,
			"invalid oneharness gate options",
		);
		const verdict = await invokeText(
			this.options,
			capabilityArguments("gate", input),
			input.event,
		);
		return verdict.trim() === "" ? null : verdict;
	}

	/** The read-write sibling of {@link gate}, driven by a rules file. */
	async mock(options: MockOptions): Promise<string | null> {
		const input = parseContract(
			MockOptionsSchema,
			options,
			"invalid oneharness mock options",
		);
		const verdict = await invokeText(
			this.options,
			capabilityArguments("mock", input),
			input.event,
		);
		return verdict.trim() === "" ? null : verdict;
	}

	/**
	 * Abort the in-flight turn of a `run --control` session, optionally
	 * redirecting it with `input`.
	 *
	 * A refusal is an answer, not a throw: the response says whether the abort
	 * was served and, when it was not, why — which is what a supervisor branches
	 * on. So a non-zero exit still yields the frame.
	 */
	async interrupt(options: InterruptOptions): Promise<InterruptResponse> {
		return await this.call(
			"interrupt",
			options,
			InterruptOptionsSchema,
			InterruptResponseSchema,
			{ acceptJsonOnNonzero: true },
		);
	}

	async history(lookup: HistoryLookup): Promise<HistoryRecord[]> {
		// The `--last`-suppresses-a-name rule is declared, not re-derived here:
		// the union deliberately accepts `{session, last: true}` and resolves it
		// to "the most recent", and the manifest binds `session` with
		// `unless: "last"` so the builder renders only `--last`. Rust, the
		// generated Zod, and this call therefore agree by construction.
		return await this.call(
			"history",
			lookup,
			HistoryLookupSchema,
			HistoryRecordsSchema,
			{
				history: true,
				optionsLabel: "invalid oneharness history options",
				contractLabel: "invalid history contract",
			},
		);
	}

	async historyList(
		options: HistoryListOptions = {},
	): Promise<HistorySessionSummary[]> {
		return await this.call(
			"historyList",
			options,
			HistoryListOptionsSchema,
			HistoryListSchema,
			{
				optionsLabel: "invalid oneharness history list options",
				contractLabel: "invalid history list contract",
			},
		);
	}

	/**
	 * Remove history sessions.
	 *
	 * A dry run until `yes` is set, and the two answers are different documents:
	 * `dry_run` discriminates them, so a caller reads `removed` only from a run
	 * that removed something.
	 */
	async historyClear(
		options: HistoryClearOptions = {},
	): Promise<HistoryClearReport> {
		return await this.call(
			"historyClear",
			options,
			HistoryClearOptionsSchema,
			HistoryClearReportSchema,
		);
	}

	/** Rewrite legacy session files to the current record version. */
	async historyMigrate(
		options: HistoryMigrateOptions = {},
	): Promise<HistoryMigrateReport> {
		return await this.call(
			"historyMigrate",
			options,
			HistoryMigrateOptionsSchema,
			HistoryMigrateReportSchema,
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
		return invokeStreamWith(
			this.options,
			capabilityArguments("historyWatch", input),
			HistoryStreamEnvelopeSchema,
			"invalid oneharness history watch contract",
			undefined,
			true,
		);
	}
}
