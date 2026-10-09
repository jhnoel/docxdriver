// Offline unit tests for the live-trial harness's pure helpers. These run
// under `npm test`; the live pi runs themselves are opt-in (PI_E2E_LIVE=1).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import {
  collectMetrics,
  finalFingerprint,
  pairedToolCalls,
  parseLine,
  trialSuccess,
  validateManifest,
  checkFinalDocument,
} from './pi-e2e/repl-live.mjs';

const here = import.meta.dirname;

function event(type, overrides = {}) {
  return { type, ...overrides };
}

function awaitManifest() {
  return readFile(join(here, 'pi-e2e', 'repl-tasks.json'), 'utf8');
}

test('manifest schema: the checked-in repl-tasks.json validates', async () => {
  const manifest = JSON.parse(await awaitManifest());
  assert.equal(validateManifest(manifest).tasks.length, 10);
  const ids = manifest.tasks.map((t) => t.id);
  assert.deepEqual(ids, [
    'task1-exact-text-correction',
    'task2-repeated-bulk-edit',
    'task3-ambiguous-occurrence',
    'task4-arbitrary-formatting-selection',
    'task5-paragraph-structure',
    'task6-mixed-atomic-edit',
    'task7-failed-preview-repair',
    'task8-source-race',
    'task9-mutation-after-review',
    'task10-package-preservation',
  ]);
  for (const task of manifest.tasks) {
    assert.ok(['contract', 'complex'].includes(task.fixture), task.id);
    assert.ok(['commit', 'blocked'].includes(task.verify), task.id);
  }
  assert.equal(manifest.tasks.find((t) => t.id === 'task8-source-race').verify, 'blocked');
  assert.equal(manifest.tasks.find((t) => t.id === 'task9-mutation-after-review').verify, 'commit');
  assert.equal(manifest.tasks.find((t) => t.id === 'task10-package-preservation').fixture, 'complex');
});

test('manifest schema: rejects malformed manifests with concrete messages', () => {
  assert.throws(() => validateManifest({ tasks: [] }), /non-empty tasks array/);
  assert.throws(() => validateManifest({ tasks: [{ id: 'task1-exact-text-correction', prompt: 'x' }] }), /missing string field fixture/);
  assert.throws(
    () => validateManifest({ tasks: [{ id: 'task1-exact-text-correction', prompt: 'x', fixture: 'bogus', verify: 'commit' }] }),
    /fixture must be contract\|complex/,
  );
  assert.throws(
    () => validateManifest({ tasks: [{ id: 'task1-exact-text-correction', prompt: 'x', fixture: 'contract', verify: 'bogus' }] }),
    /verify must be commit\|blocked/,
  );
  assert.throws(
    () => validateManifest({ tasks: [{ id: 'task1-exact-text-correction', prompt: 'x', fixture: 'contract', verify: 'commit' }] }),
    /missing required task task2/,
  );
  assert.throws(
    () => validateManifest({ tasks: [{ id: 'task9-mutation-after-review', prompt: 'x', fixture: 'contract', verify: 'commit' }] }),
    /missing required task task1/,
  );
});

test('parseLine handles JSON events and raw text lines', () => {
  assert.deepEqual(parseLine('{"type":"message_end","message":{"role":"assistant"}}'), { type: 'message_end', message: { role: 'assistant' } });
  assert.deepEqual(parseLine('not json'), { type: 'text', text: 'not json' });
});

test('pairedToolCalls pairs start/end events and reports unpaired starts', () => {
  const events = [
    event('tool_execution_start', { toolCallId: 'a', toolName: 'python', args: { code: 'x = 1' } }),
    event('tool_execution_start', { toolCallId: 'b', toolName: 'python', args: { code: 'x + 1' } }),
    event('tool_execution_end', { toolCallId: 'a', result: { content: [{ type: 'text', text: '=> 1' }] } }),
  ];
  const { calls, unpaired } = pairedToolCalls(events);
  assert.equal(calls.length, 1);
  assert.equal(calls[0].id, 'a');
  assert.equal(unpaired.length, 1);
  assert.equal(unpaired[0].id, 'b');
});

