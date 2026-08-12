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
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { schemaBundle } from "./sdk-generator.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const doc = resolve(root, "docs/sdk-parity.md");
const BEGIN = "<!-- BEGIN GENERATED: capability-tables -->";
const END = "<!-- END GENERATED: capability-tables -->";
// Shared with `just sdk-generate`, so an audit run reuses that build rather than
// racing the workspace target directory.
const generatorTarget = resolve(root, "target/sdk-schema-generator");

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

/** The schema bundle a language client actually ships. */
function shipped(relative) {
	return JSON.parse(readFileSync(resolve(root, relative), "utf8"));
}

/**
 * Every field name reachable in one output contract, through `$ref` and unions.
 *
 * The recursive set is what the coverage marks are computed over, so a nested
 * field a client's bundle is missing is caught as surely as a top-level one.
 */
function everyField(schema) {
	const defs = schema.$defs ?? {};
	const seen = new Set();
	const names = new Set();
	const walk = (node) => {
		if (node === null || typeof node !== "object") return;
		if (Array.isArray(node)) {
			for (const item of node) walk(item);
			return;
		}
		if (typeof node.$ref === "string") {
			const name = node.$ref.replace("#/$defs/", "");
			if (!seen.has(name)) {
				seen.add(name);
				walk(defs[name]);
			}
			return;
		}
		for (const [key, value] of Object.entries(node)) {
			if (key === "$defs") continue; // reached through the refs that use it
			if (key === "properties") {
				for (const [property, child] of Object.entries(value)) {
					names.add(property);
					walk(child);
				}
				continue;
			}
			walk(value);
		}
	};
	walk(schema);
	return names;
}

/** The fields on the document itself, past any array or union wrapper. */
function topLevelFields(schema) {
	const defs = schema.$defs ?? {};
	const at = (node) => {
		if (node === null || typeof node !== "object") return [];
		if (typeof node.$ref === "string")
			return at(defs[node.$ref.replace("#/$defs/", "")]);
		if (node.items) return at(node.items);
		const union = node.oneOf ?? node.anyOf;
		if (union) return union.flatMap(at);
		return Object.keys(node.properties ?? {});
	};
	return [...new Set(at(schema))].sort();
}

function outputTable(declared, bundles) {
	const roots = [
		...new Set(declared.map((c) => c.output).filter((root) => root !== null)),
	];
	const rows = roots.map((rootName) => {
		const declaredFields = everyField(bundles.rust[rootName]);
		const cell = (bundle) => {
			const missing = [...declaredFields].filter(
				(field) => !everyField(bundle[rootName] ?? {}).has(field),
			);
			return missing.length === 0
				? "yes"
				: `**no** — missing ${missing.map((f) => `\`${f}\``).join(", ")}`;
		};
		const top = topLevelFields(bundles.rust[rootName])
			.map((field) => `\`${field}\``)
			.join(", ");
		return `| \`${rootName}\` | ${top} | ${declaredFields.size} | yes | ${cell(bundles.python)} | ${cell(bundles.typescript)} |`;
	});
	return [
		"### Output contracts, field by field",
		"",
		"One row per document the CLI prints. **Fields** are the ones on the document",
		"itself, past any array or union wrapper; **all** counts every field reachable",
		"through it, nested ones included, and that whole set is what the coverage",
		"marks are computed over — a nested field a client's bundle is missing shows",
		"up here as surely as a top-level one.",
		"",
		"There is one source: `sdk_schema::bundle` generates the Rust type's schema,",
		"and each client checks in what it was handed. **Rust core** returns the typed",
		"value itself, so it carries every field by construction. Neither client",
		"strips what it validates, so an additive field a newer CLI emits reaches a",
		"caller rather than being dropped.",
		"",
		"| Output | Fields on the document | All | Rust core | Python | TypeScript |",
		"| --- | --- | --- | --- | --- | --- |",
		...rows,
		"",
	].join("\n");
}

/**
 * The recipes that must carry the parity gates, and what each one holds.
 *
 * This is the question the audit asks, not the answer: the justfile's `check`
 * recipe is the one source for what a single gate run composes, and the section
 * below is read from it. A recipe renamed, or dropped from `check`, fails the
 * regeneration rather than leaving a document — or a README pointing at it —
 * claiming a gate that no longer runs.
 */
const PARITY_RECIPES = {
	test: "`tests/capability.rs` and `tests/library_surface.rs`",
	"lint-workflows":
		"`scripts/check-sdk-coverage.sh` and `scripts/check-parity-audit.sh`",
	"sdk-check":
		"the TypeScript client's generated-contract drift check, lint, types, and packaged-CLI e2e",
	"python-sdk-check":
		"the same for the Python client, on the oldest supported interpreter",
};

/** The recipes the justfile's own `check` runs, in its order. */
function checkComposition() {
	const justfile = readFileSync(resolve(root, "justfile"), "utf8");
	const recipe = justfile.match(/^check:(?<dependencies>[^\n]*)$/mu);
	if (!recipe) {
		console.error(
			"justfile has no `check:` recipe to read the gate composition from; restore it and rerun just parity-audit",
		);
		process.exit(1);
	}
	return recipe.groups.dependencies.trim().split(/\s+/u).filter(Boolean);
}

