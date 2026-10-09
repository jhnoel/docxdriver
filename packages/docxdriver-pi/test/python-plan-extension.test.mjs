// Version 1 (Python-authored plan) surface tests: offline, real Monty + real WASM.
import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import pythonExtension, { renderExecution } from '../dist/python-plan-extension.js';
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
async function makeExtension(options) {
  const harness = stubPi();
  await pythonExtension(harness.pi, options);
  const shutdown = () => harness.handlers.session_shutdown();
  open.add(shutdown);
  return { ...harness, shutdown };
}

let baseCwd;
before(async () => {
  baseCwd = await mkdtemp(join(tmpdir(), 'docxdriver-pi-v1-'));
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

const CONTRACT_HTML = '<p>The notice period is <b>thirty days</b>.</p>';
const CORE_KEY_RE = /p1:sha256:[0-9a-f]{64}/;
const COMMIT_KEY_RE = /k1:[0-9a-f]{32}/;

async function createFixture(call, cwd) {
  await call(`docx_create("contract.docx", ${JSON.stringify(CONTRACT_HTML)})`);
  const id = (await renderMarkup(join(cwd, 'contract.docx'))).match(/<p id="([0-9A-F]{8})"/)[1];
  return id;
}

async function reviewAndCommit(call, firstId, extra = '') {
  const review = await call([
    `plan = Plan(author="Pi", change_mode="track", operations=[ReplaceText(at="${firstId}", select="thirty days", with_="sixty days")${extra ? `, ${extra}` : ''}])`,
    'review = docx_review("contract.docx", plan)',
    'commit_key = ""',
    'if review is None:',
    '    print("blocked")',
    'else:',
    '    commit_key = review.commit_key',
    '    print("KEY=" + commit_key)',
    '    print(review.edits)',
  ].join('\n'));
  assert.match(review.content[0].text, /review ok/);
  const key = review.content[0].text.match(/KEY=(k1:[0-9a-f]{32})/)?.[1];
  assert.ok(key, review.content[0].text);
  assert.match(review.content[0].text, /sixty days/);
  const commit = await call(['docx_commit(commit_key)', 'print("committed")'].join('\n'));
  return { review, commit, key };
}

test('v1: registers exactly the python tool and teaches the combined review flow', async () => {
  const { tools, shutdown } = await makeExtension();
  assert.deepEqual([...tools.keys()], ['python']);
  const tool = tools.get('python');
  assert.equal(tool.parameters.required[0], 'code');
  assert.equal(tool.parameters.properties.code.type, 'string');
  assert.ok(tool.description.includes('Plan'));
  assert.ok(tool.description.includes('SetEvenAndOddHeaders'));
  assert.ok(tool.description.includes('CommentAdd'));
  assert.ok(tool.description.includes('CommentReply'));
  assert.ok(tool.description.includes('CommentSetStatus'));
  assert.ok(tool.description.includes('CommentDelete'));
  assert.ok(tool.promptGuidelines.some((g) => g.includes('docx_review(path, plan)')));
  assert.ok(tool.promptGuidelines.some((g) => g.includes('docx_commit(review.commit_key)')));
  assert.ok(tool.promptGuidelines.some((g) => g.includes('SetEvenAndOddHeaders')));
  assert.ok(tool.promptGuidelines.some((g) => g.includes('CommentAdd')));
  assert.ok(!tool.promptGuidelines.some((g) => g.includes('docx_preview')));
  await shutdown();
  await shutdown();
});

test('v1: review validates, returns a compact key and edit neighborhoods, then commit uses that key', async () => {
  const { tools, shutdown } = await makeExtension();
  const cwd = join(baseCwd, 'authoring'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });
  const firstId = await createFixture(call, cwd);
  const { review, commit } = await reviewAndCommit(call, firstId);
  assert.match(review.content[0].text, /── review ok ──/);
  assert.match(review.content[0].text, COMMIT_KEY_RE);
  assert.match(review.content[0].text, /sixty days/);
  assert.match(commit.content[0].text, /committed/);
  const finalView = await renderMarkup(join(cwd, 'contract.docx'), 'final');
  assert.match(finalView, /sixty days/);
  assert.ok(!finalView.includes('thirty days'));
  await shutdown();
});

test('v1: structured Quote, Term, and Inline flow directly through plan review and commit', async () => {
  const { tools, shutdown } = await makeExtension({ hostOptions: { quoteAudit: { enabled: true } } });
  const cwd = join(baseCwd, 'structured-quotes'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });
  await call('docx_create("source.docx", "<p>The tenant must promptly pay the charge.</p>")');
  await call('docx_create("target.docx", "<p>Placeholder.</p>")');
  const sourceId = (await renderMarkup(join(cwd, 'source.docx'))).match(/<p id="([0-9A-F]{8})"/)[1];
  const targetId = (await renderMarkup(join(cwd, 'target.docx'))).match(/<p id="([0-9A-F]{8})"/)[1];
  const reviewed = await call([
    `q = Quote("source.docx", at="${sourceId}", select="The tenant must promptly pay the charge.")`,
    'q.omit(" promptly")',
    'q.bracket("tenant", "Tenant")',
    `plan = Plan(author="Pi", change_mode="direct", operations=[ReplaceParagraph(at="${targetId}", with_=Inline(["The term ", Term("quotation"), " introduces ", q]))])`,
    'review = docx_review("target.docx", plan)',
    'commit_key = ""',
    'if review is not None:',
    '    commit_key = review.commit_key',
    'print(review.commit_key if review is not None else "blocked")',
  ].join('\n'));
  assert.match(reviewed.content[0].text, COMMIT_KEY_RE);
  const committed = await call('docx_commit(commit_key)');
  assert.match(committed.content[0].text, /quotation audit warnings: 0/);
  const bytes = new Uint8Array(await readFile(join(cwd, 'target.docx')));
  const finalView = runRead(bytes, { kind: 'document', view: 'final' }).result.markup;
  assert.match(finalView, /The term “quotation” introduces “The \[Tenant\] must… pay the charge\.”/);
  assert.equal(runRead(bytes, { kind: 'comments' }).result.comments.length, 0);
  await shutdown();
});

