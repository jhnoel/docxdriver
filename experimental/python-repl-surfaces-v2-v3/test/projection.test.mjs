// Shared canonical-markup parser/serializer contract tests.
// Round-trip ground truth is generated with the real checked-in WASM engine
// (create + read), so parser assumptions are checked against the renderer's
// actual emission, not only hand-written strings.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { ensureInit, runCreate, runRead } from '../dist/engine.js';
import { ProjectionError, parseProjection, parseProjectionLoose, serializeProjection } from '../dist/projection.js';

const blankFmt = { bold: false, italic: false, underline: false, strike: false, superscript: false, subscript: false };

test('parses the canonical render of a simple document', () => {
  const doc = parseProjection('<p id="5E27EE16" ord="1">Payment is due within <b>30 days</b>.</p>\n<p id="69D4601B" ord="2">Second <i>terms</i>.</p>');
  assert.equal(doc.paragraphs.length, 2);
  const [first, second] = doc.paragraphs;
  assert.equal(first.id, '5E27EE16');
  assert.equal(first.tag, 'p');
  assert.equal(first.plainText, 'Payment is due within 30 days.');
  assert.deepEqual(first.items, [
    { kind: 'text', text: 'Payment is due within ', fmt: blankFmt },
    { kind: 'text', text: '30 days', fmt: { ...blankFmt, bold: true } },
    { kind: 'text', text: '.', fmt: blankFmt },
  ]);
  assert.equal(second.items[1].fmt.italic, true); // items[0] is the leading "Second " run
});

test('round-trip: serialize(parse(markup)) is stable and attribute-ordered', () => {
  const markup = '<p id="A1B2C3D4" ord="1" class="Heading1" num="1.">One <u>under</u>.</p>';
  assert.equal(serializeProjection(parseProjection(markup)), markup);
});

test('protected items: link, field, del/ins revisions and br parse without loss', () => {
  const markup = '<p id="A1B2C3D4" ord="1">See <a href="https://x.example">the x</a> in <field instr="DATE">1/1/26</field><del id="r1" author="a"> old</del><ins id="r2" author="b"> new</ins><br/> done</p>';
  const doc = parseProjection(markup);
  const items = doc.paragraphs[0].items;
  assert.equal(items.filter((i) => i.kind === 'link').length, 1);
  assert.equal(items.filter((i) => i.kind === 'revision').length, 2);
  assert.equal(serializeProjection(doc), markup);
});

test('rejects malformed markup with a located message', () => {
  for (const bad of ['<p>unclosed', '<p id="x" ord="1">', '<p id="A" ord="1"><b>no close</p>', '<div>x</div>', '<p id="A" ord="1"><unknown>x</unknown></p>']) {
    assert.throws(() => parseProjection(bad), ProjectionError, `expected rejection for ${bad}`);
  }
});

test('treats whitespace between blocks as insignificant', () => {
  const a = parseProjection('<p id="A0000001" ord="1">x</p>\n<p id="B0000002" ord="2">y</p>');
  const b = parseProjection('<p id="A0000001" ord="1">x</p><p id="B0000002" ord="2">y</p>');
  assert.deepEqual(a, b);
});

test('loose parse permits a missing id but still rejects invalid ids', () => {
  const doc = parseProjectionLoose('<p>new paragraph</p>');
  assert.equal(doc.paragraphs[0].id, null);
  assert.equal(doc.paragraphs[0].plainText, 'new paragraph');
  assert.throws(() => parseProjectionLoose('<p id="xyz">bad</p>'), ProjectionError);
  assert.throws(() => parseProjection('<p>missing id</p>'), ProjectionError);
});

test('engine round-trip: rich document (headings, fmt, link, br, tab, table)', async () => {
  await ensureInit();
  const html = '<h1>Heading</h1><p>Plain with <b>bold</b> <i>ital</i> <u>und</u> <s>strike</s> sup<sup>2</sup> sub<sub>3</sub>.</p><p>A <a href="https://x.example">link</a> and a <br/> break and a tab:\there.</p><table><tr><td><p>c1</p></td><td><p>c2</p></td></tr></table><p>After table.</p>';
  const created = runCreate(undefined, undefined, html);
  assert.ok(created.bytes, 'create produced bytes');
  const read = runRead(created.bytes, { kind: 'document', view: 'markup' });
  assert.equal(read.outcome, 'completed', read.diagnostic?.message);
  const markup = read.result.markup;
  const doc = parseProjection(markup);
  assert.equal(doc.blocks.length, 5, 'h1, p, p, table, p');
  assert.equal(doc.blocks.filter((b) => b.kind === 'protected').length, 1, 'one table block');
  assert.equal(serializeProjection(doc), markup);
});

