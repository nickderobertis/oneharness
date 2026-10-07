#!/usr/bin/env node
// The Rust line-coverage floor, enforced once over every crate's test run.
//
//   node scripts/rust-coverage.mjs <package>...
//
// Each Rust project's `test` target (scripts/cargo-test.sh) leaves the lines its
// instrumented run executed in target/coverage/<package>.lcov. This merges those
// records — a line counts as covered when ANY run executed it, which is what the
// single `cargo llvm-cov --workspace` run this replaces measured — and fails
// below the floor. Every named package must have a record: a missing one is a
// run that never happened, and leaving it out could only move the number.
//
// Windows is skipped with its reason: llvm-cov does not attribute the coverage
// of subprocess-spawned binaries there, so the binary crate reads as ~0%. The
// floor is a property of the suite, measured on Linux and macOS.
//
// Quiet on success: one line. Below the floor it names the least-covered files.
// Either way the merged numbers are written to target/coverage/rust-summary.json,
// the target's declared output, so a replayed run states the same verdict.
import { readFileSync, writeFileSync } from "node:fs";
import { relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const FLOOR = Number(process.env.COVERAGE_MIN ?? "95");
const root = resolve(fileURLToPath(import.meta.url), "../..");

if (process.platform === "win32") {
	console.log(
		"coverage: skipped on Windows (llvm-cov subprocess attribution under-reports; measured on Linux/macOS — see scripts/rust-coverage.mjs)",
	);
	process.exit(0);
}

const packages = process.argv.slice(2);
if (packages.length === 0 || !Number.isFinite(FLOOR)) {
	console.error("usage: node scripts/rust-coverage.mjs <package>...  (COVERAGE_MIN, default 95)");
	process.exit(2);
}

/** file -> line -> covered? */
const files = new Map();
for (const name of packages) {
	const path = resolve(root, "target/coverage", `${name}.lcov`);
	let text;
	try {
		text = readFileSync(path, "utf8");
	} catch {
		console.error(
			`coverage: no line record for ${name} at ${relative(root, path)}; its test target never ran instrumented. Run 'just coverage' (it runs every Rust test target first).`,
		);
		process.exit(1);
	}
	let lines = null;
	for (const row of text.split(/\r?\n/u)) {
		if (row.startsWith("SF:")) {
			const file = relative(root, resolve(root, row.slice(3)));
			lines = files.get(file) ?? new Map();
			files.set(file, lines);
		} else if (row.startsWith("DA:") && lines !== null) {
			const [line, hits] = row.slice(3).split(",");
			lines.set(line, lines.get(line) === true || Number(hits) > 0);
		} else if (row === "end_of_record") {
			lines = null;
		}
	}
}

let total = 0;
let covered = 0;
const perFile = [];
for (const [file, lines] of files) {
	const hit = [...lines.values()].filter(Boolean).length;
	total += lines.size;
	covered += hit;
	perFile.push({ file, hit, size: lines.size });
}
const percent = total === 0 ? 0 : (covered / total) * 100;
const summary = `${percent.toFixed(2)}% lines (${covered}/${total}) over ${packages.length} crate runs`;
writeFileSync(
	resolve(root, "target/coverage/rust-summary.json"),
	`${JSON.stringify({ floor: FLOOR, percent: Number(percent.toFixed(2)), covered, total, records: packages }, null, 2)}\n`,
);
if (total === 0 || percent < FLOOR) {
	console.error(`coverage: ${summary} is below the ${FLOOR}% floor.`);
	perFile
		.filter(({ hit, size }) => hit < size)
		.sort((a, b) => b.size - b.hit - (a.size - a.hit))
		.slice(0, 15)
		.forEach(({ file, hit, size }) => {
			console.error(`  ${file}: ${size - hit} uncovered of ${size}`);
		});
	console.error(
		"Cover the new behavior with a test (never lower COVERAGE_MIN); `just coverage-html` shows the uncovered lines.",
	);
	process.exit(1);
}
console.log(`coverage: ${summary} (floor ${FLOOR}%)`);