test('v1: review and commit in one Python execution are blocked by the later-execution gate', async () => {
  const { tools, shutdown } = await makeExtension();
  const cwd = join(baseCwd, 'same-feed'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });
  const firstId = await createFixture(call, cwd);
  const result = await call([
    `plan = Plan(operations=[ReplaceText(at="${firstId}", select="thirty days", with_="sixty days")])`,
    'review = docx_review("contract.docx", plan)',
    'if review is None:',
    '    print("blocked")',
    'else:',
    '    docx_commit(review.commit_key)',
    'print("done")',
  ].join('\n'));
  assert.match(result.content[0].text, /review ok/);
  assert.match(result.content[0].text, /later python execution/);
  assert.match(result.content[0].text, /done/);
  await shutdown();
});

test('v1: invalid review returns no commit key and does not write', async () => {
  const { tools, shutdown } = await makeExtension();
  const cwd = join(baseCwd, 'invalid'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });
  const firstId = await createFixture(call, cwd);
  const result = await call([
    `plan = Plan(operations=[ReplaceText(at="${firstId}", select="missing phrase", with_="sixty days")])`,
    'review = docx_review("contract.docx", plan)',
    'print(review)',
  ].join('\n'));
  assert.match(result.content[0].text, /blocked/);
  assert.ok(!result.content[0].text.includes('KEY=k1:'), result.content[0].text);
  assert.match(await renderMarkup(join(cwd, 'contract.docx'), 'final'), /thirty days/);
  await shutdown();
});

