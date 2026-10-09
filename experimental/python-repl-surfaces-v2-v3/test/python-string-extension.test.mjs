// Version 2 (raw projection string) surface tests: offline, real Monty + real WASM.
import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm, mkdir } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import pythonExtension from '../dist/python-string-extension.js';
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
  baseCwd = await mkdtemp(join(tmpdir(), 'docxdriver-pi-v2-'));
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

// Plain sentences so str.replace / re.sub work on the raw projection text
// (no inline tags to break substring matching).
const CONTRACT_HTML = [
  '<p>The notice period is thirty days.</p>',
  '<p>Payment is due within 30 days of invoice receipt.</p>',
].join('');

test('v2: registers exactly the python tool', async () => {
  const { tools, shutdown } = await makeExtension();
  assert.deepEqual([...tools.keys()], ['python']);
  const tool = tools.get('python');
  assert.equal(tool.parameters.required[0], 'code');
  assert.equal(tool.parameters.properties.code.type, 'string');
  assert.ok(tool.description.includes('proposed'));
  assert.ok(Array.isArray(tool.promptGuidelines) && tool.promptGuidelines.length > 0);
  await shutdown();
  await shutdown(); // idempotent
});

test('v2: string replace, preview, later commit', async () => {
  const { tools, shutdown } = await makeExtension();
  const cwd = join(baseCwd, 'main'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });

  const created = await call(`docx_create("contract.docx", ${JSON.stringify(CONTRACT_HTML)})`);
  assert.match(created.content[0].text, /── create ok ──/);

  const first = await call([
    'original = docx_read("contract.docx")',
    'draft = original.replace("The notice period is thirty days.", "The notice period is sixty days.")',
    'draft = re.sub(r"Payment is due within \\d+ days", "Payment is due within 45 days", draft)',
    'preview = docx_preview("contract.docx", original=original, proposed=draft)',
    'print("KEY=" + preview.key)',
  ].join('\n'));
  const text = first.content[0].text;
  assert.match(text, /KEY=p1:sha256:[0-9a-f]{64}/);
  assert.match(text, /preview ok/);
  const key = text.match(/KEY=(p1:sha256:[0-9a-f]{64})/)[1];

  const second = await call([
    'docx_commit("contract.docx", preview.key, proposed=draft)',
    'print("committed")',
  ].join('\n'));
  assert.match(second.content[0].text, /committed/);
  // Post-commit render verification must pass: no verification problem notice.
  assert.ok(!second.content[0].text.includes('verification problem'), second.content[0].text);

  // Tracked commit: the markup view keeps del/ins wrappers; the final view
  // shows the applied state as contiguous text.
  const markup = await renderMarkup(join(cwd, 'contract.docx'));
  assert.match(markup, /<ins[^>]*>sixty<\/ins> days/);
  assert.match(markup, /<ins[^>]*>45<\/ins> days/);
  const final = await renderMarkup(join(cwd, 'contract.docx'), 'final');
  assert.match(final, /sixty days/);
  assert.match(final, /Payment is due within 45 days/);
  assert.ok(!final.includes('thirty days'));
  assert.ok(!final.includes('30 days'));
  await shutdown();
});

test('v2: stale original blocks preview with a re-read diagnostic', async () => {
  const { tools, shutdown } = await makeExtension();
  const cwd = join(baseCwd, 'stale'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });
  await call(`docx_create("contract.docx", ${JSON.stringify(CONTRACT_HTML)})`);

  // A structurally valid but different projection: parses fine, fails the
  // stale-original check against the current document render.
  const bogusOriginal = '<p id="A0000001" ord="1">Something else entirely.</p>';
  const result = await call([
    'original = docx_read("contract.docx")',
    `preview = docx_preview("contract.docx", original=${JSON.stringify(bogusOriginal)}, proposed=original)`,
    'print("stale-done")',
  ].join('\n'));
  const text = result.content[0].text;
  assert.match(text, /── blocked ──/);
  assert.match(text, /original does not match the current document/);
  assert.match(text, /stale-done/);
  // Nothing was previewed: no key issued.
  assert.ok(!text.includes('preview key: p1:'), text);
  await shutdown();
});

