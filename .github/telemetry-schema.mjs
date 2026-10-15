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
    if (!object(s) || !["event","action","diff","publish","branch"].every(k=>string(s[k])) || !["source","version-only","dependency-content"].includes(s.diff) || !["off","npm","pypi","both"].includes(s.publish) || !["fork","manual","releaseOnly","macosRequired"].every(k=>typeof s[k]==="boolean") || !Array.isArray(s.runs) || (s.automation !== undefined && typeof s.automation !== "boolean")) bad();
    s.runs.forEach(run);
  }
  for (const l of value.lifecycle) {
    if (!object(l) || !["source","version-only","dependency-content"].includes(l.diff) || !["off","npm","pypi","both"].includes(l.publish) || typeof l.fork !== "boolean" || !Array.isArray(l.runs)) bad();
    for (const r of l.runs) if (!object(r) || !string(r.event) || !string(r.action)) bad();
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
