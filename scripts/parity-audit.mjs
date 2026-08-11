#!/usr/bin/env node
// Render the generated half of `docs/sdk-parity.md`.
//
// Two sources, deliberately different in kind:
//
//   * the DECLARED target — `domain::capability::CAPABILITIES`, read through the
//     schema bundle, which says what each capability is and which flag belongs
//     to which SDK option;
//   * the MEASURED surface — the method names each language client actually
//     defines, read out of its own source.
//
// The audit is the two side by side, so a row saying "covered" is a statement
// about code that exists rather than about an intention. Regenerate with
// `just parity-audit`; `scripts/check-parity-audit.sh` fails when the checked-in
// document no longer matches, so the file a reader opens is always current.
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const doc = resolve(root, "docs/sdk-parity.md");
const BEGIN = "<!-- BEGIN GENERATED: capability-tables -->";
const END = "<!-- END GENERATED: capability-tables -->";
// Shared with `just sdk-generate`, so an audit run reuses that build rather than
// racing the workspace target directory.
const generatorTarget = resolve(root, "target/sdk-schema-generator");

/** The declared manifest, straight from Rust. */
function capabilities() {
	const json = execFileSync(
		"cargo",
		[
			"run",
			"-q",
			"-p",
			"oneharness-core",
			"--features",
			"sdk-schema",
			"--example",
			"generate_core_sdk_schema",
		],
		{
			cwd: root,
			encoding: "utf8",
			env: { ...process.env, CARGO_TARGET_DIR: generatorTarget },
		},
	);
	return JSON.parse(json).capabilities;
}

/**
 * The method names the TypeScript client defines.
 *
 * Read from the source rather than imported, so the audit does not depend on a
 * build having happened — and so a method that exists only as a type is not
 * mistaken for one a caller can invoke.
 */
function typescriptMethods() {
	const source = readFileSync(
		resolve(root, "npm/oneharness-sdk/src/index.ts"),
		"utf8",
	);
	const body = source.slice(source.indexOf("export class OneHarness"));
	return new Set(
		[...body.matchAll(/^\t(?:async )?\*?([A-Za-z][A-Za-z0-9]*)\s*[(<]/gmu)].map(
			(match) => match[1],
		),
	);
}

/** The method names the Python client defines. */
function pythonMethods() {
	const source = readFileSync(
		resolve(root, "python/oneharness-sdk/src/oneharness_sdk/_client.py"),
		"utf8",
	);
	const body = source.slice(source.indexOf("class OneHarness"));
	return new Set(
		[...body.matchAll(/^ {4}(?:async )?def ([a-z][a-z0-9_]*)\s*\(/gmu)]
			.map((match) => match[1])
			.filter((name) => !name.startsWith("_")),
	);
}

const pythonName = (method) =>
	method.replace(/[A-Z]/gu, (upper) => `_${upper.toLowerCase()}`);

const mark = (covered) => (covered ? "yes" : "**no**");

function capabilityTable(declared, ts, py) {
	const rows = declared.map((capability) => {
		const output =
			capability.output === null
				? "text (see notes)"
				: capability.stdout === "jsonl"
					? `\`${capability.output}\` (one per line)`
					: `\`${capability.output}\``;
		return `| \`${capability.method}\` | \`oneharness ${capability.argv.join(" ")}\` | \`${capability.rust}\` | ${mark(py.has(pythonName(capability.method)))} | ${mark(ts.has(capability.method))} | ${output} |`;
	});
	return [
		"### Capabilities",
		"",
		"One row per thing the CLI can do. **Rust core** names the `oneharness-core`",
		"entry point a consumer calls instead of spawning the binary; **Python** and",
		"**TypeScript** say whether that client defines the method today.",
		"",
		"| Capability | CLI | Rust core | Python | TypeScript | Output |",
		"| --- | --- | --- | --- | --- | --- |",
		...rows,
		"",
	].join("\n");
}

function flagTables(declared) {
	const out = [];
	for (const capability of declared) {
		out.push(
			`#### \`${capability.method}\` — \`oneharness ${capability.argv.join(" ")}\``,
			"",
			"| CLI flag | SDK option | How it is sent |",
			"| --- | --- | --- |",
		);
		for (const fragment of capability.always.filter((f) =>
			f.startsWith("--"),
		)) {
			out.push(`| \`${fragment}\` | _(always sent)_ | fixed |`);
		}
		for (const binding of capability.bindings) {
			const flag = binding.flag
				? `\`${binding.flag}\``
				: binding.kind === "trailing"
					? "_(after `--`)_"
					: "_(positional)_";
			const how = {
				positional: "positional argument",
				value: "`--flag VALUE`",
				repeated: "`--flag VALUE` per element",
				switch: "`--flag` when true",
				"key-value": "`--flag KEY=VALUE` per entry",
				trailing: "appended verbatim",
			}[binding.kind];
			const unless = binding.unless
				? ` (suppressed by \`${binding.unless}\`)`
				: "";
			out.push(`| ${flag} | \`${binding.option}\` | ${how}${unless} |`);
		}
		for (const declined of capability.uncovered) {
			out.push(
				`| \`${declined.flag}\` | **deliberately none** | ${declined.reason} |`,
			);
		}
		out.push("");
	}
	return out.join("\n");
}

const FLAG_PREAMBLE = [
	"### Flags, per capability",
	"",
	"Every long flag each verb declares, and the SDK option that renders it. This",
	"is the **declared binding** — what a method must send once it exists — not a",
	"claim that a client implements it; the capability table above is what says",
	"which clients do. `tests/capability.rs` fails if a flag appears in",
	"`src/cli.rs` and in neither column here.",
	"",
].join("\n");

const declared = capabilities();
const ts = typescriptMethods();
const py = pythonMethods();
const generated = [
	BEGIN,
	"<!-- Generated by `just parity-audit`. The capability and flag rows come from",
	"     `domain::capability::CAPABILITIES`; the Python/TypeScript columns are read",
	"     from each client's own source. Edit those, not this block. -->",
	"",
	capabilityTable(declared, ts, py),
	FLAG_PREAMBLE,
	flagTables(declared),
	END,
].join("\n");

const current = readFileSync(doc, "utf8");
const before = current.slice(0, current.indexOf(BEGIN));
const after = current.slice(current.indexOf(END) + END.length);
const next = `${before}${generated}${after}`;

if (process.argv.includes("--check")) {
	if (next !== current) {
		console.error(
			"docs/sdk-parity.md is out of date with the capability manifest; run just parity-audit",
		);
		process.exit(1);
	}
} else {
	writeFileSync(doc, next);
}
