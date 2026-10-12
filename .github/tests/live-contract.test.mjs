import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, cpSync, writeFileSync } from "node:fs";
import { resolve, join } from "node:path";
import { execFileSync } from "node:child_process";
import { parse } from "yaml";
import { history } from "./history.mjs";
import { evaluate } from "../workflow-graph.mjs";
// Paid Mac CLI execution requires hosted credentials and a Mac runner.
// Structural command/credential checks do not claim native execution.
test("consolidated Mac job retains real live commands and credential bindings from every workflow", () => {
  const ci = parse(readFileSync(".github/workflows/ci.yml", "utf8")).jobs.check;
  const names = [
    "claude",
    "codex",
    "opencode",
    "goose",
    "qwen",
    "crush",
    "copilot",
    "cursor",
    "schema",
    "variants",
    "control",
  ];
  for (const name of names) {
    const original = parse(
      readFileSync(`.github/workflows/e2e-${name}.yml`, "utf8"),
    ).jobs.live;
    for (const step of original.steps.filter(
      (s) => s.run && (!s.if || evaluate(s.if, { runner: { os: "macOS" } })),
    )) {
      const command = (s) =>
        s
          .split("\n")
          .filter((l) => !l.trim().startsWith("#"))
          .join("\n");
      const copies = ci.steps.filter(
        (s) =>
          s.run &&
          command(s.run) === command(step.run) &&
          s.if?.includes(`outputs.${name} == 'true'`),
      );
      assert.equal(
        copies.length,
        1,
        `${name}: missing/duplicate command ${step.name}`,
      );
      assert.deepEqual(
        copies[0].env,
        step.env,
        `${name}: credential/model drift`,
      );
      const context = {
        matrix: { os: "macos-latest" },
        needs: { "macos-policy": { outputs: { macos_required: "true" } } },
        github: {
          repository: "nickderobertis/oneharness",
          event: {
            pull_request: { head: { repo: { full_name: "fork/repo" } } },
          },
        },
        steps: { "live-paths": { outputs: { [name]: "true" } } },
        runner: { os: "macOS" },
      };
      assert.equal(
        evaluate(copies[0].if, context),
        false,
        `${name}: fork must not receive live credentials`,
      );
      context.github.event.pull_request.head.repo.full_name = context.github.repository;
      assert.equal(evaluate(copies[0].if, context), true, `${name}: trusted selected lane must execute`);
      context.steps["live-paths"].outputs[name] = "false";
      assert.equal(evaluate(copies[0].if, context), false, `${name}: unselected lane must omit`);
    }
  }
  assert.equal(ci.env.OH_E2E_VARIANTS_RUN_OPENCODE, "1");
  assert.equal(ci.env.OH_E2E_VARIANTS_RUN_QWEN, "0");
});
test("real path-routing subprocess reads workflow filters and keeps former control schedule proof", () => {
  const fixture = history();
  try {
    cpSync(".github/workflows", join(fixture.cwd, ".github/workflows"), {
      recursive: true,
    });
    const output = join(fixture.cwd, "outputs");
    writeFileSync(output, "");
    execFileSync(process.execPath, [resolve(".github/live-paths.mjs")], {
      cwd: fixture.cwd,
      env: {
        ...process.env,
        BASE_SHA: "base",
        HEAD_SHA: "version-only",
        GITHUB_OUTPUT: output,
      },
    });
    const flags = Object.fromEntries(
      readFileSync(output, "utf8")
        .trim()
        .split("\n")
        .map((s) => s.split("=")),
    );
    assert.equal(flags.claude, "true");
    assert.equal(flags.codex, "true");
    assert.equal(flags.variants, "false");
    assert.equal(flags.control, "true");
  } finally {
    fixture.dispose();
  }
});
