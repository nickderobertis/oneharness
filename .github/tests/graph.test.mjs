import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, writeFileSync } from "node:fs";
import {
  workflows,
  resolveJobs,
  triggered,
  digest,
  evaluate,
  expand,
  validateWorkflow,
} from "../workflow-graph.mjs";
import { history } from "./history.mjs";
const classes = [
  ["pull_request", "opened"],
  ["pull_request", "synchronize"],
  ["pull_request", "reopened"],
  ["push", "base"],
  ["push", "release-merge"],
  ["push", "tag"],
  ["release", "published"],
  ["workflow_run", "completed"],
  ["repository_dispatch", "dispatch"],
  ["schedule", "cron"],
  ["workflow_dispatch", "default"],
  ["workflow_dispatch", "diagnostic"],
];
const all = workflows();
test("resolve actual workflow graph for every event/diff/ownership/publish route and record telemetry", () => {
  const scenarios = [];
  const fixture = history();
  const classifications = new Map();
  try {
    for (const diff of ["source", "version-only", "dependency-content"])
      for (const fork of [false, true])
        classifications.set(`${diff}/${fork}`, fixture.classify(diff, fork));

    for (const [event, action] of classes)
      for (const diff of ["source", "version-only", "dependency-content"])
        for (const fork of [false, true])
          for (const publish of ["off", "npm", "pypi", "both"]) {
            const classification = classifications.get(`${diff}/${fork}`);
            const releaseOnly =
              event === "pull_request" && classification.isReleaseOnly;
            const scenario = {
              event,
              action,
              diff,
              fork,
              publish,
              branch: action === "tag" ? "v0.22.1" : "main",
              manual: action === "diagnostic",
              releaseOnly,
              macosRequired:
                event === "pull_request" && classification.macosRequired,
              paths:
                diff === "source"
                  ? ["src/commands/interrupt.rs"]
                  : diff === "version-only"
                    ? ["Cargo.toml", "CHANGELOG.md"]
                    : ["Cargo.lock"],
            };
            const runs = Object.entries(all)
              .filter(([, w]) => triggered(w, scenario))
              .map(([file]) => ({
                workflow: file,
                jobs: resolveJobs(file, scenario, all),
              }));
            for (const run of runs)
              for (const row of run.jobs.filter((j) => j.macos)) {
                if (!scenario.manual)
                  assert.ok(
                    ["ci.yml", "release.yml"].includes(run.workflow),
                    `automatic Mac outside CI/release: ${JSON.stringify(row)}`,
                  );
                if (event === "pull_request") assert.equal(releaseOnly, false);
                if (
                  !scenario.manual &&
                  !["pull_request", "release"].includes(event)
                )
                  assert.fail(`Mac requested by ${event}`);
              }
            if (event === "pull_request") {
              const names = runs.flatMap((r) => r.jobs.map((j) => j.name));
              for (const name of [
                "check (macos-latest)",
                "check (ubuntu-latest)",
                "check (windows-latest)",
                "deny",
                "llmlint",
                "pr-title",
              ])
                assert.equal(
                  names.filter((n) => n === name).length,
                  1,
                  `required context ${name}`,
                );
            }
            scenarios.push({ ...scenario, runs });
          }
    const lifecycle = [];
    for (const diff of ["source", "version-only", "dependency-content"])
      for (const fork of [false, true])
        for (const publish of ["off", "npm", "pypi", "both"]) {
          const keys = [
            ["pull_request", "synchronize", diff, fork],
            ["push", "base", diff, false],
            ["pull_request", "synchronize", "version-only", false],
            ["push", "release-merge", "version-only", false],
            ["push", "tag", "version-only", false],
            ["release", "published", "version-only", false],
            ["workflow_run", "completed", "version-only", false],
            ["schedule", "cron", diff, false],
          ];
          const runs = keys.flatMap(([event, action, d, f]) =>
            scenarios
              .find(
                (s) =>
                  s.event === event &&
                  s.action === action &&
                  s.diff === d &&
                  s.fork === f &&
                  s.publish === publish,
              )
              .runs.filter((r) => r.jobs.some((j) => j.macos))
              .map((r) => ({ event, action, ...r })),
          );
          lifecycle.push({ diff, fork, publish, runs });
        }
    mkdirSync(".github/.telemetry", { recursive: true });
    writeFileSync(
      ".github/.telemetry/macos-policy.json",
      JSON.stringify(
        {
          schema_version: 1,
          sourceDigest: digest(),
          scenarios,
          lifecycle,
          jobsPerRun: Math.max(
            ...scenarios.flatMap((s) =>
              s.runs.map((r) => r.jobs.filter((j) => j.macos).length),
            ),
          ),
          runsPerChange: Math.max(...lifecycle.map((l) => l.runs.length)),
        },
        null,
        2,
      ) + "\n",
    );
  } finally {
    fixture.dispose();
  }
});
test("published evaluator expands matrices and refuses unresolved reusable/runner expressions", () => {
  assert.throws(
    () =>
      evaluate("needs.guard.outputs.unresolved == 'true'", {
        needs: { guard: { outputs: {} } },
      }),
    /Unresolved job output/,
  );
  assert.equal(
    evaluate("always() && needs.policy.result == 'success'", {
      needs: { policy: { result: "failure" } },
    }),
    false,
  );
  assert.deepEqual(
    expand({ include: [{ target: "a" }, { target: "b" }] }, {}),
    [{ target: "a" }, { target: "b" }],
  );
  assert.equal(
    expand({ os: ["ubuntu-latest", "windows-latest"], target: ["a", "b"] }, {})
      .length,
    4,
  );
  assert.deepEqual(
    resolveJobs(
      "caller.yml",
      { event: "push", branch: "main" },
      {
        "caller.yml": {
          jobs: {
            call: {
              uses: "./.github/workflows/reusable.yml",
              with: { runner: "macos-latest" },
            },
          },
        },
        "reusable.yml": {
          on: { workflow_call: { inputs: { runner: { type: "string" } } } },
          jobs: { nested: { "runs-on": "${{ inputs.runner }}" } },
        },
      },
    ).map((r) => [r.job, r.runner]),
    [["call/nested", "macos-latest"]],
  );
  assert.throws(
    () =>
      resolveJobs(
        "bad.yml",
        { event: "push" },
        { "bad.yml": { jobs: { bad: { "runs-on": "${{ inputs.runner }}" } } } },
      ),
    /Unresolved runner/,
  );
  assert.throws(
    () =>
      resolveJobs(
        "bad.yml",
        { event: "push" },
        {
          "bad.yml": {
            jobs: {
              bad: { uses: "owner/repo/.github/workflows/build.yml@main" },
            },
          },
        },
      ),
    /Unresolved reusable/,
  );
});

