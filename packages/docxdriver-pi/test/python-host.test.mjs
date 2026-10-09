/* Combined review/commit host protocol tests. */
import { test, before, beforeEach, after } from 'node:test';
import assert from 'node:assert/strict';
import { lstat, mkdtemp, readFile, readdir, realpath, rename, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  CommitKeyStore,
  COMMIT_KEY_RE,
  DEFAULT_HOST_LIMITS,
  MAX_PROTOCOL_LINE_BYTES,
  CommitKeyTooLargeError,
  createPlanPythonHost,
  estimateRecordBytes,
  protocolLinesFor,
  renderNotices,
  reviewEdits,
  validateCandidate,
} from '../dist/python-host.js';
import { ensureInit, runPlan } from '../dist/engine.js';
import { writeDocxBytes } from '../dist/io.js';

let cwd;

before(async () => {
  await ensureInit();
});

beforeEach(async () => {
  cwd = await mkdtemp(join(tmpdir(), 'docxdriver-pi-combined-review-'));
});

after(async () => {
  if (cwd) await rm(cwd, { recursive: true, force: true });
});

function makeHost(options = {}) {
  const notices = [];
  const store = new CommitKeyStore(options.store);
  let execution = 1;
  let generation = 0;
  const host = createPlanPythonHost(
    cwd,
    notices,
    store,
    () => execution,
    () => generation,
    undefined,
    options,
  );
  return {
    host,
    notices,
    store,
    setExecution(value) { execution = value; },
    setGeneration(value) { generation = value; },
    generation: () => generation,
  };
}

function planState(ops, author = 'test') {
  return { plan: { author, change_mode: 'track', ops } };
}

function replaceTextOp(at, select = 'old', with_ = 'new') {
  return { op: 'replace_text', at, select, with: with_ };
}

