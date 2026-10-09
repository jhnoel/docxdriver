/**
 * Reconciler contract tests: diffProjections(originalMarkup, proposedMarkup)
 * must compile structural string diffs into the narrowest safe typed core ops,
 * or reject malformed / ambiguous / unsupported changes with a readable reason.
 */
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { diffProjections, allocateInsertId } from '../dist/projection-diff.js';
import { ensureInit, runCreate, runPlan, runRead } from '../dist/engine.js';
import { sha256 } from '../dist/io.js';

const OPS = { replace_text: 1, replace_paragraph: 1, insert_paragraph: 1, delete_paragraphs: 1, format_text: 1, format_paragraph: 1 };

test('changed span maps to the narrowest replace_text', () => {
  const r = diffProjections(
    '<p id="2673269E" ord="1">The notice period is thirty days.</p>',
    '<p id="2673269E" ord="1">The notice period is sixty days.</p>');
  assert.deepEqual(r, { ok: true, ops: [{ op: 'replace_text', at: '2673269E', select: 'thirty', with: 'sixty' }] });
});

test('a single span change keeps the fmt tags out of the select and into with', () => {
  const r = diffProjections(
    '<p id="2673269E" ord="1">The <b>notice period</b> is thirty days.</p>',
    '<p id="2673269E" ord="1">The <b><u>notice period</u></b> is thirty days.</p>');
  assert.deepEqual(r, { ok: true, ops: [{ op: 'format_text', at: '2673269E', select: 'notice period', underline: true }] });
});

test('third occurrence of repeated text maps to occurrence=3', () => {
  const r = diffProjections(
    '<p id="A0000001" ord="1">the terms. the terms. the terms. the terms.</p>',
    '<p id="A0000001" ord="1">the terms. the terms. the TERMS. the terms.</p>');
  assert.deepEqual(r.ops, [{ op: 'replace_text', at: 'A0000001', select: 'the terms', with: 'the TERMS', occurrence: 3 }]);
});

test('every occurrence changed maps to sequential occurrence=1 ops', () => {
  const r = diffProjections(
    '<p id="A0000001" ord="1">30 30 30</p>',
    '<p id="A0000001" ord="1">45 45 45</p>');
  assert.deepEqual(r.ops, [
    { op: 'replace_text', at: 'A0000001', select: '30', with: '45', occurrence: 1 },
    { op: 'replace_text', at: 'A0000001', select: '30', with: '45', occurrence: 1 },
    { op: 'replace_text', at: 'A0000001', select: '30', with: '45', occurrence: 1 },
  ]);
});

test('pure insertion inside a mostly-unchanged paragraph extends the select', () => {
  const r = diffProjections(
    '<p id="A0000001" ord="1">Due.</p>',
    '<p id="A0000001" ord="1">Due within 30 days.</p>');
  assert.deepEqual(r.ops, [{ op: 'replace_text', at: 'A0000001', select: 'Due', with: 'Due within 30 days' }]);
});

test('paragraph changed by more than half maps to replace_paragraph', () => {
  const r = diffProjections(
    '<p id="A0000001" ord="1">Old wording entirely.</p>',
    '<p id="A0000001" ord="1">Brand new wording.</p>');
  assert.deepEqual(r.ops, [{ op: 'replace_paragraph', at: 'A0000001', with: 'Brand new wording.' }]);
});

test('new id-less paragraph maps to insert_paragraph', () => {
  const r = diffProjections(
    '<p id="A0000001" ord="1">one</p><p id="B0000002" ord="2">three</p>',
    '<p id="A0000001" ord="1">one</p><p>two</p><p id="B0000002" ord="2">three</p>');
  assert.deepEqual(r.ops, [{ op: 'insert_paragraph', at: 'A0000001', position: 'after', with: 'two' }]);
});

test('removed ids map to one delete_paragraphs op', () => {
  const r = diffProjections(
    '<p id="A0000001" ord="1">one</p><p id="B0000002" ord="2">two</p><p id="C0000003" ord="3">three</p>',
    '<p id="A0000001" ord="1">one</p><p id="C0000003" ord="3">three</p>');
  assert.deepEqual(r.ops, [{ op: 'delete_paragraphs', at: ['B0000002'] }]);
});

