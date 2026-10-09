#!/usr/bin/env node
// Preview/commit engine measurements, independent of live-model latency.
import { readFile, writeFile } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { spawnSync } from 'node:child_process';
import { summarize } from './lib/stats.mjs';

const repo = resolve(import.meta.dirname, '..');
const out = resolve(process.argv[2]);
const baseline = resolve(process.argv[3] ?? '/tmp/docxdriver-parity-baseline-0e5a0f9');
const compactBaseline = process.argv.includes("--compact-baseline");
const engines = {};
for (const [variant, dir] of [['old', compactBaseline ? join(repo, 'target/html-head-to-head/compact-before-astra') : join(baseline, 'packages/docxdriver')], ['new', join(repo, 'packages/docxdriver')]]) {
  const engine = await import(pathToFileURL(join(dir, 'dist/index.js')).href);
  await engine.init(await readFile(join(dir, 'wasm/docxdriver_bg.wasm'))); engines[variant] = engine;
}
const fixtureDir = join(out, 'fixtures');
const rows = [];
for (const task of ['text', 'mixed', 'equation']) {
  const name = task === 'equation' ? 'equation' : 'contract';
  const original = join(fixtureDir, name + '.docx'); const bytes = await readFile(original);
  const result = engines.new.executeRequest(bytes, { Command: { command: { kind: 'read', read_kind: 'document_ui', view: 'final' } } }).result;
  const id = text => result.paragraphs.find(p => p.text === text || p.text.startsWith(text)).id;
  const ops = variant => task === 'equation'
    ? [{ op: 'replace_equation', at: id('Equation '), ...(variant === 'old' && !compactBaseline ? { latex: '\\frac{a+b}{c}' } : { mathml: '<math><mfrac><mrow><mi>a</mi><mo>+</mo><mi>b</mi></mrow><mi>c</mi></mfrac></math>' }) }]
    : [
      { op: 'replace_text', at: id('The notice period is'), select: 'thirty days', with: '<b>sixty days</b>' },
      ...(task === 'mixed' ? [
        { op: 'format_text', at: id('The warranty period is'), select: 'warranty period', bold: true, underline: true },
        { op: 'insert_paragraph', at: id('This paragraph anchors an insertion.'), position: 'after', with: 'Inserted paragraph.' },
        { op: 'delete_paragraphs', at: [id('This paragraph will be deleted.')] },
      ] : []),
    ];
  const row = { task };
  const times = { old: { preview: [], commit: [] }, new: { preview: [], commit: [] } };
  for (let iteration = -5; iteration < 50; iteration++) {
    for (const variant of iteration % 2 ? ['new', 'old'] : ['old', 'new']) {
      const engine = engines[variant]; const plan = { base: result.source, author: 'Benchmark', change_mode: 'direct', ops: ops(variant) };
      let start = performance.now(); const preview = engine.executeRequest(bytes, { Plan: { plan } });
      const previewMs = performance.now() - start;
      if (preview.outcome !== 'previewed') throw new Error(JSON.stringify(preview));
      start = performance.now(); const commit = engine.executeRequest(bytes, { Plan: { plan, preview_key: preview.preview_key } });
      const commitMs = performance.now() - start;
      if (commit.outcome !== 'committed' || !commit.bytes) throw new Error(JSON.stringify(commit));
      if (iteration >= 0) { times[variant].preview.push(previewMs); times[variant].commit.push(commitMs); }
      if (iteration === 0) {
        const candidate = join(out, `${task}-${variant}-engine.docx`); await writeFile(candidate, commit.bytes);
        const check = spawnSync('python3', [join(repo, 'bench/html-head-to-head-verify.py'), task, original, candidate], { encoding: 'utf8' });
        if (check.status !== 0) throw new Error(check.stderr);
        const verification = JSON.parse(check.stdout); if (verification.outcome !== 'ok') throw new Error(JSON.stringify(verification));
        const { bytes: _bytes, ...commitResponse } = commit;
        row[variant] = { verification, preview_response_bytes: Buffer.byteLength(JSON.stringify(preview)), commit_response_bytes: Buffer.byteLength(JSON.stringify(commitResponse)) };
      }
    }
  }
  for (const variant of ['old', 'new']) Object.assign(row[variant], { preview_ms: summarize(times[variant].preview), commit_ms: summarize(times[variant].commit) });
  rows.push(row);
}
await writeFile(join(out, 'edit-performance.json'), JSON.stringify(rows, null, 2));
console.log(JSON.stringify(rows, null, 2));
