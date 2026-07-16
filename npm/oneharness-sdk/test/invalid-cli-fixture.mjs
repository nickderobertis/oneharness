import { spawnSync } from "node:child_process";

const mode = process.env.SDK_FIXTURE_MODE;
if (mode === "additive") {
	const completed = spawnSync(
		process.env.SDK_REAL_CLI,
		process.argv.slice(2),
		{ encoding: "utf8", env: process.env },
	);
	process.stderr.write(completed.stderr ?? "");
	for (const line of (completed.stdout ?? "").split(/\r?\n/u)) {
		if (line.trim() === "") continue;
		const value = JSON.parse(line);
		value.future_output_field = { preserved: true };
		if (value.results?.[0]) value.results[0].future_result_field = 7;
		if (value.harnesses?.[0]) value.harnesses[0].future_harness_field = 7;
		process.stdout.write(`${JSON.stringify(value)}\n`);
	}
	if (completed.error) throw completed.error;
	process.exitCode = completed.status ?? 1;
} else if (mode === "list") {
	process.stdout.write('{"schema_version":"1","harnesses":[{"id":42}]}');
} else if (mode === "detect") {
	process.stdout.write('{"schema_version":"1","detected":[{"id":42}]}');
} else if (mode === "run") {
	process.stdout.write('{"schema_version":"1","results":[{"usage":{"input_tokens":"many"}}]}');
} else if (mode === "history") {
	process.stdout.write('[{"schema_version":"1","usage":{"input_tokens":"many"}}]');
} else if (mode === "history-list") {
	process.stdout.write('[{"id":42}]');
} else if (mode === "run-stream") {
	process.stdout.write('{"type":"event","event":{}}\n');
} else if (mode === "history-watch") {
	process.stdout.write('{"type":"record","record":{}}\n');
} else {
	process.stderr.write(`unknown SDK_FIXTURE_MODE: ${mode ?? "unset"}`);
	process.exitCode = 2;
}
