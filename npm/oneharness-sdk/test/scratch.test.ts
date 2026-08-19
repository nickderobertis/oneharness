import { afterEach, expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { PREFIX, removeScratch, scratch } from "./scratch.mjs";

afterEach(removeScratch);

const here = dirname(fileURLToPath(import.meta.url));

test("a failing test still gives back the scratch directory it took", () => {
	// Driven as a real `bun test` subprocess, because the teardown that matters
	// runs after a test body has already thrown: nothing inside a passing test
	// can watch that happen. This is the regression guard for the shape that
	// leaked one directory per case, every run, onto the host.
	const fixture = resolve(here, "scratch-failure.fixture.ts");
	const run = spawnSync("bun", ["test", fixture], {
		cwd: resolve(here, ".."),
		encoding: "utf8",
	});

	expect(run.status).not.toBe(0);
	const directory = /scratch-fixture-directory (.+)/.exec(
		`${run.stdout}${run.stderr}`,
	)?.[1];
	expect(directory).toBeTruthy();
	expect(existsSync((directory as string).trim())).toBe(false);
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