test('inline markup delta maps to format_text', () => {
  const r = diffProjections(
    '<p id="C0000003" ord="1">The <b>warranty</b> period.</p>',
    '<p id="C0000003" ord="1">The <b><i>warranty</i></b> period.</p>');
  assert.deepEqual(r.ops, [{ op: 'format_text', at: 'C0000003', select: 'warranty', italic: true }]);
});

test('class change maps to format_paragraph style', () => {
  const r = diffProjections('<p id="D0000004" ord="1" class="Normal">x</p>', '<p id="D0000004" ord="1" class="Heading1">x</p>');
  assert.deepEqual(r.ops, [{ op: 'format_paragraph', at: 'D0000004', style: 'Heading1' }]);
});

test('rejects: invented id, protected content change, crossing revision boundary, consecutive id-less paragraphs, ord/num tampering', () => {
  const cases = [
    ['invented id', '<p id="A0000001" ord="1">x</p>', '<p id="DEADBEEF" ord="1">y</p>'],
    ['link text change', '<p id="A0000001" ord="1"><a href="https://x.example">x</a></p>', '<p id="A0000001" ord="1"><a href="https://x.example">y</a></p>'],
    ['field content change', '<p id="A0000001" ord="1"><field instr="DATE">1/1/26</field></p>', '<p id="A0000001" ord="1"><field instr="DATE">2/2/26</field></p>'],
    ['revision boundary', '<p id="A0000001" ord="1"><del id="r1" author="a">old</del> mid</p>', '<p id="A0000001" ord="1"><del id="r1" author="a">old</del> mid2</p>'],
    ['two consecutive new paragraphs', '<p id="A0000001" ord="1">x</p>', '<p id="A0000001" ord="1">x</p><p>y</p><p>z</p>'],
    ['num tampering', '<p id="A0000001" ord="1" num="1.">x</p>', '<p id="A0000001" ord="1" num="2.">x</p>'],
  ];
  for (const [label, orig, prop] of cases) {
    const r = diffProjections(orig, prop);
    assert.equal(r.ok, false, label);
    assert.match(r.reason, new RegExp(label.split(' ')[0]));
  }
});

test('malformed proposed projection is rejected with a located reason', () => {
  const r = diffProjections('<p id="A0000001" ord="1">x</p>', '<p id="A0000001" ord="1"><b>x</p>');
  assert.equal(r.ok, false);
  assert.match(r.reason, /malformed|parse/i);
});

// ---- additional contract tests beyond the brief's list ----

test('insertion of bold text carries the fmt tags in with', () => {
  const r = diffProjections(
    '<p id="A0000001" ord="1">ab</p>',
    '<p id="A0000001" ord="1">a<b>X</b>b</p>');
  assert.deepEqual(r.ops, [{ op: 'replace_text', at: 'A0000001', select: 'a', with: 'a<b>X</b>' }]);
});

test('text change inside an unchanged fmt run carries the kept fmt in with', () => {
  const r = diffProjections(
    '<p id="A0000001" ord="1">The <b>old</b> text.</p>',
    '<p id="A0000001" ord="1">The <b>new</b> text.</p>');
  assert.deepEqual(r.ops, [{ op: 'replace_text', at: 'A0000001', select: 'old', with: '<b>new</b>' }]);
});

test('text change with agent-added fmt carries the added tags in with', () => {
  const r = diffProjections(
    '<p id="A0000001" ord="1">The <b>old</b> text.</p>',
    '<p id="A0000001" ord="1">The <b><i>new</i></b> text.</p>');
  assert.deepEqual(r.ops, [{ op: 'replace_text', at: 'A0000001', select: 'old', with: '<b><i>new</i></b>' }]);
});

test('partial identical-run changes use sequential ranks, not 1,1', () => {
  const r = diffProjections(
    '<p id="A0000001" ord="1">30 30 30</p>',
    '<p id="A0000001" ord="1">30 45 45</p>');
  assert.deepEqual(r.ops, [
    { op: 'replace_text', at: 'A0000001', select: '30', with: '45', occurrence: 2 },
    { op: 'replace_text', at: 'A0000001', select: '30', with: '45', occurrence: 2 },
  ]);
  assert.equal(applySequentially('30 30 30', r.ops), '30 45 45');
});

