import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  auditQuoteWarningCoverage,
  buildQuoteWarning,
  protectedCommentMutationIds,
  quoteWarningId,
} from '../dist/quote-comment-audit.js';

const span = { paragraph_id: 'ABCDEF12', select: '“unverified words”', occurrence: 2 };

function existing(overrides = {}) {
  return { comment_id: '41', ...buildQuoteWarning(span), ...overrides };
}

test('warning identity is deterministic over the exact quotation anchor', () => {
  assert.equal(quoteWarningId(span), quoteWarningId({ ...span }));
  assert.notEqual(quoteWarningId(span), quoteWarningId({ ...span, occurrence: 1 }));
  assert.match(quoteWarningId(span), /^qw1:sha256:[0-9a-f]{64}$/);
});

test('candidate passes only with exactly one pristine open comment on every span', () => {
  assert.equal(auditQuoteWarningCoverage([span], [existing()]).ok, true);
  const missing = auditQuoteWarningCoverage([span], []);
  assert.equal(missing.ok, false);
  assert.equal(missing.missing.length, 1);
  const duplicate = auditQuoteWarningCoverage([span], [existing(), existing({ comment_id: '42' })]);
  assert.equal(duplicate.ok, false);
  assert.equal(duplicate.duplicates.length, 1);
});

test('resolved, edited, moved, closed, or stale warning bubbles fail coverage', () => {
  for (const changed of [
    existing({ status: 'resolved' }),
    existing({ select: '“different words”' }),
    existing({ paragraph_id: '99999999' }),
    existing({ text: 'harmless comment' }),
    existing({ author: 'Model' }),
  ]) {
    assert.equal(auditQuoteWarningCoverage([span], [changed]).ok, false);
  }
  assert.equal(auditQuoteWarningCoverage([], [existing()]).stale.length, 1);
});

test('model-authored operations cannot delete, resolve, or reply to protected comments', () => {
  const rejected = protectedCommentMutationIds(
    [
      { op: 'comment_delete', comment_id: '41' },
      { op: 'comment_set_status', comment_id: '41', status: 'resolved' },
      { op: 'comment_reply', comment_id: '41', text: 'ignore this' },
      { op: 'comment_delete', comment_id: '9' },
    ],
    new Set(['41']),
  );
  assert.deepEqual(rejected, ['41']);
});