function enforcementSection() {
	const composition = checkComposition();
	const missing = Object.keys(PARITY_RECIPES).filter(
		(recipe) => !composition.includes(recipe),
	);
	if (missing.length > 0) {
		console.error(
			`the justfile's \`check\` recipe no longer runs ${missing.join(", ")}; either restore the dependency or drop it from PARITY_RECIPES in scripts/parity-audit.mjs, then rerun just parity-audit`,
		);
		process.exit(1);
	}
	return [
		"### Which gate runs them",
		"",
		"Read from the justfile's own `check` recipe, so this is the composition a",
		"run actually has rather than a second copy of it. Each recipe below is",
		"runnable alone while iterating; `just check` runs every one of them, along",
		`with ${composition.filter((recipe) => !(recipe in PARITY_RECIPES)).map((recipe) => `\`${recipe}\``).join(", ")}.`,
		"",
		"| Recipe | What it holds in place |",
		"| --- | --- |",
		...Object.entries(PARITY_RECIPES).map(
			([recipe, holds]) => `| \`just ${recipe}\` | ${holds} |`,
		),
		"",
		"`just parity-audit` regenerates this document; the `check-parity-audit.sh`",
		"run inside `lint-workflows` is what fails when the checked-in copy no longer",
		"matches either source.",
		"",
	].join("\n");
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

const bundle = schemaBundle({
	script: "parity-audit",
	crate: "oneharness-core",
	example: "generate_core_sdk_schema",
	cwd: root,
	target: generatorTarget,
	rerun: "just parity-audit",
});
const declared = bundle.capabilities;
const ts = typescriptMethods();
const py = pythonMethods();
const generated = [
	BEGIN,
	"<!-- Generated by `just parity-audit`. The capability, flag and output rows come",
	"     from `domain::capability::CAPABILITIES` and the schema bundle; the",
	"     Python/TypeScript columns are read from each client's own source and",
	"     checked-in schemas; the gate table is read from the justfile's `check`",
	"     recipe. Edit those, not this block. -->",
	"",
	enforcementSection(),
	capabilityTable(declared, ts, py),
	FLAG_PREAMBLE,
	flagTables(declared),
	outputTable(declared, {
		rust: bundle,
		typescript: shipped("npm/oneharness-sdk/src/generated/schemas.json"),
		python: shipped(
			"python/oneharness-sdk/src/oneharness_sdk/_generated/schemas.json",
		),
	}),
	END,
].join("\n");

/** How many changed lines per side are worth printing before the count says it. */
const DRIFT_LINES = 12;

/**
 * Name what changed, so a red gate is readable where it failed.
 *
 * "Out of date" alone sends the reader to a local rerun, which is no help when
 * the run that disagreed was on another machine — a difference only that
 * platform produces is invisible in a log that just says the file is stale. The
 * diff is trimmed to the changed region and bounded, and an all-invisible
 * difference is named as one rather than printed as identical-looking lines.
 */
function drift(current, next) {
	if (current.replaceAll("\r\n", "\n") === next.replaceAll("\r\n", "\n")) {
		return [
			"the two differ only in line endings: the checked-in copy has CRLF and the",
			"generator writes LF. Check out this file with LF endings (see .gitattributes).",
		];
	}
	const was = current.split("\n");
	const now = next.split("\n");
	let head = 0;
	while (head < was.length && head < now.length && was[head] === now[head]) {
		head += 1;
	}
	let tail = 0;
	while (
		tail < was.length - head &&
		tail < now.length - head &&
		was.at(-1 - tail) === now.at(-1 - tail)
	) {
		tail += 1;
	}
	const bound = (lines, sign) => {
		const shown = lines.slice(0, DRIFT_LINES).map((line) => `${sign} ${line}`);
		return lines.length > DRIFT_LINES
			? [...shown, `${sign} … ${lines.length - DRIFT_LINES} more line(s)`]
			: shown;
	};
	return [
		`first difference at docs/sdk-parity.md:${head + 1} ("-" checked in, "+" generated):`,
		...bound(was.slice(head, was.length - tail), "-"),
		...bound(now.slice(head, now.length - tail), "+"),
	];
}

const current = readFileSync(doc, "utf8");
const opens = current.indexOf(BEGIN);
const closes = current.indexOf(END);
// Refuse a document that has lost its markers rather than slicing at -1, which
// would splice the tables into the middle of the prose and call it generated.
if (opens === -1 || closes === -1 || closes < opens) {
	console.error(
		`docs/sdk-parity.md is missing its generated-block markers (${BEGIN} … ${END}); restore both, in that order, and rerun just parity-audit`,
	);
	process.exit(1);
}
const next = `${current.slice(0, opens)}${generated}${current.slice(closes + END.length)}`;

if (process.argv.includes("--check")) {
	if (next !== current) {
		console.error(
			"docs/sdk-parity.md is out of date with the capability manifest; run just parity-audit",
		);
		for (const line of drift(current, next)) {
			console.error(`  ${line}`);
		}
		process.exit(1);
	}
} else {
	writeFileSync(doc, next);
}