test('engine round-trip: tracked revisions nest formatting inside del/ins', async () => {
  await ensureInit();
  const { createHash } = await import('node:crypto');
  const created = runCreate(undefined, undefined, '<p>Hello <b>world</b> end.</p>');
  const source = `sha256:${createHash('sha256').update(Buffer.from(created.bytes)).digest('hex')}`;
  const read0 = runRead(created.bytes, { kind: 'document', view: 'markup' });
  const id = read0.result.blocks[0].paragraphs[0];
  const plan = { base: source, author: 'Pi', change_mode: 'track', ops: [{ op: 'replace_text', at: id, select: 'world', with: 'earth' }] };
  const preview = (await import('docxdriver')).executeRequest(created.bytes, { Plan: { plan } });
  const commit = (await import('docxdriver')).executeRequest(created.bytes, { Plan: { plan, preview_key: preview.preview_key } });
  const read = runRead(commit.bytes, { kind: 'document', view: 'markup' });
  const markup = read.result.markup;
  assert.match(markup, /<del id="[^"]+" author="Pi"><b>world<\/b><\/del><ins/);
  assert.equal(serializeProjection(parseProjection(markup)), markup);
});

test('engine round-trip: real-world field-heavy document', async () => {
  await ensureInit();
  const bytes = new Uint8Array(await readFile(fileURLToPath(new URL('../../../test-docs/ctnf-18690238-data-stream.docx', import.meta.url))));
  const read = runRead(bytes, { kind: 'document', view: 'markup' });
  assert.equal(read.outcome, 'completed', read.diagnostic?.message);
  const markup = read.result.markup;
  const doc = parseProjection(markup);
  assert.ok(doc.paragraphs.length > 50, `expected a large doc, got ${doc.paragraphs.length}`);
  const fieldParagraph = doc.paragraphs.find((p) => p.items.some((i) => i.kind === 'field'));
  assert.ok(fieldParagraph, 'fixture contains fields');
  assert.equal(serializeProjection(doc), markup);
});

test('plainText concatenates decoded text, link/field/revision text, and skips markup', () => {
  const doc = parseProjection('<p id="A1B2C3D4" ord="1">a <a href="https://x.example">b</a> <field instr="DATE">c</field><del id="r1" author="a"> d</del><br/> e</p>');
  assert.equal(doc.paragraphs[0].plainText, 'a b c d e');
});

test('entities decode in text and attrs and re-escape on serialize', () => {
  const markup = '<p id="A1B2C3D4" ord="1">&lt;tag&gt; &amp; stuff <a href="https://x.example?a=1&amp;b=2">x</a></p>';
  const doc = parseProjection(markup);
  assert.equal(doc.paragraphs[0].plainText, '<tag> & stuff x');
  assert.equal(serializeProjection(doc), markup);
});

test('engine round-trip: fmt stays open across br (review finding 1)', async () => {
  await ensureInit();
  const created = runCreate(undefined, undefined, '<p><b>line1<br/>line2</b></p>');
  const read = runRead(created.bytes, { kind: 'document', view: 'markup' });
  assert.equal(read.outcome, 'completed', read.diagnostic?.message);
  const markup = read.result.markup;
  assert.match(markup, /<b>line1<br\/>line2<\/b>/);
  assert.equal(serializeProjection(parseProjection(markup)), markup);
});

test('engine round-trip: fmt stays open across image (review finding 1)', async () => {
  await ensureInit();
  const png = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==';
  const created = runCreate(undefined, undefined, `<p><b>before<image src="${png}"/>after</b></p>`);
  const read = runRead(created.bytes, { kind: 'document', view: 'markup' });
  assert.equal(read.outcome, 'completed', read.diagnostic?.message);
  const markup = read.result.markup;
  assert.match(markup, /<b>before<image w=/);
  assert.equal(serializeProjection(parseProjection(markup)), markup);
});

test('engine round-trip: equation with valueless display attr, fmt open across it (review finding 2)', async () => {
  await ensureInit();
  const created = runCreate(undefined, undefined, '<p><b>a<equation display>x^2</equation>b</b></p>');
  const read = runRead(created.bytes, { kind: 'document', view: 'markup' });
  assert.equal(read.outcome, 'completed', read.diagnostic?.message);
  const markup = read.result.markup;
  assert.match(markup, /<equation display latex=.*><math/);
  assert.equal(serializeProjection(parseProjection(markup)), markup);
});

