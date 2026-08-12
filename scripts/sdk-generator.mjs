// Run the Rust schema generator, and make its failure readable.
//
// Four scripts shell out to the same `cargo run --example generate_*sdk_schema`
// to get the contract bundle. When that build breaks, `execFileSync` throws and
// Node prints a stack trace through the generator's own argv — the reader gets
// a JavaScript backtrace for a Rust compile error, with no statement of which
// script wanted the bundle or what to run next.
//
// This wraps it so a failure says three things instead: which script needed the
// generator, the compiler's own last words (bounded — a cargo failure can run to
// hundreds of lines, and the tail is where the error is), and the command to run
// to see the whole thing.
import { execFileSync } from "node:child_process";

/** Keep the quoted cause to the tail, where the error actually is. */
const CAUSE_LINES = 20;

function tail(text) {
	const lines = String(text ?? "")
		.split("\n")
		.filter((line) => line.trim() !== "");
	if (lines.length <= CAUSE_LINES) {
		return lines;
	}
	return [
		`… ${lines.length - CAUSE_LINES} earlier line(s) omitted; run the command below for all of it`,
		...lines.slice(-CAUSE_LINES),
	];
}

/**
 * Run the schema generator and return its parsed bundle.
 *
 * On failure this prints a bounded diagnostic and exits 1 rather than throwing,
 * because every caller is a gate whose stack trace is noise to the person
 * reading it.
 *
 * @param {object} spec
 * @param {string} spec.script  the calling script, named in the diagnostic
 * @param {string} spec.crate   `-p` package holding the example
 * @param {string} spec.example the schema-emitting example
 * @param {string} spec.cwd     repository root to run in
 * @param {string} spec.target  CARGO_TARGET_DIR, shared between the generators
 * @param {string} spec.rerun   the command that re-runs the caller once fixed
 * @param {(key: string, value: unknown) => unknown} [spec.reviver] JSON.parse reviver
 */
export function schemaBundle({
	script,
	crate,
	example,
	cwd,
	target,
	rerun,
	reviver,
}) {
	const args = [
		"run",
		"-q",
		"-p",
		crate,
		"--features",
		"sdk-schema",
		"--example",
		example,
	];
	let json;
	try {
		json = execFileSync("cargo", args, {
			cwd,
			encoding: "utf8",
			env: { ...process.env, CARGO_TARGET_DIR: target },
		});
	} catch (error) {
		const cause = tail(error.stderr);
		console.error(
			`${script}: the Rust schema generator failed, so there is no contract to check against.`,
		);
		if (cause.length > 0) {
			console.error("  cargo said:");
			for (const line of cause) {
				console.error(`    ${line}`);
			}
		} else if (error.code === "ENOENT") {
			console.error("  cargo is not on PATH.");
		}
		console.error(
			`  fix: make the bundle build, then rerun \`${rerun}\`.\n` +
				`       See it in full with: cargo ${args.join(" ")}`,
		);
		process.exit(1);
	}
	try {
		return JSON.parse(json, reviver);
	} catch (error) {
		console.error(
			`${script}: the schema generator succeeded but did not emit JSON (${error.message}).`,
		);
		console.error(
			`  fix: \`${example}\` must print the bundle and nothing else — a stray \`println!\`\n` +
				`       in the example or in \`sdk_schema::bundle\` corrupts it. Then rerun \`${rerun}\`.`,
		);
		process.exit(1);
	}
}
