import { test } from 'node:test';
import assert from 'node:assert/strict';
import { ensureInit, runCreate, runRead } from '../dist/engine.js';
import {
  applyQuoteAuditComments,
  protectedQuoteCommentIds,
  quoteAuditFeatureFromEnv,
  rejectedProtectedQuoteCommentMutations,
  unverifiedQuoteSpans,
} from '../dist/quote-audit-pipeline.js';

test('feature flag is disabled by default and accepts explicit truthy values', () => {
  assert.deepEqual(quoteAuditFeatureFromEnv({}), { enabled: false, provenancePolicy: 'permissive' });
  for (const value of ['1', 'true', 'YES', 'on']) {
    assert.deepEqual(quoteAuditFeatureFromEnv({ DOCXDRIVER_QUOTE_AUDIT_COMMENTS: value }), { enabled: true, provenancePolicy: 'permissive' });
  }
});

test('provenance policy is gated by the audit flag and validates reserved modes', () => {
  assert.deepEqual(quoteAuditFeatureFromEnv({ DOCXDRIVER_QUOTE_PROVENANCE_POLICY: 'not-a-policy' }), { enabled: false, provenancePolicy: 'permissive' });
  for (const provenancePolicy of ['permissive', 'controlled', 'authoritative']) {
    assert.deepEqual(
      quoteAuditFeatureFromEnv({ DOCXDRIVER_QUOTE_AUDIT_COMMENTS: '1', DOCXDRIVER_QUOTE_PROVENANCE_POLICY: provenancePolicy }),
      { enabled: true, provenancePolicy },
    );
  }
  assert.throws(
    () => quoteAuditFeatureFromEnv({ DOCXDRIVER_QUOTE_AUDIT_COMMENTS: '1', DOCXDRIVER_QUOTE_PROVENANCE_POLICY: 'unknown' }),
    /must be permissive, controlled, or authoritative/,
  );
});

test('scanner returns exact paired and unmatched quotation anchors', () => {
  const spans = unverifiedQuoteSpans({ index: 4, id: '12345678', text: 'She said “hello” and left a "mark.' });
  assert.deepEqual(spans.map(({ select, occurrence, locator }) => ({ select, occurrence, locator })), [
    { select: '“hello”', occurrence: 1, locator: 'p4:9-16' },
    { select: '"', occurrence: 1, locator: 'p4:28-29' },
  ]);
});

test('feature pass reuses core comments and proves final exact-span coverage', async () => {
  await ensureInit();
  const created = runCreate(undefined, undefined, '<p>She said “hello” and called it <b>important</b>.</p><p>No quotation here.</p>');
  assert.equal(created.outcome, 'completed');
  const audited = applyQuoteAuditComments(created.bytes);
  assert.equal(audited.warnings, 1);
  assert.equal(audited.protectedCommentIds.length, 1);
  assert.deepEqual(protectedQuoteCommentIds(audited.bytes), audited.protectedCommentIds);
  assert.deepEqual(rejectedProtectedQuoteCommentMutations(audited.bytes, [{ op: 'comment_delete', comment_id: audited.protectedCommentIds[0] }]), audited.protectedCommentIds);
  const read = runRead(audited.bytes, { kind: 'comments' });
  assert.equal(read.outcome, 'completed');
  const thread = read.result.comments[0];
  assert.equal(thread.status, 'open');
  assert.equal(thread.root.author, 'docxdriver quotation audit');
  assert.match(thread.root.text, /UNVERIFIED QUOTATION/);
  assert.deepEqual(thread.anchor.ranges, [{ locator: 'p1:9-16' }]);
});

test('reviewed structured renderings are exempt while raw quotations still receive warnings', async () => {
  await ensureInit();
  const created = runCreate(undefined, undefined, '<p>Verified “source words” and raw “model words”.</p>');
  const audited = applyQuoteAuditComments(created.bytes, {
    trustedRenderings: [{ kind: 'quote', text: '“source words”', quote_id: 'q1:sha256:test' }],
  });
  assert.equal(audited.warnings, 1);
  const read = runRead(audited.bytes, { kind: 'comments' });
  assert.deepEqual(read.result.comments[0].anchor.ranges, [{ locator: 'p1:32-45' }]);
});

test('ambiguous structured rendering fails closed', async () => {
  await ensureInit();
  const created = runCreate(undefined, undefined, '<p>“same” and “same”.</p>');
  assert.throws(
    () => applyQuoteAuditComments(created.bytes, { trustedRenderings: [{ kind: 'term', text: '“same”' }] }),
    /missing or ambiguous/,
  );
});

test('second pass reconciles its own prior warning instead of duplicating it', async () => {
  await ensureInit();
  const created = runCreate(undefined, undefined, '<p>“Again.”</p>');
  const once = applyQuoteAuditComments(created.bytes);
  const twice = applyQuoteAuditComments(once.bytes);
  assert.equal(twice.warnings, 1);
  assert.equal(protectedQuoteCommentIds(twice.bytes).length, 1);
});

test('feature fails closed when quotation marks occur in unaddressable chrome', async () => {
  await ensureInit();
  const created = runCreate(undefined, undefined, '<header><p>Header says “draft”.</p></header><p>Body.</p>');
  assert.equal(created.outcome, 'completed');
  assert.throws(() => applyQuoteAuditComments(created.bytes), /cannot carry a comment anchor/);
});
