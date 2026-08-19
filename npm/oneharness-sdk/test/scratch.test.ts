import { afterEach, expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { PREFIX, removeScratch, scratch } from "./scratch.mjs";

afterEach(removeScratch);

const here = dirname(fileURLToPath(import.meta.url));

/**
 * Everything one fixture said while failing as its own `bun test` subprocess.
 *
 * Driven as a real subprocess, because the teardown that matters runs after a
 * test body has already thrown: nothing inside a passing test can watch that
 * happen.
 */
function runFixture(name: string): string {
	const run = spawnSync("bun", ["test", resolve(here, name)], {
		cwd: resolve(here, ".."),
		encoding: "utf8",
	});
	expect(run.status).not.toBe(0);
	return `${run.stdout}${run.stderr}`;
}

/**
 * The scratch directory a fixture printed, or a failure quoting what it said
 * instead.
 *
 * A fixture that stops printing its directory would otherwise hand `undefined`
 * to the caller's existsSync assertion, which then passes for the wrong reason —
 * so the absence is named here rather than narrowed away with a cast.
 */
function scratchDirectoryFrom(output: string): string {
	const directory = /scratch-fixture-directory (.+)/.exec(output)?.[1];
	if (!directory) {
		throw new Error(`fixture never printed its scratch directory:\n${output}`);
	}
	return directory.trim();
}

test("a failing test still gives back the scratch directory it took", () => {
	// This is the regression guard for the shape that leaked one directory per
	// case, every run, onto the host.
	const output = runFixture("scratch-failure.fixture.ts");
	expect(existsSync(scratchDirectoryFrom(output))).toBe(false);
});

test("a fixture that prints no scratch directory is reported, not assumed gone", () => {
	// The guard above is the whole difference between proving the directory was
	// removed and proving nothing at all, so it gets a fixture of its own: one
	// that fails without printing. Its complaint has to quote the real output,
	// which is the only thing that says why the marker was missing.
	const output = runFixture("scratch-silent.fixture.ts");
	expect(() => scratchDirectoryFrom(output)).toThrow(
		/never printed its scratch directory/,
	);
	expect(() => scratchDirectoryFrom(output)).toThrow(
		/the silent failure this stands in for/,
	);
});

test("scratch names carry the prefix the leak gate sweeps for", async () => {
	// `scripts/check-temp-leaks.sh` sweeps for `io::scratch::PREFIX`, and this
	// suite's names have to start with it or the sweep passes while the
	// directories pile up. `scripts/check-scratch-prefixes.sh` holds the two in
	// step across the language boundary; this asserts the names really use it.
	const directory = await scratch("prefix-probe");
	expect(existsSync(directory)).toBe(true);
	expect(resolve(directory, "..") === directory).toBe(false);
	expect(directory.split(/[\\/]/u).at(-1)).toStartWith(PREFIX);
});