test('hand-written fmt-spanning constructs round-trip byte-for-byte (review finding 4)', () => {
  const cases = [
    '<p id="A1B2C3D4" ord="1"><b>line1<br/>line2</b></p>',
    '<p id="A1B2C3D4" ord="1"><b>a<comment-start id="1"/>b</b></p>',
    '<p id="A1B2C3D4" ord="1"><b>a<footnote>note</footnote>b</b></p>',
    '<p id="A1B2C3D4" ord="1"><b>a<equation display latex="x"><math>x</math></equation>b</b></p>',
  ];
  for (const markup of cases) {
    assert.equal(serializeProjection(parseProjection(markup)), markup, `round-trip for ${markup}`);
  }
});

test('serializeProjection handles hand-built docs without a blocks field (review finding 3)', () => {
  const paragraph = {
    id: 'A1B2C3D4',
    tag: 'p',
    styleClass: null,
    attrs: { ord: '1' },
    items: [{ kind: 'text', text: 'hi', fmt: blankFmt }],
    plainText: 'hi',
    markup: '',
  };
  const doc = { paragraphs: [paragraph] };
  assert.equal(serializeProjection(doc), '<p id="A1B2C3D4" ord="1">hi</p>');
});

test('engine round-trip: crossing fmt transitions (overlapping non-nested sets, review finding round-2)', async () => {
  await ensureInit();
  const created = runCreate(undefined, undefined, '<p><b>one</b><b><i>two</i></b><i>three</i></p>');
  const read = runRead(created.bytes, { kind: 'document', view: 'markup' });
  assert.equal(read.outcome, 'completed', read.diagnostic?.message);
  const markup = read.result.markup;
  // create coalesces adjacent runs; {b,i} -> {i} must emit </i></b><i>, never a crossing </b> while <i> is open
  assert.match(markup, /<b>one<i>two<\/i><\/b><i>three<\/i><\/p>/);
  const doc = parseProjection(markup);
  const out = serializeProjection(doc);
  assert.equal(out, markup);
  assert.doesNotThrow(() => parseProjection(out)); // parser accepts its own output
});

test('hand-written crossing fmt form round-trips byte-for-byte (review finding round-2)', () => {
  const markup = '<p id="A1B2C3D4" ord="1"><b>one<i>two</i></b><i>three</i> tail</p>';
  assert.equal(serializeProjection(parseProjection(markup)), markup);
});

test('engine round-trip: tracked del/ins with fmt transitions around and inside wrappers (review finding round-2)', async () => {
  await ensureInit();
  const { createHash } = await import('node:crypto');
  const created = runCreate(undefined, undefined, '<p><b>bold</b> and <i>ital</i> end.</p>');
  const source = `sha256:${createHash('sha256').update(Buffer.from(created.bytes)).digest('hex')}`;
  const read0 = runRead(created.bytes, { kind: 'document', view: 'markup' });
  const id = read0.result.blocks[0].paragraphs[0];
  const plan = { base: source, author: 'Pi', change_mode: 'track', ops: [{ op: 'replace_text', at: id, select: 'and', with: '<u>and</u> more' }] };
  const preview = (await import('docxdriver')).executeRequest(created.bytes, { Plan: { plan } });
  const commit = (await import('docxdriver')).executeRequest(created.bytes, { Plan: { plan, preview_key: preview.preview_key } });
  const read = runRead(commit.bytes, { kind: 'document', view: 'markup' });
  const markup = read.result.markup;
  assert.match(markup, /<del id="[^"]+" author="Pi">and<\/del><ins id="[^"]+" author="Pi"><u>and<\/u> more<\/ins>/);
  assert.equal(serializeProjection(parseProjection(markup)), markup);
});

test('rejects structural errors beyond the brief list', () => {
  const cases = [
    ['text outside a paragraph', 'hello<p id="A" ord="1">x</p>'],
    ['nested paragraph', '<p id="A" ord="1"><p id="B" ord="2">x</p></p>'],
    ['unclosed table', '<p id="A" ord="1">x</p><table><tr>'],
    ['unclosed link', '<p id="A" ord="1"><a href="x">y</p>'],
    ['unclosed field at eof', '<p id="A" ord="1"><field instr="X">y'],
    ['unknown close tag', '<p id="A" ord="1">x</p></div>'],
    ['class on heading', '<h1 id="A0000001" ord="1" class="X">x</h1>'],
    ['conflicting sup/sub', '<p id="A0000001" ord="1"><sub>x<sup>y</sup></sub></p>'],
    ['tag outside paragraph at eof', '<b>x</b>'],
    ['empty input', ''],
    ['bare closing tag', '</p>'],
  ];
  for (const [label, bad] of cases) {
    assert.throws(() => parseProjection(bad), ProjectionError, `expected rejection for ${label}`);
  }
});
