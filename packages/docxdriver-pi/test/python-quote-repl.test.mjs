import { test } from 'node:test';
import assert from 'node:assert/strict';
import { PythonRepl } from '../dist/python-repl.js';
import { QUOTE_PYTHON_PRELUDE, QUOTE_PYTHON_TYPE_STUBS } from '../dist/python-quote-prelude.js';

test('Python quote builder sends a structured spec and exposes only the host result', async () => {
  const repl = await PythonRepl.create(QUOTE_PYTHON_PRELUDE, QUOTE_PYTHON_TYPE_STUBS);
  try {
    let received;
    const result = await repl.execute(
      `q = Quote("source.docx", "ABCDEF12", "alpha beta gamma")
q.omit(" beta")
q.bracket("alpha", "Alpha")
validated = quote_validate(q)
print(validated.text)`,
      {
        _quote_find: async () => [],
        _quote_validate: async (spec) => {
          received = spec;
          return { text: '“[Alpha]… gamma”', quote_id: 'q1:sha256:test' };
        },
      },
    );
    assert.equal(result.ok, true, result.error);
    assert.equal(result.stdout, '“[Alpha]… gamma”\n');
    assert.equal(received.get('source'), 'source.docx');
    assert.equal(received.get('at'), 'ABCDEF12');
    assert.equal(received.get('changes').length, 2);
  } finally {
    await repl.close();
  }
});

test('Python Inline serializes text, Term, and Quote nodes for mandatory host validation', async () => {
  const repl = await PythonRepl.create(QUOTE_PYTHON_PRELUDE, QUOTE_PYTHON_TYPE_STUBS);
  try {
    let received;
    const result = await repl.execute(
      `q = Quote("source.docx", "ABCDEF12", "source words")
content = Inline(["The word ", Term("quotation"), " differs from ", q])
compiled = inline_validate(content)
print(compiled.text)
print(compiled.audit["status"])`,
      {
        _quote_find: async () => [],
        _quote_validate: async () => ({}),
        _quote_lint: async () => [],
        _term_render: async () => ({}),
        _inline_validate: async (inline, policy) => {
          received = { inline, policy };
          return { text: 'The word “quotation” differs from “source words”', audit: { status: 'verified' } };
        },
      },
    );
    assert.equal(result.ok, true, result.error);
    assert.equal(result.stdout, 'The word “quotation” differs from “source words”\nverified\n');
    assert.equal(received.policy, 'error');
    const parts = received.inline.get('parts');
    assert.equal(parts.length, 4);
    assert.equal(parts[1].get('$kind'), 'term');
    assert.equal(parts[3].get('$kind'), 'quote');
  } finally {
    await repl.close();
  }
});
