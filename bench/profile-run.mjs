#!/usr/bin/env node
// The --cpu-prof workload target: a representative large-document session
// (renderHtml markup + tracked editHtml + read, looped). Run under:
//
//   node --cpu-prof --cpu-prof-dir=bench/profiles bench/profile-run.mjs
//
// then summarize the newest profile with bench/summarize-profile.mjs.

import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { init, execute } from '../packages/docxdriver/dist/index.js';
import { FIXTURES_DIR } from './gen-fixtures.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const WASM_PATH = path.join(here, '../packages/docxdriver/wasm/docxdriver_bg.wasm');
const LARGE = path.join(FIXTURES_DIR, 'large.docx');

const iterations = Number(process.env.DOCXDRIVER_PROFILE_ITERS ?? 40);
const warmups = 3;
const OPTIONS = { dryRun: false, now: '2025-01-01T00:00:00Z' };

await init(await readFile(WASM_PATH));
const large = await readFile(LARGE);

const results = [];
for (let i = 0; i < warmups; i++) {
  execute(large, { type: 'renderHtml', view: 'markup' }, OPTIONS);
  execute(large, { type: 'editHtml', find: 'mkr000020', replace: '<del>mkr000020</del><ins>p</ins>', author: 'bench' }, OPTIONS);
  execute(large, { type: 'read', view: 'current' }, OPTIONS);
}

for (let i = 0; i < iterations; i++) {
  const t0 = performance.now();
  const html = execute(large, { type: 'renderHtml', view: 'markup' }, OPTIONS);
  const t1 = performance.now();
  const edited = execute(large, { type: 'editHtml', find: 'mkr000020', replace: '<del>mkr000020</del><ins>p</ins>', author: 'bench' }, OPTIONS);
  const t2 = performance.now();
  const read = execute(large, { type: 'read', view: 'current' }, OPTIONS);
  const t3 = performance.now();
  if (html.status !== 'ok' || edited.status !== 'ok' || read.status !== 'ok') {
    throw new Error(`profile workload failed: ${html.summary} / ${edited.summary} / ${read.summary}`);
  }
  results.push({ renderMs: t1 - t0, editMs: t2 - t1, readMs: t3 - t2, htmlChars: html.result.html.length, readParas: read.result.paragraphs.length });
}
const retained = results.map((r) => r.htmlChars + r.readParas);
console.log(JSON.stringify({ iterations, ops: iterations * 3, retained: retained.length, totals: results.reduce((a, r) => ({ renderMs: a.renderMs + r.renderMs, editMs: a.editMs + r.editMs, readMs: a.readMs + r.readMs }), { renderMs: 0, editMs: 0, readMs: 0 }) }));
