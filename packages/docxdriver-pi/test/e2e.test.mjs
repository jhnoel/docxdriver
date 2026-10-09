// Offline contract tests. Live Pi tests are isolated under test/pi-e2e and are
// opt-in (PI_E2E_LIVE=1); normal CI never starts a model.
import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, chmod, lstat, readFile, readdir, realpath, rm, stat, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { registerDocxTools } from '../dist/tools.js';
import { resolveUnderCwd, writeDocxBytes, createDocxBytes, sha256, fsyncDirIfSupported, readBackVerify } from '../dist/io.js';
import { generatedOperationSchema } from '../dist/schema.js';

function stubPi() {
  const tools = new Map();
  return { tools, pi: { registerTool(def) { tools.set(def.name, def); } } };
}

function firstParagraphId(result) {
  assert.equal(result.blocks, undefined, 'model reads do not repeat block markup');
  assert.ok(result.markup.includes('<p '), 'typed document reads expose markup');
  const id = result.markup.match(/<(?:p|h[1-6]) id="([0-9A-F]{8})"/)?.[1];
  assert.match(id ?? '', /^[0-9A-F]{8}$/, 'blocks carry canonical paragraph IDs');
  return id;
}

let cwd; let harness;
before(async () => { cwd = await mkdtemp(join(tmpdir(), 'docxdriver-pi-offline-')); harness = stubPi(); await registerDocxTools(harness.pi); });
after(async () => { await rm(cwd, { recursive: true, force: true }); });

test('registers exactly the five stable tools', () => {
  assert.deepEqual([...harness.tools.keys()].sort(), ['docx_create', 'docx_edit', 'docx_find', 'docx_help', 'docx_read']);
});

test('schemas make edit plan-only and read kinds unified', () => {
  const edit = harness.tools.get('docx_edit').parameters;
  assert.deepEqual(Object.keys(edit.properties).sort(), ['path', 'plan', 'preview_key']);
  assert.equal(edit.properties.plan.anyOf.length, 2);
  const read = harness.tools.get('docx_read').parameters;
  assert.ok(read.properties.kind.anyOf.some((x) => x.const === 'styles'));
  assert.equal(read.properties.page, undefined);
  const generated = generatedOperationSchema();
  const operations = generated.oneOf.flatMap((variant) => variant.properties.op.enum ?? [variant.properties.op.const]);
  assert.ok(operations.includes('replace_text'));
  assert.equal(operations.includes('editHtml'), false);
});

test('generated per-operation union rejects missing required edit fields', () => {
  const generated = generatedOperationSchema();
  const variants = generated.anyOf ?? generated.oneOf;
  assert.ok(Array.isArray(variants) && variants.length > 1, 'generated editOp must be a strict per-operation union');
  const variant = (op) => variants.find((entry) => entry.properties?.op?.const === op || entry.properties?.op?.enum?.includes(op));
  for (const [op, fields] of [['replace_text', ['at', 'select', 'with']], ['insert_paragraph', ['at', 'position']]]) {
    const entry = variant(op);
    assert.ok(entry, `missing generated ${op} variant`);
    for (const field of fields) assert.ok(entry.required?.includes(field), `${op} must require ${field}`);
    assert.equal(entry.additionalProperties, false);
  }
});

test('path containment rejects lexical and symlink escapes', async () => {
  await assert.rejects(() => resolveUnderCwd(cwd, '../outside.docx'), /escapes cwd/);
  const outside = await mkdtemp(join(tmpdir(), 'docxdriver-pi-out-'));
  try { await writeFile(join(outside, 'secret.docx'), 'secret'); await symlink(outside, join(cwd, 'link')); await assert.rejects(() => resolveUnderCwd(cwd, 'link/secret.docx'), /escapes cwd/); }
  finally { await rm(outside, { recursive: true, force: true }); }
});

test('exclusive create never overwrites, atomic replacement preserves bytes', async () => {
  const target = join(cwd, 'safe.docx'); const first = new Uint8Array([1, 2, 3]); const second = new Uint8Array([4, 5]);
  await createDocxBytes(cwd, target, first); await assert.rejects(() => createDocxBytes(cwd, target, second), /EEXIST/);
  await assert.rejects(() => writeDocxBytes(cwd, target, second, sha256(new Uint8Array([9]))), /source changed/);
  await writeDocxBytes(cwd, target, second, sha256(first)); assert.deepEqual([...await readFile(target)], [...second]);
  assert.deepEqual((await readdir(cwd)).filter((name) => name.endsWith('.tmp')), []);

  const outside = await mkdtemp(join(tmpdir(), 'docxdriver-pi-race-'));
  try {
    const outsideTarget = join(outside, 'outside.docx'); await writeFile(outsideTarget, 'outside');
    const createLink = join(cwd, 'create-link.docx'); await symlink(outsideTarget, createLink);
    await assert.rejects(() => createDocxBytes(cwd, createLink, new Uint8Array([1])), /outside|exclusive/);
    const writeLink = join(cwd, 'write-link.docx'); await symlink(outsideTarget, writeLink);
    await assert.rejects(() => writeDocxBytes(cwd, writeLink, new Uint8Array([1]), sha256(new Uint8Array(Buffer.from('outside')))), /outside/);
  } finally { await rm(outside, { recursive: true, force: true }); }
});

