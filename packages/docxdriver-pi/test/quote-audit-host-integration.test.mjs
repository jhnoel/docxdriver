import { test, before } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { CommitKeyStore, createPlanPythonHost } from '../dist/python-host.js';
import { ensureInit, runRead } from '../dist/engine.js';

before(async () => ensureInit());

async function setup(enabled, provenancePolicy) {
  const cwd = await mkdtemp(join(tmpdir(), 'docxdriver-quote-audit-host-'));
  const notices = [];
  const store = new CommitKeyStore();
  let execution = 1;
  const host = createPlanPythonHost(cwd, notices, store, () => execution, () => 0, undefined, { quoteAudit: { enabled, provenancePolicy } });
  await host.externalLookup._docx_create('demo.docx', '<p>He said “old words”.</p>');
  const markup = (await host.externalLookup._docx_read('demo.docx')).markup;
  const id = markup.match(/<p id="([0-9A-F]{8})/)?.[1];
  return { cwd, notices, host, id, nextExecution: () => { execution += 1; } };
}

async function editAndCommit(bundle, changeMode = 'track') {
  const state = { plan: { author: 'Model', change_mode: changeMode, ops: [{ op: 'replace_text', at: bundle.id, select: 'old words', with: 'new words' }] } };
  const review = await bundle.host.externalLookup._docx_review('demo.docx', state);
  assert.ok(review);
  bundle.nextExecution();
  await bundle.host.externalLookup._docx_commit(review.commit_key);
  return new Uint8Array(await readFile(join(bundle.cwd, 'demo.docx')));
}

test('feature disabled preserves the existing commit path without audit comments', async () => {
  const bundle = await setup(false);
  const bytes = await editAndCommit(bundle);
  const comments = runRead(bytes, { kind: 'comments' });
  assert.equal(comments.outcome, 'completed');
  assert.equal(comments.result.comments.length, 0);
  assert.doesNotMatch(bundle.notices.at(-1).summary, /quotation audit/);
});

test('feature enabled attaches a protected exact-span bubble before write', async () => {
  const bundle = await setup(true);
  const bytes = await editAndCommit(bundle, 'direct');
  const comments = runRead(bytes, { kind: 'comments' });
  assert.equal(comments.outcome, 'completed');
  assert.equal(comments.result.comments.length, 1, JSON.stringify(bundle.notices));
  const warning = comments.result.comments[0];
  assert.equal(warning.status, 'open');
  assert.equal(warning.root.author, 'docxdriver quotation audit');
  assert.match(warning.root.text, /UNVERIFIED QUOTATION/);
  assert.deepEqual(warning.anchor.ranges, [{ locator: 'p1:8-19' }]);
  assert.match(bundle.notices.at(-1).summary, /quotation audit warnings: 1/);
});

test('reserved controlled and authoritative policies fail closed at review', async () => {
  for (const policy of ['controlled', 'authoritative']) {
    const bundle = await setup(true, policy);
    const state = { plan: { author: 'Model', change_mode: 'direct', ops: [{ op: 'replace_text', at: bundle.id, select: 'old words', with: 'new words' }] } };
    const review = await bundle.host.externalLookup._docx_review('demo.docx', state);
    assert.equal(review, null);
    assert.match(bundle.notices.at(-1).summary, new RegExp(`policy ${policy} is reserved but not implemented`));
  }
});

test('feature-enabled review rejects explicit mutation of a protected warning', async () => {
  const bundle = await setup(true);
  const bytes = await editAndCommit(bundle, 'direct');
  const comments = runRead(bytes, { kind: 'comments' });
  const commentId = comments.result.comments[0].id;
  bundle.nextExecution();
  const review = await bundle.host.externalLookup._docx_review('demo.docx', {
    plan: { author: 'Model', change_mode: 'track', ops: [{ op: 'comment_delete', comment_id: commentId }] },
  });
  assert.equal(review, null);
  assert.match(bundle.notices.at(-1).summary, /protected quotation audit comment/);
});

test('feature fails closed when tracked revision wrappers broaden the physical anchor', async () => {
  const bundle = await setup(true);
  const bytes = await editAndCommit(bundle, 'track');
  const comments = runRead(bytes, { kind: 'comments' });
  assert.equal(comments.result.comments.length, 0);
  assert.match(bundle.notices.at(-1).summary, /quotation audit failed.*coverage failed/);
});

test('commit rejects a structured quotation when its source changes after review', async () => {
  const bundle = await setup(true);
  await bundle.host.externalLookup._docx_create('source.docx', '<p>Exact source words.</p>');
  const source = (await bundle.host.externalLookup._docx_find('source.docx', 'Exact source'))[0];
  const state = {
    plan: {
      author: 'Model',
      change_mode: 'direct',
      ops: [{
        op: 'replace_paragraph',
        at: bundle.id,
        with: { $kind: 'quote', source: 'source.docx', at: source.id, select: source.text, occurrence: 1, style: 'double', changes: [] },
      }],
    },
  };
  const review = await bundle.host.externalLookup._docx_review('demo.docx', state);
  assert.ok(review);
  const sourcePath = join(bundle.cwd, 'source.docx');
  const sourceBytes = new Uint8Array(await readFile(sourcePath));
  const sourceRead = runRead(sourceBytes, { kind: 'document', view: 'markup' });
  const sourceId = sourceRead.result.markup.match(/<p id="([0-9A-F]{8})/)?.[1];
  const rewrite = {
    plan: { author: 'Tamper', change_mode: 'direct', ops: [{ op: 'replace_paragraph', at: sourceId, with: 'Changed source words.' }] },
  };
  const tamperReview = await bundle.host.externalLookup._docx_review('source.docx', rewrite);
  bundle.nextExecution();
  await bundle.host.externalLookup._docx_commit(tamperReview.commit_key);
  bundle.nextExecution();
  await bundle.host.externalLookup._docx_commit(review.commit_key);
  assert.match(bundle.notices.at(-1).summary, /source changed after review/);
});
