/* Generated from oneharness-core. Do not edit. */
export type FlagKind =
	"positional" | "value" | "repeated" | "switch" | "key-value" | "trailing";

/**
 * What both members of a suppressed pair rendering an argument means.
 *
 * Absent — a manifest older than the annotation — is `refuse`, the safe half:
 * a contradiction reported is recoverable, a contradiction edited out is a call
 * that ran as something the caller never asked for.
 */
export type UnlessResolution = "refuse" | "prefer";

/** One SDK option and the CLI flag it renders to. */
export type OptionBinding = {
	readonly option: string;
	/** Empty for a positional or trailing binding, which have no flag. */
	readonly flag: string;
	readonly kind: FlagKind;
	/** Another option that suppresses this one when it renders an argument. */
	readonly unless: string | null;
	/** Only present beside an `unless`, which it says how to resolve. */
	readonly unless_resolution?: UnlessResolution;
};

/** How a verb's stdout reaches a caller. */
export type StdoutShape = "json" | "jsonl" | "text";

/** One thing the CLI can do, and how this client reaches it. */
export type Capability = {
	readonly method: string;
	readonly argv: readonly string[];
	readonly options: string | null;
	readonly output: string | null;
	readonly stdout: StdoutShape;
	readonly stdin: boolean;
	readonly rust: string;
	readonly always: readonly string[];
	readonly bindings: readonly OptionBinding[];
	readonly uncovered: readonly {
		readonly flag: string;
		readonly reason: string;
	}[];
};

