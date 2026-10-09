// Offline unit tests for the TOML benchmark harness's pure helpers. These run
// under `npm test`; the live pi runs are opt-in (PI_E2E_LIVE=1).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { validateManifest } from './pi-e2e/repl-live.mjs';
import { collectTomlMetrics, trialSuccess } from './pi-e2e/toml-bench-live.mjs';

const here = import.meta.dirname;

function event(type, overrides = {}) {
  return { type, ...overrides };
}

function textResult(text) {
  return { result: { content: [{ type: 'text', text }] } };
}

test('toml manifest: the checked-in toml-tasks.json validates and matches the python task ids', async () => {
  const toml = JSON.parse(await readFile(join(here, 'pi-e2e', 'toml-tasks.json'), 'utf8'));
  assert.equal(validateManifest(toml).tasks.length, 10);
  const python = JSON.parse(await readFile(join(here, 'pi-e2e', 'repl-tasks.json'), 'utf8'));
  assert.deepEqual(
    toml.tasks.map((t) => t.id),
    python.tasks.map((t) => t.id),
  );
  for (const task of toml.tasks) {
    const py = python.tasks.find((t) => t.id === task.id);
    assert.equal(task.fixture, py.fixture, task.id);
    assert.equal(task.verify, py.verify, task.id);
  }
});

test('collectTomlMetrics: preview, commit, blocked commit, write/edit chars', () => {
  const events = [
    event('tool_execution_start', { toolCallId: '1', toolName: 'write', args: { path: 'plan.toml', content: 'base = "x"\n[[ops]]\nop = "replace_text"' } }),
    event('tool_execution_end', { toolCallId: '1', result: { content: [{ type: 'text', text: 'wrote plan.toml' }] } }),
    event('tool_execution_start', { toolCallId: '2', toolName: 'docx_edit', args: { path: 'contract.docx', plan: { file: 'plan.toml' } } }),
    event('tool_execution_end', {
      toolCallId: '2',
      result: { content: [{ type: 'text', text: '{"path":"contract.docx","outcome":"previewed","preview_key":"p1:sha256:abcd","report":{}}' }] },
    }),
    event('tool_execution_start', { toolCallId: '3', toolName: 'docx_edit', args: { path: 'contract.docx', plan: { file: 'plan.toml' }, preview_key: 'p1:sha256:abcd' } }),
    event('tool_execution_end', { toolCallId: '3', result: { content: [{ type: 'text', text: '{"path":"contract.docx","outcome":"committed","report":{}}' }] } }),
    event('tool_execution_start', { toolCallId: '4', toolName: 'edit', args: { path: 'plan.toml', oldText: 'sixty days', newText: 'ninety days' } }),
    event('tool_execution_end', { toolCallId: '4', result: { content: [{ type: 'text', text: 'edited plan.toml' }] } }),
    event('tool_execution_start', { toolCallId: '5', toolName: 'docx_edit', args: { path: 'contract.docx', plan: { file: 'plan.toml' }, preview_key: 'p1:sha256:abcd' } }),
    event('tool_execution_end', {
      toolCallId: '5',
      result: { content: [{ type: 'text', text: '{"path":"contract.docx","outcome":"rejected","diagnostic":{"code":"preview_key_mismatch","message":"preview key does not match source and canonical plan"}}' }] },
    }),
  ];
  const metrics = collectTomlMetrics(events);
  assert.equal(metrics.toolCalls, 5);
  assert.equal(metrics.previewAttempts, 1);
  assert.equal(metrics.commits, 1);
  assert.equal(metrics.invalidWriteAttempts, 1);
  assert.equal(metrics.blockedCount, 1);
  assert.equal(metrics.explainableCommit, true);
  const expectedChars =
    'base = "x"\n[[ops]]\nop = "replace_text"'.length +
    JSON.stringify({ file: 'plan.toml' }).length +
    JSON.stringify({ file: 'plan.toml' }).length +
    'sixty days'.length +
    'ninety days'.length +
    JSON.stringify({ file: 'plan.toml' }).length;
  assert.equal(metrics.charsAuthored, expectedChars);
});

test('collectTomlMetrics: a rejected preview counts as blocked, not a preview', () => {
  const events = [
    event('tool_execution_start', { toolCallId: '1', toolName: 'docx_edit', args: { path: 'contract.docx', plan: { file: 'plan.toml' } } }),
    event('tool_execution_end', {
      toolCallId: '1',
      result: { content: [{ type: 'text', text: '{"outcome":"rejected","diagnostic":{"message":"blocked in paragraph ..."}}' }] },
    }),
  ];
  const metrics = collectTomlMetrics(events);
  assert.equal(metrics.previewAttempts, 0);
  assert.equal(metrics.blockedCount, 1);
  assert.equal(metrics.commits, 0);
});

test('collectTomlMetrics: a write failure counts as blocked', () => {
  const events = [
    event('tool_execution_start', { toolCallId: '1', toolName: 'write', args: { path: 'plan.toml', content: 'x' } }),
    event('tool_execution_end', { toolCallId: '1', result: { content: [{ type: 'text', text: 'failed' }] }, isError: true }),
  ];
  const metrics = collectTomlMetrics(events);
  assert.equal(metrics.blockedCount, 1);
  assert.equal(metrics.charsAuthored, 1);
});

test('collectTomlMetrics: unanchored commit is not explainable', () => {
  const events = [
    event('tool_execution_start', { toolCallId: '1', toolName: 'docx_edit', args: { path: 'contract.docx', plan: { file: 'plan.toml' }, preview_key: 'p1:sha256:x' } }),
    event('tool_execution_end', { toolCallId: '1', result: { content: [{ type: 'text', text: '{"outcome":"committed"}' }] } }),
  ];
  assert.equal(collectTomlMetrics(events).explainableCommit, false);
});

test('trialSuccess is verification-based', () => {
  assert.equal(trialSuccess({ verification: { outcome: 'ok' } }), true);
  assert.equal(trialSuccess({ verification: { outcome: 'error', message: 'boom' } }), false);
  assert.equal(trialSuccess({ verification: undefined }), false);
});
