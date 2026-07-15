const mode = process.env.SDK_FIXTURE_MODE;
if (mode === "invalid-json") process.stdout.write("not json");
else if (mode === "invalid-contract") process.stdout.write("{}");
else if (mode === "fail") {
  process.stderr.write("fixture failure");
  process.exitCode = 7;
}