export const CAPABILITIES = {
	run: {
		method: "run",
		argv: ["run"],
		options: "run_options",
		output: "run_report",
		stdout: "json",
		stdin: false,
		rust: "oneharness_core::io::run::run",
		always: ["--compact", "--no-stream"],
		bindings: [
			{
				option: "prompt",
				flag: "--prompt",
				kind: "value",
				unless: null,
			},
			{
				option: "batchPrompts",
				flag: "--prompt",
				kind: "repeated",
				unless: null,
			},
			{
				option: "promptFiles",
				flag: "--prompt-file",
				kind: "repeated",
				unless: null,
			},
			{
				option: "harnesses",
				flag: "--harness",
				kind: "repeated",
				unless: null,
			},
			{
				option: "mockHarnesses",
				flag: "--mock-harness",
				kind: "repeated",
				unless: null,
			},
			{
				option: "all",
				flag: "--all",
				kind: "switch",
				unless: "harnesses",
				unless_resolution: "refuse",
			},
			{
				option: "exclude",
				flag: "--exclude",
				kind: "repeated",
				unless: null,
			},
			{
				option: "models",
				flag: "--model",
				kind: "repeated",
				unless: null,
			},
			{
				option: "system",
				flag: "--system",
				kind: "value",
				unless: null,
			},
			{
				option: "systemFile",
				flag: "--system-file",
				kind: "value",
				unless: "system",
				unless_resolution: "refuse",
			},
			{
				option: "reasoning",
				flag: "--reasoning",
				kind: "value",
				unless: null,
			},
			{
				option: "resume",
				flag: "--resume",
				kind: "value",
				unless: null,
			},
			{
				option: "fork",
				flag: "--fork",
				kind: "switch",
				unless: null,
			},
			{
				option: "session",
				flag: "--session",
				kind: "value",
				unless: null,
			},
			{
				option: "sessionDir",
				flag: "--session-dir",
				kind: "value",
				unless: null,
			},
			{
				option: "control",
				flag: "--control",
				kind: "switch",
				unless: null,
			},
			{
				option: "outputFormat",
				flag: "--output-format",
				kind: "value",
				unless: null,
			},
			{
				option: "events",
				flag: "--events",
				kind: "switch",
				unless: null,
			},
			{
				option: "mockRules",
				flag: "--mock-rules",
				kind: "value",
				unless: null,
			},
			{
				option: "spyFile",
				flag: "--spy-file",
				kind: "value",
				unless: null,
			},
			{
				option: "schema",
				flag: "--schema",
				kind: "value",
				unless: null,
			},
			{
				option: "schemaMaxRetries",
				flag: "--schema-max-retries",
				kind: "value",
				unless: null,
			},
			{
				option: "outputDir",
				flag: "--output-dir",
				kind: "value",
				unless: null,
			},
			{
				option: "timeoutSeconds",
				flag: "--timeout",
				kind: "value",
				unless: null,
			},
			{
				option: "cwd",
				flag: "--cwd",
				kind: "value",
				unless: null,
			},
			{
				option: "env",
				flag: "--env",
				kind: "key-value",
				unless: null,
			},
			{
				option: "mode",
				flag: "--mode",
				kind: "value",
				unless: null,
			},
			{
				option: "permitPrompts",
				flag: "--permit-prompts",
				kind: "switch",
				unless: null,
			},
			{
				option: "config",
				flag: "--config",
				kind: "value",
				unless: "noConfig",
				unless_resolution: "refuse",
			},
			{
				option: "noConfig",
				flag: "--no-config",
				kind: "switch",
				unless: null,
			},
			{
				option: "maxParallel",
				flag: "--max-parallel",
				kind: "value",
				unless: null,
			},
			{
				option: "batchStrategy",
				flag: "--batch-strategy",
				kind: "value",
				unless: null,
			},
			{
				option: "runMode",
				flag: "--run-mode",
				kind: "value",
				unless: null,
			},
			{
				option: "printCommand",
				flag: "--print-command",
				kind: "switch",
				unless: null,
			},
			{
				option: "bins",
				flag: "--bin",
				kind: "key-value",
				unless: null,
			},
			{
				option: "requireAvailable",
				flag: "--require-available",
				kind: "switch",
				unless: null,
			},
			{
				option: "history",
				flag: "--history",
				kind: "switch",
				unless: "noHistory",
				unless_resolution: "refuse",
			},
			{
				option: "noHistory",
				flag: "--no-history",
				kind: "switch",
				unless: null,
			},
			{
				option: "historyDir",
				flag: "--history-dir",
				kind: "value",
				unless: null,
			},
			{
				option: "historyName",
				flag: "--history-name",
				kind: "value",
				unless: null,
			},
			{
				option: "historyLabels",
				flag: "--history-label",
				kind: "key-value",
				unless: null,
			},
			{
				option: "passthrough",
				flag: "",
				kind: "trailing",
				unless: null,
			},
		],
		uncovered: [
			{
				flag: "--bypass",
				reason: '`mode: "bypass"` is the same request',
			},
			{
				flag: "--no-bypass",
				reason: '`mode: "default"` is the same request',
			},
			{
				flag: "--stream",
				reason:
					"`runStream()` is the streaming method; this one returns one report",
			},
		],
	},
	runStream: {
		method: "runStream",
		argv: ["run"],
		options: "run_options",
		output: "run_stream_envelope",
		stdout: "jsonl",
		stdin: false,
		rust: "oneharness_core::io::run::run",
		always: ["--compact", "--stream"],
		bindings: [
			{
				option: "prompt",
				flag: "--prompt",
				kind: "value",
				unless: null,
			},
			{
				option: "batchPrompts",
				flag: "--prompt",
				kind: "repeated",
				unless: null,
			},
			{
				option: "promptFiles",
				flag: "--prompt-file",
				kind: "repeated",
				unless: null,
			},
			{
				option: "harnesses",
				flag: "--harness",
				kind: "repeated",
				unless: null,
			},
			{
				option: "mockHarnesses",
				flag: "--mock-harness",
				kind: "repeated",
				unless: null,
			},
			{
				option: "all",
				flag: "--all",
				kind: "switch",
				unless: "harnesses",
				unless_resolution: "refuse",
			},
			{
				option: "exclude",
				flag: "--exclude",
				kind: "repeated",
				unless: null,
			},
			{
				option: "models",
				flag: "--model",
				kind: "repeated",
				unless: null,
			},
			{
				option: "system",
				flag: "--system",
				kind: "value",
				unless: null,
			},
			{
				option: "systemFile",
				flag: "--system-file",
				kind: "value",
				unless: "system",
				unless_resolution: "refuse",
			},
			{
				option: "reasoning",
				flag: "--reasoning",
				kind: "value",
				unless: null,
			},
			{
				option: "resume",
				flag: "--resume",
				kind: "value",
				unless: null,
			},
			{
				option: "fork",
				flag: "--fork",
				kind: "switch",
				unless: null,
			},
			{
				option: "session",
				flag: "--session",
				kind: "value",
				unless: null,
			},
			{
				option: "sessionDir",
				flag: "--session-dir",
				kind: "value",
				unless: null,
			},
			{
				option: "control",
				flag: "--control",
				kind: "switch",
				unless: null,
			},
			{
				option: "outputFormat",
				flag: "--output-format",
				kind: "value",
				unless: null,
			},
			{
				option: "events",
				flag: "--events",
				kind: "switch",
				unless: null,
			},
			{
				option: "mockRules",
				flag: "--mock-rules",
				kind: "value",
				unless: null,
			},
			{
				option: "spyFile",
				flag: "--spy-file",
				kind: "value",
				unless: null,
			},
			{
				option: "schema",
				flag: "--schema",
				kind: "value",
				unless: null,
			},
			{
				option: "schemaMaxRetries",
				flag: "--schema-max-retries",
				kind: "value",
				unless: null,
			},
			{
				option: "outputDir",
				flag: "--output-dir",
				kind: "value",
				unless: null,
			},
			{
				option: "timeoutSeconds",
				flag: "--timeout",
				kind: "value",
				unless: null,
			},
			{
				option: "cwd",
				flag: "--cwd",
				kind: "value",
				unless: null,
			},
			{
				option: "env",
				flag: "--env",
				kind: "key-value",
				unless: null,
			},
			{
				option: "mode",
				flag: "--mode",
				kind: "value",
				unless: null,
			},
			{
				option: "permitPrompts",
				flag: "--permit-prompts",
				kind: "switch",
				unless: null,
			},
			{
				option: "config",
				flag: "--config",
				kind: "value",
				unless: "noConfig",
				unless_resolution: "refuse",
			},
			{
				option: "noConfig",
				flag: "--no-config",
				kind: "switch",
				unless: null,
			},
			{
				option: "maxParallel",
				flag: "--max-parallel",
				kind: "value",
				unless: null,
			},
			{
				option: "batchStrategy",
				flag: "--batch-strategy",
				kind: "value",
				unless: null,
			},
			{
				option: "runMode",
				flag: "--run-mode",
				kind: "value",
				unless: null,
			},
			{
				option: "printCommand",
				flag: "--print-command",
				kind: "switch",
				unless: null,
			},
			{
				option: "bins",
				flag: "--bin",
				kind: "key-value",
				unless: null,
			},
			{
				option: "requireAvailable",
				flag: "--require-available",
				kind: "switch",
				unless: null,
			},
			{
				option: "history",
				flag: "--history",
				kind: "switch",
				unless: "noHistory",
				unless_resolution: "refuse",
			},
			{
				option: "noHistory",
				flag: "--no-history",
				kind: "switch",
				unless: null,
			},
			{
				option: "historyDir",
				flag: "--history-dir",
				kind: "value",
				unless: null,
			},
			{
				option: "historyName",
				flag: "--history-name",
				kind: "value",
				unless: null,
			},
			{
				option: "historyLabels",
				flag: "--history-label",
				kind: "key-value",
				unless: null,
			},
			{
				option: "passthrough",
				flag: "",
				kind: "trailing",
				unless: null,
			},
		],
		uncovered: [
			{
				flag: "--bypass",
				reason: '`mode: "bypass"` is the same request',
			},
			{
				flag: "--no-bypass",
				reason: '`mode: "default"` is the same request',
			},
			{
				flag: "--no-stream",
				reason:
					"this method streams by definition, so the negative half cannot apply",
			},
		],
	},
	list: {
		method: "list",
		argv: ["list"],
		options: null,
		output: "list_report",
		stdout: "json",
		stdin: false,
		rust: "oneharness_core::io::registry::list",
		always: ["--compact"],
		bindings: [],
		uncovered: [],
	},
	detect: {
		method: "detect",
		argv: ["detect"],
		options: "detect_options",
		output: "detect_report",
		stdout: "json",
		stdin: false,
		rust: "oneharness_core::io::detect::detect",
		always: ["--compact"],
		bindings: [
			{
				option: "harnesses",
				flag: "--harness",
				kind: "repeated",
				unless: null,
			},
			{
				option: "all",
				flag: "--all",
				kind: "switch",
				unless: "harnesses",
				unless_resolution: "refuse",
			},
			{
				option: "exclude",
				flag: "--exclude",
				kind: "repeated",
				unless: null,
			},
			{
				option: "bins",
				flag: "--bin",
				kind: "key-value",
				unless: null,
			},
			{
				option: "config",
				flag: "--config",
				kind: "value",
				unless: "noConfig",
				unless_resolution: "refuse",
			},
			{
				option: "noConfig",
				flag: "--no-config",
				kind: "switch",
				unless: null,
			},
			{
				option: "requireAvailable",
				flag: "--require-available",
				kind: "switch",
				unless: null,
			},
		],
		uncovered: [],
	},
	config: {
		method: "config",
		argv: ["config"],
		options: "config_options",
		output: "config_report",
		stdout: "json",
		stdin: false,
		rust: "oneharness_core::domain::config::explain",
		always: ["--compact"],
		bindings: [
			{
				option: "cwd",
				flag: "--cwd",
				kind: "value",
				unless: null,
			},
			{
				option: "config",
				flag: "--config",
				kind: "value",
				unless: "noConfig",
				unless_resolution: "refuse",
			},
			{
				option: "noConfig",
				flag: "--no-config",
				kind: "switch",
				unless: null,
			},
		],
		uncovered: [],
	},
	sync: {
		method: "sync",
		argv: ["sync"],
		options: "sync_options",
		output: "sync_report",
		stdout: "json",
		stdin: false,
		rust: "oneharness_core::io::sync::sync",
		always: ["--compact"],
		bindings: [
			{
				option: "cwd",
				flag: "--cwd",
				kind: "value",
				unless: null,
			},
			{
				option: "harnesses",
				flag: "--harness",
				kind: "repeated",
				unless: null,
			},
			{
				option: "check",
				flag: "--check",
				kind: "switch",
				unless: null,
			},
			{
				option: "global",
				flag: "--global",
				kind: "switch",
				unless: null,
			},
			{
				option: "config",
				flag: "--config",
				kind: "value",
				unless: "noConfig",
				unless_resolution: "refuse",
			},
			{
				option: "noConfig",
				flag: "--no-config",
				kind: "switch",
				unless: null,
			},
		],
		uncovered: [],
	},
	init: {
		method: "init",
		argv: ["init"],
		options: "init_options",
		output: null,
		stdout: "text",
		stdin: false,
		rust: "oneharness_core::io::init::init",
		always: [],
		bindings: [
			{
				option: "path",
				flag: "",
				kind: "positional",
				unless: null,
			},
			{
				option: "force",
				flag: "--force",
				kind: "switch",
				unless: null,
			},
		],
		uncovered: [],
	},
	usage: {
		method: "usage",
		argv: ["usage"],
		options: "usage_options",
		output: "usage_report",
		stdout: "json",
		stdin: false,
		rust: "oneharness_core::io::usage::report",
		always: ["--compact"],
		bindings: [
			{
				option: "harnesses",
				flag: "--harness",
				kind: "repeated",
				unless: null,
			},
			{
				option: "all",
				flag: "--all",
				kind: "switch",
				unless: "harnesses",
				unless_resolution: "refuse",
			},
			{
				option: "exclude",
				flag: "--exclude",
				kind: "repeated",
				unless: null,
			},
			{
				option: "bins",
				flag: "--bin",
				kind: "key-value",
				unless: null,
			},
			{
				option: "cwd",
				flag: "--cwd",
				kind: "value",
				unless: null,
			},
			{
				option: "timeoutSeconds",
				flag: "--timeout",
				kind: "value",
				unless: null,
			},
			{
				option: "config",
				flag: "--config",
				kind: "value",
				unless: "noConfig",
				unless_resolution: "refuse",
			},
			{
				option: "noConfig",
				flag: "--no-config",
				kind: "switch",
				unless: null,
			},
		],
		uncovered: [
			{
				flag: "--format",
				reason:
					"the SDKs consume the JSON contract; `--format text` is the human-readable view of the same data, carrying nothing the JSON does not",
			},
		],
	},
	gate: {
		method: "gate",
		argv: ["gate"],
		options: "gate_options",
		output: null,
		stdout: "text",
		stdin: true,
		rust: "oneharness_core::domain::gate::render_deny",
		always: [],
		bindings: [
			{
				option: "harness",
				flag: "",
				kind: "positional",
				unless: null,
			},
			{
				option: "denyIfContains",
				flag: "--deny-if-contains",
				kind: "value",
				unless: null,
			},
			{
				option: "reason",
				flag: "--reason",
				kind: "value",
				unless: null,
			},
		],
		uncovered: [],
	},
	mock: {
		method: "mock",
		argv: ["mock"],
		options: "mock_options",
		output: null,
		stdout: "text",
		stdin: true,
		rust: "oneharness_core::domain::mock::decide",
		always: [],
		bindings: [
			{
				option: "harness",
				flag: "",
				kind: "positional",
				unless: null,
			},
			{
				option: "rules",
				flag: "--rules",
				kind: "value",
				unless: null,
			},
			{
				option: "spyFile",
				flag: "--spy-file",
				kind: "value",
				unless: null,
			},
		],
		uncovered: [],
	},
	interrupt: {
		method: "interrupt",
		argv: ["interrupt"],
		options: "interrupt_options",
		output: "interrupt_response",
		stdout: "json",
		stdin: false,
		rust: "oneharness_core::io::control::send",
		always: ["--compact"],
		bindings: [
			{
				option: "session",
				flag: "--session",
				kind: "value",
				unless: null,
			},
			{
				option: "input",
				flag: "--input",
				kind: "value",
				unless: null,
			},
			{
				option: "sessionDir",
				flag: "--session-dir",
				kind: "value",
				unless: null,
			},
			{
				option: "cwd",
				flag: "--cwd",
				kind: "value",
				unless: null,
			},
		],
		uncovered: [],
	},
	history: {
		method: "history",
		argv: ["history", "show"],
		options: "history_lookup",
		output: "history_records",
		stdout: "json",
		stdin: false,
		rust: "oneharness_core::io::history::read_session",
		always: ["--compact"],
		bindings: [
			{
				option: "session",
				flag: "",
				kind: "positional",
				unless: "last",
				unless_resolution: "prefer",
			},
			{
				option: "last",
				flag: "--last",
				kind: "switch",
				unless: null,
			},
			{
				option: "all",
				flag: "--all",
				kind: "switch",
				unless: null,
			},
			{
				option: "project",
				flag: "--project",
				kind: "value",
				unless: "allProjects",
				unless_resolution: "refuse",
			},
			{
				option: "allProjects",
				flag: "--all-projects",
				kind: "switch",
				unless: null,
			},
			{
				option: "historyDir",
				flag: "--history-dir",
				kind: "value",
				unless: null,
			},
			{
				option: "config",
				flag: "--config",
				kind: "value",
				unless: "noConfig",
				unless_resolution: "refuse",
			},
			{
				option: "noConfig",
				flag: "--no-config",
				kind: "switch",
				unless: null,
			},
		],
		uncovered: [
			{
				flag: "--format",
				reason:
					"the SDKs consume the JSON contract; `--format text` is the human-readable view of the same data, carrying nothing the JSON does not",
			},
		],
	},
	historyList: {
		method: "historyList",
		argv: ["history", "list"],
		options: "history_list_options",
		output: "history_list",
		stdout: "json",
		stdin: false,
		rust: "oneharness_core::io::history::list_sessions",
		always: ["--compact"],
		bindings: [
			{
				option: "variant",
				flag: "--variant",
				kind: "value",
				unless: null,
			},
			{
				option: "project",
				flag: "--project",
				kind: "value",
				unless: "allProjects",
				unless_resolution: "refuse",
			},
			{
				option: "allProjects",
				flag: "--all-projects",
				kind: "switch",
				unless: null,
			},
			{
				option: "historyDir",
				flag: "--history-dir",
				kind: "value",
				unless: null,
			},
			{
				option: "config",
				flag: "--config",
				kind: "value",
				unless: "noConfig",
				unless_resolution: "refuse",
			},
			{
				option: "noConfig",
				flag: "--no-config",
				kind: "switch",
				unless: null,
			},
		],
		uncovered: [
			{
				flag: "--format",
				reason:
					"the SDKs consume the JSON contract; `--format text` is the human-readable view of the same data, carrying nothing the JSON does not",
			},
		],
	},
	historyWatch: {
		method: "historyWatch",
		argv: ["history", "watch"],
		options: "history_watch_options",
		output: "history_stream_envelope",
		stdout: "jsonl",
		stdin: false,
		rust: "oneharness_core::io::history::HistoryWatcher",
		always: ["--format", "jsonl"],
		bindings: [
			{
				option: "after",
				flag: "--after",
				kind: "value",
				unless: null,
			},
			{
				option: "labels",
				flag: "--label",
				kind: "key-value",
				unless: null,
			},
			{
				option: "variant",
				flag: "--variant",
				kind: "value",
				unless: null,
			},
			{
				option: "project",
				flag: "--project",
				kind: "value",
				unless: "allProjects",
				unless_resolution: "refuse",
			},
			{
				option: "allProjects",
				flag: "--all-projects",
				kind: "switch",
				unless: null,
			},
			{
				option: "historyDir",
				flag: "--history-dir",
				kind: "value",
				unless: null,
			},
			{
				option: "events",
				flag: "--events",
				kind: "switch",
				unless: null,
			},
			{
				option: "config",
				flag: "--config",
				kind: "value",
				unless: "noConfig",
				unless_resolution: "refuse",
			},
			{
				option: "noConfig",
				flag: "--no-config",
				kind: "switch",
				unless: null,
			},
		],
		uncovered: [],
	},
	historyClear: {
		method: "historyClear",
		argv: ["history", "clear"],
		options: "history_clear_options",
		output: "history_clear_report",
		stdout: "json",
		stdin: false,
		rust: "oneharness_core::io::history::remove_sessions",
		always: ["--compact"],
		bindings: [
			{
				option: "project",
				flag: "--project",
				kind: "value",
				unless: "allProjects",
				unless_resolution: "refuse",
			},
			{
				option: "allProjects",
				flag: "--all-projects",
				kind: "switch",
				unless: null,
			},
			{
				option: "yes",
				flag: "--yes",
				kind: "switch",
				unless: null,
			},
			{
				option: "historyDir",
				flag: "--history-dir",
				kind: "value",
				unless: null,
			},
			{
				option: "config",
				flag: "--config",
				kind: "value",
				unless: "noConfig",
				unless_resolution: "refuse",
			},
			{
				option: "noConfig",
				flag: "--no-config",
				kind: "switch",
				unless: null,
			},
		],
		uncovered: [],
	},
	historyMigrate: {
		method: "historyMigrate",
		argv: ["history", "migrate"],
		options: "history_migrate_options",
		output: "history_migrate_report",
		stdout: "json",
		stdin: false,
		rust: "oneharness_core::io::history::migrate",
		always: ["--compact"],
		bindings: [
			{
				option: "historyDir",
				flag: "--history-dir",
				kind: "value",
				unless: null,
			},
			{
				option: "config",
				flag: "--config",
				kind: "value",
				unless: "noConfig",
				unless_resolution: "refuse",
			},
			{
				option: "noConfig",
				flag: "--no-config",
				kind: "switch",
				unless: null,
			},
		],
		uncovered: [],
	},
} as const satisfies Record<string, Capability>;

export type CapabilityMethod = keyof typeof CAPABILITIES;