test('v1: the deterministic core key never appears in returned values, details, or rendered output', async () => {
  const { tools, shutdown } = await makeExtension();
  const cwd = join(baseCwd, 'key-isolation'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });
  const firstId = await createFixture(call, cwd);
  const review = await call([
    `plan = Plan(operations=[ReplaceText(at="${firstId}", select="thirty days", with_="sixty days")])`,
    'review = docx_review("contract.docx", plan)',
    'print(review)',
  ].join('\n'));
  assert.ok(!CORE_KEY_RE.test(JSON.stringify(review)), JSON.stringify(review));
  const commit = await call(['docx_commit(review.commit_key)', 'print("committed")'].join('\n'));
  assert.ok(!CORE_KEY_RE.test(JSON.stringify(commit)), JSON.stringify(commit));
  await shutdown();
});

test('v1: the removed preview and page-based review APIs are rejected', async () => {
  const { tools, shutdown } = await makeExtension();
  const cwd = join(baseCwd, 'removed-api'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });
  const oldPreview = await call('docx_preview("contract.docx", Plan(operations=[]))');
  assert.equal(oldPreview.details.status, 'error');
  assert.match(oldPreview.content[0].text, /docx_preview|not defined|unknown/);
  const oldPageReview = await call('docx_review("k1:" + "a" * 32, page=1)');
  assert.equal(oldPageReview.details.status, 'error');
  assert.match(oldPageReview.content[0].text, /unexpected keyword|page|docx_review/);
  await shutdown();
});

test('v1: changing the plan after review cannot change the committed operation', async () => {
  const { tools, shutdown } = await makeExtension();
  const cwd = join(baseCwd, 'immutable-plan'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });
  const firstId = await createFixture(call, cwd);
  await call([
    `plan = Plan(operations=[ReplaceText(at="${firstId}", select="thirty days", with_="sixty days")])`,
    'review = docx_review("contract.docx", plan)',
    'commit_key = ""',
    'if review is None:',
    '    print("blocked")',
    'else:',
    '    commit_key = review.commit_key',
    'plan.operations[0].with_ = "ninety days"',
  ].join('\n'));
  const commit = await call('docx_commit(commit_key)\nprint("done")');
  assert.match(commit.content[0].text, /committed/);
  assert.match(await renderMarkup(join(cwd, 'contract.docx'), 'final'), /sixty days/);
  assert.ok(!commit.content[0].text.includes('ninety days'));
  await shutdown();
});

test('v1: shutdown expires a reviewed key and prevents replay in a fresh runtime', async () => {
  const { tools, shutdown } = await makeExtension();
  const cwd = join(baseCwd, 'shutdown-key'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });
  const firstId = await createFixture(call, cwd);
  const reviewed = await call([
    `plan = Plan(operations=[ReplaceText(at="${firstId}", select="thirty days", with_="sixty days")])`,
    'review = docx_review("contract.docx", plan)',
    'if review is None:',
    '    print("blocked")',
    'else:',
    '    print(review.commit_key)',
  ].join('\n'));
  const key = reviewed.content[0].text.match(/k1:[0-9a-f]{32}/)?.[0];
  assert.ok(key, reviewed.content[0].text);
  await shutdown();
  const replay = await call(`docx_commit("${key}")`);
  assert.match(replay.content[0].text, /expired|unknown commit key/);
  await shutdown();
});