test('collectMetrics counts review, compact key, commit, blocks, chars, recoveries', () => {
  const events = [
    event('tool_execution_start', { toolCallId: '1', toolName: 'python', args: { code: 'original = docx_read("contract.docx")' } }),
    event('tool_execution_end', { toolCallId: '1', result: { content: [{ type: 'text', text: '<p id="A">hello</p>' }] } }),
    event('tool_execution_start', { toolCallId: '2', toolName: 'python', args: { code: 'review = docx_review("contract.docx", plan); print(review.edits)' } }),
    event('tool_execution_end', {
      toolCallId: '2',
      result: { content: [{ type: 'text', text: '── review ok ── path: contract.docx\nreview ok: 1 ops\ncommit_key: k1:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n[{"context":[{"markup":"<p>changed</p>"}]}]' }] },
    }),
    event('tool_execution_start', { toolCallId: '3', toolName: 'python', args: { code: 'docx_commit(review.commit_key)' } }),
    event('tool_execution_end', {
      toolCallId: '3',
      result: { content: [{ type: 'text', text: '── commit ok ── path: contract.docx\ncommitted: 1 ops\ncommit_key: k1:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' }] },
    }),
    event('tool_execution_start', { toolCallId: '4', toolName: 'python', args: { code: 'docx_commit(review.commit_key)' } }),
    event('tool_execution_end', {
      toolCallId: '4',
      result: { content: [{ type: 'text', text: '── blocked ── path: contract.docx\ncommit key already consumed — review again' }] },
    }),
    event('tool_execution_start', { toolCallId: '5', toolName: 'python', args: { code: 'reset' } }),
    event('tool_execution_end', { toolCallId: '5', result: { content: [{ type: 'text', text: 'session was reset after a crash: Python state was lost' }] } }),
  ];
  const metrics = collectMetrics(events, true);
  assert.equal(metrics.toolCalls, 5);
  assert.equal(metrics.reviewAttempts, 1);
  assert.equal(metrics.commits, 1);
  assert.equal(metrics.blockedCount, 1);
  assert.equal(metrics.invalidWriteAttempts, 1);
  assert.equal(metrics.recoveries, 1);
  assert.equal(metrics.tamperApplied, true);
  assert.equal(metrics.hasCompactCommitKey, true);
  assert.equal(metrics.hasReviewPayload, true);
  assert.equal(metrics.explainableCommit, true);
  const chars = events
    .filter((e) => e.type === 'tool_execution_start')
    .reduce((sum, e) => sum + String(e.args.code).length, 0);
  assert.equal(metrics.pythonChars, chars);
});

test('collectMetrics: a commit without review is not explainable', () => {
  const events = [
    event('tool_execution_start', { toolCallId: '1', toolName: 'python', args: { code: 'x' } }),
    event('tool_execution_end', { toolCallId: '1', result: { content: [{ type: 'text', text: '── commit ok ── path: contract.docx\ncommitted: 1 ops' }] } }),
  ];
  const metrics = collectMetrics(events);
  assert.equal(metrics.reviewAttempts, 0);
  assert.equal(metrics.commits, 1);
  assert.equal(metrics.hasReviewPayload, false);
  assert.equal(metrics.explainableCommit, false);
});

test('collectMetrics: a successful review without edit context is not explainable', () => {
  const events = [
    event('tool_execution_start', { toolCallId: '1', toolName: 'python', args: { code: 'review = docx_review("contract.docx", plan)' } }),
    event('tool_execution_end', { toolCallId: '1', result: { content: [{ type: 'text', text: '── review ok ── path: contract.docx\\ncommit_key: k1:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' }] } }),
    event('tool_execution_start', { toolCallId: '2', toolName: 'python', args: { code: 'docx_commit(review.commit_key)' } }),
    event('tool_execution_end', { toolCallId: '2', result: { content: [{ type: 'text', text: '── commit ok ── path: contract.docx\\ncommitted: 1 ops' }] } }),
  ];
  const metrics = collectMetrics(events);
  assert.equal(metrics.reviewAttempts, 1);
  assert.equal(metrics.hasReviewPayload, false);
  assert.equal(metrics.explainableCommit, false);
});

test('collectMetrics: a blocked review is not a successful review', () => {
  const events = [
    event('tool_execution_start', { toolCallId: '1', toolName: 'python', args: { code: 'x' } }),
    event('tool_execution_end', { toolCallId: '1', result: { content: [{ type: 'text', text: '── blocked ──\nreview blocked: selection matched nothing' }] } }),
  ];
  const metrics = collectMetrics(events);
  assert.equal(metrics.reviewAttempts, 0);
  assert.equal(metrics.blockedCount, 1);
});

