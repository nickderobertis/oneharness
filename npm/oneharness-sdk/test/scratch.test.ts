import { expect, test } from "bun:test";
import { spawn, spawnSync } from "node:child_process";
import {
	chmodSync,
	existsSync,
	mkdirSync,
	readdirSync,
	readFileSync,
	realpathSync,
	rmSync,
	writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
	controlScratch,
	PREFIX,
	placeScratch,
	placeScratchSync,
	RUN_ENV,
	RUN_MARKER,
	removeScratch,
	removeScratchAsync,
	retireScratch,
	retireScratchSync,
	scratch,
	scratchSync,
} from "./scratch.mjs";
import { registerScratchCleanup, SLOW_CLEANUP_MS } from "./scratch-hook.mjs";

registerScratchCleanup();

const here = dirname(fileURLToPath(import.meta.url));

/**
 * What either case below may spend driving a slow fixture.
 *
 * A test timeout rather than a cleanup one: each pays `SLOW_CLEANUP_MS` plus a
 * whole `bun test` startup, so bun's own default test budget would cut it off
 * long before its fixture had a verdict to report.
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
 * one: a suite cannot watch its own teardown be waited out. Unlike `runFixture`
 * this one must pass, so a non-zero status is quoted rather than asserted away:
 * the message bun prints on a hook it cut short is the whole diagnosis.
 *
 * Run from this directory rather than the package root, so the package
 * `bunfig.toml`'s coverage threshold — a property of the whole suite — is not
 * what decides a subprocess that loads one file on purpose. `runFixture` above
 * can ignore that, because a non-zero status is all its callers ever want.
 */