test('v1: SetEvenAndOddHeaders and Comment* dataclasses review and commit', async () => {
  const { tools, shutdown } = await makeExtension();
  const cwd = join(baseCwd, 'comments-headers'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });
  const firstId = await createFixture(call, cwd);

  const shapes = await call([
    'print(SetEvenAndOddHeaders(even_and_odd=True).to_dict())',
    `print(CommentAdd(at="${firstId}", text="please review", select="thirty days").to_dict())`,
    'print(CommentReply(comment_id="0", text="ack").to_dict())',
    'print(CommentSetStatus(comment_id="0", status="resolved").to_dict())',
    'print(CommentDelete(comment_id="0").to_dict())',
  ].join('\n'));
  assert.match(shapes.content[0].text, /set_even_and_odd_headers/);
  assert.match(shapes.content[0].text, /comment_add/);
  assert.match(shapes.content[0].text, /comment_reply/);
  assert.match(shapes.content[0].text, /comment_set_status/);
  assert.match(shapes.content[0].text, /comment_delete/);

  const evenOddReview = await call([
    'plan = Plan(operations=[SetEvenAndOddHeaders(even_and_odd=True)])',
    'review = docx_review("contract.docx", plan)',
    'if review is None:',
    '    print("blocked")',
    'else:',
    '    print("KEY=" + review.commit_key)',
  ].join('\n'));
  assert.match(evenOddReview.content[0].text, /review ok/);
  const evenOddKey = evenOddReview.content[0].text.match(/KEY=(k1:[0-9a-f]{32})/)?.[1];
  assert.ok(evenOddKey, evenOddReview.content[0].text);
  const evenOddCommit = await call(`docx_commit("${evenOddKey}")`);
  assert.match(evenOddCommit.content[0].text, /committed/);

  const commentReview = await call([
    `plan = Plan(operations=[CommentAdd(at="${firstId}", select="thirty days", text="please review")])`,
    'review = docx_review("contract.docx", plan)',
    'if review is None:',
    '    print("blocked")',
    'else:',
    '    print("KEY=" + review.commit_key)',
    '    print(review.edits)',
  ].join('\n'));
  assert.match(commentReview.content[0].text, /review ok/);
  const commentKey = commentReview.content[0].text.match(/KEY=(k1:[0-9a-f]{32})/)?.[1];
  assert.ok(commentKey, commentReview.content[0].text);
  const commentCommit = await call(`docx_commit("${commentKey}")`);
  assert.match(commentCommit.content[0].text, /committed/);

  const commentsKw = await call([
    'import json',
    'c = docx_read("contract.docx", kind="comments")',
    'print(json.dumps(c))',
  ].join('\n'));
  assert.match(commentsKw.content[0].text, /please review/);
  assert.match(commentsKw.content[0].text, /"0"/);

  const commentsPos = await call([
    'import json',
    'c = docx_read("contract.docx", "comments")',
    'print(json.dumps(c))',
  ].join('\n'));
  assert.match(commentsPos.content[0].text, /please review/);
  assert.match(commentsPos.content[0].text, /"0"/);

  const followUpReview = await call([
    'plan = Plan(operations=[CommentReply(comment_id="0", text="ack"), CommentSetStatus(comment_id="0", status="resolved")])',
    'review = docx_review("contract.docx", plan)',
    'if review is None:',
    '    print("blocked")',
    'else:',
    '    print("KEY=" + review.commit_key)',
  ].join('\n'));
  assert.match(followUpReview.content[0].text, /review ok/);
  const followUpKey = followUpReview.content[0].text.match(/KEY=(k1:[0-9a-f]{32})/)?.[1];
  assert.ok(followUpKey, followUpReview.content[0].text);
  const followUpCommit = await call(`docx_commit("${followUpKey}")`);
  assert.match(followUpCommit.content[0].text, /committed/);
  await shutdown();
});

test('v1: renderExecution keeps compact review and commit protocol output visible', () => {
  const execution = { stdout: 'x'.repeat(60_000), value: '', error: undefined, reset: false };
  const notices = [
    { kind: 'review', path: 'a.docx', phase: 'review', status: 'ok', commitKey: `k1:${'a'.repeat(32)}`, summary: 'review ok: 1 ops' },
    { kind: 'commit', path: 'a.docx', phase: 'commit', status: 'ok', commitKey: `k1:${'a'.repeat(32)}`, summary: 'committed: 1 ops' },
  ];
  const rendered = renderExecution(execution, notices);
  assert.match(rendered, /review: k1:[0-9a-f]{32}/);
  assert.match(rendered, /commit: k1:[0-9a-f]{32}/);
  assert.match(rendered, /output truncated/);
});

