// Version 3 (structured Python document model) surface tests: offline, real
// Monty + real WASM.
import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm, mkdir } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import pythonExtension from '../dist/python-model-extension.js';
import { ensureInit, runRead } from '../dist/engine.js';

function stubPi() {
  const tools = new Map();
  const handlers = {};
  return {
    tools,
    handlers,
    pi: {
      registerTool(def) { tools.set(def.name, def); },
      on(name, fn) { handlers[name] = fn; },
    },
  };
}

const open = new Set();
async function makeExtension() {
  const harness = stubPi();
  await pythonExtension(harness.pi);
  const shutdown = () => harness.handlers.session_shutdown();
  open.add(shutdown);
  return { ...harness, shutdown };
}

let baseCwd;
before(async () => {
  baseCwd = await mkdtemp(join(tmpdir(), 'docxdriver-pi-v3-'));
  await ensureInit();
});
after(async () => {
  await Promise.all([...open].map((fn) => fn().catch(() => undefined)));
  open.clear();
  await rm(baseCwd, { recursive: true, force: true });
});

async function renderMarkup(abs, view = 'markup') {
  const output = runRead(new Uint8Array(await readFile(abs)), { kind: 'document', view });
  assert.equal(output.outcome, 'completed', output.diagnostic?.message);
  return output.result.markup;
}

// Plain sentences so para.replace / doc.select work on the model's plain text.
const CONTRACT_HTML = [
  '<p>The notice period is thirty days.</p>',
  '<p>Payment is due within 30 days of invoice receipt.</p>',
  '<p>Payment is due within 45 days of invoice receipt.</p>',
  '<p>Payment is due within 60 days of invoice receipt.</p>',
  '<p>This paragraph will be deleted.</p>',
  '<p>This paragraph anchors an insertion.</p>',
].join('');

test('v3: registers exactly the python tool', async () => {
  const { tools, shutdown } = await makeExtension();
  assert.deepEqual([...tools.keys()], ['python']);
  const tool = tools.get('python');
  assert.equal(tool.parameters.required[0], 'code');
  assert.equal(tool.parameters.properties.code.type, 'string');
  assert.ok(tool.description.includes('Document'));
  assert.ok(Array.isArray(tool.promptGuidelines) && tool.promptGuidelines.length > 0);
  await shutdown();
  await shutdown(); // idempotent
});

test('v3: traversal, selection, preview, later commit', async () => {
  const { tools, shutdown } = await makeExtension();
  const cwd = join(baseCwd, 'main'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });

  const created = await call(`docx_create("contract.docx", ${JSON.stringify(CONTRACT_HTML)})`);
  assert.match(created.content[0].text, /── create ok ──/);

  const first = await call([
    'doc = docx_open("contract.docx")',
    'for para in doc.paragraphs:',
    '    if "thirty days" in para.text:',
    '        para.replace("thirty days", "sixty days")',
    'doc.select("sixty days").bold = True',
    'preview = docx_preview(doc)',
    'print("KEY=" + preview.key)',
  ].join('\n'));
  const text = first.content[0].text;
  assert.match(text, /KEY=p1:sha256:[0-9a-f]{64}/);
  assert.match(text, /preview ok/);
  assert.match(text, /replace_text applied/);
  assert.match(text, /format_text applied/);
  const key = text.match(/KEY=(p1:sha256:[0-9a-f]{64})/)[1];

  const second = await call([
    'docx_commit(doc, preview.key)',
    'print("committed")',
  ].join('\n'));
  assert.match(second.content[0].text, /committed/);
  assert.ok(!second.content[0].text.includes('verification problem'), second.content[0].text);

  const final = await renderMarkup(join(cwd, 'contract.docx'), 'final');
  assert.match(final, /sixty days/);
  assert.match(final, /<b>sixty days<\/b>/);
  assert.ok(!final.includes('thirty days'));
  await shutdown();
});

test('v3: stale para.replace raises ValueError naming the paragraph', async () => {
  const { tools, shutdown } = await makeExtension();
  const cwd = join(baseCwd, 'stale-replace'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });
  await call(`docx_create("contract.docx", ${JSON.stringify(CONTRACT_HTML)})`);
  const firstId = (await renderMarkup(join(cwd, 'contract.docx'))).match(/<p id="([0-9A-F]{8})"/)[1];

  const result = await call([
    'doc = docx_open("contract.docx")',
    'para = doc.paragraphs[0]',
    'try:',
    '    para.replace("missing phrase", "x")',
    'except ValueError as e:',
    '    print("VALUE-ERROR", e)',
  ].join('\n'));
  assert.match(result.content[0].text, /VALUE-ERROR/);
  assert.match(result.content[0].text, /not found in paragraph/);
  assert.match(result.content[0].text, new RegExp(firstId));
  await shutdown();
});