test('partial identical-run changes: sparse ranks adjust sequentially', () => {
  const r = diffProjections(
    '<p id="A0000001" ord="1">30 30 30 30</p>',
    '<p id="A0000001" ord="1">30 45 30 45</p>');
  assert.deepEqual(r.ops, [
    { op: 'replace_text', at: 'A0000001', select: '30', with: '45', occurrence: 2 },
    { op: 'replace_text', at: 'A0000001', select: '30', with: '45', occurrence: 3 },
  ]);
  assert.equal(applySequentially('30 30 30 30', r.ops), '30 45 30 45');
});

test('bold removal over replaced text drops the tags from with', () => {
  const r = diffProjections(
    '<p id="A0000001" ord="1">The <b>old</b> text.</p>',
    '<p id="A0000001" ord="1">The new text.</p>');
  assert.deepEqual(r.ops, [{ op: 'replace_text', at: 'A0000001', select: 'old', with: 'new' }]);
});

test('bold removal over a length-changing replacement drops the tags from with', () => {
  const r = diffProjections(
    '<p id="A0000001" ord="1">The <b>old</b> text.</p>',
    '<p id="A0000001" ord="1">The newer text.</p>');
  assert.deepEqual(r.ops, [{ op: 'replace_text', at: 'A0000001', select: 'old', with: 'newer' }]);
});

test('fmt preserved across a length-changing text edit is not rejected', () => {
  const r = diffProjections(
    '<p id="A0000001" ord="1">The <b>old</b> text.</p>',
    '<p id="A0000001" ord="1">The <b>older</b> text.</p>');
  assert.equal(r.ok, true);
  assert.deepEqual(r.ops, [{ op: 'replace_text', at: 'A0000001', select: 'The old', with: 'The <b>older</b>' }]);
});

test('bold insert before an unchanged bold run is not rejected', () => {
  const r = diffProjections(
    '<p id="A0000001" ord="1">a<b>b</b></p>',
    '<p id="A0000001" ord="1">a<b>X</b><b>b</b></p>');
  assert.equal(r.ok, true);
  assert.deepEqual(r.ops, [{ op: 'replace_text', at: 'A0000001', select: 'a', with: 'a<b>X</b>' }]);
});

test('fmt change partially covering unchanged text is still rejected', () => {
  const r = diffProjections(
    '<p id="A0000001" ord="1">a X b</p>',
    '<p id="A0000001" ord="1"><b>a Y</b> b</p>');
  assert.equal(r.ok, false);
  assert.match(r.reason, /formatting change overlaps a text change/);
});

// Realistic corpus input (conformance task 3): only the 3rd "the terms" of the
// repeated subsequence changes to "THE TERMS". The token-LCS must not split
// the change into two overlapping ops across the "and " context.
const CORPUS_ORIG = 'The parties agree that the terms shall govern, and the terms shall prevail, and the terms shall bind, and the terms shall endure.';
function corpusDiff(nth, replaceWith = 'THE TERMS') {
  const positions = [];
  let start = 0;
  for (;;) {
    const at = CORPUS_ORIG.indexOf('the terms', start);
    if (at === -1) break;
    positions.push(at);
    start = at + 1;
  }
  const at = positions[nth - 1];
  const proposed = CORPUS_ORIG.slice(0, at) + replaceWith + CORPUS_ORIG.slice(at + 'the terms'.length);
  return diffProjections(`<p id="033873CC" ord="7">${CORPUS_ORIG}</p>`, `<p id="033873CC" ord="7">${proposed}</p>`);
}

test('corpus 3rd-occurrence change compiles to ONE anchored op (not two overlapping)', () => {
  const r = corpusDiff(3);
  assert.deepEqual(r.ops, [{ op: 'replace_text', at: '033873CC', select: 'the terms', with: 'THE TERMS', occurrence: 3 }]);
  // The failing shape must not appear: two overlapping ops with "and the".
  assert.equal(r.ops.filter((op) => op.select === 'and the').length, 0);
  assert.equal(applySequentially(CORPUS_ORIG, r.ops), CORPUS_ORIG.replace('and the terms shall bind', 'and THE TERMS shall bind'));
});

test('corpus 2nd-occurrence change compiles to ONE anchored op', () => {
  const r = corpusDiff(2);
  assert.deepEqual(r.ops, [{ op: 'replace_text', at: '033873CC', select: 'the terms', with: 'THE TERMS', occurrence: 2 }]);
  assert.equal(applySequentially(CORPUS_ORIG, r.ops), CORPUS_ORIG.replace('and the terms shall prevail', 'and THE TERMS shall prevail'));
});