test('v1: equations are discoverable and editable as native MathML', async () => {
  const { tools, shutdown } = await makeExtension();
  const cwd = join(baseCwd, 'equation-edit'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });
  await call('docx_create("math.docx", "<p>Value <equation>x</equation>.</p>")');
  const review = await call([
    'equations = docx_read("math.docx").equations',
    'print(equations)',
    'eq = equations[0]',
    'plan = Plan(author="Pi", operations=[ReplaceEquation(at=eq["at"], mathml="<math><mfrac><mi>a</mi><mi>b</mi></mfrac></math>")])',
    'review = docx_review("math.docx", plan)',
    'commit_key = ""',
    'if review is not None:',
    '    commit_key = review.commit_key',
    '    print("KEY=" + commit_key)',
  ].join('\n'));
  assert.match(review.content[0].text, /KEY=k1:/);
  const commit = await call('docx_commit(commit_key)');
  assert.match(commit.content[0].text, /committed/);
  assert.match(await renderMarkup(join(cwd, 'math.docx'), 'final'), /frac/);
  assert.match(await renderMarkup(join(cwd, 'math.docx'), 'original'), />x</);
  await shutdown();
});

test('v1: equations can be deleted and restored through tracked revisions', async () => {
  const { tools, shutdown } = await makeExtension();
  const cwd = join(baseCwd, 'equation-delete'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });
  await call('docx_create("math.docx", "<p>Value <equation>x</equation>.</p>")');
  const review = await call([
    'eq = docx_read("math.docx").equations[0]',
    'plan = Plan(author="Pi", change_mode="track", operations=[DeleteEquation(at=eq["at"], equation=eq["equation"])])',
    'review = docx_review("math.docx", plan)',
    'if review is None:',
    '    print("BLOCKED")',
    'else:',
    '    print("KEY=" + review.commit_key)',
  ].join('\n'));
  const keyMatch = review.content[0].text.match(/KEY=(k1:[0-9a-f]+)/);
  assert.ok(keyMatch, review.content[0].text);
  const key = keyMatch[1];
  await call(`docx_commit(${JSON.stringify(key)})`);
  assert.doesNotMatch(await renderMarkup(join(cwd, 'math.docx'), 'final'), /<math/);
  assert.match(await renderMarkup(join(cwd, 'math.docx'), 'original'), /<math/);
  await shutdown();
});

test('v1: complete document reads survive expression, print and later execution delivery limits', async () => {
  const { tools, shutdown } = await makeExtension();
  const cwd = join(baseCwd, 'full-document-delivery'); await mkdir(cwd, { recursive: true });
  const call = (code) => tools.get('python').execute('id', { code }, undefined, undefined, { cwd });
  const html = Array.from({length: 600}, (_, i) => `<p>Paragraph ${i}: 😀 Complete context must remain available. ${'Text '.repeat(30)}</p>`).join('\n');
  await call(`docx_create("large.docx", ${JSON.stringify(html)})`);
  const markup = await renderMarkup(join(cwd, 'large.docx'));
  assert.ok(Buffer.byteLength(markup) > 50_000);
  for (const code of ['docx_read("large.docx").markup', 'saved = docx_read("large.docx")\nprint(saved.markup)', 'saved.markup', 'docx_read("large.docx")']) {
    const result = await call(code);
    assert.ok(result.content[0].text.includes(markup) || result.content[0].text.includes(JSON.stringify(markup).slice(1,-1)) || result.content[0].text.includes(markup.replace(/\n/g, "\\n")), result.content[0].text.slice(-300));
    assert.doesNotMatch(result.content[0].text, /output truncated|value truncated/);
    assert.equal(result.content[0].text.split('<section data-docx-section=').length-1, 1);
  }
  await shutdown();
});

test('v1: complete HTML is delivered once when printed and returned together', () => {
  const markup = '<section data-docx-section="1"><p>' + '😀 context '.repeat(8000) + '</p></section>';
  const rendered = renderExecution({ok:true,stdout:markup+'\n',value:markup,reset:false}, []);
  assert.equal(rendered, markup);
});