test('v3: doc.select on missing text raises ValueError', async () => {
  const { tools, shutdown } = await makeExtension();
  const cwd = join(baseCwd, 'stale-select'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });
  await call(`docx_create("contract.docx", ${JSON.stringify(CONTRACT_HTML)})`);

  const result = await call([
    'doc = docx_open("contract.docx")',
    'try:',
    '    doc.select("missing")',
    'except ValueError as e:',
    '    print("VALUE-ERROR", e)',
  ].join('\n'));
  assert.match(result.content[0].text, /VALUE-ERROR/);
  assert.match(result.content[0].text, /not found in any paragraph/);
  await shutdown();
});

test('v3: mutation after preview blocks the commit', async () => {
  const { tools, shutdown } = await makeExtension();
  const cwd = join(baseCwd, 'mutation'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });
  await call(`docx_create("contract.docx", ${JSON.stringify(CONTRACT_HTML)})`);

  await call([
    'doc = docx_open("contract.docx")',
    'para = doc.paragraphs[0]',
    'para.replace("thirty days", "sixty days")',
    'preview = docx_preview(doc)',
    'print("previewed")',
  ].join('\n'));

  // Mutate the persistent model between preview and commit.
  const mutated = await call(['para.replace("sixty days", "ninety days")', 'print("mutated")'].join('\n'));
  assert.match(mutated.content[0].text, /mutated/);

  const commit = await call([
    'docx_commit(doc, preview.key)',
    'print("done")',
  ].join('\n'));
  const text = commit.content[0].text;
  assert.match(text, /state changed after preview/);
  assert.match(text, /done/);
  // Nothing was written: the file still has the ORIGINAL text.
  const final = await renderMarkup(join(cwd, 'contract.docx'), 'final');
  assert.match(final, /thirty days/);
  assert.ok(!final.includes('sixty days'));
  assert.ok(!final.includes('ninety days'));
  await shutdown();
});

test('v3: para.text setter produces a replace_paragraph', async () => {
  const { tools, shutdown } = await makeExtension();
  const cwd = join(baseCwd, 'text-setter'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });
  await call(`docx_create("contract.docx", ${JSON.stringify(CONTRACT_HTML)})`);

  const first = await call([
    'doc = docx_open("contract.docx")',
    'para = doc.paragraphs[0]',
    'para.text = "Complete replacement."',
    'preview = docx_preview(doc)',
    'print("KEY=" + preview.key)',
  ].join('\n'));
  const text = first.content[0].text;
  assert.match(text, /KEY=p1:sha256:[0-9a-f]{64}/);
  assert.match(text, /replace_paragraph applied/);
  const key = text.match(/KEY=(p1:sha256:[0-9a-f]{64})/)[1];

  const second = await call([
    'docx_commit(doc, preview.key)',
    'print("committed")',
  ].join('\n'));
  assert.match(second.content[0].text, /committed/);
  assert.ok(!second.content[0].text.includes('verification problem'), second.content[0].text);

  const final = await renderMarkup(join(cwd, 'contract.docx'), 'final');
  assert.match(final, /Complete replacement\./);
  assert.ok(!final.includes('The notice period is thirty days'));
  await shutdown();
});

test('v3: delete + insert in one preview commit atomically', async () => {
  const { tools, shutdown } = await makeExtension();
  const cwd = join(baseCwd, 'delete-insert'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });
  await call(`docx_create("contract.docx", ${JSON.stringify(CONTRACT_HTML)})`);

  const first = await call([
    'doc = docx_open("contract.docx")',
    'for p in doc.paragraphs:',
    '    if "will be deleted" in p.text:',
    '        p.delete()',
    '    if "anchors an insertion" in p.text:',
    '        doc.insert_after(p, "Inserted paragraph.")',
    'preview = docx_preview(doc)',
    'print("KEY=" + preview.key)',
  ].join('\n'));
  const text = first.content[0].text;
  assert.match(text, /KEY=p1:sha256:[0-9a-f]{64}/);
  assert.match(text, /delete_paragraphs applied/);
  assert.match(text, /insert_paragraph applied/);
  const key = text.match(/KEY=(p1:sha256:[0-9a-f]{64})/)[1];

  const second = await call([
    'docx_commit(doc, preview.key)',
    'print("committed")',
  ].join('\n'));
  assert.match(second.content[0].text, /committed/);
  assert.ok(!second.content[0].text.includes('verification problem'), second.content[0].text);

  const final = await renderMarkup(join(cwd, 'contract.docx'), 'final');
  assert.match(final, /Inserted paragraph\./);
  assert.match(final, /This paragraph anchors an insertion\./);
  assert.ok(!final.includes('will be deleted'));
  await shutdown();
});