test('directory fsync is platform-gated and never throws for a real directory', async () => {
  // No-throw is the contract on darwin (fsync on an O_RDONLY directory fd
  // returns EINVAL, swallowed) and Linux (real directory sync).
  await fsyncDirIfSupported(cwd);
  const dir = await mkdtemp(join(tmpdir(), 'docxdriver-pi-dirsync-'));
  try { await fsyncDirIfSupported(dir); } finally { await rm(dir, { recursive: true, force: true }); }
});

test('readBackVerify asserts exact bytes and reports a mismatch as an infrastructure error', async () => {
  const target = join(cwd, 'readback.docx');
  const bytes = new Uint8Array([1, 2, 3, 4]);
  await writeFile(target, bytes);
  await readBackVerify(target, sha256(bytes), bytes.length); // exact match: no throw
  // Stat-first size probe: a size mismatch is a mismatch without reading.
  await assert.rejects(() => readBackVerify(target, sha256(bytes), 5), (e) => e.code === 'readback_mismatch');
  await assert.rejects(() => readBackVerify(target, sha256(new Uint8Array([9])), bytes.length), (e) => e.code === 'readback_mismatch');
  // The custom reader seam is honored.
  await assert.rejects(() => readBackVerify(target, sha256(bytes), bytes.length, async () => new Uint8Array([0])), (e) => e.code === 'readback_mismatch');
});

test('writeDocxBytes preserves the original file mode across atomic replacement', async () => {
  const target = join(cwd, 'mode.docx');
  await writeFile(target, new Uint8Array([1]));
  await chmod(target, 0o640);
  await writeDocxBytes(cwd, target, new Uint8Array([2, 3]), sha256(new Uint8Array([1])));
  const st = await stat(target);
  assert.equal(st.mode & 0o777, 0o640, 'the 0o600 temp mode must not replace the original file mode');
  assert.deepEqual([...await readFile(target)], [2, 3]);
});

test('writeDocxBytes rejects an in-window path-identity swap and honors the preview-bound root', async () => {
  // Byte-identical alias swapped in for the canonical path: containment and
  // source-hash checks pass, only expectedRealPath can reject it, and the
  // symlink survives (rename never runs).
  const real = join(cwd, 'real.docx');
  await writeFile(real, new Uint8Array([1]));
  const target = join(cwd, 'identity.docx');
  await writeFile(target, new Uint8Array([1]));
  const canonical = await realpath(target);
  await rm(target);
  await symlink(real, target);
  await assert.rejects(
    () => writeDocxBytes(cwd, target, new Uint8Array([2]), sha256(new Uint8Array([1])), { expectedRealPath: canonical }),
    (e) => e.code === 'path_changed',
  );
  assert.ok((await lstat(target)).isSymbolicLink(), 'the symlink must survive the rejected write');
  assert.deepEqual([...await readFile(real)], [1], 'the alias target must be untouched');
  assert.deepEqual((await readdir(cwd)).filter((name) => name.endsWith('.tmp')), []);

  // Preview-bound root: writing from a different cwd rejects against the
  // wrong root but succeeds when the preview-bound root is passed.
  const other = await mkdtemp(join(tmpdir(), 'docxdriver-pi-root-'));
  try {
    await writeFile(target, new Uint8Array([3]));
    await assert.rejects(() => writeDocxBytes(other, target, new Uint8Array([4]), sha256(new Uint8Array([3]))), /outside cwd/);
    await writeDocxBytes(other, target, new Uint8Array([4]), sha256(new Uint8Array([3])), { root: await realpath(cwd) });
    assert.deepEqual([...await readFile(target)], [4]);
  } finally { await rm(other, { recursive: true, force: true }); }
});