function runSlowCleanupFixture(): { elapsed: number; output: string } {
	// Monotonic, because a wall clock stepped mid-run once read 5963ms across a
	// fixture that had slept 6000ms of it.
	const started = performance.now();
	const run = spawnSync(
		"bun",
		["test", resolve(here, "scratch-slow.fixture.ts")],
		{ cwd: here, encoding: "utf8" },
	);
	const elapsed = performance.now() - started;
	const output = `${run.stdout}${run.stderr}`;
	if (run.status !== 0) {
		throw new Error(`the guarded cleanup fixture did not pass:\n${output}`);
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

test("scratch names end in the id of the process that made them", () => {
	// The leak gate reads that suffix to leave another checkout's live directory
	// out of its verdict; a name without it is counted as this run's leak.
	const directory = scratchSync("pid-probe");
	expect(directory.split(/[\\/]/u).at(-1)).toEndWith(`-${process.pid}`);
});

/**
 * Run `body` with the leak gate's run token set to `token`, or unset when it is
 * `undefined`, restoring whatever this run inherited afterwards.
 */
async function underRunToken(
	token: string | undefined,
	body: () => Promise<void>,
): Promise<void> {
	const inherited = process.env[RUN_ENV];
	if (token === undefined) delete process.env[RUN_ENV];
	else process.env[RUN_ENV] = token;
	try {
		await body();
	} finally {
		if (inherited === undefined) delete process.env[RUN_ENV];
		else process.env[RUN_ENV] = inherited;
	}
}

test("every scratch directory made under the leak gate names its run", async () => {
	// `scripts/check-temp-leaks.sh` reads that marker to tell this run's
	// scratch from a sibling run's without reading this process's environment,
	// which it cannot do on Windows. Each of the three ways to make one writes
	// it, since each makes its directory on its own.
	await underRunToken("4242.node-probe", async () => {
		for (const directory of [
			await scratch("marker-probe"),
			await controlScratch("mark"),
			scratchSync("marker-sync-probe"),
		]) {
			expect(readFileSync(resolve(directory, RUN_MARKER), "utf8")).toBe(
				"4242.node-probe",
			);
		}
	});
});

test(
	"a sibling gate never sees a scratch directory without its marker",
	async () => {
		// The gate on node-sdk:e2e sweeps the same temp dir while node-sdk:test
		// makes and removes its scratch. On Windows it cannot read that
		// process's environment, so any swept directory it found unmarked counted
		// as its own leak. So another process churns here while this one lists
		// the root throughout, and every swept name must hold the churner's
		// marker while it exists.
		const root = await scratch("observed");
		const token = "4242.node-churn";
		const churner = spawn(
			process.execPath,
			[resolve(here, "scratch-churn.fixture.ts"), "500"],
			{
				env: {
					...process.env,
					TMPDIR: root,
					TEMP: root,
					TMP: root,
					[RUN_ENV]: token,
				},
				stdio: ["ignore", "ignore", "inherit"],
			},
		);
		const exited = new Promise<number | null>((done) =>
			churner.on("exit", done),
		);
		let running = true;
		void exited.then(() => {
			running = false;
		});
		let seen = 0;
		const unmarked: string[] = [];
		while (running) {
			for (const name of readdirSync(root)) {
				if (!name.startsWith(PREFIX)) continue;
				seen++;
				const directory = resolve(root, name);
				let marker: string | undefined;
				try {
					marker = readFileSync(resolve(directory, RUN_MARKER), "utf8");
				} catch {}
				// Each cycle's name is its own, so a directory still there after a
				// failed read is the one that was read.
				if (marker !== token && existsSync(directory)) unmarked.push(name);
			}
			await new Promise((next) => setImmediate(next));
		}
		expect(await exited).toBe(0);
		expect(seen).toBeGreaterThan(0);
		expect(unmarked).toEqual([]);
		// A directory that really is left behind is still the churner's leak: it
		// stands under the swept name, marked for its run, and nothing staged or
		// retired is left beside it.
		const [leaked, ...beside] = readdirSync(root).filter(
			(name) => name !== RUN_MARKER,
		);
		expect(beside).toEqual([]);
		expect(leaked).toStartWith(`${PREFIX}leaked-`);
		expect(readFileSync(resolve(root, `${leaked}`, RUN_MARKER), "utf8")).toBe(
			token,
		);
	},
	SLOW_FIXTURE_CASE_TIMEOUT_MS,
);

test("a scratch directory that cannot be made leaves nothing behind", async () => {
	// The marker is written under a name the gate does not sweep, so a failure
	// there would leave a directory no gate could ever report. Each maker
	// refuses loudly and gives that staging space back.
	const root = await scratch("unusable-root");
	const missing = resolve(root, "missing");
	const inherited = {
		TMPDIR: process.env.TMPDIR,
		TEMP: process.env.TEMP,
		TMP: process.env.TMP,
	};
	Object.assign(process.env, { TMPDIR: missing, TEMP: missing, TMP: missing });
	try {
		await underRunToken("4242.node-unusable", async () => {
			expect(() => scratchSync("unusable-sync")).toThrow();
			await expect(scratch("unusable-async")).rejects.toThrow();
		});
	} finally {
		for (const [name, value] of Object.entries(inherited)) {
			if (value === undefined) delete process.env[name];
			else process.env[name] = value;
		}
	}
	expect(readdirSync(root).filter((name) => name !== RUN_MARKER)).toEqual([]);
});

test("a scratch directory that cannot be placed leaves nothing behind", async () => {
	// The marker is written under a name the gate does not sweep, so a failure
	// once that staging exists would leave a directory no gate could ever
	// report. A real directory already standing where the scratch would go
	// makes the rename into place fail, on every platform.
	const root = await scratch("collision");
	const taken = resolve(root, `${PREFIX}taken`);
	mkdirSync(taken);
	writeFileSync(resolve(taken, "held"), "someone else's");
	await underRunToken("4242.node-collision", async () => {
		expect(() => placeScratchSync(taken)).toThrow();
		await expect(placeScratch(taken)).rejects.toThrow();
	});
	expect(readdirSync(root).filter((name) => name !== RUN_MARKER)).toEqual([
		`${PREFIX}taken`,
	]);
	expect(readFileSync(resolve(taken, "held"), "utf8")).toBe("someone else's");
});

test("a scratch directory already gone is given back without complaint", async () => {
	// Removal renames a directory off the sweep first, and falls back to
	// removing it in place when that rename fails, as it does on Windows
	// while a handle is open inside, or here, where nothing is left.
	for (const remove of [removeScratch, removeScratchAsync]) {
		const directory = scratchSync("already-gone");
		rmSync(directory, { recursive: true });
		await remove();
		expect(existsSync(directory)).toBe(false);
	}
});

/**
 * Whether this user can be refused a write by a directory's permissions:
 * Windows ignores the mode bits, and a privileged user writes through them.
 */
function permissionsBind(): boolean {
	return process.platform !== "win32" && process.getuid?.() !== 0;
}

/** What `root` holds besides the gate's marker for this run, if any. */
function entries(root: string): string[] {
	return readdirSync(root)
		.filter((name) => name !== RUN_MARKER)
		.sort();
}

const retirements = [
	["sync", retireScratchSync],
	["async", retireScratch],
] as const;

test.skipIf(!permissionsBind())(
	"a partly removed scratch directory returns to the sweep with its marker",
	async () => {
		// Removal renames a directory off the sweep and removes its marker before
		// anything else. If the rest cannot be removed, the remainder must not
		// stay hidden, nor come back unmarked: it returns still owned by its
		// run, which is what makes it that run's leak in the gate.
		const root = await scratch("unremovable");
		for (const [mode, retire] of retirements) {
			const token = `4242.node-unremovable-${mode}`;
			const directory = resolve(root, `${PREFIX}stuck-${mode}`);
			await underRunToken(token, async () => {
				placeScratchSync(directory);
			});
			const locked = resolve(directory, "locked");
			mkdirSync(locked);
			writeFileSync(resolve(locked, "file"), "held");
			chmodSync(locked, 0o500);
			try {
				await retire(directory);
			} catch {
				// The removal says why it failed; what it left is the test.
			} finally {
				chmodSync(locked, 0o700);
			}
			expect(entries(root)).toContain(`${PREFIX}stuck-${mode}`);
			expect(readFileSync(resolve(directory, RUN_MARKER), "utf8")).toBe(token);
		}
		expect(entries(root).filter((name) => name.startsWith("."))).toEqual([]);
	},
);

test.skipIf(!permissionsBind())(
	"a marked scratch directory that cannot be renamed is left whole",
	async () => {
		// Removing it in place would take its marker first and leave it visible
		// unmarked, the race the staging names exist to close. So it stays as
		// it was: under its swept name, owned by its run.
		const root = await scratch("unrenamable");
		for (const [mode, retire] of retirements) {
			const token = `4242.node-unrenamable-${mode}`;
			const directory = resolve(root, `${PREFIX}stuck-${mode}`);
			await underRunToken(token, async () => {
				placeScratchSync(directory);
			});
			writeFileSync(resolve(directory, "inside.txt"), "content");
			chmodSync(root, 0o500);
			const said: unknown[][] = [];
			const report = console.error;
			console.error = (...line: unknown[]) => said.push(line);
			try {
				await retire(directory);
			} finally {
				console.error = report;
				chmodSync(root, 0o700);
			}
			expect(readFileSync(resolve(directory, RUN_MARKER), "utf8")).toBe(token);
			expect(existsSync(resolve(directory, "inside.txt"))).toBe(true);
			// Giving up is said, beside the leak its run's gate will report.
			expect(String(said[0]?.[0])).toContain(
				`scratch: left ${directory} in place`,
			);
		}
	},
);

test("a scratch directory made with no gate watching carries no marker", async () => {
	await underRunToken(undefined, async () => {
		for (const directory of [
			await scratch("unmarked-probe"),
			await controlScratch("unmk"),
			scratchSync("unmarked-sync-probe"),
		]) {
			expect(existsSync(resolve(directory, RUN_MARKER))).toBe(false);
		}
	});
});

test("a control store is rooted at /tmp rather than the temp dir", async () => {
	// Its path becomes a socket address, and macOS's per-user temp dir alone
	// spends about half of `sun_path` — the interrupt test's refusal there. The
	// budget itself is the CLI's to enforce, which that test drives for real.
	const store = await controlScratch("int");
	expect(existsSync(store)).toBe(true);
	expect(dirname(store)).toBe(
		process.platform === "win32" ? tmpdir() : realpathSync("/tmp"),
	);
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
		// passed, so the shared timeout really replaced bun's default; it took
		// that long, so the runner awaited the removal rather than moving on; and
		// the directory is gone, so what it awaited ran to completion.
		const { elapsed, output } = runSlowCleanupFixture();
		expect(elapsed).toBeGreaterThanOrEqual(SLOW_CLEANUP_MS);
		expect(existsSync(scratchDirectoryFrom(output))).toBe(false);
	},
	SLOW_FIXTURE_CASE_TIMEOUT_MS,
);

test(
	"a teardown of the same length is cut short under bun's own budget",
	() => {
		// The drift gate under `SLOW_CLEANUP_MS`. Bun owns its default and can
		// raise it; if it rose past that span, the case above would still pass
		// while attributing to the shared timeout a teardown bun would have waited
		// out anyway. So the same span is waited under a hook registered without
		// the shared timeout, and bun has to cut it off. That fixture touches no
		// scratch space at all — every hook that gives a directory back goes
		// through `registerScratchCleanup` — so the length of the wait is the only
		// thing under test here.
		const output = runFixture("bun-hook-budget.fixture.ts");
		expect(output).toContain("hook timed out");
	},
	SLOW_FIXTURE_CASE_TIMEOUT_MS,
);
