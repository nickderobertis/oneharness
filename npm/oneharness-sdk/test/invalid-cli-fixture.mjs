const mode = process.env.SDK_FIXTURE_MODE;
if (mode === "list") {
	process.stdout.write('{"schema_version":"1","harnesses":[{"id":42}]}');
} else if (mode === "detect") {
	process.stdout.write('{"schema_version":"1","detected":[{"id":42}]}');
} else {
	process.stderr.write(`unknown SDK_FIXTURE_MODE: ${mode ?? "unset"}`);
	process.exitCode = 2;
}