async function createFixture(host, html = '<p>Hello old world</p>') {
  await host.externalLookup._docx_create('demo.docx', html);
  const document = await host.externalLookup._docx_read('demo.docx');
  const id = document.markup.match(/<p id="([0-9A-F]{8})/)?.[1];
  assert.ok(id, `fixture markup must contain a paragraph id: ${document.markup}`);
  return { id, bytes: await readFile(join(cwd, 'demo.docx')) };
}

async function reviewFixture(bundle, op = replaceTextOp(bundle.id)) {
  const result = await bundle.host.externalLookup._docx_review('demo.docx', planState([op]));
  assert.ok(result, 'review should succeed');
  assert.match(result.commit_key, /^k1:[0-9a-f]{32}$/);
  return result;
}

test('combined review validates once, returns compact key and edit context, then key-only commit consumes it', async () => {
  const bundle = makeHost();
  const fixture = await createFixture(bundle.host);
  const result = await reviewFixture(bundle, replaceTextOp(fixture.id));

  assert.equal(result.edits.length, 1);
  assert.equal(result.edits[0].index, 1);
  assert.equal(result.edits[0].outcome, 'applied');
  assert.match(JSON.stringify(result.edits[0].context), /new/);
  assert.equal(typeof result.truncated, 'boolean');
  assert.equal(bundle.notices.at(-1).kind, 'review');
  assert.equal(bundle.notices.at(-1).commitKey, result.commit_key);

  bundle.setExecution(2);
  await bundle.host.externalLookup._docx_commit(result.commit_key);
  assert.equal(bundle.notices.at(-1).kind, 'commit');
  assert.equal(bundle.notices.at(-1).commitKey, result.commit_key);
  assert.equal(bundle.store.size, 0);
  assert.match((await bundle.host.externalLookup._docx_read('demo.docx', 'final')).markup, /new/);

  await bundle.host.externalLookup._docx_commit(result.commit_key);
  assert.match(bundle.notices.at(-1).summary, /already consumed|unknown commit key/);
});

test('invalid review creates no commit key and plan reports must be complete before key issuance', async () => {
  const bundle = makeHost();
  const fixture = await createFixture(bundle.host);
  const result = await bundle.host.externalLookup._docx_review(
    'demo.docx',
    planState([replaceTextOp(fixture.id, 'missing', 'new')]),
  );
  assert.equal(result, null);
  assert.equal(bundle.store.size, 0);
  assert.equal(bundle.notices.at(-1).status, 'blocked');
  assert.ok(!bundle.notices.at(-1).commitKey);
});

test('same-execution commit is blocked while the reviewed key remains usable', async () => {
  const bundle = makeHost();
  const fixture = await createFixture(bundle.host);
  const result = await reviewFixture(bundle, replaceTextOp(fixture.id));

  await bundle.host.externalLookup._docx_commit(result.commit_key);
  assert.match(bundle.notices.at(-1).summary, /later python execution/);
  assert.equal(bundle.store.size, 1);
  bundle.setExecution(2);
  await bundle.host.externalLookup._docx_commit(result.commit_key);
  assert.equal(bundle.notices.at(-1).kind, 'commit');
});

test('source race consumes the key and preserves the changed file', async () => {
  const bundle = makeHost();
  const fixture = await createFixture(bundle.host);
  const result = await reviewFixture(bundle, replaceTextOp(fixture.id));
  const changed = Buffer.from(fixture.bytes).toString('base64') + '\nchanged';
  await writeFile(join(cwd, 'demo.docx'), changed);

  bundle.setExecution(2);
  await bundle.host.externalLookup._docx_commit(result.commit_key);
  assert.match(bundle.notices.at(-1).summary, /source changed/);
  assert.equal(bundle.store.consumedSize, 1);
  assert.equal((await readFile(join(cwd, 'demo.docx'), 'utf8')).endsWith('changed'), true);
});

test('path alias swap during commit is rejected and preserves the symlink', async () => {
  let armed = false;
  const bundle = makeHost({
    io: {
      readFile: async (abs) => {
        if (armed) {
          const alias = join(cwd, 'alias.docx');
          await rename(abs, alias);
          await symlink(alias, abs);
        }
        return new Uint8Array(await readFile(abs));
      },
    },
  });
  const fixture = await createFixture(bundle.host);
  const result = await reviewFixture(bundle, replaceTextOp(fixture.id));
  armed = true;
  bundle.setExecution(2);
  await bundle.host.externalLookup._docx_commit(result.commit_key);
  assert.match(bundle.notices.at(-1).summary, /path changed after review/);
  assert.equal(bundle.store.consumedSize, 1);
  assert.equal((await lstat(join(cwd, 'demo.docx'))).isSymbolicLink(), true);
});

test('read-back failure after rename consumes the key and reports completed mutation', async () => {
  const bundle = makeHost({ readBack: async () => { throw new Error('injected read-back failure'); } });
  const fixture = await createFixture(bundle.host);
  const result = await reviewFixture(bundle, replaceTextOp(fixture.id));
  bundle.setExecution(2);
  await assert.rejects(() => bundle.host.externalLookup._docx_commit(result.commit_key), /injected read-back failure/);
  assert.equal(bundle.store.consumedSize, 1);
  assert.match(bundle.notices.at(-1).summary, /write completed/);
  assert.match((await bundle.host.externalLookup._docx_read('demo.docx', 'final')).markup, /new/);
});

test('cancellation after rename reports success and consumes the key', async () => {
  const controller = new AbortController();
  const bundle = makeHost({
    signal: controller.signal,
    io: {
      writeDocxBytes: async (...args) => {
        const result = await writeDocxBytes(...args);
        controller.abort();
        return result;
      },
    },
  });
  const fixture = await createFixture(bundle.host);
  const result = await reviewFixture(bundle, replaceTextOp(fixture.id));
  bundle.setExecution(2);
  await bundle.host.externalLookup._docx_commit(result.commit_key);
  assert.equal(bundle.store.consumedSize, 1);
  assert.match(bundle.notices.at(-1).summary, /cancellation arrived during commit; write completed/);
});

test('pre-write commit timeout leaves the key retryable', async () => {
  let armed = false;
  const bundle = makeHost({
    limits: { maxHostCallbackMs: 100 },
    io: {
      readFile: async (abs) => {
        if (armed) await new Promise((resolve) => setTimeout(resolve, 250));
        return new Uint8Array(await readFile(abs));
      },
    },
  });
  const fixture = await createFixture(bundle.host);
  const result = await reviewFixture(bundle, replaceTextOp(fixture.id));
  armed = true;
  bundle.setExecution(2);
  await assert.rejects(() => bundle.host.externalLookup._docx_commit(result.commit_key), /timed out|host callback/);
  assert.equal(bundle.store.lookup(result.commit_key, bundle.generation()).record.state, 'reviewed');
});

test('successful commit is atomic and leaves no temporary sibling', async () => {
  const bundle = makeHost();
  const fixture = await createFixture(bundle.host);
  const result = await reviewFixture(bundle, replaceTextOp(fixture.id));
  const before = await readFile(join(cwd, 'demo.docx'));

  bundle.setExecution(2);
  await bundle.host.externalLookup._docx_commit(result.commit_key);
  const afterBytes = await readFile(join(cwd, 'demo.docx'));
  assert.notDeepEqual(afterBytes, before);
  assert.match((await bundle.host.externalLookup._docx_read('demo.docx', 'final')).markup, /new/);
  const leftovers = (await readdir(cwd)).filter((name) => name.includes('.tmp') || name.includes('.docxdriver'));
  assert.deepEqual(leftovers, []);
});

test('candidate validation failure consumes the key without writing', async () => {
  const bundle = makeHost({ validateCandidate: async () => ({ ok: false, message: 'injected candidate rejection' }) });
  const fixture = await createFixture(bundle.host);
  const result = await reviewFixture(bundle, replaceTextOp(fixture.id));
  const before = await readFile(join(cwd, 'demo.docx'));

  bundle.setExecution(2);
  await bundle.host.externalLookup._docx_commit(result.commit_key);
  assert.match(bundle.notices.at(-1).summary, /candidate verification failed/);
  assert.deepEqual(await readFile(join(cwd, 'demo.docx')), before);
  assert.equal(bundle.store.consumedSize, 1);
});

test('pre-write infrastructure failure keeps the key retryable', async () => {
  let fail = true;
  const bundle = makeHost({
    io: {
      writeDocxBytes: async (...args) => {
        if (fail) {
          fail = false;
          throw new Error('injected write failure');
        }
        return writeDocxBytes(...args);
      },
    },
  });
  const fixture = await createFixture(bundle.host);
  const result = await reviewFixture(bundle, replaceTextOp(fixture.id));

  bundle.setExecution(2);
  await assert.rejects(() => bundle.host.externalLookup._docx_commit(result.commit_key), /injected write failure/);
  assert.equal(bundle.store.size, 1);
  bundle.setExecution(3);
  await bundle.host.externalLookup._docx_commit(result.commit_key);
  assert.equal(bundle.store.size, 0);
});

test('cancellation before write keeps the key retryable and target untouched', async () => {
  const controller = new AbortController();
  const bundle = makeHost({
    signal: controller.signal,
    validateCandidate: async (candidate, report, count) => {
      const result = await validateCandidate(candidate, report, count);
      controller.abort();
      return result;
    },
  });
  const fixture = await createFixture(bundle.host);
  const result = await reviewFixture(bundle, replaceTextOp(fixture.id));
  const before = await readFile(join(cwd, 'demo.docx'));

  bundle.setExecution(2);
  await assert.rejects(() => bundle.host.externalLookup._docx_commit(result.commit_key), /aborted|cancel/);
  assert.deepEqual(await readFile(join(cwd, 'demo.docx')), before);
  assert.equal(bundle.store.size, 1);
  assert.equal(bundle.store.lookup(result.commit_key, bundle.generation()).record.state, 'reviewed');
});

test('generation invalidation expires keys', async () => {
  const bundle = makeHost();
  const fixture = await createFixture(bundle.host);
  const result = await reviewFixture(bundle, replaceTextOp(fixture.id));
  bundle.setGeneration(1);
  bundle.setExecution(2);
  await bundle.host.externalLookup._docx_commit(result.commit_key);
  assert.match(bundle.notices.at(-1).summary, /commit key expired/);
});

test('FIFO eviction tombstones the oldest key and lifecycle invalidation expires active keys', async () => {
  const store = new CommitKeyStore({ maxRecords: 1, maxBytes: 4096 });
  const record = (id) => ({
    id,
    state: 'reviewed',
    runtimeGeneration: 0,
    cwdRoot: '/tmp',
    canonicalAbsPath: '/tmp/a.docx',
    displayPath: 'a.docx',
    sourceHash: 'a'.repeat(64),
    canonicalPlan: '{}',
    corePreviewKey: 'p1:sha256:' + 'b'.repeat(64),
    issuedExecution: 1,
  });
  const first = 'k1:' + '1'.repeat(32);
  const second = 'k1:' + '2'.repeat(32);
  store.put(record(first));
  store.put(record(second));
  assert.equal(store.lookup(first, 0).kind, 'expired');
  assert.equal(store.lookup(second, 0).kind, 'ok');
  store.invalidateAll();
  assert.equal(store.lookup(second, 0).kind, 'expired');
});

test('candidate exceeding the input limit is rejected before write and consumes the key', async () => {
  const setup = makeHost({ limits: { maxDocxBytes: 2 * 1024 * 1024 } });
  await createFixture(setup.host, '<p>Hello old world</p>');
  const sourceBytes = await readFile(join(cwd, 'demo.docx'));
  const bounded = makeHost({ limits: { maxDocxBytes: sourceBytes.length } });
  const document = await bounded.host.externalLookup._docx_read('demo.docx');
  const id = document.markup.match(/<p id="([0-9A-F]{8})/)?.[1];
  const result = await bounded.host.externalLookup._docx_review(
    'demo.docx',
    planState([replaceTextOp(id, 'old', 'x'.repeat(500_000))]),
  );
  assert.ok(result);
  bounded.setExecution(2);
  await bounded.host.externalLookup._docx_commit(result.commit_key);
  assert.match(bounded.notices.at(-1).summary, /candidate exceeds input limit/);
  assert.equal(bounded.store.consumedSize, 1);
  assert.deepEqual(await readFile(join(cwd, 'demo.docx')), sourceBytes);
});

test('review context limits are explicit and aggregate result limits are enforced', async () => {
  const bounded = makeHost({ limits: { maxEditContextBytes: 16, maxReviewResultBytes: 512 } });
  const fixture = await createFixture(bounded.host, `<p>${'old '.repeat(100)}</p>`);
  const result = await reviewFixture(bounded, { ...replaceTextOp(fixture.id, 'old '.repeat(100), 'new'), occurrence: 1 });
  assert.equal(result.truncated, true);
  assert.ok(Buffer.byteLength(JSON.stringify(result), 'utf8') <= 512);
  assert.equal(result.edits[0].context_truncated, true);
  for (const context of result.edits[0].context) {
    assert.ok(Buffer.byteLength(JSON.stringify(context), 'utf8') <= 16);
  }

  const rejected = makeHost({ limits: { maxReviewResultBytes: 32 } });
  const rejectedFixture = await createFixture(rejected.host);
  const rejectedResult = await rejected.host.externalLookup._docx_review(
    'demo.docx',
    planState([{ ...replaceTextOp(rejectedFixture.id), occurrence: 1 }]),
  );
  assert.equal(rejectedResult, null);
  assert.equal(rejected.store.size, 0);
  assert.match(rejected.notices.at(-1).summary, /review result exceeds/);
});

test('per-edit context budget is aggregate and preserves entry order', () => {
  const report = {
    ops: [{
      index: 0,
      op: 'replace_text',
      outcome: 'applied',
      summary: 'changed',
      affected: [
        { para_id: 'AAAA0001', markup: 'first context' },
        { para_id: 'BBBB0002', markup: 'second context' },
        { para_id: 'CCCC0003', markup: 'third context' },
      ],
    }],
  };
  const bounded = reviewEdits(report, 64, 4096);
  const edit = bounded.edits[0];
  assert.equal(edit.context_truncated, true);
  assert.equal(edit.context.length, 1);
  assert.equal(edit.context[0].para_id, 'AAAA0001');
  assert.ok(Buffer.byteLength(JSON.stringify(edit.context), 'utf8') <= 64);
  assert.equal(bounded.withinLimit, true);

  const exact = Buffer.byteLength(JSON.stringify({ commit_key: `k1:${'a'.repeat(32)}`, edits: bounded.edits, truncated: bounded.truncated }), 'utf8');
  const atBoundary = reviewEdits(report, 64, exact);
  assert.equal(atBoundary.withinLimit, true);
  assert.ok(Buffer.byteLength(JSON.stringify({ commit_key: `k1:${'a'.repeat(32)}`, edits: atBoundary.edits, truncated: atBoundary.truncated }), 'utf8') <= exact);
});

test('store byte accounting and protected protocol use compact commit keys', () => {
  const key = 'k1:' + 'a'.repeat(32);
  assert.equal(COMMIT_KEY_RE.test(key), true);
  const store = new CommitKeyStore({ maxRecords: 1, maxBytes: 4096 });
  const record = {
    id: key,
    state: 'reviewed',
    runtimeGeneration: 0,
    cwdRoot: '/tmp',
    canonicalAbsPath: '/tmp/a.docx',
    displayPath: 'a.docx',
    sourceHash: 'a'.repeat(64),
    canonicalPlan: '{}',
    corePreviewKey: 'p1:sha256:' + 'b'.repeat(64),
    issuedExecution: 1,
  };
  assert.ok(estimateRecordBytes(record) > 0);
  store.put(record);
  assert.equal(store.size, 1);
  const notices = [
    { kind: 'review', status: 'ok', commitKey: key, summary: 'review ok' },
    { kind: 'commit', status: 'ok', commitKey: key, summary: 'committed: 1 ops' },
  ];
  assert.deepEqual(protocolLinesFor(notices), [`review: ${key} — review ok`, `commit: ${key} — committed: 1 ops`]);
  assert.ok(protocolLinesFor(notices).every((line) => Buffer.byteLength(line, 'utf8') <= MAX_PROTOCOL_LINE_BYTES));
  assert.match(renderNotices(notices), /commit_key: k1:a{32}/);
  assert.match(protocolLinesFor(notices).at(-1), /^commit: k1:a{32}/);
  const oversized = new CommitKeyStore({ maxBytes: 1 });
  assert.throws(() => oversized.put(record), CommitKeyTooLargeError);
  assert.match(new CommitKeyTooLargeError(1, 2).message, /commit key exceeds/);
});

test('read and find remain available through the combined host', async () => {
  const bundle = makeHost();
  const fixture = await createFixture(bundle.host, '<p>Hello old world</p><p>Another old paragraph</p>');
  const read = await bundle.host.externalLookup._docx_read('demo.docx');
  assert.match(read.markup, /Hello old world/);
  const matches = await bundle.host.externalLookup._docx_find('demo.docx', 'old');
  assert.equal(matches.length, 2);
  assert.ok(fixture.id);
});

test('_docx_read supports comments kind listing', async () => {
  const bundle = makeHost();
  await createFixture(bundle.host);
  const comments = await bundle.host.externalLookup._docx_read('demo.docx', 'markup', 'comments');
  assert.equal(comments.kind, 'comments');
  assert.ok(Array.isArray(comments.comments));
  assert.equal(comments.comments.length, 0);
  const document = await bundle.host.externalLookup._docx_read('demo.docx');
  assert.match(document.markup, /Hello/);
});

// Keep the imported core runner exercised by the host test suite: the key
// stores the exact canonical plan that commit replays against the core key.
test('stored reviewed plan is core-canonical', async () => {
  const bundle = makeHost();
  const fixture = await createFixture(bundle.host);
  const state = planState([replaceTextOp(fixture.id)]);
  const reviewed = await bundle.host.externalLookup._docx_review('demo.docx', state);
  const source = await readFile(join(cwd, 'demo.docx'));
  const output = runPlan(source, JSON.parse(bundle.store.lookup(reviewed.commit_key, 0).record.canonicalPlan));
  assert.equal(output.outcome, 'previewed');
});
