// llmlint: ignore-file[new_code_lands_in_a_project] The rule presumes an Nx project graph; this repository has none by a recorded decision (`AGENTS.md`: root `just` delegates to Cargo/Bun without Nx because the two-package graph is static), so no project definition can cover this file and `scratch.test.ts` is what runs it.
// The same slow cleanup as `scratch-slow.fixture.ts`, registered WITHOUT this
// suite's shared timeout, so bun's own default budget is what decides it.
//
// This is the drift gate under `SLOW_REMOVAL_MS`. Bun owns that default and can
// raise it, and a release that raised it past this delay would leave the
// passing fixture beside this one attributing to the shared timeout a cleanup
// bun would have waited out anyway — proving nothing while staying green.
// `scratch.test.ts` therefore requires this run to fail on a timed-out hook,
// and gives back the directory the induced timeout abandons. Named
// `.fixture.ts` so bun's own test glob leaves it to that one caller.
import { afterEach, test } from "bun:test";
import { setTimeout as sleep } from "node:timers/promises";
import { removeScratchAsync, scratch } from "./scratch.mjs";
import { SLOW_REMOVAL_MS } from "./scratch-hook.mjs";

// Deliberately the bare registration this suite stopped using.
afterEach(async () => {
	await sleep(SLOW_REMOVAL_MS);
	await removeScratchAsync();
});

test("takes scratch space an unguarded hook is not given time to release", async () => {
	console.log(
		`scratch-fixture-directory ${await scratch("unguarded-cleanup-probe")}`,
	);
});