test("matrix include/exclude and event filters follow actual Actions data semantics", () => {
  assert.deepEqual(
    expand(
      {
        os: ["ubuntu", "windows"],
        tool: ["cargo", "npm"],
        exclude: [{ os: "windows", tool: "npm" }],
        include: [
          { os: "ubuntu", audit: "yes" },
          { os: "macos", tool: "cargo" },
        ],
      },
      {},
    ),
    [
      { os: "ubuntu", tool: "cargo", audit: "yes" },
      { os: "ubuntu", tool: "npm", audit: "yes" },
      { os: "windows", tool: "cargo" },
      { os: "macos", tool: "cargo" },
    ],
  );
  const scenario = {
    event: "push",
    branch: "main",
    paths: ["src/main.rs"],
    action: "base",
  };
  assert.equal(triggered({ on: "push" }, scenario), true);
  assert.equal(triggered({ on: ["push", "pull_request"] }, scenario), true);
  assert.equal(
    triggered({ on: { push: { "paths-ignore": ["src/**"] } } }, scenario),
    false,
  );
  assert.equal(
    triggered(
      { on: { push: { paths: ["src/**", "!src/main.rs"] } } },
      scenario,
    ),
    false,
  );
  assert.equal(
    triggered(
      {
        on: { workflow_run: { workflows: ["Release"], types: ["completed"] } },
      },
      {
        ...scenario,
        event: "workflow_run",
        action: "completed",
        upstream: "CI",
      },
    ),
    false,
  );
  assert.equal(
    triggered(
      {
        on: { workflow_run: { workflows: ["Release"], types: ["completed"] } },
      },
      {
        ...scenario,
        event: "workflow_run",
        action: "completed",
        upstream: "Release",
      },
    ),
    true,
  );
});

