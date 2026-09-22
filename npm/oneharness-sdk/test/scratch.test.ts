import { expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { PREFIX, removeScratch, scratch, scratchSync } from "./scratch.mjs";
import {
	BUN_DEFAULT_HOOK_TIMEOUT_MS,
	registerScratchCleanup,
} from "./scratch-hook.mjs";

registerScratchCleanup();

const here = dirname(fileURLToPath(import.meta.url));

/**
 * What the case below may spend driving the slow fixture.
 *
 * A test timeout rather than a cleanup one: this case pays the fixture's
 * deliberate delay plus a whole `bun test` startup, so bun's own five-second
 * default would cut it off long before the fixture had a verdict to report.
 */
const SLOW_FIXTURE_CASE_TIMEOUT_MS = 60_000;

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
 * Everything the slow-cleanup fixture said, and how long saying it took.
 *
 * Its own `bun test` subprocess, for the same reason the failing fixture gets
 * one: a suite cannot watch its own teardown be waited out. A non-zero status
 * is quoted rather than asserted away, because the message bun prints when it
 * cuts a hook off at five seconds is the whole diagnosis.
 *
 * Run from this directory rather than the package root, so the package
 * `bunfig.toml`'s coverage threshold — a property of the whole suite — is not
 * what decides a subprocess that loads one file on purpose. The fixtures above
 * can ignore that because a non-zero status is what they assert.
 */
function runSlowFixture(): { elapsed: number; output: string } {
	const started = Date.now();
	const run = spawnSync(
		"bun",
		["test", resolve(here, "scratch-slow.fixture.ts")],
		{ cwd: here, encoding: "utf8" },
	);
	const elapsed = Date.now() - started;
	const output = `${run.stdout}${run.stderr}`;
	if (run.status !== 0) {
		throw new Error(`the slow-cleanup fixture did not pass:\n${output}`);
	}
	return { elapsed, output };
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

test("the exit handler's synchronous removal gives its directory back", () => {
	// Every hook here awaits `removeScratchAsync`, so this suite would otherwise
	// never reach the synchronous one — and `package-e2e.mjs` is the caller that
	// cannot stop using it: it runs under plain node with no framework to hang a
	// teardown on, and a `process.on("exit")` handler has no `await` to spend. An
	// unexercised removal is how that script would start leaking quietly.
	const directory = scratchSync("sync-removal-probe");
	expect(existsSync(directory)).toBe(true);
	removeScratch();
	expect(existsSync(directory)).toBe(false);
});

test(
	"the shared cleanup hook waits out a removal past bun's default budget",
	() => {
		// The regression guard for the shape that failed an otherwise-passing test
		// in its teardown whenever the disk was contended. The fixture registers
		// this suite's own `registerScratchCleanup` over a removal that takes
		// longer than bun would wait unasked, so one run says three things: it
		// passed, so the shared timeout really replaced the five-second default;
		// it took that long, so the runner awaited the removal rather than moving
		// on; and the directory is gone, so what it awaited ran to completion.
		const { elapsed, output } = runSlowFixture();
		expect(elapsed).toBeGreaterThan(BUN_DEFAULT_HOOK_TIMEOUT_MS);
		expect(existsSync(scratchDirectoryFrom(output))).toBe(false);
	},
	SLOW_FIXTURE_CASE_TIMEOUT_MS,
);