test('collectMetrics: review must precede commit', () => {
  const events = [
    event('tool_execution_start', { toolCallId: '1', toolName: 'python', args: { code: 'x' } }),
    event('tool_execution_end', { toolCallId: '1', result: { content: [{ type: 'text', text: '── commit ok ── path: contract.docx\ncommitted: 1 ops' }] } }),
    event('tool_execution_start', { toolCallId: '2', toolName: 'python', args: { code: 'y' } }),
    event('tool_execution_end', { toolCallId: '2', result: { content: [{ type: 'text', text: '── review ok ── path: contract.docx\ncommit_key: k1:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' }] } }),
  ];
  assert.equal(collectMetrics(events).explainableCommit, false);
});

test('checkFinalDocument: contract-task structural checks', () => {
  const ok = '<p id="A" ord="1">The notice period is <b>sixty days</b>.</p>';
  assert.doesNotThrow(() => checkFinalDocument('task1-exact-text-correction', ok, [], {}));
  assert.throws(() => checkFinalDocument('task1-exact-text-correction', '<p>sixty days</p>', [], {}), /bold/);
  assert.throws(() => checkFinalDocument('task1-exact-text-correction', '<p><b>sixty days</b> thirty days</p>', [], {}), /gone/);

  const bulk = '<p>Payment is due within 45 days of invoice receipt.</p>'.repeat(3);
  assert.doesNotThrow(() => checkFinalDocument('task2-repeated-bulk-edit', bulk, [], {}));
  assert.throws(() => checkFinalDocument('task2-repeated-bulk-edit', bulk.replace('45 days', '30 days'), [], {}), /30-day term/);

  const occ = 'the terms. the terms. THE TERMS. the terms.';
  assert.doesNotThrow(() => checkFinalDocument('task3-ambiguous-occurrence', occ, [], {}));
  assert.throws(() => checkFinalDocument('task3-ambiguous-occurrence', 'the terms. THE TERMS. THE TERMS. the terms.', [], {}), /exactly one/);
});

test('checkFinalDocument: task10 requires an edited paragraph and unchanged parts', () => {
  const extra = {
    verifyParts: () => ({
      before: new Map([['word/document.xml', 'a'], ['word/media/x.png', 'deadbeef'], ['[Content_Types].xml', 'beef']]),
      after: new Map([['word/document.xml', 'b'], ['word/media/x.png', 'deadbeef'], ['[Content_Types].xml', 'beef']]),
    }),
  };
  assert.doesNotThrow(() => checkFinalDocument('task10-package-preservation', '<p>x</p>', ['Some text [edited]'], extra));
  assert.throws(() => checkFinalDocument('task10-package-preservation', '<p>x</p>', ['Some text'], extra), /\[edited\]/);
  assert.throws(
    () =>
      checkFinalDocument('task10-package-preservation', '<p>x</p>', ['Some text [edited]'], {
        verifyParts: () => ({
          before: new Map([['word/document.xml', 'a'], ['word/media/x.png', 'deadbeef']]),
          after: new Map([['word/document.xml', 'b'], ['word/media/x.png', 'cafebabe']]),
        }),
      }),
    /word\/media\/x\.png must be byte-identical/,
  );
});

test('trialSuccess fails closed: turn-cap or timeout termination is never success', () => {
  assert.equal(trialSuccess({ verification: { outcome: 'ok' } }), true);
  assert.equal(trialSuccess({ verification: { outcome: 'ok' }, turn_capped: true }), false);
  assert.equal(trialSuccess({ verification: { outcome: 'ok' }, timed_out: true }), false);
  assert.equal(trialSuccess({ verification: { outcome: 'error', message: 'boom' } }), false);
  assert.equal(trialSuccess({ verification: undefined }), false);
});

test('finalFingerprint normalizes markup to paragraph plain texts', () => {
  const fp = finalFingerprint('<p id="A0000001" ord="1">one <b>bold</b></p>\n<p id="B0000002" ord="2">two</p>');
  assert.equal(fp, 'one bold\u0000two');
});
