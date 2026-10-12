import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { execFileSync } from "node:child_process";
import { parse } from "yaml";
import { history } from "./history.mjs";
test("real routing adapter overrides event identity and writes workflow-consumed output keys", () => {
  const fixture = history();
  try {
    const event = join(fixture.cwd, "route-event.json"),
      output = join(fixture.cwd, "outputs");
    const run = (value, name) => {
      writeFileSync(event, JSON.stringify(value));
      writeFileSync(output, "");
      execFileSync(process.execPath, [resolve(".github/macos-route.mjs")], {
        cwd: fixture.cwd,
        env: {
          ...process.env,
          EVENT_NAME: name,
          GITHUB_EVENT_PATH: event,
          GITHUB_OUTPUT: output,
          BASE_SHA: "base",
          HEAD_SHA: "version-only",
        },
      });
      return Object.fromEntries(
        readFileSync(output, "utf8")
          .trim()
          .split("\n")
          .map((s) => s.split("=")),
      );
    };
    const payload = {
      event_name: "push",
      repository: { full_name: "nickderobertis/oneharness" },
      pull_request: {
        user: { login: "nickderobertis" },
        head: {
          ref: "release-plz-2026-10-12T13-43-26Z",
          repo: { full_name: "nickderobertis/oneharness" },
        },
      },
    };
    assert.deepEqual(run(payload, "pull_request"), {
      macos_required: "false",
      is_release_only: "true",
    });
    for (const bad of [null, "bad", [], {}])
      assert.deepEqual(run(bad, "pull_request"), {
        macos_required: "true",
        is_release_only: "false",
      });
    assert.deepEqual(run(payload, "schedule"), {
      macos_required: "false",
      is_release_only: "false",
    });
    const workflow = parse(readFileSync(".github/workflows/ci.yml", "utf8"));
    const keys = Object.keys(workflow.jobs["macos-policy"].outputs);
    assert.deepEqual(
      keys.sort(),
      Object.keys(run(payload, "pull_request")).sort(),
    );
  } finally {
    fixture.dispose();
  }
});

test('real automation guard produces its declared enabled output for secret boundary outcomes',()=>{
 const fixture=history();
 try{
  const workflow=parse(readFileSync('.github/workflows/release-plz.yml','utf8'));
  const guard=workflow.jobs.guard, output=join(fixture.cwd,'guard-output');
  assert.equal(guard.outputs.enabled,'${{ steps.check.outputs.enabled }}');
  const command=guard.steps.find(s=>s.id==='check').run;
  for(const [release,cargo,expected] of [['','','false'],['present','','false'],['','present','false'],['present','present','true']]){
   writeFileSync(output,'');
   execFileSync('bash',['-e','-c',command],{env:{...process.env,RELEASE_TOKEN:release,CARGO_TOKEN:cargo,GITHUB_OUTPUT:output},stdio:['ignore','pipe','pipe']});
   assert.equal(readFileSync(output,'utf8').trim(),`enabled=${expected}`);
  }
 }finally{fixture.dispose();}
});
