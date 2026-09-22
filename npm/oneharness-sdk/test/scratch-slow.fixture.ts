// A passing test whose scratch cleanup outlasts bun's default hook budget.
//
// `scratch.test.ts` runs this file as its own `bun test` subprocess: the shared
// timeout is the only reason the run passes at all, and how long it took plus
// the missing directory are what say the runner really awaited the removal
// rather than abandoning it at five seconds. Nothing inside a suite can watch
// its own teardown be waited out, which is why this is a subprocess. Named
// `.fixture.ts` so bun's own test glob leaves it to that one caller.
import { test } from "bun:test";
import { setTimeout as sleep } from "node:timers/promises";
import { removeScratchAsync, scratch } from "./scratch.mjs";
import {
	BUN_DEFAULT_HOOK_TIMEOUT_MS,
	registerScratchCleanup,
} from "./scratch-hook.mjs";

// The registration under test, over a removal that takes a second longer than
// bun would wait on its own.
registerScratchCleanup(async () => {
	await sleep(BUN_DEFAULT_HOOK_TIMEOUT_MS + 1_000);
	await removeScratchAsync();
});

test("takes scratch space that is slow to give back", async () => {
	console.log(
		`scratch-fixture-directory ${await scratch("slow-cleanup-probe")}`,
	);
});
