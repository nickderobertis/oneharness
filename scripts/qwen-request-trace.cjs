// Qwen-only preload: retain request timing after an outer deadline, without
// logging URLs, headers, credentials, prompts, or response bodies.
const { subscribe } = require("node:diagnostics_channel");
const requests = new WeakMap();
let nextId = 0;
function trace(request, event) {
	const state = requests.get(request);
	if (!state) return;
	process.stderr.write(
		`qwen-request id=${state.id} event=${event} at=${new Date().toISOString()} elapsed_ms=${Date.now() - state.started}\n`,
	);
}
subscribe("undici:request:create", ({ request }) => {
	requests.set(request, { id: ++nextId, started: Date.now() });
	trace(request, "start");
});
subscribe("undici:request:headers", ({ request, response }) => {
	trace(request, `headers:${response.statusCode}`);
});
subscribe("undici:request:error", ({ request }) => trace(request, "error"));
