#!/usr/bin/env node
// Fail when a CLI capability has no counterpart in a language SDK.
//
// `sdk-check` and `python-sdk-check` compare SCHEMAS and TYPES, which is why
// five verbs once had no SDK method at all with nothing objecting: a method
// that does not exist has no shape to drift from. This is the missing half —
// coverage, not drift.
//
// The expectation is DERIVED, never listed here: `domain::capability::CAPABILITIES`
// says what the CLI can do, and each client's own source says what it defines.
// A capability added in Rust therefore fails this gate until both clients grow
// its method, and a method deleted from a client fails it too.
//
// Quiet on success, one line. On failure it names each missing method and where
// to add it.
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
// Shared with `just sdk-generate` and the parity audit, so this reuses that
// build rather than racing the workspace target directory.
const generatorTarget = resolve(root, "target/sdk-schema-generator");

// The clients to read. Overridable by argument so the gate can be pointed at a
// candidate client — which is how `check-sdk-coverage-test.sh` proves it still
// goes red, without having to mutate the real manifest to do it.
const [
	TYPESCRIPT = "npm/oneharness-sdk/src/index.ts",
	PYTHON = "python/oneharness-sdk/src/oneharness_sdk/_client.py",
] = process.argv.slice(2);

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
 * The method names a client defines, read from its source.
 *
 * Read rather than imported, so the gate does not depend on a build having
 * happened — and so a method that exists only as a type is not mistaken for one
 * a caller can invoke.
 */
function methods(relative, className, pattern) {
	const source = readFileSync(resolve(root, relative), "utf8");
	const body = source.slice(source.indexOf(className));
	return new Set(
		[...body.matchAll(pattern)]
			.map((match) => match[1])
			.filter((name) => !name.startsWith("_")),
	);
}

const pythonName = (method) =>
	method.replace(/[A-Z]/gu, (upper) => `_${upper.toLowerCase()}`);

const surfaces = [
	{
		label: "TypeScript",
		file: TYPESCRIPT,
		defined: methods(
			TYPESCRIPT,
			"export class OneHarness",
			/^\t(?:async )?\*?([A-Za-z][A-Za-z0-9]*)\s*[(<]/gmu,
		),
		name: (method) => method,
	},
	{
		label: "Python",
		file: PYTHON,
		defined: methods(
			PYTHON,
			"class OneHarness",
			/^ {4}(?:async )?def ([a-z][a-z0-9_]*)\s*\(/gmu,
		),
		name: pythonName,
	},
];

const declared = capabilities();
const missing = [];
for (const surface of surfaces) {
	for (const capability of declared) {
		const expected = surface.name(capability.method);
		if (!surface.defined.has(expected))
			missing.push({ surface, capability, expected });
	}
}

if (missing.length > 0) {
	for (const { surface, capability, expected } of missing) {
		console.error(
			`sdk-coverage: ${surface.label} has no \`${expected}\` for \`oneharness ${capability.argv.join(" ")}\``,
		);
		console.error(`  fix: add the method to ${surface.file}. It renders its`);
		console.error(
			`       own argv from the manifest's \`${capability.method}\` bindings, so it is the`,
		);
		console.error(
			`       shared call path plus this capability's option and output contracts.`,
		);
	}
	console.error(
		"A capability with no SDK method is a consumer dropping to raw argv, which is the",
	);
	console.error(
		"defect this gate exists to make impossible. Declare it deliberately uncovered in",
	);
	console.error(
		"`domain::capability` if it genuinely does not belong on an SDK.",
	);
	process.exit(1);
}

console.log(
	`sdk-coverage: ${declared.length} capabilities, each with a method in ${surfaces
		.map((surface) => surface.label)
		.join(" and ")}`,
);
