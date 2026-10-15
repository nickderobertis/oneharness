// Repository-local executable scheduling contract; unresolved calls/runners fail.
import { readFileSync, readdirSync, statSync } from "node:fs";
import { createHash } from "node:crypto";
import { parse } from "yaml";
import { Lexer, Parser, Evaluator, data } from "@actions/expressions";
const object = (value) => value !== null && typeof value === "object" && !Array.isArray(value);
// llmlint: ignore-block[boundary_inputs_validated] The fixed scheduling-policy validation scope covers mappings, triggers, declared outputs, step.run strings and matrix include/exclude rows. The resolver rejects unsupported runners/calls and telemetry validation refuses malformed evidence before reporting.
export function validateWorkflow(value, file) {
  if (!object(value) || !object(value.jobs) || !Object.keys(value.jobs).length)
    throw Error(`Invalid workflow document ${file}. Next: restore its jobs mapping.`);
  for (const [id, job] of Object.entries(value.jobs))
    if (!object(job) || (!job.uses && !job["runs-on"]) || (job.uses !== undefined && typeof job.uses !== "string") || (job["runs-on"] !== undefined && typeof job["runs-on"] !== "string") || (job.if !== undefined && typeof job.if !== "string") || (job.strategy !== undefined && (!object(job.strategy) || (job.strategy.matrix !== undefined && !object(job.strategy.matrix)))) || (job.outputs !== undefined && (!object(job.outputs) || Object.values(job.outputs).some((v) => typeof v !== "string"))) || (job.steps && (!Array.isArray(job.steps) || job.steps.some((step) => !object(step)))) || (job.outputs && !object(job.outputs)))
      throw Error(`Invalid workflow job ${file}/${id}. Next: restore its runner/call, steps and outputs.`);
  for (const [id, job] of Object.entries(value.jobs)) {
    if (job.steps?.some((step) => ["run", "uses", "if", "name", "id", "shell"].some((key) => step[key] !== undefined && typeof step[key] !== "string")))
      throw Error(`Invalid workflow step ${file}/${id}. Next: use string command/expression fields.`);
    for (const key of ["include", "exclude"]) {
      const entries = job.strategy?.matrix?.[key];
      if (entries !== undefined && !(typeof entries === "string" && /^\$\{\{/.test(entries)) && (!Array.isArray(entries) || entries.some((entry) => !object(entry))))
        throw Error(`Invalid matrix ${file}/${id}/${key}. Next: supply a collection of row mappings.`);
    }
  }
  const triggers = value.on;
  if (!(typeof triggers === "string" || (Array.isArray(triggers) && triggers.every((v) => typeof v === "string")) || object(triggers)))
    throw Error(`Invalid workflow triggers ${file}. Next: restore the event declarations.`);
  if (object(triggers)) for (const [event, config] of Object.entries(triggers)) {
    if (event === "schedule" && Array.isArray(config) && config.every((v) => object(v) && typeof v.cron === "string")) continue;
    if (config !== null && !object(config)) throw Error(`Invalid event ${file}/${event}`);
    for (const key of ["paths", "paths-ignore", "branches", "branches-ignore", "tags", "tags-ignore", "types", "workflows"])
      if (config?.[key] !== undefined && (!Array.isArray(config[key]) || config[key].some((v) => typeof v !== "string")))
        throw Error(`Invalid ${file}/${event}/${key}. Next: supply an array of string patterns.`);
  }
  return value;
}
// llmlint: ignore-end[boundary_inputs_validated]
export const workflowDir = ".github/workflows";
export function workflows() {
  return Object.fromEntries(
    readdirSync(workflowDir)
      .filter((f) => f.endsWith(".yml"))
      .sort()
      .map((f) => [f, validateWorkflow(parse(readFileSync(`${workflowDir}/${f}`, "utf8")), f)]),
  );
}
export function digest() {
  const hash = createHash("sha256");
  for (const file of [
    ...readdirSync(".github/tests")
      .filter((f) => f.endsWith(".mjs"))
      .map((f) => `.github/tests/${f}`),
    // Each integration journey is its own project in a subdirectory.
    ...readdirSync(".github/integration", { recursive: true })
      .map((f) => `.github/integration/${f.replaceAll("\\", "/")}`)
      .filter((f) => statSync(f).isFile()),
    "scripts/ci-verdict.sh",
    "scripts/release-bundle.mjs",
    "scripts/release-install.mjs",
    "scripts/release-payload.mjs",
    ...readdirSync("tools/release-tooling/payload-contract").map((f) => `tools/release-tooling/payload-contract/${f}`),
    ...readdirSync(workflowDir).map((f) => `${workflowDir}/${f}`),
    ...readdirSync(".github")
      .filter(
        (f) => f.endsWith(".mjs") || f.endsWith(".json") || f.endsWith(".lock"),
      )
      .map((f) => `.github/${f}`),
    "release-platforms.toml",
  ].sort())
    hash.update(file).update(readFileSync(file));
  return hash.digest("hex");
}
export function evaluate(expression, context) {
  for (const [, job, output] of expression.matchAll(
    /needs\.([A-Za-z0-9_-]+)\.outputs\.([A-Za-z0-9_-]+)/g,
  )) {
    if (context.needs?.[job]?.outputs?.[output] === undefined)
      throw Error(`Unresolved job output: needs.${job}.outputs.${output}`);
  }
  const source = expression.replace(/^\s*\$\{\{\s*|\s*\}\}\s*$/g, "");
  const statuses = ["always", "success", "failure", "cancelled"];
  const definitions = statuses.map((name) => ({
    name,
    minArgs: 0,
    maxArgs: 0,
  }));
  const ast = new Parser(
    new Lexer(source).lex().tokens,
    Object.keys(context),
    definitions,
  ).parse();
  const funcs = new Map(
    statuses.map((name) => [
      name,
      {
        ...definitions.find((d) => d.name === name),
        call: () =>
          new data.BooleanData(
            name === "always" ||
              (name === "success" &&
                !Object.values(context.needs ?? {}).some(
                  (n) => n.result !== "success",
                )) ||
              (name === "failure" &&
                Object.values(context.needs ?? {}).some(
                  (n) => n.result === "failure",
                )) ||
              (name === "cancelled" &&
                Object.values(context.needs ?? {}).some(
                  (n) => n.result === "cancelled",
                )),
          ),
      },
    ]),
  );
  const result = new Evaluator(
    ast,
    JSON.parse(JSON.stringify(context), data.reviver),
    funcs,
  ).evaluate();
  return JSON.parse(JSON.stringify(result, data.replacer));
}
export function render(value, context) {
  if (typeof value !== "string") return value;
  if (/^\s*\$\{\{[\s\S]*\}\}\s*$/.test(value)) return evaluate(value, context);
  return value.replace(/\$\{\{([\s\S]*?)\}\}/g, (_, e) =>
    String(evaluate(e, context)),
  );
}
export function expand(matrix, context) {
  if (!matrix) return [{}];
  matrix = Object.fromEntries(
    Object.entries(matrix).map(([key, value]) => [key, render(value, context)]),
  );
  let rows = [{}];
  const keys = Object.keys(matrix).filter(
    (k) => !["include", "exclude"].includes(k),
  );
  for (const key of keys) {
    if (!Array.isArray(matrix[key]))
      throw Error(`Unresolved matrix ${key}: ${JSON.stringify(matrix[key])}`);
    rows = rows.flatMap((row) =>
      matrix[key].map((v) => ({ ...row, [key]: v })),
    );
  }
  rows = rows.filter(
    (row) =>
      !(matrix.exclude ?? []).some((ex) =>
        Object.entries(ex).every(([k, v]) => row[k] === v),
      ),
  );
  if (!keys.length) rows = [];
  const originals = [...rows];
  for (const inc of matrix.include ?? []) {
    if (!keys.length) {
      rows.push(inc);
      continue;
    }
    const matched = originals.filter((row) =>
      keys.every((k) => !(k in inc) || row[k] === inc[k]),
    );
    if (matched.length) for (const row of matched) Object.assign(row, inc);
    else rows.push(inc);
  }
  if (!rows.length) throw Error("Empty/unresolved workflow matrix");
  return rows;
}
export function matches(path, patterns) {
  let included = false;
  for (let pattern of patterns) {
    const negative = pattern.startsWith("!");
    if (negative) pattern = pattern.slice(1);
    const regex = new RegExp(
      "^" +
        pattern
          .split("**")
          .map((p) =>
            p
              .split("*")
              .map((s) => s.replace(/[.+?^${}()|[\]\\]/g, "\\$&"))
              .join("[^/]*"),
          )
          .join(".*") +
        "$",
    );
    if (regex.test(path)) included = !negative;
  }
  return included;
}
export function triggered(workflow, scenario) {
  const events =
    typeof workflow.on === "string"
      ? { [workflow.on]: {} }
      : Array.isArray(workflow.on)
        ? Object.fromEntries(workflow.on.map((e) => [e, {}]))
        : workflow.on;
  if (!events || !(scenario.event in events)) return false;
  const config = events[scenario.event];
  if (config?.types && !config.types.includes(scenario.action)) return false;
  const tag = scenario.event === "push" && (scenario.action === "tag" || scenario.refType === "tag");
  if (scenario.event === "push" || scenario.event === "pull_request" || scenario.event === "workflow_run") {
    const key = tag ? "tags" : "branches", opposite = tag ? "branches" : "tags";
    if (scenario.event === "push" && !config?.[key] && !config?.[`${key}-ignore`] && (config?.[opposite] || config?.[`${opposite}-ignore`])) return false;
    if (config?.[key] && !matches(scenario.branch, config[key])) return false;
    if (config?.[`${key}-ignore`] && matches(scenario.branch, config[`${key}-ignore`])) return false;
  }
  if (!tag && config?.paths && !scenario.paths.some((p) => matches(p, config.paths))) return false;
  if (!tag && config?.["paths-ignore"] && scenario.paths.every((p) => matches(p, config["paths-ignore"]))) return false;
  if (
    scenario.event === "workflow_run" &&
    config?.workflows &&
    !config.workflows.includes(scenario.upstream)
  )
    return false;
  return true;
}
export function resolveJobs(
  file,
  scenario,
  all = workflows(),
  stack = [],
  inputOverrides = {},
) {
  if (stack.includes(file))
    throw Error(`Recursive workflow call: ${[...stack, file]}`);
  const workflow = all[file];
  if (!workflow) throw Error(`Unresolved workflow call: ${file}`);
  const event = {
    event_name: scenario.event,
    repository: { full_name: "nickderobertis/oneharness" },
    pull_request:
      scenario.event === "pull_request"
        ? {
            head: {
              repo: {
                full_name: scenario.fork
                  ? "fork/repo"
                  : "nickderobertis/oneharness",
              },
              ref: scenario.headRef ?? (scenario.releaseOnly
                ? "release-plz-2026-10-12T13-43-26Z"
                : "feature"),
            },
            base: { ref: "main" },
          }
        : undefined,
    workflow_run: { conclusion: "success" },
  };
  // A job that did not succeed still exposes every output it declares; one its
  // producer step never wrote reads as the empty string, as in Actions. A
  // scenario may supply the value a failed producer wrote before failing.
  const unwritten = (job, override) => Object.fromEntries(Object.keys(job.outputs ?? {}).map((key) => [key, override?.outputs && key in override.outputs ? override.outputs[key] : ""]));
  const needs = Object.fromEntries(Object.entries(workflow.jobs).map(([id, job]) => {
    const override = scenario.outcomes?.[id];
    const result = override?.result ?? "success";
    if (!["success", "failure", "cancelled", "skipped"].includes(result)) throw Error(`Invalid job outcome ${id}/${result}`);
    if (result !== "success") return [id, { result, outputs: unwritten(job, override) }];
    const outputs = {};
    for (const [key, expression] of Object.entries(job.outputs ?? {})) {
      if (override?.outputs && key in override.outputs) outputs[key] = override.outputs[key];
      else if (file === "ci.yml" && id === "macos-policy" && ["macos_required", "is_release_only"].includes(key) && expression === "${{ steps.policy.outputs." + key + " }}")
        outputs[key] = String(key === "macos_required" ? scenario.macosRequired : scenario.releaseOnly);
      else if (file === "release-plz.yml" && id === "guard" && key === "enabled" && expression === "${{ steps.check.outputs.enabled }}") outputs[key] = "true";
      else if (file === "release.yml" && id === "gate" && key === "needs_check" && expression === "${{ steps.verdict.outputs.needs_check }}") outputs[key] = "false";
      else throw Error(`Unresolved producer output ${file}/${id}/${key}. Next: prove its real producer before modeling its output.`);
    }
    return [id, { result, outputs }];
  }));
  const context = {
    github: {
      event_name: scenario.event,
      event,
      repository: "nickderobertis/oneharness",
      ref: `refs/${scenario.action === "tag" || scenario.refType === "tag" ? "tags" : "heads"}/${scenario.branch}`,
      head_ref: event.pull_request?.head.ref ?? "",
    },
    inputs: {
      ...Object.fromEntries(
        Object.entries(
          workflow.on?.workflow_dispatch?.inputs ??
            workflow.on?.workflow_call?.inputs ??
            {},
        ).map(([key, input]) => [key, input.default]),
      ),
      ...(scenario.manual && workflow.on?.workflow_dispatch?.inputs?.os
        ? { os: "all" }
        : {}),
      ...inputOverrides,
    },
    needs,
    vars: {
      NPM_PUBLISH: String(["npm", "both"].includes(scenario.publish)),
      PYPI_PUBLISH: String(["pypi", "both"].includes(scenario.publish)),
    },
    matrix: {},
    runner: { os: "Linux" },
  };
  const rows = [];
  const scheduled = new Map();
  function active(id, visiting = []) {
    if (scheduled.has(id)) return scheduled.get(id);
    if (visiting.includes(id)) throw Error(`Cyclic job dependencies: ${[...visiting, id]}`);
    const job = workflow.jobs[id];
    if (!job) throw Error(`Unresolved required job ${file}/${id}`);
    const dependencies = typeof job.needs === "string" ? [job.needs] : job.needs ?? [];
    for (const dependency of dependencies) if (!active(dependency, [...visiting, id])) needs[dependency] = {result:"skipped",outputs:unwritten(workflow.jobs[dependency])};
    const local = {...context, needs:Object.fromEntries(dependencies.map(d=>[d,needs[d]]))};
    const explicitStatus = /\b(always|success|failure|cancelled)\s*\(/.test(job.if ?? "");
    const success = dependencies.every(d=>needs[d].result === "success");
    const enabled = (explicitStatus || success) && (!job.if || evaluate(job.if, local));
    scheduled.set(id,enabled);
    return enabled;
  }
  for (const [id, job] of Object.entries(workflow.jobs)) {
    if (!active(id)) continue;
    if (job.uses) {
      if (!job.uses.startsWith("./.github/workflows/"))
        throw Error(`Unresolved reusable workflow: ${job.uses}`);
      for (const matrix of expand(job.strategy?.matrix, context)) {
        const inputs = Object.fromEntries(
          Object.entries(job.with ?? {}).map(([key, value]) => [
            key,
            render(value, { ...context, matrix }),
          ]),
        );
        rows.push(
          ...resolveJobs(
            job.uses.slice("./.github/workflows/".length),
            scenario,
            all,
            [...stack, file],
            inputs,
          ).map((row) => ({ ...row, job: `${id}/${row.job}` })),
        );
      }
      continue;
    }
    for (const matrix of expand(job.strategy?.matrix, context)) {
      const local = { ...context, matrix };
      const runner = render(job["runs-on"], local);
      if (
        typeof runner !== "string" ||
        !runner ||
        runner.includes("${{") ||
        !/^ubuntu-|^windows-|^macos-/.test(runner)
      )
        throw Error(
          `Unresolved runner ${file}/${id}: ${JSON.stringify(runner)}`,
        );
      const name = job.name
        ? render(job.name, local)
        : id +
          (Object.keys(matrix).length
            ? ` (${Object.values(matrix).join(", ")})`
            : "");
      rows.push({
        workflow: file,
        event: scenario.event,
        job: id,
        name,
        runner,
        target: matrix.target ?? "",
        architectures: matrix.target
          ? [matrix.target]
          : (job.steps
              ?.map((step) => step.run ?? "")
              .join("\n")
              .match(/for target in ([^;\n]+); do/)?.[1]
              .split(/\s+/) ?? []),
        macos: runner.startsWith("macos-"),
      });
    }
  }
  return rows;
}
