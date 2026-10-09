#!/usr/bin/env node
// WASM-side benchmarks over the built docxdriver npm package.
//
// Phases (one per process; the runner orchestrates):
//   node bench/wasm-bench.mjs validate           # engine sanity-check of fixtures
//   node bench/wasm-bench.mjs cold               # cold init in fresh processes (parent)
//   node bench/wasm-bench.mjs cold-child <wasm>  # one fresh-process sample (spawned by cold)
//   node bench/wasm-bench.mjs warm               # warm execute latency per command (work.json)
//   node bench/wasm-bench.mjs batch              # sequential chains vs native batch lists
//   node bench/wasm-bench.mjs memory             # memoryUsage before/after init + high-water
//
// Every phase prints one JSON document to stdout. All timed loops check
// status/bytes per sample; expensive read-back verification runs once per
// command OUTSIDE the timed loop (see verifyOnce). Mutation iterations always
// start from pristine fixture bytes (stateless ABI) — equivalent input per
// iteration by construction.

import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { init, execute } from '../packages/docxdriver/dist/index.js';
import { FIXTURES_DIR, FIXTURE_SPECS, buildFixture } from './gen-fixtures.mjs';
import { buildWork, batchLists, workJsonPath } from './lib/work.mjs';
import { summarize } from './lib/stats.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.dirname(here);
export const WASM_PATH = path.join(REPO, 'packages/docxdriver/wasm/docxdriver_bg.wasm');
export const RESULTS_DIR = path.join(here, 'results');

const phase = process.argv[2];

async function loadFixtureBytes(file) {
  return readFile(path.join(FIXTURES_DIR, file));
}

function assertOk(out, label) {
  if (out.status !== 'ok') {
    throw new Error(`${label}: engine returned ${out.status}: ${out.summary}`);
  }
}

function assertDocxBytes(bytes, label) {
  if (!bytes || bytes.length < 4 || bytes[0] !== 0x50 || bytes[1] !== 0x4b) {
    throw new Error(`${label}: mutation did not return a ZIP/DOCX byte buffer`);
  }
}

// Read-back verification for one command result, OUTSIDE timed regions.
async function verifyOnce(fixtureBytes, entry, out) {
  const verify = entry.verify;
  if (!verify) return { kind: 'none' };
  switch (verify.kind) {
    case 'editHtml': {
      assertOk(out, entry.name);
      assertDocxBytes(out.bytes, entry.name);
      const read = execute(out.bytes, { type: 'read', view: 'current' });
      assertOk(read, `${entry.name} read-back`);
      const allText = read.result.paragraphs.map((p) => p.text).join('\n');
      if (!allText.includes(verify.replacement)) {
        throw new Error(`${entry.name}: replacement text not present after edit`);
      }
      if (allText.includes(verify.marker)) {
        throw new Error(`${entry.name}: marker still present after edit`);
      }
      return { kind: 'editHtml', revisions: out.result?.revisionIds?.length ?? 0 };
    }
    case 'addComment': {
      assertOk(out, entry.name);
      assertDocxBytes(out.bytes, entry.name);
      const comments = execute(out.bytes, { type: 'listComments' });
      assertOk(comments, `${entry.name} listComments`);
      if (comments.result.comments.length < 1) {
        throw new Error(`${entry.name}: expected ≥1 comment, got ${comments.result.comments.length}`);
      }
      return { kind: 'addComment', comments: comments.result.comments.length };
    }
    case 'setParagraphFormat': {
      assertOk(out, entry.name);
      if (out.result?.para !== verify.para) {
        throw new Error(`${entry.name}: expected para ${verify.para}, got ${out.result?.para}`);
      }
      assertDocxBytes(out.bytes, entry.name);
      return { kind: 'setParagraphFormat', para: out.result.para };
    }
    case 'acceptAllRevisions': {
      assertOk(out, entry.name);
      assertDocxBytes(out.bytes, entry.name);
      const before = execute(fixtureBytes, { type: 'listRevisions' });
      assertOk(before, `${entry.name} listRevisions-before`);
      const after = execute(out.bytes, { type: 'listRevisions' });
      assertOk(after, `${entry.name} listRevisions-after`);
      if (after.result.revisions.length !== 0) {
        throw new Error(`${entry.name}: ${after.result.revisions.length} revisions remain after acceptAll`);
      }
      return { kind: 'acceptAllRevisions', accepted: before.result.revisions.length };
    }
    case 'mutating': {
      assertOk(out, entry.name);
      assertDocxBytes(out.bytes, entry.name);
      return { kind: 'mutating' };
    }
    default:
      throw new Error(`${entry.name}: unknown verify kind ${verify.kind}`);
  }
}

const OPTIONS_OBJ = { dryRun: false, now: '2025-01-01T00:00:00Z' };

