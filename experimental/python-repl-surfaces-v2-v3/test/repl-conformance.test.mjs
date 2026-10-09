// Deterministic offline conformance suite: the spec's 10 benchmark tasks run
// against all three REPL surfaces on identical fresh document copies, with
// evaluation metrics recorded and cross-surface equivalence asserted.
//
// Real Monty + real checked-in WASM, temp dirs, no network, no model. One
// trial (one persistent Monty session) per (task x surface); the corpus
// fixture is created once and byte-copied per trial so every surface sees
// the same paragraph ids.
import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { copyFile, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { REPL_SURFACES, makeTrial } from './repl-trial.mjs';
import { createContractFixture, createComplexFixture, renderDocxMarkup, zipPartHashes } from './repl-fixtures.mjs';
import { ensureInit } from '../dist/engine.js';
import { parseProjection, parseProjectionLoose } from '../dist/projection.js';
import { renderNotices } from '../dist/python-host.js';

const PACKAGE_ROOT = dirname(dirname(fileURLToPath(import.meta.url)));

// ---------------------------------------------------------------------------
// Corpus constants
// ---------------------------------------------------------------------------

/** The 13 visible paragraph texts of the untouched contract fixture. */
const BASE_TEXTS = [
  'Service Agreement',
  'The notice period is thirty days.',
  'Payment is due within 30 days of invoice receipt.',
  'Payment is due within 45 days of invoice receipt.',
  'Payment is due within 60 days of invoice receipt.',
  'Confidential Information means any information disclosed by either party.',
  'The parties agree that the terms shall govern, and the terms shall prevail, and the terms shall bind, and the terms shall endure.',
  'Net 30. Net 45. Net 60. Net 90.',
  'The warranty period is twelve months from delivery.',
  'This paragraph will be replaced entirely.',
  'This paragraph anchors an insertion.',
  'This paragraph will be deleted.',
  'Final paragraph. Signatures follow.',
];

/** task id → expected final visible paragraph texts (deleted remnants excluded). */
const EXPECTED_TEXTS = {
  'task1-exact-text-correction': BASE_TEXTS.map((t) => (t === 'The notice period is thirty days.' ? 'The notice period is sixty days.' : t)),
  'task2-repeated-bulk-edit': BASE_TEXTS.map((t) => (t.startsWith('Payment is due within') ? 'Payment is due within 45 days of invoice receipt.' : t)),
  'task3-ambiguous-occurrence': BASE_TEXTS.map((t) => (t.startsWith('The parties agree') ? 'The parties agree that the terms shall govern, and the terms shall prevail, and THE TERMS shall bind, and the terms shall endure.' : t)),
  'task4-arbitrary-formatting-selection': BASE_TEXTS,
  'task5-paragraph-structure': ['Service Agreement', 'The notice period is thirty days.', 'Payment is due within 30 days of invoice receipt.', 'Payment is due within 45 days of invoice receipt.', 'Payment is due within 60 days of invoice receipt.', 'Confidential Information means any information disclosed by either party.', 'The parties agree that the terms shall govern, and the terms shall prevail, and the terms shall bind, and the terms shall endure.', 'Net 30. Net 45. Net 60. Net 90.', 'The warranty period is twelve months from delivery.', 'Replacement paragraph.', 'This paragraph anchors an insertion.', 'Inserted paragraph.', 'Final paragraph. Signatures follow.'],
  'task6-mixed-atomic-edit': ['Service Agreement', 'The notice period is sixty days.', 'Payment is due within 30 days of invoice receipt.', 'Payment is due within 45 days of invoice receipt.', 'Payment is due within 60 days of invoice receipt.', 'Confidential Information means any information disclosed by either party.', 'The parties agree that the terms shall govern, and the terms shall prevail, and the terms shall bind, and the terms shall endure.', 'Net 30. Net 45. Net 60. Net 90.', 'The warranty period is twelve months from delivery.', 'This paragraph will be replaced entirely.', 'This paragraph anchors an insertion.', 'Inserted paragraph.', 'Final paragraph. Signatures follow.'],
  'task7-failed-preview-repair': BASE_TEXTS.map((t) => (t === 'The notice period is thirty days.' ? 'The notice period is sixty days.' : t)),
  'task8-source-race': BASE_TEXTS, // nothing commits
  'task9-mutation-after-preview': BASE_TEXTS, // nothing commits
};

// ---------------------------------------------------------------------------
// Per-trial context: drives one surface trial through a task's scripted feeds
// ---------------------------------------------------------------------------

function makeCtx(trial, fixturePath, taskId) {
  let seenNotices = 0;
  let lastText = '';
  const ctx = {
    surface: trial.surface,
    fixturePath,
    /** Run one scripted Python feed (array of lines or a string). */
    async run(code) {
      const source = Array.isArray(code) ? code.join('\n') : code;
      const execution = await trial.execute(source);
      const fresh = trial.notices.slice(seenNotices);
      seenNotices = trial.notices.length;
      const parts = [];
      if (execution.stdout !== '') parts.push(execution.stdout.replace(/\n$/, ''));
      if (execution.value !== undefined) parts.push(`=> ${typeof execution.value === 'string' ? execution.value : JSON.stringify(execution.value)}`);
      if (!execution.ok && execution.error !== undefined) parts.push(execution.error);
      if (execution.reset) parts.push('session was reset after a crash: Python state was lost');
      if (fresh.length > 0) parts.push(renderNotices(fresh));
      lastText = parts.join('\n');
      return execution;
    },
    /** Combined model-visible text of the most recent feed. */
    get text() {
      return lastText;
    },
    lastKey() {
      return trial.lastPreviewKey();
    },
    planFor() {
      return trial.planFor(taskId);
    },
    async render(view = 'final') {
      return renderDocxMarkup(fixturePath, view);
    },
    async tamper(fn) {
      const bytes = await readFile(fixturePath);
      const out = fn(Buffer.from(bytes));
      await writeFile(fixturePath, out);
      return out;
    },
    async rawBytes() {
      return Buffer.from(await readFile(fixturePath));
    },
    async sha256() {
      return createHash('sha256').update(await readFile(fixturePath)).digest('hex');
    },
  };
  return ctx;
}

async function setupTrial(surface, taskId, fixtureSource, complex = false) {
  const cwd = join(BASE_CWD, `${taskId}__${surface}`);
  await mkdir(cwd, { recursive: true });
  const fixturePath = join(cwd, complex ? 'complex.docx' : 'contract.docx');
  await copyFile(fixtureSource, fixturePath);
  const { trial } = await makeTrial(surface, cwd);
  return { trial, ctx: makeCtx(trial, fixturePath, taskId) };
}

/** Assert the final rendered markup has exactly the expected visible texts. */
function assertFinalTexts(expected, markup, label) {
  const doc = parseProjection(markup);
  const texts = doc.paragraphs.map((p) => p.plainText).filter((t) => t !== '');
  assert.deepEqual(texts, expected, `${label}: final paragraph texts differ\nactual:   ${JSON.stringify(texts)}\nexpected: ${JSON.stringify(expected)}`);
}

/**
 * Structural fingerprint of a final document for cross-surface equality:
 * per paragraph — exact id (the excluded tasks never insert, so ids must
 * match across surfaces), tag, style class, and the fmt flags per text span.
 * The projection parser merges adjacent same-fmt runs, so the span stream is
 * fmt-transition-based and stable across surfaces (e.g. task 1's V2 folded
 * `<b>sixty</b>` + original bold ` days` renders as one `<b>sixty days</b>`
 * span, identical to V1/V3). Empty paragraphs (tracked-delete remnants) are
 * filtered exactly as in `assertFinalTexts`.
 */
function finalDocFingerprint(markup) {
  const doc = parseProjection(markup);
  return doc.paragraphs
    .filter((p) => p.plainText !== '')
    .map((p) => ({
      id: p.id,
      tag: p.tag,
      styleClass: p.styleClass,
      spans: p.items.map((i) => ({
        text: i.text,
        fmt: {
          bold: i.fmt.bold,
          italic: i.fmt.italic,
          underline: i.fmt.underline,
          strike: i.fmt.strike,
          superscript: i.fmt.superscript,
          subscript: i.fmt.subscript,
        },
      })),
    }));
}

/**
 * Cross-surface final-document structural equality (equivalence rule a): the
 * three surfaces' final renders must have the same paragraph count/order,
 * exact ids, same plain texts, and same fmt flags per span. This is the
 * real computation behind the `equivalent` flag for the tasks whose plans
 * are not op-for-op comparable (tasks 1-2, 9; task 8 compares raw bytes).
 */
function assertFinalDocsEquivalent(taskId, markups) {
  const fingerprints = markups.map(finalDocFingerprint);
  assert.ok(fingerprints.every((f) => f.length > 0), `${taskId}: final document fingerprint must not be empty`);
  assert.deepEqual(fingerprints[0], fingerprints[1], `${taskId}: final document differs (plan vs string)`);
  assert.deepEqual(fingerprints[1], fingerprints[2], `${taskId}: final document differs (string vs model)`);
}

function assertPreviewOk(ctx, taskId) {
  assert.match(ctx.text, /KEY=p1:sha256:[0-9a-f]{64}/, `${taskId}/${ctx.surface}: preview key missing:\n${ctx.text}`);
  assert.ok(!ctx.text.includes('── blocked ──'), `${taskId}/${ctx.surface}: preview was blocked:\n${ctx.text}`);
}

function assertCommitOk(ctx, taskId) {
  assert.match(ctx.text, /committed/, `${taskId}/${ctx.surface}: commit feed did not report success:\n${ctx.text}`);
  assert.ok(!ctx.text.includes('── blocked ──'), `${taskId}/${ctx.surface}: commit was blocked:\n${ctx.text}`);
  assert.ok(!ctx.text.includes('verification problem'), `${taskId}/${ctx.surface}: post-commit render verification failed:\n${ctx.text}`);
}

/** The canonical-plan comparison for a task's three surfaces. */

const FMT_KEYS = ['bold', 'italic', 'underline', 'strike', 'superscript', 'subscript'];

/**
 * When a replace_text `with` is one uniform fmt-wrapped span (e.g.
 * "<b>sixty</b>"), return { text, flags }; otherwise null. The model surface
 * expresses the same effect as an explicit format_text op, so folding vs
 * explicit is an authoring-model difference the comparison must normalize.
 */
function foldedFmtInfo(withText) {
  let doc;
  try {
    doc = parseProjectionLoose(`<p>${withText}</p>`);
  } catch {
    return null;
  }
  const para = doc.paragraphs[0];
  if (para === undefined || para.items.length === 0) return null;
  const flags = {};
  for (const key of FMT_KEYS) flags[key] = false;
  let text = '';
  for (const item of para.items) {
    text += item.text;
    for (const key of FMT_KEYS) if (item.fmt[key]) flags[key] = true;
  }
  if (!FMT_KEYS.some((key) => flags[key])) return null; // no fmt → nothing to fold
  for (const item of para.items) {
    if (!FMT_KEYS.every((key) => item.fmt[key] === flags[key])) return null; // not uniform
  }
  return { text, flags };
}

/**
 * Normalize folded-vs-explicit formatting so plans are comparable: a
 * replace_text whose `with` is uniformly fmt-wrapped splits into a plain
 * replace_text plus a format_text carrying those flags (the plan/string
 * surfaces fold the fmt into the replacement markup; the model surface emits
 * them as two explicit ops). Plain withs and non-uniform/mixed withs are
 * left untouched. `base`/`as`/allocated-id masking rules are unchanged.
 */
function normalizeFoldedFormatting(plan) {
  if (!plan) return null;
  const out = [];
  for (const op of plan.ops) {
    if (op.op === 'replace_text' && typeof op.with === 'string') {
      const folded = foldedFmtInfo(op.with);
      if (folded !== null) {
        const flags = {};
        for (const key of FMT_KEYS) if (folded.flags[key]) flags[key] = true;
        out.push({ ...op, with: folded.text });
        out.push({ op: 'format_text', at: op.at, select: folded.text, ...flags });
        continue;
      }
    }
    out.push(op);
  }
  return { ...plan, ops: out };
}

function comparableOps(plan) {
  if (!plan) return null;
  const parsed = typeof plan === 'string' ? JSON.parse(plan) : plan;
  return parsed.ops.map((op) => {
    switch (op.op) {
      case 'replace_text':
        // Span granularity is an authoring-model difference (V1 authors wider
        // selects than the reconciler's narrowest span): compare target and
        // occurrence, not select/with.
        return { op: op.op, at: op.at, occurrence: op.occurrence ?? null };
      case 'format_text':
        // `select` is dropped because it is span-granularity, not authoring
        // content: a format_text derived from a folded `with` carries that
        // with's own span text, while the model surface's explicit format op
        // carries its select — the same cross-surface span-granularity
        // difference already masked for replace_text ("sixty" vs
        // "sixty days"). The target paragraph (`at`), the applied flags, and
        // the occurrence are the comparable core; the affected span is
        // verified by each task's final-document assertions and, for the
        // plan-comparable tasks, by `assertFinalDocsEquivalent`.
        return {
          op: op.op,
          at: op.at,
          bold: op.bold ?? null,
          italic: op.italic ?? null,
          underline: op.underline ?? null,
          strike: op.strike ?? null,
          superscript: op.superscript ?? null,
          subscript: op.subscript ?? null,
          occurrence: op.occurrence ?? null,
        };
      default:
        return op;
    }
  });
}

function assertPlansComparable(taskId, plans) {
  const parsed = plans.map((p) => (typeof p === 'string' ? JSON.parse(p) : p));
  const comparable = parsed.map(normalizeFoldedFormatting).map(comparableOps);
  assert.ok(comparable.every((c) => c !== null), `${taskId}: every surface must produce a previewed canonical plan`);
  const names = (c) => c.map((op) => op.op);
  assert.deepEqual(names(comparable[0]), names(comparable[1]), `${taskId}: op-name sequence mismatch (plan vs string)`);
  assert.deepEqual(names(comparable[1]), names(comparable[2]), `${taskId}: op-name sequence mismatch (string vs model)`);
  assert.deepEqual(comparable[0], comparable[1], `${taskId}: comparable op mismatch (plan vs string)`);
  assert.deepEqual(comparable[1], comparable[2], `${taskId}: comparable op mismatch (string vs model)`);
}

// ---------------------------------------------------------------------------
// V1 shared Python helper snippets (id discovery via the read projection)
// ---------------------------------------------------------------------------

const V1_READ_ITEMS = [
  'import re',
  'original = docx_read("contract.docx")',
  'items = re.findall(r\'<p id="([0-9A-F]{8})"[^>]*>(.*?)</p>\', original)',
];

/** Python lines finding the paragraph id whose content contains `needle`. */
function v1FindId(needle, varName) {
  return [`${varName} = ""`, `for item in items:`, `    if ${JSON.stringify(needle)} in item[1]:`, `        ${varName} = item[0]`];
}

// ---------------------------------------------------------------------------
// The ten tasks: { id, expected, complex?, run: { plan, string, model } }
// ---------------------------------------------------------------------------

const TASKS = [
  {
    id: 'task1-exact-text-correction',
    expected: EXPECTED_TEXTS['task1-exact-text-correction'],
    run: {
      plan: async (ctx) => {
        await ctx.run([
          ...V1_READ_ITEMS,
          ...v1FindId('The notice period is', 'pid'),
          'plan = Plan(operations=[ReplaceText(at=pid, select="thirty days", with_="<b>sixty days</b>")])',
          'preview = docx_preview("contract.docx", plan)',
          'print("KEY=" + preview.key)',
        ]);
        assertPreviewOk(ctx, 'task1');
        await ctx.run([`docx_commit("contract.docx", ${JSON.stringify(ctx.lastKey())}, plan)`, 'print("committed")']);
        assertCommitOk(ctx, 'task1');
      },
      string: async (ctx) => {
        await ctx.run([
          'original = docx_read("contract.docx")',
          'draft = original.replace("The notice period is <b>thirty days</b>.", "The notice period is <b>sixty days</b>.")',
          'preview = docx_preview("contract.docx", original=original, proposed=draft)',
          'print("KEY=" + preview.key)',
        ]);
        assertPreviewOk(ctx, 'task1');
        await ctx.run([`docx_commit("contract.docx", ${JSON.stringify(ctx.lastKey())}, proposed=draft)`, 'print("committed")']);
        assertCommitOk(ctx, 'task1');
      },
      model: async (ctx) => {
        await ctx.run([
          'doc = docx_open("contract.docx")',
          'for p in doc.paragraphs:',
          '    if "notice period" in p.text:',
          '        p.replace("thirty days", "sixty days")',
          'doc.select("sixty days").bold = True',
          'preview = docx_preview(doc)',
          'print("KEY=" + preview.key)',
        ]);
        assertPreviewOk(ctx, 'task1');
        await ctx.run([`docx_commit(doc, ${JSON.stringify(ctx.lastKey())})`, 'print("committed")']);
        assertCommitOk(ctx, 'task1');
      },
    },
  },
  {
    id: 'task2-repeated-bulk-edit',
    expected: EXPECTED_TEXTS['task2-repeated-bulk-edit'],
    run: {
      plan: async (ctx) => {
        await ctx.run([
          ...V1_READ_ITEMS,
          ...v1FindId('within 30 days', 'id30'),
          ...v1FindId('within 60 days', 'id60'),
          'plan = Plan(operations=[',
          '    ReplaceText(at=id30, select="30 days", with_="45 days"),',
          '    ReplaceText(at=id60, select="60 days", with_="45 days"),',
          '])',
          'preview = docx_preview("contract.docx", plan)',
          'print("KEY=" + preview.key)',
        ]);
        assertPreviewOk(ctx, 'task2');
        await ctx.run([`docx_commit("contract.docx", ${JSON.stringify(ctx.lastKey())}, plan)`, 'print("committed")']);
        assertCommitOk(ctx, 'task2');
      },
      string: async (ctx) => {
        await ctx.run([
          'original = docx_read("contract.docx")',
          'draft = re.sub(r"Payment is due within \\d+ days", "Payment is due within 45 days", original)',
          'preview = docx_preview("contract.docx", original=original, proposed=draft)',
          'print("KEY=" + preview.key)',
        ]);
        assertPreviewOk(ctx, 'task2');
        await ctx.run([`docx_commit("contract.docx", ${JSON.stringify(ctx.lastKey())}, proposed=draft)`, 'print("committed")']);
        assertCommitOk(ctx, 'task2');
      },
      model: async (ctx) => {
        await ctx.run([
          'doc = docx_open("contract.docx")',
          'for p in doc.paragraphs:',
          '    if "Payment is due within" in p.text:',
          '        amount = p.text[p.text.index("within") + 7:p.text.index(" of")].strip()',
          '        if amount != "45 days":',
          '            p.replace(amount, "45 days")',
          'preview = docx_preview(doc)',
          'print("KEY=" + preview.key)',
        ]);
        assertPreviewOk(ctx, 'task2');
        await ctx.run([`docx_commit(doc, ${JSON.stringify(ctx.lastKey())})`, 'print("committed")']);
        assertCommitOk(ctx, 'task2');
      },
    },
  },
  {
    id: 'task3-ambiguous-occurrence',
    expected: EXPECTED_TEXTS['task3-ambiguous-occurrence'],
    run: {
      plan: async (ctx) => {
        await ctx.run([
          ...V1_READ_ITEMS,
          ...v1FindId('the terms shall govern', 'pid'),
          'plan = Plan(operations=[ReplaceText(at=pid, select="the terms", with_="THE TERMS", occurrence=3)])',
          'preview = docx_preview("contract.docx", plan)',
          'print("KEY=" + preview.key)',
        ]);
        assertPreviewOk(ctx, 'task3');
        await ctx.run([`docx_commit("contract.docx", ${JSON.stringify(ctx.lastKey())}, plan)`, 'print("committed")']);
        assertCommitOk(ctx, 'task3');
      },
      string: async (ctx) => {
        await ctx.run([
          'original = docx_read("contract.docx")',
          'part = "the terms"',
          'pos = []',
          'start = 0',
          'while True:',
          '    i = original.find(part, start)',
          '    if i == -1:',
          '        break',
          '    pos.append(i)',
          '    start = i + len(part)',
          'if len(pos) < 3:',
          '    raise ValueError("expected at least 3 occurrences")',
          'draft = original[:pos[2]] + "THE TERMS" + original[pos[2] + len(part):]',
          'preview = docx_preview("contract.docx", original=original, proposed=draft)',
          'print("KEY=" + preview.key)',
        ]);
        assertPreviewOk(ctx, 'task3');
        await ctx.run([`docx_commit("contract.docx", ${JSON.stringify(ctx.lastKey())}, proposed=draft)`, 'print("committed")']);
        assertCommitOk(ctx, 'task3');
      },
      model: async (ctx) => {
        await ctx.run([
          'doc = docx_open("contract.docx")',
          'for p in doc.paragraphs:',
          '    if "the terms shall" in p.text:',
          '        p.replace("the terms", "THE TERMS", occurrence=3)',
          'preview = docx_preview(doc)',
          'print("KEY=" + preview.key)',
        ]);
        assertPreviewOk(ctx, 'task3');
        await ctx.run([`docx_commit(doc, ${JSON.stringify(ctx.lastKey())})`, 'print("committed")']);
        assertCommitOk(ctx, 'task3');
      },
    },
  },
  {
    id: 'task4-arbitrary-formatting-selection',
    expected: EXPECTED_TEXTS['task4-arbitrary-formatting-selection'],
    run: {
      plan: async (ctx) => {
        await ctx.run([
          ...V1_READ_ITEMS,
          ...v1FindId('The warranty period is', 'pid'),
          'plan = Plan(operations=[FormatText(at=pid, select="warranty period", bold=True, underline=True)])',
          'preview = docx_preview("contract.docx", plan)',
          'print("KEY=" + preview.key)',
        ]);
        assertPreviewOk(ctx, 'task4');
        await ctx.run([`docx_commit("contract.docx", ${JSON.stringify(ctx.lastKey())}, plan)`, 'print("committed")']);
        assertCommitOk(ctx, 'task4');
      },
      string: async (ctx) => {
        await ctx.run([
          'original = docx_read("contract.docx")',
          'draft = original.replace("warranty period", "<b><u>warranty period</u></b>")',
          'preview = docx_preview("contract.docx", original=original, proposed=draft)',
          'print("KEY=" + preview.key)',
        ]);
        assertPreviewOk(ctx, 'task4');
        await ctx.run([`docx_commit("contract.docx", ${JSON.stringify(ctx.lastKey())}, proposed=draft)`, 'print("committed")']);
        assertCommitOk(ctx, 'task4');
      },
      model: async (ctx) => {
        await ctx.run([
          'doc = docx_open("contract.docx")',
          'sel = doc.select("warranty period")',
          'sel.bold = True',
          'sel.underline = True',
          'preview = docx_preview(doc)',
          'print("KEY=" + preview.key)',
        ]);
        assertPreviewOk(ctx, 'task4');
        await ctx.run([`docx_commit(doc, ${JSON.stringify(ctx.lastKey())})`, 'print("committed")']);
        assertCommitOk(ctx, 'task4');
      },
    },
  },
  {
    id: 'task5-paragraph-structure',
    expected: EXPECTED_TEXTS['task5-paragraph-structure'],
    run: {
      plan: async (ctx) => {
        await ctx.run([
          ...V1_READ_ITEMS,
          ...v1FindId('replaced entirely', 'idrepl'),
          ...v1FindId('anchors an insertion', 'idanchor'),
          ...v1FindId('will be deleted', 'idgone'),
          'plan = Plan(operations=[',
          '    ReplaceParagraph(at=idrepl, with_="Replacement paragraph."),',
          '    InsertParagraph(at=idanchor, position="after", with_="Inserted paragraph."),',
          '    DeleteParagraphs(at=[idgone]),',
          '])',
          'preview = docx_preview("contract.docx", plan)',
          'print("KEY=" + preview.key)',
        ]);
        assertPreviewOk(ctx, 'task5');
        await ctx.run([`docx_commit("contract.docx", ${JSON.stringify(ctx.lastKey())}, plan)`, 'print("committed")']);
        assertCommitOk(ctx, 'task5');
      },
      string: async (ctx) => {
        await ctx.run([
          'original = docx_read("contract.docx")',
          'draft = original.replace("This paragraph will be replaced entirely.", "Replacement paragraph.")',
          'draft = draft.replace("This paragraph anchors an insertion.</p>", "This paragraph anchors an insertion.</p>\\n<p>Inserted paragraph.</p>")',
          'draft = re.sub(r"<p [^>]*>This paragraph will be deleted\\.</p>", "", draft)',
          'preview = docx_preview("contract.docx", original=original, proposed=draft)',
          'print("KEY=" + preview.key)',
        ]);
        assertPreviewOk(ctx, 'task5');
        await ctx.run([`docx_commit("contract.docx", ${JSON.stringify(ctx.lastKey())}, proposed=draft)`, 'print("committed")']);
        assertCommitOk(ctx, 'task5');
      },
      model: async (ctx) => {
        await ctx.run([
          'doc = docx_open("contract.docx")',
          'for p in doc.paragraphs:',
          '    if "anchors an insertion" in p.text:',
          '        doc.insert_after(p, "Inserted paragraph.")',
          '    if "replaced entirely" in p.text:',
          '        p.text = "Replacement paragraph."',
          '    if "will be deleted" in p.text:',
          '        p.delete()',
          'preview = docx_preview(doc)',
          'print("KEY=" + preview.key)',
        ]);
        assertPreviewOk(ctx, 'task5');
        await ctx.run([`docx_commit(doc, ${JSON.stringify(ctx.lastKey())})`, 'print("committed")']);
        assertCommitOk(ctx, 'task5');
      },
    },
  },
  {
    id: 'task6-mixed-atomic-edit',
    expected: EXPECTED_TEXTS['task6-mixed-atomic-edit'],
    run: {
      plan: async (ctx) => {
        await ctx.run([
          ...V1_READ_ITEMS,
          ...v1FindId('notice period', 'idnotice'),
          ...v1FindId('The warranty period is', 'idwarranty'),
          ...v1FindId('anchors an insertion', 'idanchor'),
          ...v1FindId('will be deleted', 'idgone'),
          'plan = Plan(operations=[',
          '    ReplaceText(at=idnotice, select="thirty days", with_="<b>sixty days</b>"),',
          '    FormatText(at=idwarranty, select="warranty period", bold=True, underline=True),',
          '    InsertParagraph(at=idanchor, position="after", with_="Inserted paragraph."),',
          '    DeleteParagraphs(at=[idgone]),',
          '])',
          'preview = docx_preview("contract.docx", plan)',
          'print("KEY=" + preview.key)',
        ]);
        assertPreviewOk(ctx, 'task6');
        await ctx.run([`docx_commit("contract.docx", ${JSON.stringify(ctx.lastKey())}, plan)`, 'print("committed")']);
        assertCommitOk(ctx, 'task6');
      },
      string: async (ctx) => {
        await ctx.run([
          'original = docx_read("contract.docx")',
          'draft = original.replace("The notice period is <b>thirty days</b>.", "The notice period is <b>sixty days</b>.")',
          'draft = draft.replace("warranty period", "<b><u>warranty period</u></b>")',
          'draft = draft.replace("This paragraph anchors an insertion.</p>", "This paragraph anchors an insertion.</p>\\n<p>Inserted paragraph.</p>")',
          'draft = re.sub(r"<p [^>]*>This paragraph will be deleted\\.</p>", "", draft)',
          'preview = docx_preview("contract.docx", original=original, proposed=draft)',
          'print("KEY=" + preview.key)',
        ]);
        assertPreviewOk(ctx, 'task6');
        await ctx.run([`docx_commit("contract.docx", ${JSON.stringify(ctx.lastKey())}, proposed=draft)`, 'print("committed")']);
        assertCommitOk(ctx, 'task6');
      },
      model: async (ctx) => {
        await ctx.run([
          'doc = docx_open("contract.docx")',
          'for p in doc.paragraphs:',
          '    if "notice period" in p.text:',
          '        p.replace("thirty days", "sixty days")',
          '    if "anchors an insertion" in p.text:',
          '        doc.insert_after(p, "Inserted paragraph.")',
          '    if "will be deleted" in p.text:',
          '        p.delete()',
          'doc.select("sixty days").bold = True',
          'sel = doc.select("warranty period")',
          'sel.bold = True',
          'sel.underline = True',
          'preview = docx_preview(doc)',
          'print("KEY=" + preview.key)',
        ]);
        assertPreviewOk(ctx, 'task6');
        await ctx.run([`docx_commit(doc, ${JSON.stringify(ctx.lastKey())})`, 'print("committed")']);
        assertCommitOk(ctx, 'task6');
      },
    },
  },
  {
    id: 'task7-failed-preview-repair',
    expected: EXPECTED_TEXTS['task7-failed-preview-repair'],
    run: {
      plan: async (ctx) => {
        await ctx.run([
          ...V1_READ_ITEMS,
          ...v1FindId('notice period', 'pid'),
          'op = ReplaceText(at=pid, select="missing phrase", with_="x")',
          'plan = Plan(operations=[op])',
          'blocked = docx_preview("contract.docx", plan)',
          'print("BLOCKED" if blocked is None else "UNEXPECTED-SUCCESS")',
        ]);
        assert.match(ctx.text, /BLOCKED/, 'task7/plan: bad select must block the first preview');
        assert.match(ctx.text, /select matched nothing|── blocked ──/, 'task7/plan: blocked notice must explain the failed select');
        await ctx.run([
          'op.select = "thirty days"',
          'op.with_ = "<b>sixty days</b>"',
          'preview = docx_preview("contract.docx", plan)',
          'print("KEY=" + preview.key)',
        ]);
        assertPreviewOk(ctx, 'task7');
        await ctx.run([`docx_commit("contract.docx", ${JSON.stringify(ctx.lastKey())}, plan)`, 'print("committed")']);
        assertCommitOk(ctx, 'task7');
      },
      string: async (ctx) => {
        await ctx.run([
          'original = docx_read("contract.docx")',
          'draft = original.replace("The notice period is <b>thirty days</b>.", \'<p id="DEADBEEF" ord="99">The notice period is <b>sixty days</b>.</p>\')',
          'blocked = docx_preview("contract.docx", original=original, proposed=draft)',
          'print("BLOCKED" if blocked is None else "UNEXPECTED-SUCCESS")',
        ]);
        assert.match(ctx.text, /BLOCKED/, 'task7/string: invented id must block the first preview');
        assert.match(ctx.text, /invented paragraph id|── blocked ──/, 'task7/string: blocked notice must explain the invented id');
        await ctx.run([
          'draft = original.replace("The notice period is <b>thirty days</b>.", "The notice period is <b>sixty days</b>.")',
          'preview = docx_preview("contract.docx", original=original, proposed=draft)',
          'print("KEY=" + preview.key)',
        ]);
        assertPreviewOk(ctx, 'task7');
        await ctx.run([`docx_commit("contract.docx", ${JSON.stringify(ctx.lastKey())}, proposed=draft)`, 'print("committed")']);
        assertCommitOk(ctx, 'task7');
      },
      model: async (ctx) => {
        await ctx.run([
          'doc = docx_open("contract.docx")',
          'para = doc.paragraphs[0]',
          'for p in doc.paragraphs:',
          '    if "notice period" in p.text:',
          '        para = p',
          'try:',
          '    para.replace("missing phrase", "x")',
          '    print("UNEXPECTED-SUCCESS")',
          'except ValueError as e:',
          '    print("EXPECTED-ERROR", e)',
        ]);
        assert.match(ctx.text, /EXPECTED-ERROR/, 'task7/model: stale selector must raise a Python diagnostic');
        assert.match(ctx.text, /not found in paragraph/, 'task7/model: diagnostic must name the failure');
        await ctx.run([
          'para.replace("thirty days", "sixty days")',
          'doc.select("sixty days").bold = True',
          'preview = docx_preview(doc)',
          'print("KEY=" + preview.key)',
        ]);
        assertPreviewOk(ctx, 'task7');
        await ctx.run([`docx_commit(doc, ${JSON.stringify(ctx.lastKey())})`, 'print("committed")']);
        assertCommitOk(ctx, 'task7');
      },
    },
  },
  {
    id: 'task8-source-race',
    expected: EXPECTED_TEXTS['task8-source-race'],
    run: {
      plan: async (ctx) => {
        await ctx.run([
          ...V1_READ_ITEMS,
          ...v1FindId('notice period', 'pid'),
          'plan = Plan(operations=[ReplaceText(at=pid, select="thirty days", with_="<b>sixty days</b>")])',
          'preview = docx_preview("contract.docx", plan)',
          'print("KEY=" + preview.key)',
        ]);
        assertPreviewOk(ctx, 'task8');
        const tampered = await ctx.tamper((bytes) => Buffer.concat([bytes.subarray(0, 40), Buffer.from('XX'), bytes.subarray(40)]));
        await ctx.run([`docx_commit("contract.docx", ${JSON.stringify(ctx.lastKey())}, plan)`, 'print("done")']);
        assert.match(ctx.text, /source changed after preview/, 'task8/plan: commit must reject a changed source');
        assert.deepEqual(await ctx.rawBytes(), tampered, 'task8/plan: tampered bytes must be preserved');
      },
      string: async (ctx) => {
        await ctx.run([
          'original = docx_read("contract.docx")',
          'draft = original.replace("The notice period is <b>thirty days</b>.", "The notice period is <b>sixty days</b>.")',
          'preview = docx_preview("contract.docx", original=original, proposed=draft)',
          'print("KEY=" + preview.key)',
        ]);
        assertPreviewOk(ctx, 'task8');
        const tampered = await ctx.tamper((bytes) => Buffer.concat([bytes.subarray(0, 40), Buffer.from('XX'), bytes.subarray(40)]));
        await ctx.run([`docx_commit("contract.docx", ${JSON.stringify(ctx.lastKey())}, proposed=draft)`, 'print("done")']);
        assert.match(ctx.text, /source changed after preview/, 'task8/string: commit must reject a changed source');
        assert.deepEqual(await ctx.rawBytes(), tampered, 'task8/string: tampered bytes must be preserved');
      },
      model: async (ctx) => {
        await ctx.run([
          'doc = docx_open("contract.docx")',
          'for p in doc.paragraphs:',
          '    if "notice period" in p.text:',
          '        p.replace("thirty days", "sixty days")',
          'preview = docx_preview(doc)',
          'print("KEY=" + preview.key)',
        ]);
        assertPreviewOk(ctx, 'task8');
        const tampered = await ctx.tamper((bytes) => Buffer.concat([bytes.subarray(0, 40), Buffer.from('XX'), bytes.subarray(40)]));
        await ctx.run([`docx_commit(doc, ${JSON.stringify(ctx.lastKey())})`, 'print("done")']);
        assert.match(ctx.text, /source changed after preview/, 'task8/model: commit must reject a changed source');
        assert.deepEqual(await ctx.rawBytes(), tampered, 'task8/model: tampered bytes must be preserved');
      },
    },
  },
  {
    id: 'task9-mutation-after-preview',
    expected: EXPECTED_TEXTS['task9-mutation-after-preview'],
    run: {
      plan: async (ctx) => {
        await ctx.run([
          ...V1_READ_ITEMS,
          ...v1FindId('notice period', 'pid'),
          'op = ReplaceText(at=pid, select="thirty days", with_="<b>sixty days</b>")',
          'plan = Plan(operations=[op])',
          'preview = docx_preview("contract.docx", plan)',
          'print("KEY=" + preview.key)',
        ]);
        assertPreviewOk(ctx, 'task9');
        const before = await ctx.sha256();
        await ctx.run(['op.with_ = "<b>ninety days</b>"', 'print("mutated")']);
        assert.match(ctx.text, /mutated/, 'task9/plan: mutation feed must run');
        await ctx.run([`docx_commit("contract.docx", ${JSON.stringify(ctx.lastKey())}, plan)`, 'print("done")']);
        assert.match(ctx.text, /state changed after preview/, 'task9/plan: commit must reject mutated state');
        assert.equal(await ctx.sha256(), before, 'task9/plan: file must be untouched');
      },
      string: async (ctx) => {
        await ctx.run([
          'original = docx_read("contract.docx")',
          'draft = original.replace("The notice period is <b>thirty days</b>.", "The notice period is <b>sixty days</b>.")',
          'preview = docx_preview("contract.docx", original=original, proposed=draft)',
          'print("KEY=" + preview.key)',
        ]);
        assertPreviewOk(ctx, 'task9');
        const before = await ctx.sha256();
        await ctx.run(['draft = draft.replace("sixty days", "ninety days")', 'print("mutated")']);
        assert.match(ctx.text, /mutated/, 'task9/string: mutation feed must run');
        await ctx.run([`docx_commit("contract.docx", ${JSON.stringify(ctx.lastKey())}, proposed=draft)`, 'print("done")']);
        assert.match(ctx.text, /state changed after preview/, 'task9/string: commit must reject mutated state');
        assert.equal(await ctx.sha256(), before, 'task9/string: file must be untouched');
      },
      model: async (ctx) => {
        await ctx.run([
          'doc = docx_open("contract.docx")',
          'for p in doc.paragraphs:',
          '    if "notice period" in p.text:',
          '        p.replace("thirty days", "sixty days")',
          'preview = docx_preview(doc)',
          'print("KEY=" + preview.key)',
        ]);
        assertPreviewOk(ctx, 'task9');
        const before = await ctx.sha256();
        await ctx.run([
          'for p in doc.paragraphs:',
          '    if "notice period" in p.text:',
          '        p.replace("sixty days", "ninety days")',
          'print("mutated")',
        ]);
        assert.match(ctx.text, /mutated/, 'task9/model: mutation feed must run');
        await ctx.run([`docx_commit(doc, ${JSON.stringify(ctx.lastKey())})`, 'print("done")']);
        assert.match(ctx.text, /state changed after preview/, 'task9/model: commit must reject mutated state');
        assert.equal(await ctx.sha256(), before, 'task9/model: file must be untouched');
      },
    },
  },
  {
    id: 'task10-package-preservation',
    complex: true,
    run: {
      plan: async (ctx) => {
        await ctx.run([
          'import re',
          'original = docx_read("complex.docx")',
          'items = re.findall(r\'<p id="([0-9A-F]{8})"[^>]*>(.*?)</p>\', original)',
          `pid = ""`,
          `for item in items:`,
          `    if ${JSON.stringify(ctx.select)} in item[1]:`,
          `        pid = item[0]`,
          `plan = Plan(operations=[ReplaceText(at=pid, select=${JSON.stringify(ctx.select)}, with_=${JSON.stringify(ctx.select + ' [edited]')})])`,
          'preview = docx_preview("complex.docx", plan)',
          'print("KEY=" + preview.key)',
        ]);
        assertPreviewOk(ctx, 'task10');
        await ctx.run([`docx_commit("complex.docx", ${JSON.stringify(ctx.lastKey())}, plan)`, 'print("committed")']);
        assertCommitOk(ctx, 'task10');
      },
      string: async (ctx) => {
        await ctx.run([
          'original = docx_read("complex.docx")',
          `draft = original.replace(${JSON.stringify(ctx.select)}, ${JSON.stringify(ctx.select + ' [edited]')})`,
          'preview = docx_preview("complex.docx", original=original, proposed=draft)',
          'print("KEY=" + preview.key)',
        ]);
        assertPreviewOk(ctx, 'task10');
        await ctx.run([`docx_commit("complex.docx", ${JSON.stringify(ctx.lastKey())}, proposed=draft)`, 'print("committed")']);
        assertCommitOk(ctx, 'task10');
      },
      model: async (ctx) => {
        await ctx.run([
          'doc = docx_open("complex.docx")',
          `for p in doc.paragraphs:`,
          `    if ${JSON.stringify(ctx.select)} in p.text:`,
          `        p.replace(${JSON.stringify(ctx.select)}, ${JSON.stringify(ctx.select + ' [edited]')})`,
          'preview = docx_preview(doc)',
          'print("KEY=" + preview.key)',
        ]);
        assertPreviewOk(ctx, 'task10');
        await ctx.run([`docx_commit(doc, ${JSON.stringify(ctx.lastKey())})`, 'print("committed")']);
        assertCommitOk(ctx, 'task10');
      },
    },
  },
];

// ---------------------------------------------------------------------------
// Setup + metrics
// ---------------------------------------------------------------------------

let BASE_CWD;
let contractSource;
let complexSource;
const RESULT_CELLS = [];
const TASK_SUMMARY = [];

before(async () => {
  BASE_CWD = await mkdtemp(join(tmpdir(), 'docxdriver-repl-conformance-'));
  await ensureInit();
  contractSource = await createContractFixture(BASE_CWD);
  complexSource = await createComplexFixture(BASE_CWD);
});

after(async () => {
  await mkdir(join(PACKAGE_ROOT, 'test-results'), { recursive: true });
  await writeFile(
    join(PACKAGE_ROOT, 'test-results', 'repl-conformance.json'),
    JSON.stringify(
      {
        generated: new Date().toISOString(),
        cells: RESULT_CELLS,
        tasks: TASK_SUMMARY,
      },
      null,
      2,
    ),
  );
  await rm(BASE_CWD, { recursive: true, force: true });
});

/** Pick a clean, unique, regex-safe, uniformly PLAIN target inside the
 * complex fixture. Uniformly plain matters: the V2 reconciler folds the
 * source span's fmt into the replacement `with`, so a fmt'd target would
 * make the string surface's plan split (format_text) while the plan/model
 * surfaces stay plain — a non-comparable authoring difference. A plain
 * target keeps all three surfaces' withs plain ([replace_text] each). */
async function pickComplexTarget() {
  const markup = await renderDocxMarkup(complexSource, 'markup');
  const doc = parseProjection(markup);
  for (const p of doc.paragraphs) {
    if (!p.id || p.items.length === 0) continue;
    const plain = p.items.every(
      (i) => i.kind === 'text' && !i.fmt.bold && !i.fmt.italic && !i.fmt.underline && !i.fmt.strike && !i.fmt.superscript && !i.fmt.subscript,
    );
    if (!plain) continue;
    const head = p.plainText.slice(0, 40).trim();
    if (head.length < 12) continue;
    if (!/^[A-Za-z0-9 .-]+$/.test(head)) continue;
    if (markup.split(head).length - 1 !== 1) continue;
    return { id: p.id, select: head, text: p.plainText };
  }
  throw new Error('no clean unique plain text-only paragraph found in the complex fixture');
}

function runTask(task) {
  return async () => {
    const perSurface = {};
    const failures = [];
    const complexTarget = task.complex ? await pickComplexTarget() : null;

    for (const surface of REPL_SURFACES) {
      const { trial, ctx } = await setupTrial(surface, task.id, task.complex ? complexSource : contractSource, task.complex);
      trial.task = task.id;
      if (complexTarget !== null) ctx.select = complexTarget.select;
      try {
        await task.run[surface](ctx);

        // Task 8's file is intentionally corrupted: the run already asserted
        // the raw bytes are preserved, so no final-view render is possible —
        // capture the raw sha instead for cross-surface byte equality.
        // Task 10 asserts its own expected texts computed from the complex
        // fixture (task.expected is intentionally undefined there).
        if (task.id === 'task8-source-race') {
          perSurface[surface] = { ok: true, plan: ctx.planFor(), trial, rawSha: await ctx.sha256() };
        } else if (task.expected !== undefined) {
          const final = await ctx.render('final');
          assertFinalTexts(task.expected, final, `${task.id}/${surface}`);
          perSurface[surface] = { ok: true, plan: ctx.planFor(), trial, finalMarkup: final };
        } else {
          perSurface[surface] = { ok: true, plan: ctx.planFor(), trial };
        }

        // Per-task effect assertions beyond visible texts.
        if (task.id === 'task1-exact-text-correction' || task.id === 'task6-mixed-atomic-edit' || task.id === 'task7-failed-preview-repair') {
          const final = await ctx.render('final');
          assert.match(final, /<b>sixty/, `${task.id}/${surface}: sixty must be bold`);
          assert.ok(!final.includes('thirty days'), `${task.id}/${surface}: old text must be gone (final view)`);
        }
        if (task.id === 'task2-repeated-bulk-edit') {
          const final = await ctx.render('final');
          assert.ok(!final.includes('within 30 days'), `${task.id}/${surface}: 30-day term must be gone`);
          assert.ok(!final.includes('within 60 days'), `${task.id}/${surface}: 60-day term must be gone`);
        }
        if (task.id === 'task3-ambiguous-occurrence') {
          const final = await ctx.render('final');
          assert.equal(final.split('THE TERMS').length - 1, 1, `${task.id}/${surface}: exactly one uppercased occurrence`);
          assert.equal(final.split('the terms').length - 1, 3, `${task.id}/${surface}: three lowercase occurrences remain`);
        }
        if (task.id === 'task4-arbitrary-formatting-selection' || task.id === 'task6-mixed-atomic-edit') {
          const final = await ctx.render('final');
          assert.ok(final.includes('<b><u>warranty period</u></b>'), `${task.id}/${surface}: warranty period must be bold+underlined`);
        }
        if (task.id === 'task5-paragraph-structure' || task.id === 'task6-mixed-atomic-edit') {
          const final = await ctx.render('final');
          assert.ok(!final.includes('will be deleted'), `${task.id}/${surface}: deleted paragraph text must be gone`);
          assert.ok(final.includes('Inserted paragraph.'), `${task.id}/${surface}: inserted paragraph must be present`);
        }
        if (task.id === 'task10-package-preservation') {
          const final = await ctx.render('final');
          assert.ok(final.includes(`${complexTarget.select} [edited]`), `${task.id}/${surface}: edited text must be present`);
          const before = await zipPartHashes(complexSource);
          const after = await zipPartHashes(ctx.fixturePath);
          for (const [name, hash] of before) {
            if (name === 'word/document.xml') continue;
            assert.equal(after.get(name), hash, `${task.id}/${surface}: part ${name} must be byte-identical`);
          }
          const expected10 = parseProjection(await renderDocxMarkup(complexSource, 'final')).paragraphs.map((p) => p.plainText).filter((t) => t !== '');
          const idx = expected10.indexOf(complexTarget.text);
          assert.ok(idx !== -1, `${task.id}/${surface}: target paragraph found in base texts`);
          expected10[idx] = complexTarget.text.replace(complexTarget.select, `${complexTarget.select} [edited]`);
          assertFinalTexts(expected10, final, `${task.id}/${surface}`);
        }
      } catch (error) {
        perSurface[surface] = { ok: false, error, trial };
        failures.push(`${surface}: ${error && error.stack ? error.stack : String(error)}`);
      }
    }

    // Cross-surface equivalence.
    const surfaces = REPL_SURFACES;
    const allOk = surfaces.every((s) => perSurface[s].ok);
    const plans = surfaces.map((s) => perSurface[s].plan);
    let comparable = false;
    if (allOk) {
      if (task.id === 'task8-source-race') {
        // All three surfaces rejected the commit and preserved the same raw
        // bytes (identical fixture copies + identical tamper): cross-surface
        // byte equality is the genuine equivalence check for this task.
        const shas = surfaces.map((s) => perSurface[s].rawSha);
        assert.deepEqual(shas, [shas[0], shas[0], shas[0]], `${task.id}: surfaces must preserve identical bytes`);
        comparable = true;
      } else if (task.id === 'task1-exact-text-correction' || task.id === 'task2-repeated-bulk-edit' || task.id === 'task9-mutation-after-preview') {
        // Plans are not op-for-op comparable by design for these tasks (V1
        // authors wider selects than the reconciler's narrowest span; the
        // model folds formatting into an explicit op). Equivalence is the
        // cross-surface final-document structural equality (rule a), computed
        // from the three real final renders — not a constant.
        assertFinalDocsEquivalent(task.id, surfaces.map((s) => perSurface[s].finalMarkup));
        comparable = true;
      } else {
        assertPlansComparable(task.id, plans);
        comparable = true;
      }
    }

    for (const surface of surfaces) {
      const { trial } = perSurface[surface];
      RESULT_CELLS.push({
        surface,
        task: task.id,
        success: perSurface[surface].ok,
        toolCalls: trial.toolCalls,
        previewAttempts: trial.previewAttempts,
        commitAttempts: trial.commitAttempts,
        blockedCount: trial.blockedCount,
        recoveries: trial.recoveries,
        pythonChars: trial.pythonChars,
        wallMs: trial.wallMs,
        canonicalPlan: perSurface[surface].ok ? (perSurface[surface].plan ?? null) : null,
      });
      await trial.close();
    }

    TASK_SUMMARY.push({ task: task.id, equivalent: allOk && comparable, success: Object.fromEntries(surfaces.map((s) => [s, perSurface[s].ok])) });
    console.log(`task ${task.id}: plan=${perSurface.plan.ok ? 'ok' : 'FAIL'} string=${perSurface.string.ok ? 'ok' : 'FAIL'} model=${perSurface.model.ok ? 'ok' : 'FAIL'} equivalent=${allOk && comparable}`);

    if (failures.length > 0) {
      throw new Error(`task ${task.id} failed:\n${failures.join('\n')}`);
    }
    assert.ok(allOk && comparable, `task ${task.id}: equivalence assertion failed`);
  };
}

// ---------------------------------------------------------------------------
// Focused coverage for the folded-vs-explicit formatting normalization
// ---------------------------------------------------------------------------

test('comparability normalizes folded-vs-explicit formatting', () => {
  // The plan/string surfaces fold the fmt into the replacement markup; the
  // model surface emits a separate format_text. After normalization the three
  // must compare equal even though the replace_text span granularity differs
  // ("thirty" vs "thirty days" — already a masked field).
  const folded = {
    base: 'sha256:masked',
    author: 'docxdriver',
    change_mode: 'track',
    ops: [{ op: 'replace_text', at: '23E6DD62', select: 'thirty', with: '<b>sixty</b>' }],
  };
  const explicit = {
    base: 'sha256:masked',
    author: 'docxdriver',
    change_mode: 'track',
    ops: [
      { op: 'replace_text', at: '23E6DD62', select: 'thirty days', with: 'sixty days' },
      { op: 'format_text', at: '23E6DD62', select: 'sixty days', bold: true },
    ],
  };
  assertPlansComparable('test-fold', [folded, explicit, explicit]);

  // Multi-flag folded markup maps to one format_text with all flags.
  const foldedBoldUnderline = {
    ...folded,
    ops: [{ op: 'replace_text', at: '7B1502CA', select: 'warranty', with: '<b><u>warranty period</u></b>' }],
  };
  const explicitBoldUnderline = {
    ...explicit,
    ops: [
      { op: 'replace_text', at: '7B1502CA', select: 'warranty period', with: 'warranty period' },
      { op: 'format_text', at: '7B1502CA', select: 'warranty period', bold: true, underline: true },
    ],
  };
  assertPlansComparable('test-fold-flags', [foldedBoldUnderline, explicitBoldUnderline, explicitBoldUnderline]);

  // Plain withs (and non-text ops) are left untouched: no phantom format op.
  const plain = {
    ...folded,
    ops: [
      { op: 'replace_text', at: '23E6DD62', select: 'thirty days', with: 'sixty days' },
      { op: 'insert_paragraph', at: '7A7F5E65', position: 'after', with: 'Inserted paragraph.' },
      { op: 'delete_paragraphs', at: ['020555DF'] },
    ],
  };
  assertPlansComparable('test-plain', [plain, plain, plain]);

  // Negative: a replace_text whose with carries fmt but is NOT uniform (mixed
  // plain + fmt spans) must NOT split — only uniformly fmt-wrapped withs fold.
  const mixed = {
    ...folded,
    ops: [{ op: 'replace_text', at: '23E6DD62', select: 'The thirty days', with: 'The <b>sixty</b> days' }],
  };
  assert.equal(normalizeFoldedFormatting(mixed).ops.length, 1, 'mixed-fmt with must not split');
  assertPlansComparable('test-mixed', [mixed, mixed, mixed]);

  // Negative-adjacent: a folded split must not disturb an explicit format_text
  // that follows it — the derived op is inserted immediately after its parent
  // replace_text, before any authored ops.
  const adjacent = {
    ...folded,
    ops: [
      { op: 'replace_text', at: '23E6DD62', select: 'thirty', with: '<b>sixty</b>' },
      { op: 'format_text', at: '7B1502CA', select: 'warranty', underline: true },
    ],
  };
  assert.deepEqual(
    normalizeFoldedFormatting(adjacent).ops.map((op) => op.op),
    ['replace_text', 'format_text', 'format_text'],
    'folded split must precede the explicit format op',
  );
});

test('final-document structural equality catches fmt and id divergence', () => {
  const bold = '<p id="A0000001" ord="1">The notice period is <b>sixty days</b>.</p>';
  const plain = '<p id="A0000001" ord="1">The notice period is sixty days.</p>';
  const otherId = '<p id="B0000002" ord="1">The notice period is <b>sixty days</b>.</p>';
  // Identical final documents pass.
  assert.doesNotThrow(() => assertFinalDocsEquivalent('test-equal', [bold, bold, bold]));
  // Same visible text but divergent fmt (bold vs plain) must fail — the
  // equivalence metric now catches fmt-span divergence, not just plain text.
  assert.throws(() => assertFinalDocsEquivalent('test-fmt', [bold, plain, plain]), /final document differs/);
  // Divergent ids must fail (the excluded tasks never insert: ids are exact).
  assert.throws(() => assertFinalDocsEquivalent('test-id', [bold, otherId, otherId]), /final document differs/);
});

for (const task of TASKS) {
  test(task.id, { timeout: 300000 }, runTask(task));
}
