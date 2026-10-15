import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { PLAN_CAPABILITIES } from '../../docxdriver-pi/scripts/plan-capability-manifest.mjs';
import { init, executeRequest } from '../dist/index.js';

const matrix = JSON.parse(await readFile(new URL('../schema/formatting-capabilities.json', import.meta.url)));
const schema = JSON.parse(await readFile(new URL('../schema/typed-request.schema.json', import.meta.url)));
const ops = new Map(schema.$defs.editOp.oneOf.map(v => [v.properties.op.enum[0], v]));
await init(await readFile(new URL('../wasm/docxdriver_bg.wasm', import.meta.url)));

// Capabilities are deliberately separate from generated request types: they
// describe fidelity and unsupported/preserved-only features, too.
test('formatting capability matrix covers the typed fields and Python exposure', () => {
  assert.equal(matrix.version, 1);
  const ids = new Set();
  for (const row of matrix.capabilities) {
    assert.equal(ids.has(row.id), false, `duplicate ${row.id}`);
    ids.add(row.id);
    assert.ok(['supported', 'partial', 'preserved_only'].includes(row.read), row.id);
    assert.ok(['supported', 'partial', 'unsupported'].includes(row.html_authoring), row.id);
    assert.ok(['supported', 'unsupported'].includes(row.typed_edit), row.id);
    assert.ok(['exposed', 'not_exposed', 'unsupported'].includes(row.python_repl), row.id);
    assert.ok(row.note?.length, row.id);
    const rowOps = row.operations ?? (row.operation ? [row.operation] : []);
    if (row.typed_edit === 'supported') assert.ok(rowOps.length, `${row.id} needs an operation binding`);
    for (const op of rowOps) {
      assert.ok(ops.has(op), `${row.id}: ${op}`);
      assert.equal(row.python_repl, PLAN_CAPABILITIES.ops.includes(op) ? 'exposed' : 'not_exposed', row.id);
    }
    if (row.operation) {
      assert.ok(ops.has(row.operation), row.id);
      for (const field of row.fields) assert.ok(field in ops.get(row.operation).properties, `${row.id}.${field}`);
      assert.equal(row.typed_edit, 'supported', row.id);
      assert.equal(row.python_repl, PLAN_CAPABILITIES.ops.includes(row.operation) ? 'exposed' : 'not_exposed', row.id);
    }
    if (row.typed_edit === 'unsupported') assert.equal(row.python_repl, 'unsupported', row.id);
  }
  for (const op of ['format_text', 'format_paragraph', 'set_page_margins']) {
    const covered = new Set(matrix.capabilities.filter(r => r.operation === op).flatMap(r => r.fields));
    const fields = Object.keys(ops.get(op).properties).filter(f => !['op', 'at', 'select', 'occurrence', 'clear'].includes(f));
    assert.deepEqual([...covered].sort(), fields.sort(), `${op} fields require capability entries`);
  }
});

for (const row of matrix.capabilities.filter(r => r.html_example)) {
  test(`WASM capability: ${row.id}`, () => {
    const result = executeRequest(undefined, { Command: { command: { kind: 'create', html: row.html_example } } });
    if (row.html_authoring === 'unsupported') {
      assert.equal(result.outcome, 'rejected', JSON.stringify(result));
      assert.equal(result.bytes, undefined);
      assert.match(result.diagnostic.message, /unsupported|out of range/);
    } else {
      assert.equal(result.outcome, 'completed', JSON.stringify(result));
      const read = executeRequest(result.bytes, { Command: { command: { kind: 'read', read_kind: 'document_ui', view: 'final' } } });
      assert.equal(read.outcome, 'completed');
      if (row.read_contains) assert.ok(read.result.html.includes(row.read_contains), `${row.id}: ${read.result.html}`);
    }
  });
}