test('corpus 4th-occurrence change compiles to ONE anchored op', () => {
  const r = corpusDiff(4);
  assert.deepEqual(r.ops, [{ op: 'replace_text', at: '033873CC', select: 'the terms', with: 'THE TERMS', occurrence: 4 }]);
  assert.equal(applySequentially(CORPUS_ORIG, r.ops), CORPUS_ORIG.replace('and the terms shall endure', 'and THE TERMS shall endure'));
});

// Six-occurrence variant: multiple changes within ONE paragraph must not let a
// later occurrence's split pairs leak into the expansion path (the merge's
// pendingEqual must reset per pair).
const SIX_CORPUS_ORIG =
  'The parties agree that the terms shall govern, and the terms shall prevail, and the terms shall bind, and the terms shall endure, and the terms shall settle, and the terms shall rest.';
function sixCorpusDiff(changed) {
  const positions = [];
  let start = 0;
  for (;;) {
    const at = SIX_CORPUS_ORIG.indexOf('the terms', start);
    if (at === -1) break;
    positions.push(at);
    start = at + 1;
  }
  let proposed = SIX_CORPUS_ORIG;
  for (const nth of [...changed].sort((a, b) => b - a)) {
    const at = positions[nth - 1];
    proposed = proposed.slice(0, at) + 'THE TERMS' + proposed.slice(at + 'the terms'.length);
  }
  return diffProjections(`<p id="033873CC" ord="7">${SIX_CORPUS_ORIG}</p>`, `<p id="033873CC" ord="7">${proposed}</p>`);
}

test('two changes in one paragraph emit exactly two merged ops with sequential ranks', () => {
  const r = sixCorpusDiff([1, 3]);
  assert.deepEqual(r.ops, [
    { op: 'replace_text', at: '033873CC', select: 'the terms', with: 'THE TERMS', occurrence: 1 },
    { op: 'replace_text', at: '033873CC', select: 'the terms', with: 'THE TERMS', occurrence: 2 },
  ]);
  assert.equal(
    applySequentially(SIX_CORPUS_ORIG, r.ops),
    SIX_CORPUS_ORIG.replace('that the terms shall govern', 'that THE TERMS shall govern').replace('and the terms shall bind', 'and THE TERMS shall bind'),
  );
});

test('six-occurrence single changes still emit one op each', () => {
  for (const nth of [2, 3]) {
    const r = sixCorpusDiff([nth]);
    assert.deepEqual(r.ops, [{ op: 'replace_text', at: '033873CC', select: 'the terms', with: 'THE TERMS', occurrence: nth }]);
  }
});

test('identical-select fmt spans keep their ORIGINAL ranks (format_text does not shift matches)', () => {
  const r = diffProjections(
    '<p id="A0000001" ord="1">30 30</p>',
    '<p id="A0000001" ord="1"><b>30</b> <b>30</b></p>');
  assert.deepEqual(r.ops, [
    { op: 'format_text', at: 'A0000001', select: '30', bold: true, occurrence: 1 },
    { op: 'format_text', at: 'A0000001', select: '30', bold: true, occurrence: 2 },
  ]);
});

test('multiple deleted ids group into one delete_paragraphs op', () => {
  const r = diffProjections(
    '<p id="A0000001" ord="1">one</p><p id="B0000002" ord="2">two</p><p id="C0000003" ord="3">three</p>',
    '<p id="C0000003" ord="3">three</p>');
  assert.deepEqual(r.ops, [{ op: 'delete_paragraphs', at: ['A0000001', 'B0000002'] }]);
});

test('id-less paragraph at document start inserts before the first paragraph', () => {
  const r = diffProjections(
    '<p id="A0000001" ord="1">one</p>',
    '<p>zero</p><p id="A0000001" ord="1">one</p>');
  assert.deepEqual(r.ops, [{ op: 'insert_paragraph', at: 'A0000001', position: 'before', with: 'zero' }]);
});