// ---------------- validate ----------------
async function phaseValidate() {
  await init(await readFile(WASM_PATH));
  const results = {};
  for (const name of Object.keys(FIXTURE_SPECS)) {
    const { bytes, meta } = buildFixture(name);
    const read = execute(bytes, { type: 'read', view: 'current' });
    assertOk(read, `${name} read`);
    if (read.result.paragraphs.length !== meta.paragraphs) {
      throw new Error(`${name}: expected ${meta.paragraphs} paragraphs, got ${read.result.paragraphs.length}`);
    }
    const count = execute(bytes, { type: 'wordCount' });
    assertOk(count, `${name} wordCount`);
    if (count.result.words !== meta.words) {
      throw new Error(`${name}: wordCount ${count.result.words} != expected ${meta.words}`);
    }
    const outline = execute(bytes, { type: 'outline' });
    assertOk(outline, `${name} outline`);
    if (outline.result.headings.length !== meta.headings) {
      throw new Error(`${name}: expected ${meta.headings} headings, got ${outline.result.headings.length}`);
    }
    const html = execute(bytes, { type: 'renderHtml', view: 'markup' });
    assertOk(html, `${name} renderHtml`);
    for (const i of meta.markers) {
      if (!html.result.html.includes(`mkr${String(i).padStart(6, '0')}`)) {
        throw new Error(`${name}: markup missing marker ${i}`);
      }
    }
    results[name] = {
      paragraphs: read.result.paragraphs.length,
      words: count.result.words,
      headings: outline.result.headings.length,
      htmlChars: html.result.html.length,
      fileBytes: bytes.length,
    };
  }
  console.log(JSON.stringify(results));
}

// ---------------- cold ----------------
async function coldChild(wasmPath) {
  const t0 = performance.now();
  const bytes = await readFile(wasmPath);
  const t1 = performance.now();
  await init(bytes);
  const t2 = performance.now();
  // First real call: forces the glue's lazy one-time setup (externref table…).
  const help = execute(undefined, { type: 'help', topic: 'editHtml' });
  if (help.status !== 'ok') throw new Error(`cold-child first execute failed: ${help.summary}`);
  const t3 = performance.now();
  const small = await loadFixtureBytes('small.docx');
  const t4 = performance.now();
  const read = execute(small, { type: 'read', view: 'current' }, OPTIONS_OBJ);
  assertOk(read, 'cold-child first DOCX execute');
  const t5 = performance.now();
  console.log(JSON.stringify({
    readMs: t1 - t0,
    initMs: t2 - t1,
    firstExecMs: t3 - t2,
    firstDocxExecMs: t5 - t4,
  }));
}

async function phaseCold() {
  const samples = [];
  const N = 8;
  for (let i = 0; i < N; i++) {
    const childStart = performance.now();
    const res = spawnSync(process.execPath, [process.argv[1], 'cold-child', WASM_PATH], {
      cwd: REPO,
      encoding: 'utf8',
      timeout: 60_000,
    });
    if (res.status !== 0) {
      throw new Error(`cold-child ${i} failed:\n${res.stderr}`);
    }
    samples.push({ ...JSON.parse(res.stdout.trim()), processElapsedMs: performance.now() - childStart });
  }
  // Baseline: a fresh Node process that does nothing (module load + startup).
  const baseline = [];
  for (let i = 0; i < 3; i++) {
    const t0 = performance.now();
    const res = spawnSync(process.execPath, ['-e', ''], { cwd: REPO, timeout: 30_000 });
    baseline.push(performance.now() - t0);
  }
  const moduleBytes = (await readFile(WASM_PATH)).length;
  const moduleSha = createHash('sha256').update(await readFile(WASM_PATH)).digest('hex');
  console.log(
    JSON.stringify({
      samples,
      initMs: summarize(samples.map((s) => s.initMs)),
      readMs: summarize(samples.map((s) => s.readMs)),
      firstExecMs: summarize(samples.map((s) => s.firstExecMs)),
      firstDocxExecMs: summarize(samples.map((s) => s.firstDocxExecMs)),
      processElapsedMs: summarize(samples.map((s) => s.processElapsedMs)),
      nodeBaselineSpawnMs: summarize(baseline),
      moduleBytes,
      moduleSha,
    }),
  );
}