test('v2: draft mutation after preview blocks the commit', async () => {
  const { tools, shutdown } = await makeExtension();
  const cwd = join(baseCwd, 'mutation'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });
  await call(`docx_create("contract.docx", ${JSON.stringify(CONTRACT_HTML)})`);

  await call([
    'original = docx_read("contract.docx")',
    'draft = original.replace("The notice period is thirty days.", "The notice period is sixty days.")',
    'preview = docx_preview("contract.docx", original=original, proposed=draft)',
    'print("previewed")',
  ].join('\n'));

  // Mutate the persistent draft between preview and commit.
  const mutated = await call(['draft = draft.replace("sixty days", "ninety days")', 'print("mutated")'].join('\n'));
  assert.match(mutated.content[0].text, /mutated/);

  const commit = await call([
    'docx_commit("contract.docx", preview.key, proposed=draft)',
    'print("done")',
  ].join('\n'));
  const text = commit.content[0].text;
  assert.match(text, /state changed after preview/);
  assert.match(text, /done/);
  // Nothing was written: the file still has the ORIGINAL text (a preview never
  // writes, and this commit was blocked).
  const final = await renderMarkup(join(cwd, 'contract.docx'), 'final');
  assert.match(final, /thirty days/);
  assert.ok(!final.includes('sixty days'));
  assert.ok(!final.includes('ninety days'));
  await shutdown();
});

test('v2: invented paragraph id in draft blocks with the reconciler reason', async () => {
  const { tools, shutdown } = await makeExtension();
  const cwd = join(baseCwd, 'invented'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });
  await call(`docx_create("contract.docx", ${JSON.stringify(CONTRACT_HTML)})`);
  const firstId = (await renderMarkup(join(cwd, 'contract.docx'))).match(/<p id="([0-9A-F]{8})"/)[1];

  const result = await call([
    'original = docx_read("contract.docx")',
    `draft = original.replace("${firstId}", "DEADBEEF")`,
    'preview = docx_preview("contract.docx", original=original, proposed=draft)',
    'print("invented-done")',
  ].join('\n'));
  const text = result.content[0].text;
  assert.match(text, /── blocked ──/);
  assert.match(text, /invented paragraph id/);
  assert.match(text, /invented-done/);
  // No key and no write.
  assert.ok(!text.includes('preview key: p1:'), text);
  assert.match(await renderMarkup(join(cwd, 'contract.docx')), new RegExp(firstId));
  await shutdown();
});

test('v2: deleting a paragraph commits cleanly (tracked merge into deleted element)', async () => {
  const { tools, shutdown } = await makeExtension();
  const cwd = join(baseCwd, 'delete-middle'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });
  const three = [
    '<p>First paragraph stays.</p>',
    '<p>Second paragraph will be deleted.</p>',
    '<p>Third paragraph stays.</p>',
  ].join('');
  await call(`docx_create("contract.docx", ${JSON.stringify(three)})`);

  const first = await call([
    'original = docx_read("contract.docx")',
    'draft = re.sub(r"<p [^>]*>Second paragraph will be deleted\\.</p>", "", original)',
    'preview = docx_preview("contract.docx", original=original, proposed=draft)',
    'print("KEY=" + preview.key)',
  ].join('\n'));
  assert.match(first.content[0].text, /KEY=p1:sha256:[0-9a-f]{64}/);
  assert.match(first.content[0].text, /delete_paragraphs applied/);

  const second = await call([
    'docx_commit("contract.docx", preview.key, proposed=draft)',
    'print("committed")',
  ].join('\n'));
  assert.match(second.content[0].text, /committed/);
  // The tracked delete merges the following paragraph's content into the
  // deleted paragraph's element: post-commit verification must not flag it.
  assert.ok(!second.content[0].text.includes('verification problem'), second.content[0].text);

  const final = await renderMarkup(join(cwd, 'contract.docx'), 'final');
  assert.match(final, /First paragraph stays/);
  assert.match(final, /Third paragraph stays/);
  assert.ok(!final.includes('will be deleted'));
  await shutdown();
});

test('v2: deleting the last paragraph commits cleanly (empty remnant tolerated)', async () => {
  const { tools, shutdown } = await makeExtension();
  const cwd = join(baseCwd, 'delete-last'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });
  const three = [
    '<p>First paragraph stays.</p>',
    '<p>Second paragraph stays.</p>',
    '<p>Third paragraph will be deleted.</p>',
  ].join('');
  await call(`docx_create("contract.docx", ${JSON.stringify(three)})`);

  const first = await call([
    'original = docx_read("contract.docx")',
    'draft = re.sub(r"<p [^>]*>Third paragraph will be deleted\\.</p>", "", original)',
    'preview = docx_preview("contract.docx", original=original, proposed=draft)',
    'print("KEY=" + preview.key)',
  ].join('\n'));
  assert.match(first.content[0].text, /KEY=p1:sha256:[0-9a-f]{64}/);

  const second = await call([
    'docx_commit("contract.docx", preview.key, proposed=draft)',
    'print("committed")',
  ].join('\n'));
  assert.match(second.content[0].text, /committed/);
  // The engine leaves the deleted paragraph as an empty element in the final
  // view; post-commit verification must tolerate the empty remnant.
  assert.ok(!second.content[0].text.includes('verification problem'), second.content[0].text);

  const final = await renderMarkup(join(cwd, 'contract.docx'), 'final');
  assert.match(final, /First paragraph stays/);
  assert.match(final, /Second paragraph stays/);
  assert.ok(!final.includes('will be deleted'));
  await shutdown();
});

