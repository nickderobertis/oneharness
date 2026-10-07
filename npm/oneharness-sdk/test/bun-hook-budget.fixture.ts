// A teardown that only waits, registered without this suite's shared timeout,
// so bun's own default hook budget is what decides it.
//
// This is the drift gate under `SLOW_CLEANUP_MS`. Bun owns that default and can
// raise it, and a release that raised it past that span would leave
// `scratch-slow.fixture.ts` passing while attributing to the shared timeout a
// teardown bun would have waited out anyway — proving nothing while staying
// green. `scratch.test.ts` therefore requires this run to fail on a timed-out
// hook.
//
// It takes no scratch space and removes none. The question here is only how
// long bun waits unasked, and a hook that really gives a directory back belongs
// to `registerScratchCleanup` and its shared timeout without exception. Named
// `.fixture.ts` so bun's own test glob leaves it to that one caller.
import { afterEach, test } from "bun:test";
import { setTimeout as sleep } from "node:timers/promises";
import { SLOW_CLEANUP_MS } from "./scratch-hook.mjs";

afterEach(() => sleep(SLOW_CLEANUP_MS));

test("ends with a teardown longer than bun waits unasked", () => {
	// Nothing to assert: the subject is the hook above, and all this body owes
	// it is an ending to run after.
});
