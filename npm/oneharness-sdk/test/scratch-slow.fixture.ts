// llmlint: ignore-file[new_code_lands_in_a_project] The rule presumes an Nx project graph; this repository has none by a recorded decision (`AGENTS.md`: root `just` delegates to Cargo/Bun without Nx because the two-package graph is static), so no project definition can cover this file and `scratch.test.ts` is what runs it.
// A passing test whose scratch cleanup outlasts bun's default hook budget.
//
// `scratch.test.ts` runs this file as its own `bun test` subprocess: the shared
// timeout is the only reason the run passes at all, and how long it took plus
// the missing directory are what say the runner really awaited the removal
// rather than abandoning it. Nothing inside a suite can watch its own teardown
// be waited out, which is why this is a subprocess. `scratch-unguarded.fixture
// .ts` is its other half, holding the delay above bun's own default. Named
// `.fixture.ts` so bun's own test glob leaves it to that one caller.
import { test } from "bun:test";
import { setTimeout as sleep } from "node:timers/promises";
import { removeScratchAsync, scratch } from "./scratch.mjs";
import { registerScratchCleanup, SLOW_REMOVAL_MS } from "./scratch-hook.mjs";

// The registration under test, over a removal bun would not wait out unasked.
registerScratchCleanup(async () => {
	await sleep(SLOW_REMOVAL_MS);
	await removeScratchAsync();
});

test("takes scratch space that is slow to give back", async () => {
	console.log(
		`scratch-fixture-directory ${await scratch("slow-cleanup-probe")}`,
	);
});
