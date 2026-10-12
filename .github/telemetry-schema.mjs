export function assertTelemetry(value) {
  const bad = () => {
    throw Error(
      "Malformed Mac telemetry. Next: run node --test .github/tests/*.test.mjs to record the actual workflow graph.",
    );
  };
  const object = (x) => x && typeof x === "object" && !Array.isArray(x);
  const count = (x) => Number.isSafeInteger(x) && x >= 0;
  const string = (x) => typeof x === "string";
  const run = (r) => {
    if (!object(r) || !string(r.workflow) || !Array.isArray(r.jobs)) bad();
    for (const j of r.jobs)
      if (
        !object(j) ||
        !["workflow", "event", "job", "name", "runner", "target"].every((k) =>
          string(j[k]),
        ) ||
        typeof j.macos !== "boolean" ||
        j.macos !== j.runner.startsWith("macos-") ||
        !Array.isArray(j.architectures) ||
        !j.architectures.every(string)
      )
        bad();
  };
  if (
    !object(value) ||
    value.schema_version !== 1 ||
    typeof value.sourceDigest !== "string" ||
    !/^[a-f0-9]{64}$/.test(value.sourceDigest) ||
    !count(value.jobsPerRun) ||
    !count(value.runsPerChange) ||
    !Array.isArray(value.scenarios) ||
    !value.scenarios.length ||
    !Array.isArray(value.lifecycle) ||
    !value.lifecycle.length
  )
    bad();
  for (const s of value.scenarios) {
    if (!object(s) || !string(s.event) || !Array.isArray(s.runs)) bad();
    s.runs.forEach(run);
  }
  for (const l of value.lifecycle) {
    if (!object(l) || !Array.isArray(l.runs)) bad();
    l.runs.forEach(run);
  }
  if (
    value.jobsPerRun !==
      Math.max(
        ...value.scenarios.flatMap((s) =>
          s.runs.map((r) => r.jobs.filter((j) => j.macos).length),
        ),
      ) ||
    value.runsPerChange !==
      Math.max(...value.lifecycle.map((l) => l.runs.length))
  )
    bad();
  return value;
}