test('v2: two consecutive id-less paragraphs commit in order (chained aliases)', async () => {
  const { tools, shutdown } = await makeExtension();
  const cwd = join(baseCwd, 'chained-inserts'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });
  const three = [
    '<p>First paragraph stays.</p>',
    '<p>Second paragraph stays.</p>',
    '<p>Third paragraph stays.</p>',
  ].join('');
  await call(`docx_create("contract.docx", ${JSON.stringify(three)})`);

  const first = await call([
    'original = docx_read("contract.docx")',
    'parts = original.split("\\n")',
    'draft = parts[0] + "\\n<p>Insert A.</p>\\n<p>Insert B.</p>\\n" + "\\n".join(parts[1:])',
    'preview = docx_preview("contract.docx", original=original, proposed=draft)',
    'print("KEY=" + preview.key)',
  ].join('\n'));
  assert.match(first.content[0].text, /KEY=p1:sha256:[0-9a-f]{64}/);
  assert.match(first.content[0].text, /insert_paragraph applied/);
  const key = first.content[0].text.match(/KEY=(p1:sha256:[0-9a-f]{64})/)[1];

  const second = await call([
    'docx_commit("contract.docx", preview.key, proposed=draft)',
    'print("committed")',
  ].join('\n'));
  assert.match(second.content[0].text, /committed/);
  assert.ok(!second.content[0].text.includes('verification problem'), second.content[0].text);

  const final = await renderMarkup(join(cwd, 'contract.docx'), 'final');
  assert.match(final, /First paragraph stays/);
  const aIdx = final.indexOf('Insert A.');
  const bIdx = final.indexOf('Insert B.');
  assert.ok(aIdx !== -1 && bIdx !== -1, final);
  assert.ok(aIdx < bIdx, final);
  assert.ok(bIdx < final.indexOf('Second paragraph stays'), final);
  await shutdown();
});

test('v2: docx_find returns tuples and its raw markup feeds a proposed replace', async () => {
  const { tools, shutdown } = await makeExtension();
  const cwd = join(baseCwd, 'find'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });
  const html = '<p>The <b>notice period</b> is thirty days.</p><p>Unrelated paragraph.</p>';
  await call(`docx_create("contract.docx", ${JSON.stringify(html)})`);

  const first = await call([
    'original = docx_read("contract.docx")',
    "found = docx_find('contract.docx', r'notice period')",
    'pid, text, raw = found[0]',
    'draft = original.replace(raw, raw.replace("thirty days", "sixty days"))',
    'preview = docx_preview("contract.docx", original=original, proposed=draft)',
    'print("KEY=" + preview.key)',
  ].join('\n'));
  const text = first.content[0].text;
  assert.match(text, /KEY=p1:sha256:[0-9a-f]{64}/);
  assert.match(text, /preview ok/);

  const second = await call([
    'docx_commit("contract.docx", preview.key, proposed=draft)',
    'print("committed")',
  ].join('\n'));
  assert.match(second.content[0].text, /committed/);
  assert.ok(!second.content[0].text.includes('verification problem'), second.content[0].text);
  const final = await renderMarkup(join(cwd, 'contract.docx'), 'final');
  assert.match(final, /sixty days/);
  assert.ok(!final.includes('thirty days'));
  await shutdown();
});

test('v2: docx_preview requires both original and proposed keyword arguments', async () => {
  const { tools, shutdown } = await makeExtension();
  const cwd = join(baseCwd, 'kwargs'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });
  await call(`docx_create("contract.docx", ${JSON.stringify(CONTRACT_HTML)})`);

  const result = await call([
    'original = docx_read("contract.docx")',
    'preview = docx_preview("contract.docx", original=original)',
    'print("kw-done")',
  ].join('\n'));
  assert.equal(result.details.status, 'error');
  assert.match(result.content[0].text, /proposed/);
  assert.match(result.content[0].text, /missing|TypeError/);
  await shutdown();
});
