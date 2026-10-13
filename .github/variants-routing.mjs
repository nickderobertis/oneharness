import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { parseArgs } from 'node:util';
import { parse } from 'yaml';
const { values } = parseArgs({ options: {
  workflow: { type: 'string' }, ci: { type: 'string' },
} });
for (const name of ['workflow', 'ci']) {
  if (!values[name]?.trim()) throw Error(`--${name} requires a workflow file path`);
}
const object = value => value !== null && typeof value === 'object' && !Array.isArray(value);
function job(path, id) {
  const document = parse(readFileSync(path, 'utf8'));
  if (!object(document) || !object(document.jobs) || !object(document.jobs[id]))
    throw Error(`Invalid ${path}: restore the jobs.${id} mapping`);
  return document.jobs[id];
}
const live = job(values.workflow, 'live');
if (!object(live.strategy) || !object(live.strategy.matrix) ||
    !Array.isArray(live.strategy.matrix.include) ||
    live.strategy.matrix.include.some(row => !object(row) ||
      ['os', 'run_opencode', 'run_qwen'].some(key => typeof row[key] !== 'string')))
  throw Error('Invalid variants matrix: restore include rows with string runner/selector fields');
for (const [os, opencode, qwen] of [
  ['ubuntu-latest', '1', '1'], ['windows-latest', '0', '1'],
]) {
  const row = live.strategy.matrix.include.find(row => row.os === os);
  assert.equal(row?.run_opencode, opencode, `${os}: preserve OpenCode variant selector`);
  assert.equal(row?.run_qwen, qwen, `${os}: preserve Qwen variant selector`);
}
const check = job(values.ci, 'check');
if (!object(check.env) || !Array.isArray(check.steps) ||
    check.steps.some(step => !object(step) ||
      ['run', 'if'].some(key => step[key] !== undefined && typeof step[key] !== 'string')))
  throw Error('Invalid Mac CI job: restore its env mapping and string command/condition steps');
assert.equal(check.env.OH_E2E_VARIANTS_RUN_OPENCODE, '1', 'Mac CI: preserve OpenCode variant selector');
assert.equal(check.env.OH_E2E_VARIANTS_RUN_QWEN, '0', 'Mac CI: preserve Qwen variant exclusion');
assert.ok(check.steps.some(step => step.run === 'just live-variants' && step.if?.includes("matrix.os == 'macos-latest'")), 'Mac CI must run real live-variants command');