test('v3: stale selection after a text change blocks preview with a paragraph diagnostic', async () => {
  const { tools, shutdown } = await makeExtension();
  const cwd = join(baseCwd, 'stale-fmt'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });
  await call(`docx_create("contract.docx", ${JSON.stringify(CONTRACT_HTML)})`);
  const firstId = (await renderMarkup(join(cwd, 'contract.docx'))).match(/<p id="([0-9A-F]{8})"/)[1];

  // Format a selection, then change the text so the selection no longer matches.
  const result = await call([
    'doc = docx_open("contract.docx")',
    'doc.select("thirty days").bold = True',
    'para = doc.paragraphs[0]',
    'para.replace("thirty days", "sixty days")',
    'preview = docx_preview(doc)',
    'print("stale-done")',
  ].join('\n'));
  const text = result.content[0].text;
  assert.match(text, /── blocked ──/);
  assert.match(text, /stale selection in paragraph/);
  assert.match(text, new RegExp(firstId));
  assert.match(text, /stale-done/);
  // No key issued and nothing written.
  assert.ok(!text.includes('preview key: p1:'), text);
  assert.match(await renderMarkup(join(cwd, 'contract.docx'), 'final'), /thirty days/);
  await shutdown();
});

test('v3: replace over a bold span yields a plain replacement (engine semantics)', async () => {
  const { tools, shutdown } = await makeExtension();
  const cwd = join(baseCwd, 'bold-plain'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });
  const boldHtml = ['<p>The <b>notice</b> period is thirty days.</p>'].join('');
  await call(`docx_create("contract.docx", ${JSON.stringify(boldHtml)})`);

  const first = await call([
    'doc = docx_open("contract.docx")',
    'para = doc.paragraphs[0]',
    'para.replace("notice", "word")',
    'preview = docx_preview(doc)',
    'print("KEY=" + preview.key)',
  ].join('\n'));
  assert.match(first.content[0].text, /KEY=p1:sha256:[0-9a-f]{64}/);
  const key = first.content[0].text.match(/KEY=(p1:sha256:[0-9a-f]{64})/)[1];

  const second = await call([
    'docx_commit(doc, preview.key)',
    'print("committed")',
  ].join('\n'));
  assert.match(second.content[0].text, /committed/);
  assert.ok(!second.content[0].text.includes('verification problem'), second.content[0].text);

  // The engine builds the replacement runs from the plain `with`: the final
  // view shows the replacement PLAIN even though the deleted span was bold.
  const final = await renderMarkup(join(cwd, 'contract.docx'), 'final');
  assert.match(final, /The word period is thirty days\./);
  assert.ok(!final.includes('<b>'), final);
  await shutdown();
});

test('v3: replace over bold then select-bold yields a bold replacement', async () => {
  const { tools, shutdown } = await makeExtension();
  const cwd = join(baseCwd, 'bold-then-select'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });
  const boldHtml = ['<p>The <b>notice</b> period is thirty days.</p>'].join('');
  await call(`docx_create("contract.docx", ${JSON.stringify(boldHtml)})`);

  const first = await call([
    'doc = docx_open("contract.docx")',
    'para = doc.paragraphs[0]',
    'para.replace("notice", "word")',
    'doc.select("word").bold = True',
    'preview = docx_preview(doc)',
    'print("KEY=" + preview.key)',
  ].join('\n'));
  assert.match(first.content[0].text, /KEY=p1:sha256:[0-9a-f]{64}/);
  const key = first.content[0].text.match(/KEY=(p1:sha256:[0-9a-f]{64})/)[1];

  const second = await call([
    'docx_commit(doc, preview.key)',
    'print("committed")',
  ].join('\n'));
  assert.match(second.content[0].text, /committed/);
  assert.ok(!second.content[0].text.includes('verification problem'), second.content[0].text);

  const final = await renderMarkup(join(cwd, 'contract.docx'), 'final');
  assert.match(final, /<b>word<\/b>/);
  assert.ok(!final.includes('notice'), final);
  await shutdown();
});