// ---------------- warm ----------------
async function phaseWarm() {
  await init(await readFile(WASM_PATH));
  const work = buildWork();
  const results = [];
  for (const entry of work.commands) {
    const fixtureBytes = await loadFixtureBytes(entry.fixture);
    // Verification pass (untimed): status, mutation bytes, read-back.
    const out = execute(fixtureBytes, entry.command, { dryRun: false, now: '2025-01-01T00:00:00Z' });
    const verified = await verifyOnce(fixtureBytes, entry, out);

    for (let i = 0; i < entry.warmups; i++) {
      const w = execute(fixtureBytes, entry.command, OPTIONS_OBJ);
      assertOk(w, `${entry.name} warmup ${i}`);
    }
    const times = [];
    for (let i = 0; i < entry.iterations; i++) {
      const t0 = performance.now();
      const r = execute(fixtureBytes, entry.command, OPTIONS_OBJ);
      const elapsed = performance.now() - t0;
      assertOk(r, `${entry.name} sample ${i}`);
      if (entry.verify) assertDocxBytes(r.bytes, `${entry.name} sample ${i}`);
      times.push(elapsed);
    }
    results.push({ fixture: entry.fixture, name: entry.name, stats: summarize(times), verified });
  }
  console.log(JSON.stringify(results));
}

// ---------------- batch vs sequential ----------------
async function phaseBatch() {
  await init(await readFile(WASM_PATH));
  const rounds = 11;
  const results = [];
  for (const list of batchLists()) {
    const fixtureBytes = await loadFixtureBytes(list.fixture);
    const seqTimes = [];
    const batchTimes = [];
    let lastSeq = null;
    let lastBatch = null;
    const runSequential = () => {
      const seqOps = [];
      let state = fixtureBytes;
      const t0 = performance.now();
      for (const op of list.commands) {
        const out = execute(state, op, OPTIONS_OBJ);
        assertOk(out, `${list.label} seq op ${op.type}`);
        seqOps.push(out.status);
        if (out.bytes) state = out.bytes;
      }
      return { elapsed: performance.now() - t0, state, statuses: seqOps };
    };
    const runNativeBatch = () => {
      const t1 = performance.now();
      const out = execute(fixtureBytes, list.commands, OPTIONS_OBJ);
      const elapsed = performance.now() - t1;
      assertOk(out, `${list.label} batch`);
      if (out.result.applied !== list.commands.length || out.result.failed !== 0) {
        throw new Error(
          `${list.label} batch: applied ${out.result.applied}/${list.commands.length}, failed ${out.result.failed}`,
        );
      }
      return {
        elapsed,
        state: out.bytes ?? fixtureBytes,
        statuses: out.result.ops.map((op) => op.status),
      };
    };

    // Warm both paths, then alternate order to reduce systematic cache bias.
    runSequential();
    runNativeBatch();
    for (let r = 0; r < rounds; r++) {
      const first = r % 2 === 0 ? runSequential : runNativeBatch;
      const second = r % 2 === 0 ? runNativeBatch : runSequential;
      const a = first();
      const b = second();
      const seq = r % 2 === 0 ? a : b;
      const bat = r % 2 === 0 ? b : a;
      if (JSON.stringify(bat.statuses) !== JSON.stringify(seq.statuses)) {
        throw new Error(`${list.label}: batch/sequential per-op statuses differ`);
      }
      seqTimes.push(seq.elapsed);
      batchTimes.push(bat.elapsed);
      lastSeq = seq.state;
      lastBatch = bat.state;
    }
    // Serialization strategy can change ZIP byte layout, so compare every
    // public projection affected by this workload rather than claiming byte
    // identity from a single read view.
    const projections = [
      { type: 'read', view: 'current' },
      { type: 'read', view: 'all' },
      { type: 'renderHtml', view: 'markup' },
      { type: 'listComments' },
      { type: 'listRevisions' },
    ];
    for (const command of projections) {
      const seqFinal = execute(lastSeq, command);
      const batchFinal = execute(lastBatch, command);
      assertOk(seqFinal, `${list.label} seq final ${command.type}`);
      assertOk(batchFinal, `${list.label} batch final ${command.type}`);
      if (JSON.stringify(seqFinal.result) !== JSON.stringify(batchFinal.result)) {
        throw new Error(`${list.label}: sequential and batch ${command.type} projections differ`);
      }
    }
    results.push({
      label: list.label,
      ops: list.commands.length,
      mutationOps: list.commands.filter((c) => c.type !== 'read' && c.type !== 'findText' && c.type !== 'wordCount' && c.type !== 'outline' && c.type !== 'renderHtml' && c.type !== 'listRevisions' && c.type !== 'listComments' && c.type !== 'listMedia' && c.type !== 'listStyles').length,
      sequentialMs: summarize(seqTimes),
      batchMs: summarize(batchTimes),
      speedup: summarize(seqTimes).median / summarize(batchTimes).median,
      comparedFinalProjections: projections.map((p) => `${p.type}:${p.view ?? ''}`),
    });
  }
  console.log(JSON.stringify(results));
}

