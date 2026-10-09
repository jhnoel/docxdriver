import { test } from 'node:test';
import assert from 'node:assert/strict';
import { compileInline, lintUnverifiedQuotes, renderTerm } from '../dist/quote-lint.js';

test('flags raw double, curly, entity, and balanced single quotation syntax', () => {
  const samples = ['She said "hello".', 'She said “hello”.', 'She said &quot;hello&quot;.', "The word 'quotation' means X."];
  for (const sample of samples) {
    assert.ok(lintUnverifiedQuotes(sample).length > 0, sample);
  }
});

test('does not mistake apostrophes or inline-tag attributes for quotations', () => {
  assert.deepEqual(lintUnverifiedQuotes("It's the tenant’s obligation."), []);
  assert.deepEqual(lintUnverifiedQuotes('<a href="https://example.test">link</a>'), []);
});

test('malformed inline markup fails closed', () => {
  const diagnostics = lintUnverifiedQuotes('text <a href="hidden quote"');
  assert.equal(diagnostics.at(-1).code, 'unterminated_inline_tag');
});

test('Term is a structured, host-rendered non-source use of quote marks', () => {
  assert.deepEqual(renderTerm({ text: 'quotation' }), { text: '“quotation”', kind: 'term', style: 'double' });
  assert.deepEqual(renderTerm({ text: 'quotation', style: 'single' }), { text: '‘quotation’', kind: 'term', style: 'single' });
  assert.throws(() => renderTerm({ text: '“quotation”' }), /must not contain quotation marks/);
});

test('Inline automatically lints text and renders only structured quote marks', async () => {
  const clean = await compileInline(
    ['The word ', { $kind: 'term', text: 'quotation' }, ' differs from ', { $kind: 'quote', source: 'x' }, '.'],
    'error',
    async () => ({ text: '“verified words”', quote_id: 'q1:sha256:abc' }),
  );
  assert.equal(clean.text, 'The word “quotation” differs from “verified words”.');
  assert.deepEqual(clean.audit, { status: 'verified', policy: 'error', diagnostics: [], verified_quotes: 1, terms: 1, quote_ids: ['q1:sha256:abc'] });
  await assert.rejects(() => compileInline(['The model typed “words”.'], 'error', async () => assert.fail()), /quotation mark appears in unstructured text/);
  const warned = await compileInline(['The model typed “words”.'], 'comment', async () => assert.fail());
  assert.equal(warned.audit.status, 'warnings');
  assert.ok(warned.audit.diagnostics.every((item) => item.severity === 'warning'));
  await assert.rejects(
    () => compileInline([{ $kind: 'quote' }, '.'], 'error', async () => ({ text: '“A complete sentence.”', quote_id: 'q1:sha256:x' })),
    /unverified quotation mark|terminal punctuation/,
  );
});