test('v3: selection format assigned False clears the mark', async () => {
  const { tools, shutdown } = await makeExtension();
  const cwd = join(baseCwd, 'fmt-false'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });
  const boldHtml = ['<p>The <b>notice</b> period is thirty days.</p>'].join('');
  await call(`docx_create("contract.docx", ${JSON.stringify(boldHtml)})`);

  const first = await call([
    'doc = docx_open("contract.docx")',
    'sel = doc.select("notice")',
    'sel.bold = True',
    'sel.bold = False',
    'preview = docx_preview(doc)',
    'print("KEY=" + preview.key)',
  ].join('\n'));
  assert.match(first.content[0].text, /KEY=p1:sha256:[0-9a-f]{64}/);
  const key = first.content[0].text.match(/KEY=(p1:sha256:[0-9a-f]{64})/)[1];

  const second = await call([
    'docx_commit(doc, preview.key)',
    'print("committed")',
  ].join('\n'));
  assert.match(second.content[0].text, /committed/);
  assert.ok(!second.content[0].text.includes('verification problem'), second.content[0].text);

  // Assigned False is recorded (format_text bold:false), so the mark is
  // removed rather than silently left True.
  const final = await renderMarkup(join(cwd, 'contract.docx'), 'final');
  assert.match(final, /The notice period is thirty days\./);
  assert.ok(!final.includes('<b>'), final);
  await shutdown();
});

test('v3: two insert_after calls at the same anchor commit in order', async () => {
  const { tools, shutdown } = await makeExtension();
  const cwd = join(baseCwd, 'insert-chain'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });
  await call(`docx_create("contract.docx", ${JSON.stringify(CONTRACT_HTML)})`);

  const first = await call([
    'doc = docx_open("contract.docx")',
    'for p in doc.paragraphs:',
    '    if "anchors an insertion" in p.text:',
    '        doc.insert_after(p, "First insert.")',
    '        doc.insert_after(p, "Second insert.")',
    'preview = docx_preview(doc)',
    'print("KEY=" + preview.key)',
  ].join('\n'));
  assert.match(first.content[0].text, /KEY=p1:sha256:[0-9a-f]{64}/);
  assert.match(first.content[0].text, /insert_paragraph applied/);
  const key = first.content[0].text.match(/KEY=(p1:sha256:[0-9a-f]{64})/)[1];

  const second = await call([
    'docx_commit(doc, preview.key)',
    'print("committed")',
  ].join('\n'));
  assert.match(second.content[0].text, /committed/);
  assert.ok(!second.content[0].text.includes('verification problem'), second.content[0].text);

  const final = await renderMarkup(join(cwd, 'contract.docx'), 'final');
  const firstIdx = final.indexOf('First insert.');
  const secondIdx = final.indexOf('Second insert.');
  const anchorIdx = final.indexOf('This paragraph anchors an insertion.');
  assert.ok(firstIdx !== -1 && secondIdx !== -1 && anchorIdx !== -1, final);
  assert.ok(anchorIdx < firstIdx && firstIdx < secondIdx, final);
  await shutdown();
});

test('v3: insert_after and insert_before at the same anchor keep the agent order', async () => {
  const { tools, shutdown } = await makeExtension();
  const cwd = join(baseCwd, 'mixed-insert'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });
  await call(`docx_create("contract.docx", ${JSON.stringify(CONTRACT_HTML)})`);

  const first = await call([
    'doc = docx_open("contract.docx")',
    'for p in doc.paragraphs:',
    '    if "anchors an insertion" in p.text:',
    '        doc.insert_after(p, "After text.")',
    '        doc.insert_before(p, "Before text.")',
    'preview = docx_preview(doc)',
    'print("KEY=" + preview.key)',
  ].join('\n'));
  assert.match(first.content[0].text, /KEY=p1:sha256:[0-9a-f]{64}/);
  const key = first.content[0].text.match(/KEY=(p1:sha256:[0-9a-f]{64})/)[1];

  const second = await call([
    'docx_commit(doc, preview.key)',
    'print("committed")',
  ].join('\n'));
  assert.match(second.content[0].text, /committed/);
  assert.ok(!second.content[0].text.includes('verification problem'), second.content[0].text);

  // The before-insert precedes the anchor, the after-insert follows it.
  const final = await renderMarkup(join(cwd, 'contract.docx'), 'final');
  const beforeIdx = final.indexOf('Before text.');
  const afterIdx = final.indexOf('After text.');
  const anchorIdx = final.indexOf('This paragraph anchors an insertion.');
  assert.ok(beforeIdx !== -1 && afterIdx !== -1 && anchorIdx !== -1, final);
  assert.ok(beforeIdx < anchorIdx && anchorIdx < afterIdx, final);
  await shutdown();
});