test('typed create/read/preview/commit and stale-key rejection are wired end to end', async () => {
  const path = 'typed.docx';
  const created = await harness.tools.get('docx_create').execute('id', { path, paragraphs: ['hello'] }, undefined, undefined, { cwd });
  assert.equal(created.details.outcome, 'completed');
  const read = await harness.tools.get('docx_read').execute('id', { path }, undefined, undefined, { cwd });
  const id = firstParagraphId(read.details.result);
  const plan = { operations: [{ op: 'replace_text', at: id, select: 'hello', with: 'world' }] };
  const preview = await harness.tools.get('docx_edit').execute('id', { path, plan }, undefined, undefined, { cwd });
  assert.equal(preview.details.outcome, 'previewed'); assert.match(preview.details.preview_key, /^p1:sha256:/);
  const stale = await harness.tools.get('docx_edit').execute('id', { path, plan, preview_key: 'p1:sha256:stale' }, undefined, undefined, { cwd });
  assert.equal(stale.details.outcome, 'rejected'); assert.equal(stale.details.diagnostic.code, 'preview_key_mismatch');
  const committed = await harness.tools.get('docx_edit').execute('id', { path, plan, preview_key: preview.details.preview_key }, undefined, undefined, { cwd });
  assert.equal(committed.details.outcome, 'committed'); assert.equal(committed.details.bytes, undefined, 'binary output stays in the host');
});

test('file plans preserve core TOML diagnostics for repair', async () => {
  const path = 'invalid-file-plan.docx';
  await harness.tools.get('docx_create').execute('id', { path, paragraphs: ['hello'] }, undefined, undefined, { cwd });
  await writeFile(join(cwd, 'broken.toml'), 'base = [not valid');
  const syntax = await harness.tools.get('docx_edit').execute('id', { path, plan: { file: 'broken.toml' } }, undefined, undefined, { cwd });
  assert.equal(syntax.details.outcome, 'rejected');
  assert.equal(syntax.details.diagnostic.code, 'invalid_toml');
  assert.ok(syntax.details.diagnostic.span);

  await writeFile(join(cwd, 'broken.toml'), 'base = "sha256:0000000000000000000000000000000000000000000000000000000000000000"\nauthor = "a"\nchange_mode = "track"\n\n[[ops]]\nop = "replace_text"\nat = "12345678"\nselect = "x"\nwith = "y"\nunknown = true\n');
  const semantic = await harness.tools.get('docx_edit').execute('id', { path, plan: { file: 'broken.toml' } }, undefined, undefined, { cwd });
  assert.equal(semantic.details.outcome, 'rejected');
  assert.equal(semantic.details.diagnostic.code, 'invalid_plan');
  assert.equal(semantic.details.diagnostic.path, 'ops[0].unknown');
  assert.ok(semantic.details.diagnostic.span);
});

test('registered read tool routes all four kinds without synthetic pagination', async () => {
  const path = 'read-kinds.docx';
  await harness.tools.get('docx_create').execute('id', { path, paragraphs: ['first page', 'second page'] }, undefined, undefined, { cwd });
  const read = harness.tools.get('docx_read');
  for (const kind of ['document', 'styles', 'comments', 'revisions']) {
    const result = await read.execute('id', { path, kind }, undefined, undefined, { cwd });
    assert.equal(result.details.outcome, 'completed', `${kind}: ${JSON.stringify(result.details)}`);
    assert.equal(result.details.result.kind, kind);
  }
  const document = await read.execute('id', { path, kind: 'document' }, undefined, undefined, { cwd });
  assert.equal(document.details.outcome, 'completed');
  assert.equal(document.details.result.kind, 'document');
  assert.equal(document.details.result.page, undefined);
  assert.equal(document.details.result.pages, undefined);
  const listingWithView = await read.execute('id', { path, kind: 'styles', view: 'markup' }, undefined, undefined, { cwd });
  assert.equal(listingWithView.details.outcome, 'completed', JSON.stringify(listingWithView.details));
  assert.equal(listingWithView.details.result.kind, 'styles');
});

test('source re-read rejects a changed target before commit', async () => {
  const path = 'changed.docx'; await harness.tools.get('docx_create').execute('id', { path, paragraphs: ['hello'] }, undefined, undefined, { cwd });
  const read = await harness.tools.get('docx_read').execute('id', { path }, undefined, undefined, { cwd });
  const plan = { operations: [{ op: 'replace_text', at: firstParagraphId(read.details.result), select: 'hello', with: 'world' }] };
  const preview = await harness.tools.get('docx_edit').execute('id', { path, plan }, undefined, undefined, { cwd });
  await writeFile(join(cwd, path), Buffer.from('changed externally'));
  const result = await harness.tools.get('docx_edit').execute('id', { path, plan, preview_key: preview.details.preview_key }, undefined, undefined, { cwd });
  assert.equal(result.details.outcome, 'rejected'); assert.ok(['source_changed', 'preview_key_mismatch'].includes(result.details.diagnostic.code));
});