test('consecutive id-less paragraphs chain via deterministic as ids', () => {
  const r = diffProjections(
    '<p id="A0000001" ord="1">x</p>',
    '<p id="A0000001" ord="1">x</p><p>y</p><p>z</p>',
    { sourceSha: 'sha256:deadbeef' });
  assert.equal(r.ok, true);
  assert.equal(r.ops.length, 2);
  assert.deepEqual(Object.keys(r.ops[0]).sort(), ['as', 'at', 'op', 'position', 'with']);
  assert.equal(r.ops[0].op, 'insert_paragraph');
  assert.equal(r.ops[0].at, 'A0000001');
  assert.equal(r.ops[0].position, 'after');
  assert.equal(r.ops[0].with, 'y');
  assert.match(r.ops[0].as, /^P[0-9A-F]{8}$/);
  // Subsequent chain ops anchor at the previous insert's alias ($-prefixed
  // engine address syntax), always after it so document order is preserved.
  assert.deepEqual(r.ops[1], { op: 'insert_paragraph', at: `$${r.ops[0].as}`, position: 'after', with: 'z' });
  // deterministic: same input, same allocated id
  const again = diffProjections(
    '<p id="A0000001" ord="1">x</p>',
    '<p id="A0000001" ord="1">x</p><p>y</p><p>z</p>',
    { sourceSha: 'sha256:deadbeef' });
  assert.equal(again.ops[0].as, r.ops[0].as);
});

test('text change after a protected link is allowed when it does not cross', () => {
  const r = diffProjections(
    '<p id="A0000001" ord="1">See <a href="https://x.example">the x</a> now.</p>',
    '<p id="A0000001" ord="1">See <a href="https://x.example">the x</a> later.</p>');
  assert.deepEqual(r.ops, [{ op: 'replace_text', at: 'A0000001', select: 'now', with: 'later' }]);
});

test('allocateInsertId is deterministic, a valid engine alias, and skips existing ids', () => {
  const a = allocateInsertId('sha256:abc', 0, new Set());
  const b = allocateInsertId('sha256:abc', 0, new Set());
  assert.equal(a, b);
  // Engine valid_alias rule: [A-Za-z_][A-Za-z0-9_]* — a digit-leading id is
  // rejected by insert_paragraph with "invalid alias".
  assert.match(a, /^P[0-9A-F]{8}$/);
  assert.match(a, /^[A-Za-z_][A-Za-z0-9_]*$/);
  const c = allocateInsertId('sha256:abc', 0, new Set([a]));
  assert.notEqual(c, a);
  assert.match(c, /^P[0-9A-F]{8}$/);
});

/** Apply replace_text ops sequentially (occurrence semantics) to plain text. */
function applySequentially(text, ops) {
  let out = text;
  for (const op of ops) {
    if (op.op !== 'replace_text') continue;
    let count = 0;
    let from = 0;
    let at = -1;
    for (;;) {
      const idx = out.indexOf(op.select, from);
      if (idx === -1) throw new Error('select not found');
      count += 1;
      if (count === (op.occurrence ?? 1)) {
        at = idx;
        break;
      }
      from = idx + 1;
    }
    out = out.slice(0, at) + op.with + out.slice(at + op.select.length);
  }
  return out;
}

/**
 * Engine-verify that the two format_text ops (occurrences 1 and 2) format BOTH
 * '30' spans: format_text does not change text, so a plain-text simulator cannot
 * show the effect — the real WASM engine is the sequential-application check.
 */
test('engine: identical-select fmt spans with original ranks format both spans', async () => {
  await ensureInit();
  const created = runCreate(undefined, undefined, '<p>30 30</p>');
  const src = created.bytes;
  const at = runRead(src, { kind: 'document', view: 'markup' }).result.markup.match(/id="([0-9A-F]{8})"/)[1];
  const base = `sha256:${sha256(src)}`;
  const ops = [
    { op: 'format_text', at, select: '30', bold: true, occurrence: 1 },
    { op: 'format_text', at, select: '30', bold: true, occurrence: 2 },
  ];
  const preview = runPlan(src, { base, author: 't', change_mode: 'track', ops });
  assert.equal(preview.outcome, 'previewed', preview.diagnostic?.message);
  const commit = runPlan(src, { base, author: 't', change_mode: 'track', ops }, preview.preview_key);
  const final = runRead(commit.bytes, { kind: 'document', view: 'final' }).result.markup;
  assert.equal(final.match(/<b>30<\/b>/g)?.length, 2, `both spans must be bold: ${final}`);
});
