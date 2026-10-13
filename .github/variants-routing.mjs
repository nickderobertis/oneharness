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
const live = parse(readFileSync(values.workflow, 'utf8')).jobs.live;
for (const [os, opencode, qwen] of [
  ['ubuntu-latest', '1', '1'], ['windows-latest', '0', '1'],
]) {
  const row = live.strategy.matrix.include.find(row => row.os === os);
  assert.equal(row?.run_opencode, opencode, `${os}: preserve OpenCode variant selector`);
  assert.equal(row?.run_qwen, qwen, `${os}: preserve Qwen variant selector`);
}
const check = parse(readFileSync(values.ci, 'utf8')).jobs.check;
assert.equal(check.env.OH_E2E_VARIANTS_RUN_OPENCODE, '1', 'Mac CI: preserve OpenCode variant selector');
assert.equal(check.env.OH_E2E_VARIANTS_RUN_QWEN, '0', 'Mac CI: preserve Qwen variant exclusion');
assert.ok(check.steps.some(step => step.run === 'just live-variants' && step.if.includes("matrix.os == 'macos-latest'")), 'Mac CI must run real live-variants command');
