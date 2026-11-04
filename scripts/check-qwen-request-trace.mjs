// Drive Node's real HTTP client through the Qwen preload; observe stderr at
// the subprocess boundary, including an aborted request and secret redaction.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
// Only the external converter is doubled to induce otherwise unavailable errors.
for (const converterExit of ["1", "0"]) {
	const failedConversion = spawn("bash", ["-c", 'set -euo pipefail; source "$1/scripts/e2e-lib.sh"; cygpath() { return "$2"; }; oh_qwen_live_setup', "_", root, converterExit], { stdio: ["ignore", "pipe", "pipe"] });
	let conversionError = "";
	failedConversion.stderr.setEncoding("utf8").on("data", (chunk) => { conversionError += chunk; });
	const conversionCode = await new Promise((resolve, reject) => {
		failedConversion.on("error", reject);
		failedConversion.on("close", resolve);
	});
	assert.equal(conversionCode, 1, "a failed cygpath must stop Qwen setup; inspect oh_qwen_live_setup in scripts/e2e-lib.sh and rerun node scripts/check-qwen-request-trace.mjs");
	assert.match(conversionError, /qwen: could not convert .*qwen-request-trace.cjs.*check cygpath/, "missing conversion diagnostic; inspect oh_qwen_live_setup in scripts/e2e-lib.sh and rerun node scripts/check-qwen-request-trace.mjs");
}

const server = createServer((request, response) => {
	if (request.url === "/ok") response.end("ok");
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
try {
	const origin = `http://127.0.0.1:${server.address().port}`;
	const child = spawn("bash", ["-c", 'set -euo pipefail; source "$1/scripts/e2e-lib.sh"; unset OH_TIMEOUT; oh_qwen_live_setup; [ "$OH_TIMEOUT" = 300 ]; OH_TIMEOUT=7; oh_qwen_live_setup; [ "$OH_TIMEOUT" = 7 ]; exec "$2" --input-type=module -e "$3"', "_", root, process.execPath, `
		const headers = { Authorization: "Bearer PRIVATE_SENTINEL" };
		const response = await fetch("${origin}/ok", { headers });
		if (await response.text() !== "ok") process.exit(2);
		try {
			await fetch("${origin}/stall", { headers, signal: AbortSignal.timeout(100) });
			process.exit(3);
		} catch (error) {
			if (error.name !== "TimeoutError") throw error;
		}
	`], { stdio: ["ignore", "pipe", "pipe"] });
	let stderr = "";
	child.stderr.setEncoding("utf8").on("data", (chunk) => { stderr += chunk; });
	const code = await new Promise((resolve, reject) => {
		child.on("error", reject);
		child.on("close", resolve);
	});
	assert.equal(code, 0, `request trace fixture failed; rerun node scripts/check-qwen-request-trace.mjs: ${stderr}`);
	assert.match(stderr, /id=1 event=start at=\d{4}-/, "missing request start timestamp; inspect scripts/qwen-request-trace.cjs and rerun node scripts/check-qwen-request-trace.mjs");
	assert.match(stderr, /id=1 event=headers:200 .*elapsed_ms=\d+/, "missing successful response timing; inspect scripts/qwen-request-trace.cjs and rerun node scripts/check-qwen-request-trace.mjs");
	assert.match(stderr, /id=2 event=start /, "missing stalled request start; inspect scripts/qwen-request-trace.cjs and rerun node scripts/check-qwen-request-trace.mjs");
	assert.match(stderr, /id=2 event=error .*elapsed_ms=\d+/, "missing aborted request timing; inspect scripts/qwen-request-trace.cjs and rerun node scripts/check-qwen-request-trace.mjs");
	assert.doesNotMatch(stderr, /PRIVATE_SENTINEL|Authorization|\/stall|\/ok/, "request trace leaked sensitive request data; inspect scripts/qwen-request-trace.cjs and rerun node scripts/check-qwen-request-trace.mjs");
} finally {
	server.closeAllConnections();
	await new Promise((resolve) => server.close(resolve));
}