test("real budget commands report existing graph telemetry and reject stale or malformed inputs", async () => {
  const { mkdtempSync, rmSync, existsSync, readFileSync } = await import(
    "node:fs"
  );
  const { join, resolve } = await import("node:path");
  const { tmpdir } = await import("node:os");
  const { spawnSync } = await import("node:child_process");
  const dir = mkdtempSync(
    join(
      process.env.ONEPIPELINE_NODE_SCRATCH_DIR ?? tmpdir(),
      "budget-reporter-",
    ),
  );
  try {
    const actual = JSON.parse(
      readFileSync(".github/.telemetry/macos-policy.json", "utf8"),
    );
    const file = join(dir, "telemetry.json"),
      result = join(dir, "report.json");
    for (const [metric, key] of [
      ["jobs-per-run", "jobsPerRun"],
      ["runs-per-change", "runsPerChange"],
    ]) {
      writeFileSync(file, JSON.stringify(actual));
      const run = spawnSync(
        process.execPath,
        [".github/macos-budget.mjs", "--metric", metric, "--telemetry", file],
        {
          encoding: "utf8",
          env: { ...process.env, ONEBUDGETSPEC_RESULT: result },
        },
      );
      assert.equal(run.status, 0, run.stderr);
      const report = JSON.parse(readFileSync(result, "utf8"));
      assert.equal(report.value, actual[key]);
      assert.ok(
        JSON.parse(report.detail).scenarios.some((s) =>
          s.runs.some((r) => r.jobs.some((j) => j.macos)),
        ),
      );
    }
    for (const changed of [
      { sourceDigest: "0".repeat(64) },
      { jobsPerRun: -1 },
      {
        scenarios: [{ event: "push", runs: [{ workflow: "bad", jobs: null }] }],
      },
    ]) {
      rmSync(result, { force: true });
      writeFileSync(file, JSON.stringify({ ...actual, ...changed }));
      const run = spawnSync(
        process.execPath,
        [
          ".github/macos-budget.mjs",
          "--metric",
          "jobs-per-run",
          "--telemetry",
          file,
        ],
        {
          encoding: "utf8",
          env: { ...process.env, ONEBUDGETSPEC_RESULT: result },
        },
      );
      assert.notEqual(run.status, 0);
      assert.match(run.stderr, /Stale Mac telemetry|Malformed Mac telemetry/);
      assert.equal(existsSync(result), false);
    }
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});


test("workflow boundaries and producer outcomes refuse unproven outputs", () => {
  for (const bad of [null, [], {}, {on: {pull_request: {paths: [3]}}, jobs: {job: {"runs-on": "ubuntu-latest"}}}])
    assert.throws(() => validateWorkflow(bad, "bad.yml"), /Invalid/);
  const scenario = {event: "pull_request", branch: "main", macosRequired: true};
  for (const result of ["failure", "cancelled", "skipped"])
    assert.throws(() => resolveJobs("ci.yml", {...scenario, outcomes: {"macos-policy": {result}}}, all), /Unresolved job output/);
  const forged = structuredClone(all);
  forged["ci.yml"].jobs["macos-policy"].outputs.unproven = "${{ steps.any.outputs.unproven }}";
  assert.throws(() => resolveJobs("ci.yml", scenario, forged), /Unresolved producer output/);
});
