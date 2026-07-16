const mode = process.env.SDK_FIXTURE_MODE;
if (mode === "list") {
	process.stdout.write('{"schema_version":"1","harnesses":[{"id":42}]}');
} else if (mode === "detect") {
	process.stdout.write('{"schema_version":"1","detected":[{"id":42}]}');
} else if (mode === "run") {
	process.stdout.write('{"schema_version":"1","results":[{"usage":{"input_tokens":"many"}}]}');
} else if (mode === "history") {
	process.stdout.write('[{"schema_version":"1","usage":{"input_tokens":"many"}}]');
} else if (mode === "history-list") {
	process.stdout.write('[{"id":42}]');
} else {
	process.stderr.write(`unknown SDK_FIXTURE_MODE: ${mode ?? "unset"}`);
	process.exitCode = 2;
}
