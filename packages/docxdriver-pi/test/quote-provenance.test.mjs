import { test } from 'node:test';
import assert from 'node:assert/strict';
import { resolveQuote, QuoteValidationError } from '../dist/quote-provenance.js';

const paragraph = 'The tenant must promptly pay the charge, including every documented fee, within thirty days.';
const base = {
  source: 'lease.docx',
  at: '89ABCDEF',
  select: paragraph,
};

test('resolves exact quote and host-renders bracket and omission adaptations', () => {
  const result = resolveQuote(
    {
      ...base,
      changes: [
        { kind: 'omit', select: ', including every documented fee,' },
        { kind: 'bracket', select: 'tenant', replacement: 'Tenant' },
        { kind: 'bracket', select: 'thirty', replacement: '30' },
      ],
    },
    paragraph,
    'source-hash',
  );
  assert.equal(result.content, 'The [Tenant] must promptly pay the charge… within [30] days.');
  assert.equal(result.text, '“The [Tenant] must promptly pay the charge… within [30] days.”');
  assert.equal(result.original, paragraph);
  assert.deepEqual(result.span, { start: 0, end: paragraph.length, occurrence: 1 });
  assert.equal(result.changes[0].original, 'tenant');
  assert.equal(result.source_sha256, 'source-hash');
  assert.match(result.quote_id, /^q1:sha256:[0-9a-f]{64}$/);
  assert.match(result.text_sha256, /^[0-9a-f]{64}$/);
  assert.match(result.paragraph_sha256, /^[0-9a-f]{64}$/);
});

test('quote occurrence disambiguates repeated exact spans', () => {
  const result = resolveQuote({ ...base, select: 'the charge', occurrence: 2 }, `${paragraph} Then the charge was disputed.`, 'h');
  assert.equal(result.span.start, `${paragraph} Then `.length);
  assert.equal(result.original, 'the charge');
});

test('change selectors always address the original quote', () => {
  const result = resolveQuote(
    { ...base, select: 'fee fee', changes: [{ kind: 'bracket', select: 'fee', replacement: 'cost', occurrence: 2 }] },
    'fee fee',
    'h',
  );
  assert.equal(result.text, '“fee [cost]”');
});

test('fails closed for fabricated, ambiguous, invalid, and overlapping adaptations', () => {
  assert.throws(() => resolveQuote({ ...base, select: 'Tenant must pay' }, paragraph, 'h'), QuoteValidationError);
  assert.throws(
    () => resolveQuote({ ...base, changes: [{ kind: 'omit', select: 'missing words' }] }, paragraph, 'h'),
    /was not found exactly/,
  );
  assert.throws(
    () => resolveQuote({ ...base, changes: [{ kind: 'bracket', select: 'tenant', replacement: '[Tenant]' }] }, paragraph, 'h'),
    /must not contain brackets/,
  );
  assert.throws(
    () => resolveQuote({ ...base, changes: [{ kind: 'omit', select: 'tenant must' }, { kind: 'bracket', select: 'must promptly', replacement: 'shall promptly' }] }, paragraph, 'h'),
    /overlap/,
  );
  assert.throws(() => resolveQuote({ ...base, style: 'loud' }, paragraph, 'h'), /style/);
});