test('v3: replacement text with markup characters survives as literal text', async () => {
  const { tools, shutdown } = await makeExtension();
  const cwd = join(baseCwd, 'escaped-with'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });
  await call(`docx_create("contract.docx", ${JSON.stringify(CONTRACT_HTML)})`);

  const first = await call([
    'doc = docx_open("contract.docx")',
    'para = doc.paragraphs[0]',
    'para.replace("notice period is thirty days", "notice period is < 30 & 45 > days")',
    'preview = docx_preview(doc)',
    'print("KEY=" + preview.key)',
  ].join('\n'));
  assert.match(first.content[0].text, /KEY=p1:sha256:[0-9a-f]{64}/);
  const key = first.content[0].text.match(/KEY=(p1:sha256:[0-9a-f]{64})/)[1];

  const second = await call([
    'docx_commit(doc, preview.key)',
    'print("committed")',
  ].join('\n'));
  assert.match(second.content[0].text, /committed/);
  assert.ok(!second.content[0].text.includes('verification problem'), second.content[0].text);

  // The user text is escaped in the with dialect, so it lands in the document
  // as literal text — never parsed as tags.
  const final = await renderMarkup(join(cwd, 'contract.docx'), 'final');
  assert.match(final, /notice period is &lt; 30 &amp; 45 &gt; days/);
  assert.ok(!final.includes('< 30 & 45 >'), final);
  await shutdown();
});

test('v3: a deleted paragraph with pending format changes blocks preview', async () => {
  const { tools, shutdown } = await makeExtension();
  const cwd = join(baseCwd, 'deleted-pending'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });
  await call(`docx_create("contract.docx", ${JSON.stringify(CONTRACT_HTML)})`);

  const result = await call([
    'doc = docx_open("contract.docx")',
    'sel = doc.select("will be deleted")',
    'sel.bold = True',
    'for p in doc.paragraphs:',
    '    if "will be deleted" in p.text:',
    '        p.delete()',
    'preview = docx_preview(doc)',
    'print("blocked-done")',
  ].join('\n'));
  const text = result.content[0].text;
  assert.match(text, /── blocked ──/);
  assert.match(text, /deleted paragraph/);
  assert.match(text, /pending changes/);
  assert.match(text, /blocked-done/);
  assert.ok(!text.includes('preview key: p1:'), text);
  await shutdown();
});

test('v3: doc.regex formats every match', async () => {
  const { tools, shutdown } = await makeExtension();
  const cwd = join(baseCwd, 'regex'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });
  await call(`docx_create("contract.docx", ${JSON.stringify(CONTRACT_HTML)})`);

  const first = await call([
    'doc = docx_open("contract.docx")',
    'for match in doc.regex(r"\\b\\d+ days\\b"):',
    '    match.bold = True',
    'preview = docx_preview(doc)',
    'print("KEY=" + preview.key)',
  ].join('\n'));
  const text = first.content[0].text;
  assert.match(text, /KEY=p1:sha256:[0-9a-f]{64}/);
  const key = text.match(/KEY=(p1:sha256:[0-9a-f]{64})/)[1];

  const second = await call([
    'docx_commit(doc, preview.key)',
    'print("committed")',
  ].join('\n'));
  assert.match(second.content[0].text, /committed/);
  assert.ok(!second.content[0].text.includes('verification problem'), second.content[0].text);

  const final = await renderMarkup(join(cwd, 'contract.docx'), 'final');
  const bold = final.match(/<b>\d+ days<\/b>/g) ?? [];
  assert.equal(bold.length, 3, `expected 3 bolded day phrases in: ${final}`);
  await shutdown();
});