// ---------------- memory ----------------
async function phaseMemory() {
  if (typeof global.gc !== 'function') {
    console.error('memory phase requires --expose-gc (runner passes it)');
    process.exit(2);
  }
  const snap = () => {
    const m = process.memoryUsage();
    return { rss: m.rss, heapUsed: m.heapUsed, heapTotal: m.heapTotal, external: m.external, arrayBuffers: m.arrayBuffers };
  };
  const large = await loadFixtureBytes('large.docx');
  const mediumRevised = await loadFixtureBytes('medium-revised.docx');

  global.gc();
  const baseline = snap();

  const t0 = performance.now();
  await init(await readFile(WASM_PATH));
  const initMs = performance.now() - t0;
  global.gc();
  const afterInit = snap();

  // Warm workload: representative reads + heavy render on the large fixture.
  for (let i = 0; i < 5; i++) assertOk(execute(large, { type: 'read' }), 'mem read');
  for (let i = 0; i < 3; i++) assertOk(execute(large, { type: 'renderHtml', view: 'markup' }), 'mem render');
  for (let i = 0; i < 3; i++) assertOk(execute(large, { type: 'editHtml', find: 'mkr000020', replace: '<del>mkr000020</del><ins>mem</ins>', author: 'bench' }), 'mem edit');
  global.gc();
  const afterWarm = snap();

  // High-water test: repeatedly process the large document, retaining outputs
  // so the peak is visible, tracking max rss/external/arrayBuffers.
  const retained = [];
  let peak = { rss: 0, external: 0, arrayBuffers: 0, heapUsed: 0 };
  const iterations = 30;
  for (let i = 0; i < iterations; i++) {
    const html = execute(large, { type: 'renderHtml', view: 'markup' });
    assertOk(html, `highwater render ${i}`);
    retained.push(html.result.html);
    const edited = execute(large, { type: 'editHtml', find: 'mkr000020', replace: '<del>mkr000020</del><ins>hw</ins>', author: 'bench' });
    assertOk(edited, `highwater edit ${i}`);
    retained.push(edited.bytes);
    const read = execute(mediumRevised, { type: 'read', view: 'all' });
    assertOk(read, `highwater read-all ${i}`);
    retained.push(read.result.paragraphs.length);
    const m = process.memoryUsage();
    peak.rss = Math.max(peak.rss, m.rss);
    peak.external = Math.max(peak.external, m.external);
    peak.arrayBuffers = Math.max(peak.arrayBuffers, m.arrayBuffers);
    peak.heapUsed = Math.max(peak.heapUsed, m.heapUsed);
    if (i % 5 === 4) global.gc();
  }
  global.gc();
  const afterHighWater = snap(); // retained outputs still live
  const highWaterPeak = peak;
  retained.length = 0;
  global.gc();
  const afterRelease = snap();

  const growth = (a, b, key) => b[key] - a[key];
  console.log(
    JSON.stringify({
      initMs,
      baseline,
      afterInit,
      afterWarm,
      afterHighWater,
      afterRelease,
      highWaterPeak,
      growth: {
        rssInit: growth(baseline, afterInit, 'rss'),
        rssWarm: growth(baseline, afterWarm, 'rss'),
        rssHighWater: growth(baseline, afterHighWater, 'rss'),
        externalInit: growth(baseline, afterInit, 'external'),
        externalHighWater: growth(baseline, afterHighWater, 'external'),
        arrayBuffersInit: growth(baseline, afterInit, 'arrayBuffers'),
        arrayBuffersHighWater: growth(baseline, afterHighWater, 'arrayBuffers'),
        heapUsedWarm: growth(baseline, afterWarm, 'heapUsed'),
      },
      highWaterIterations: iterations,
    }),
  );
}

// ---------------- dispatch ----------------
if (process.argv[1] === fileURLToPath(import.meta.url)) {
  if (!phase) {
    console.error('usage: node bench/wasm-bench.mjs <validate|cold|cold-child|warm|batch|memory>');
    process.exit(2);
  }
  switch (phase) {
    case 'validate':
      phaseValidate().then(() => {}).catch((e) => { console.error(e); process.exit(1); });
      break;
    case 'cold':
      phaseCold().then(() => {}).catch((e) => { console.error(e); process.exit(1); });
      break;
    case 'cold-child':
      coldChild(process.argv[3]).then(() => {}).catch((e) => { console.error(e); process.exit(1); });
      break;
    case 'warm':
      phaseWarm().then(() => {}).catch((e) => { console.error(e); process.exit(1); });
      break;
    case 'batch':
      phaseBatch().then(() => {}).catch((e) => { console.error(e); process.exit(1); });
      break;
    case 'memory':
      phaseMemory().then(() => {}).catch((e) => { console.error(e); process.exit(1); });
      break;
    default:
      console.error(`unknown phase: ${phase}`);
      process.exit(2);
  }
}
