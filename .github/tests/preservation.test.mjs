import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { parse } from "yaml";
import { isDeepStrictEqual } from "node:util";
import { assertRevision } from "../git-input.mjs";
import { workflows, validateWorkflow, triggered, resolveJobs, expand, render, evaluate } from "../workflow-graph.mjs";

// The tree this change merges into supplies the actual workflow declarations,
// rather than a hand-maintained inventory that could forget a retained lane.
// A fixed commit would read every lane main gained since as a removal.
// llmlint: ignore-block[shell_test_tiers_stay_split] Reading the required immutable workflow baseline uses local Git only; no fixture history, installation, provider process or network is involved.
function baseRef() {
  const base = assertRevision(process.env.OH_WORKFLOW_BASELINE ?? "origin/main", "OH_WORKFLOW_BASELINE");
  try {
    return execFileSync("git", ["merge-base", "HEAD", base], { encoding: "utf8" }).trim();
  } catch (error) {
    throw Error(`Cannot find the merge base of HEAD and ${base}. Next: fetch origin/main with full history, or set OH_WORKFLOW_BASELINE to the base ref.`, { cause: error });
  }
}
const baselineRef = baseRef();
function baseline() {
  try {
    const files = execFileSync("git", ["ls-tree", "--full-tree", "--name-only", `${baselineRef}:.github/workflows`], { encoding: "utf8" }).trim().split("\n");
    return Object.fromEntries(files.filter(file => file.endsWith(".yml")).map(file => [file,
      validateWorkflow(parse(execFileSync("git", ["show", `${baselineRef}:.github/workflows/${file}`], { encoding: "utf8" })), file),
    ]));
  } catch (error) {
    throw Error(`Cannot read pre-change workflow tree ${baselineRef}. Next: fetch the repository's full history.`, { cause: error });
  }
}
// llmlint: ignore-end[shell_test_tiers_stay_split]
const dependencies = job => typeof job.needs === "string" ? [job.needs] : job.needs ?? [];
const laneKey = row => JSON.stringify([row.job, row.name, row.runner, row.target]);
const retained = rows => rows.filter(row => /^(ubuntu-|windows-)/.test(row.runner));
const pathExample = pattern => pattern.replace(/^!/, "").replaceAll("**", "contract/fixture").replaceAll("*", "fixture").replaceAll("?", "x");
function* scenarios(workflow) {
  for (const [event, config] of Object.entries(workflow.on)) {
    const paths = ["README.md", "src/commands/interrupt.rs", ...(config?.paths ?? []).map(pathExample), ...(config?.["paths-ignore"] ?? []).map(pathExample)];
    const actions = config?.types ?? (event === "pull_request" ? ["opened", "synchronize", "reopened"] : event === "release" ? ["published"] : [""]);
    const inputs = event === "workflow_dispatch" ? Object.entries(config?.inputs ?? {}) : [];
    let choices = [{}];
    for (const [name, input] of inputs) choices = choices.flatMap(choice => (input.options ?? [input.default]).map(value => ({ ...choice, [name]: value })));
    for (const action of actions)
      for (const branch of event === "push" ? ["main", "not-selected", "v0.23.1"] : ["main"])
        for (const path of event === "pull_request" ? [...new Set(paths)] : ["src/commands/interrupt.rs"])
          for (const fork of event === "pull_request" ? [false, true] : [false])
            for (const publish of event === "release" ? ["off", "npm", "pypi", "both"] : ["off"])
              for (const input of choices) yield {
                event, action: event === "push" && branch.startsWith("v") ? "tag" : action,
                branch, paths: [path], fork, publish, manual: false,
                macosRequired: event === "pull_request", releaseOnly: false, input,
              };
  }
}
function context(workflow, job, scenario, matrix = {}, proof = {}) {
  const needs = Object.fromEntries(dependencies(job).map(id => [id, {
    result: scenario.outcomes?.[id]?.result ?? "success",
    outputs: { enabled: "true", needs_check: "false", macos_required: String(scenario.macosRequired), is_release_only: "false" },
  }]));
  return {
    github: { event_name: scenario.event, repository: "nickderobertis/oneharness", ref: `refs/heads/${scenario.branch}`,
      event: { pull_request: { head: { repo: { full_name: scenario.fork ? "fork/repo" : "nickderobertis/oneharness" } } } } },
    secrets: Object.fromEntries([...JSON.stringify(workflow).matchAll(/secrets\.([A-Za-z0-9_]+)/g)].map(([, name]) => [name, `secret-binding:${name}`])),
    inputs: { ...Object.fromEntries(Object.entries(workflow.on.workflow_dispatch?.inputs ?? {}).map(([key, value]) => [key, value.default])), ...scenario.input },
    needs, matrix, runner: { os: matrix.os?.startsWith("windows-") ? "Windows" : "Linux" },
    vars: { NPM_PUBLISH: String(["npm", "both"].includes(scenario.publish)), PYPI_PUBLISH: String(["pypi", "both"].includes(scenario.publish)) },
    steps: { tier: { outputs: { tier: proof.tier ?? "affected" } }, "shell-tools": { outputs: { "cache-hit": proof.cacheHit ?? "false" } }, verdict: { outputs: { needs_check: proof.needsCheck ?? "false" } } },
  };
}
function matrixFor(workflow, row, scenario) {
  const job = workflow.jobs[row.job];
  assert.ok(job && !job.uses, `Unresolved retained job ${row.job}; reusable lanes need explicit comparison`);
  const initial = context(workflow, job, scenario);
  const candidates = expand(job.strategy?.matrix, initial).filter(matrix => {
    const local = context(workflow, job, scenario, matrix);
    const name = job.name ? render(job.name, local) : row.job + (Object.keys(matrix).length ? ` (${Object.values(matrix).join(", ")})` : "");
    return render(job["runs-on"], local) === row.runner && name === row.name && (matrix.target ?? "") === row.target;
  });
  assert.equal(candidates.length, 1, `Unresolved/duplicate matrix for ${laneKey(row)}`);
  return candidates[0];
}
function stepSignature(step, file, jobId) {
  const { if: condition, ...signature } = step;
  if (signature.run) signature.run = signature.run.split("\n").filter(line => !line.trim().startsWith("#")).join("\n");
  if (file === "e2e-control.yml" && jobId === "report" && signature.env?.BODY) {
    // The notification now describes Linux scheduling and the relocated Mac
    // proof; its command, credentials, URL and failure dependency must remain.
    signature.env = { ...signature.env, BODY: "turn-control platform notification" };
  }
  return signature;
}
function compareSteps(file, before, after, row, scenario) {
  const oldJob = before.jobs[row.job], newJob = after.jobs[row.job];
  const oldMatrix = matrixFor(before, row, scenario), newMatrix = matrixFor(after, row, scenario);
  const proofs = row.job === "check" && file === "ci.yml"
    ? ["affected", "all"].flatMap(tier => ["true", "false"].map(cacheHit => ({ tier, cacheHit })))
    : row.job === "gate" && file === "release.yml" ? [{ needsCheck: "true" }, { needsCheck: "false" }] : [{}];
  for (const proof of proofs) {
    const oldContext = context(before, oldJob, scenario, oldMatrix, proof), newContext = context(after, newJob, scenario, newMatrix, proof);
    oldContext.runner.os = newContext.runner.os = row.runner.startsWith("windows-") ? "Windows" : "Linux";
    for (const [key, value] of Object.entries(oldJob.env ?? {})) {
      assert.deepEqual(newJob.env?.[key], value, `${file}/${row.name}: changed environment binding ${key}`);
      assert.deepEqual(render(newJob.env?.[key], newContext), render(value, oldContext), `${file}/${row.name}: changed environment selector ${key}`);
    }
    const selected = (job, local) => (job.steps ?? []).filter(step => !step.if || evaluate(step.if, local));
    const current = selected(newJob, newContext).map(step => stepSignature(step, file, row.job));
    let offset = 0;
    for (const step of selected(oldJob, oldContext)) {
      const signature = stepSignature(step, file, row.job);
      const index = current.findIndex((candidate, position) => position >= offset && isDeepStrictEqual(candidate, signature));
      assert.ok(index >= 0, `${file}/${row.name}: removed or changed retained assertion/install step ${step.name ?? step.run ?? step.uses}`);
      offset = index + 1;
    }
  }
}
function compare(before, after) {
  const visited = new Set();
  let selectedPaths = 0, excludedPaths = 0, outcomeJourneys = 0;
  for (const [file, oldWorkflow] of Object.entries(before)) {
    const newWorkflow = after[file];
    assert.ok(newWorkflow, `Removed retained workflow ${file}`);
    if (oldWorkflow.defaults) assert.deepEqual(newWorkflow.defaults, oldWorkflow.defaults, `${file}: changed workflow execution defaults`);
    for (const [event, config] of Object.entries(oldWorkflow.on)) {
      assert.ok(event in newWorkflow.on, `${file}: removed trigger ${event}`);
      if (Array.isArray(config)) assert.deepEqual(newWorkflow.on[event], config, `${file}: changed schedule declarations`);
      for (const key of ["types", "branches", "branches-ignore", "tags", "tags-ignore", "paths-ignore", "workflows"])
        if (config?.[key] !== undefined) assert.deepEqual(newWorkflow.on[event]?.[key], config[key], `${file}: changed ${event}/${key} filter`);
      let position = 0;
      for (const path of config?.paths ?? []) {
        const index = (newWorkflow.on[event]?.paths ?? []).indexOf(path, position);
        assert.ok(index >= 0, `${file}: removed/reordered retained path filter ${path}`);
        position = index + 1;
      }
      for (const [name, input] of Object.entries(config?.inputs ?? {}))
        for (const key of ["type", "default", "options", "required"])
          if (input[key] !== undefined) assert.deepEqual(newWorkflow.on[event]?.inputs?.[name]?.[key], input[key], `${file}: changed diagnostic input ${name}/${key}`);
    }
    for (const [id, oldJob] of Object.entries(oldWorkflow.jobs)) {
      assert.ok(newWorkflow.jobs[id], `Removed retained job ${file}/${id}`);
      assert.equal(newWorkflow.jobs[id]["continue-on-error"] ?? false, oldJob["continue-on-error"] ?? false, `${file}/${id}: changed failure enforcement`);
      if (oldJob.defaults) assert.deepEqual(newWorkflow.jobs[id].defaults, oldJob.defaults, `${file}/${id}: changed execution defaults`);
      if (oldJob.permissions) assert.deepEqual(newWorkflow.jobs[id].permissions, oldJob.permissions, `${file}/${id}: changed installation/publication permissions`);
      assert.equal(newWorkflow.jobs[id].strategy?.["fail-fast"] ?? true, oldJob.strategy?.["fail-fast"] ?? true, `${file}/${id}: changed matrix failure propagation`);
      for (const dependency of dependencies(oldJob))
        assert.ok(dependencies(newWorkflow.jobs[id]).includes(dependency), `${file}/${id}: removed aggregate dependency ${dependency}`);
    }
    for (const scenario of scenarios(oldWorkflow)) {
      const oldSelected = triggered(oldWorkflow, scenario), newSelected = triggered(newWorkflow, scenario);
      assert.equal(newSelected, oldSelected, `${file}: changed retained event/path selection ${JSON.stringify(scenario)}`);
      if (scenario.event === "pull_request") oldSelected ? selectedPaths++ : excludedPaths++;
      if (!oldSelected) continue;
      const originalRows = retained(resolveJobs(file, scenario, before, [], scenario.input));
      const currentRows = retained(resolveJobs(file, scenario, after, [], scenario.input));
      const currentKeys = new Set(currentRows.map(laneKey));
      for (const row of originalRows) {
        assert.ok(currentKeys.has(laneKey(row)), `${file}: removed retained Linux/Windows lane ${laneKey(row)}`);
        compareSteps(file, oldWorkflow, newWorkflow, row, scenario);
        visited.add(`${file}/${laneKey(row)}`);
      }
      // Evaluate each prerequisite's failure/cancellation against the real job
      // conditions. An aggregate cannot silently stop observing a failed leg.
      const prerequisites = new Set(Object.values(oldWorkflow.jobs).flatMap(dependencies));
      for (const prerequisite of prerequisites) for (const result of ["failure", "cancelled"]) {
        const failed = { ...scenario, outcomes: { [prerequisite]: { result } } };
        const oldRows = new Set(retained(resolveJobs(file, failed, before, [], scenario.input)).map(laneKey));
        const newRows = new Set(retained(resolveJobs(file, failed, after, [], scenario.input)).map(laneKey));
        for (const row of originalRows) assert.equal(newRows.has(laneKey(row)), oldRows.has(laneKey(row)), `${file}/${row.name}: changed prerequisite ${prerequisite}/${result} propagation`);
        for (const row of retained(resolveJobs(file, failed, before, [], scenario.input))) {
          assert.ok(newRows.has(laneKey(row)), `${file}: lost failure/cancellation aggregate ${row.name}`);
          compareSteps(file, oldWorkflow, newWorkflow, row, failed);
          visited.add(`${file}/${laneKey(row)}`);
        }
        outcomeJourneys++;
      }
    }
  }
  assert.ok(selectedPaths && excludedPaths && outcomeJourneys, "Preservation proof must exercise selected/excluded paths and prerequisite outcomes");
  return { lanes: visited.size, selectedPaths, excludedPaths, outcomeJourneys };
}
// llmlint: ignore-block[shell_test_tiers_stay_split] The required offline preservation contract reads an immutable Git workflow tree and evaluates actual Actions expressions; it runs no installs, harnesses, model calls or network operations.
test("pre-change Linux/Windows assertions, installation, routing and aggregate dependencies survive the finished workflow graph", () => {
  const evidence = compare(baseline(), workflows());
  console.log(`Linux/Windows preservation: ${JSON.stringify(evidence)}; baseline ${baselineRef}`);
});
test("preservation rejects a removed live assertion and a removed aggregate dependency", () => {
  const original = baseline();
  const current = workflows();
  const missingAssertion = structuredClone(current);
  assert.ok(missingAssertion["e2e-codex.yml"].jobs.live.steps.some(step => step.run === "just live-codex"));
  missingAssertion["e2e-codex.yml"].jobs.live.steps = missingAssertion["e2e-codex.yml"].jobs.live.steps.filter(step => step.run !== "just live-codex");
  assert.throws(() => compare(original, missingAssertion), /removed or changed retained assertion\/install step Live e2e against the real CLI/);
  const missingDependency = structuredClone(current);
  delete missingDependency["e2e-control.yml"].jobs.report.needs;
  assert.throws(() => compare(original, missingDependency), /removed aggregate dependency live/);
});
// llmlint: ignore-end[shell_test_tiers_stay_split]
